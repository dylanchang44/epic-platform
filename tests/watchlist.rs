#![cfg(feature = "ssr")]
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use epic_platform::{
    config::AppConfig,
    jobs::{domain::*, runner, service::JobService},
    research::{
        domain::{Company, ResearchError},
        repository::ResearchRepository,
        service::ResearchService,
        source::normalize_response,
    },
    review::service::ReviewService,
    state::{AppState, PortfolioState},
    symbol::StockSymbol,
    watchlist::domain::*,
};
use std::{sync::Arc, time::Duration};
use tempfile::{TempDir, tempdir};
use tower::ServiceExt;

struct Harness {
    dir: TempDir,
    portfolio: Arc<PortfolioState>,
    research: Arc<ResearchService>,
    review: Arc<ReviewService>,
    jobs: Arc<JobService>,
}
impl Harness {
    async fn new(symbols: &str) -> Self {
        let dir = tempdir().unwrap();
        let portfolio = PortfolioState::new(AppConfig {
            schwab_data_dir: "tests/fixtures/research".into(),
            ..AppConfig::default()
        });
        // Deliberately do not load portfolio. Source client cannot fetch anything.
        let research = ResearchService::with_source(
            &dir.path().join("research.db"),
            Err(ResearchError::SourceUnavailable),
        )
        .await;
        let review = ReviewService::open(&dir.path().join("execution.db")).await;
        let jobs = JobService::with_watchlist(&review, WatchlistInput::parse(symbols));
        Self {
            dir,
            portfolio,
            research,
            review,
            jobs,
        }
    }
    fn input(&self) -> JobInput {
        JobInput::WatchlistBriefing {
            symbols: self.jobs.watchlist().submission_input().unwrap(),
        }
    }
    async fn enqueue(&self) -> Job {
        self.jobs
            .repository()
            .unwrap()
            .enqueue_input(&self.input(), None)
            .await
            .unwrap()
    }
    async fn run(&self) {
        assert!(
            runner::run_one(
                self.jobs.clone(),
                self.portfolio.clone(),
                self.research.clone(),
                self.review.clone()
            )
            .await
            .unwrap()
        );
    }
    async fn start(&self) -> runner::Worker {
        runner::start(
            self.jobs.clone(),
            self.portfolio.clone(),
            self.research.clone(),
            self.review.clone(),
        )
        .await
        .unwrap()
    }
    async fn terminal(&self, id: i64) -> Job {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let job = self.jobs.get(id).await.unwrap();
                if !job.status.is_active() {
                    break job;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap()
    }
    async fn saved(&self, id: i64) -> SavedBriefing {
        self.jobs
            .watchlist()
            .repository()
            .unwrap()
            .get(id)
            .await
            .unwrap()
    }
    async fn seed(&self, newer: bool) {
        let mut snapshot = normalize_response(
            &Company::find(&StockSymbol::parse("MSFT").unwrap()).unwrap(),
            include_bytes!("fixtures/research/forecast.json"),
            "2025-08-02T00:00:00Z",
        )
        .unwrap();
        if newer {
            snapshot.period.ended_on = "2026-03-31".into();
            snapshot.period.label = "Q1 2026".into();
            snapshot.revenue += rust_decimal::Decimal::ONE;
        }
        ResearchRepository::open(&self.dir.path().join("research.db"))
            .await
            .unwrap()
            .save(&snapshot)
            .await
            .unwrap();
    }
    fn app(&self) -> axum::Router {
        epic_platform::server::router(AppState {
            leptos_options: leptos::prelude::get_configuration(Some("Cargo.toml"))
                .unwrap()
                .leptos_options,
            portfolio: self.portfolio.clone(),
            research: self.research.clone(),
            review: self.review.clone(),
            jobs: self.jobs.clone(),
        })
    }
}
async fn call(
    app: &axum::Router,
    method: &str,
    path: &str,
    key: Option<&str>,
) -> (StatusCode, serde_json::Value) {
    let mut request = Request::builder().method(method).uri(path);
    if let Some(key) = key {
        request = request.header("Idempotency-Key", key);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    if status == StatusCode::ACCEPTED {
        assert!(
            response.headers()["location"]
                .to_str()
                .unwrap()
                .starts_with("/api/jobs/")
        );
    }
    (
        status,
        serde_json::from_slice(&to_bytes(response.into_body(), 4_000_000).await.unwrap()).unwrap(),
    )
}

#[test]
fn normalization_empty_and_invalid_are_explicit() {
    let input = WatchlistInput::parse(" msft , AMD,msft, brk.b ").unwrap();
    assert_eq!(
        input
            .symbols()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["AMD", "BRK.B", "MSFT"]
    );
    assert_eq!(input, WatchlistInput::parse("BRK.B,MSFT,AMD").unwrap());
    assert_eq!(
        WatchlistInput::parse("  ").unwrap().validate_submission(),
        Err(WatchlistError::Empty)
    );
    for value in ["MSFT,", "MSFT,,AMD", "MSFT260101C100", "$CASH", "BRK..B"] {
        assert_eq!(
            WatchlistInput::parse(value),
            Err(WatchlistError::InvalidConfiguration)
        );
    }
    assert!(WatchlistInput::parse(&vec!["AMD"; 33].join(",")).is_err());
}

#[tokio::test]
async fn briefing_without_portfolio_preserves_missing_and_exact_research_history() {
    let h = Harness::new("MSFT,AMD,ZZZZ").await;
    h.seed(false).await;
    let job = h.enqueue().await;
    h.run().await;
    let job = h.jobs.get(job.id).await.unwrap();
    assert_eq!(job.status, JobStatus::Succeeded);
    let Some(JobResult::WatchlistBriefing { id }) = job.result else {
        panic!("wrong workflow result")
    };
    let saved = h.saved(id).await;
    assert_eq!(saved.document.entries.len(), 3);
    assert_eq!(
        saved.document.entries[0].status,
        BriefingStatus::NeverRefreshed
    );
    assert_eq!(
        saved.document.entries[2].status,
        BriefingStatus::NeverRefreshed
    );
    let original = saved.document.entries[1].research.as_ref().unwrap();
    assert!(original.id > 0);
    assert!(!original.snapshot.sources.is_empty());
    h.seed(true).await;
    h.portfolio.reload().await;
    assert_eq!(h.saved(id).await, saved);
    let next = h.enqueue().await;
    h.run().await;
    let new = h
        .saved(h.jobs.get(next.id).await.unwrap().result.unwrap().id())
        .await;
    assert_ne!(
        new.document.entries[1].research.as_ref().unwrap().id,
        original.id
    );
    assert_eq!(
        h.jobs
            .watchlist()
            .repository()
            .unwrap()
            .history()
            .await
            .unwrap()[0]
            .id,
        new.id
    );
    let pool = h.review.execution_pool().unwrap();
    assert!(
        sqlx::query("UPDATE watchlist_briefings SET created_at='changed' WHERE id=?")
            .bind(id)
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM watchlist_briefings WHERE id=?")
            .bind(id)
            .execute(&pool)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn two_workflows_share_one_worker_api_and_scoped_idempotency() {
    let h = Harness::new("MSFT,AMD").await;
    h.portfolio.reload().await;
    let worker = h.start().await;
    let app = h.app();
    let (status, briefing) = call(&app, "POST", "/api/watchlist/briefings", Some("same-key")).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(
        call(&app, "POST", "/api/watchlist/briefings", Some("same-key"))
            .await
            .1["job_id"],
        briefing["job_id"]
    );
    let (_, review) = call(&app, "POST", "/api/reviews", Some("same-key")).await;
    assert_ne!(review["job_id"], briefing["job_id"]);
    let b = h.terminal(briefing["job_id"].as_i64().unwrap()).await;
    let r = h.terminal(review["job_id"].as_i64().unwrap()).await;
    assert!(matches!(
        b.result,
        Some(JobResult::WatchlistBriefing { .. })
    ));
    assert!(matches!(r.result, Some(JobResult::Review { .. })));
    assert_eq!(
        call(&app, "GET", "/api/jobs?kind=watchlist_briefing", None)
            .await
            .1
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let repo = h.jobs.repository().unwrap();
    let same = JobInput::WatchlistBriefing {
        symbols: WatchlistInput::parse("amd, MSFT,amd").unwrap(),
    };
    assert_eq!(
        repo.enqueue_input(&same, Some("same-key"))
            .await
            .unwrap()
            .id,
        b.id
    );
    let changed = JobInput::WatchlistBriefing {
        symbols: WatchlistInput::parse("GOOGL").unwrap(),
    };
    assert_ne!(
        repo.enqueue_input(&changed, Some("same-key"))
            .await
            .unwrap()
            .id,
        b.id
    );
    assert_eq!(h.review.history().await.unwrap().len(), 1);
    assert_eq!(
        call(&app, "GET", "/api/watchlist/briefings/99999", None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, "GET", "/api/watchlist/briefings/0", None)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    worker.shutdown().await;
}

#[tokio::test]
async fn restart_keeps_queued_input_and_recovers_committed_and_abandoned_jobs() {
    let mut h = Harness::new("MSFT,AMD").await;
    h.seed(false).await;
    let committed = h.enqueue().await;
    let repo = h.jobs.repository().unwrap();
    repo.claim().await.unwrap();
    let saved = h
        .jobs
        .watchlist()
        .create_for_job(
            &h.research,
            h.jobs.watchlist().submission_input().unwrap(),
            committed.id,
            |_| async { Ok(()) },
        )
        .await
        .unwrap();
    let abandoned = h.enqueue().await;
    repo.claim().await.unwrap();
    let queued = h.enqueue().await;
    // Restart composition with different configuration, but same durable queue.
    h.review = ReviewService::open(&h.dir.path().join("execution.db")).await;
    h.jobs = JobService::with_watchlist(&h.review, WatchlistInput::parse("GOOGL"));
    runner::reconcile(&h.jobs, &h.review).await.unwrap();
    assert_eq!(
        h.jobs.get(committed.id).await.unwrap().result,
        Some(JobResult::WatchlistBriefing { id: saved.id })
    );
    assert_eq!(
        h.jobs.get(abandoned.id).await.unwrap().status,
        JobStatus::Interrupted
    );
    h.run().await;
    let after = h
        .saved(h.jobs.get(queued.id).await.unwrap().result.unwrap().id())
        .await;
    assert_eq!(
        after
            .document
            .entries
            .iter()
            .map(|e| e.symbol.as_str())
            .collect::<Vec<_>>(),
        ["AMD", "MSFT"]
    );
    assert_eq!(h.saved(saved.id).await, saved);
    // Idempotent execution returns the original result even if passed new input.
    let again = h
        .jobs
        .watchlist()
        .create_for_job(
            &h.research,
            WatchlistInput::parse("GOOGL").unwrap(),
            committed.id,
            |_| async { panic!("must not execute") },
        )
        .await
        .unwrap();
    assert_eq!(again, saved);
    let worker = h.start().await;
    h.jobs.retry(abandoned.id).await.unwrap();
    let retried = h.terminal(abandoned.id).await;
    assert_eq!(retried.status, JobStatus::Succeeded);
    assert_eq!(retried.attempt_count, 2);
    assert_eq!(retried.previous_failures.len(), 1);
    worker.shutdown().await;
}

#[tokio::test]
async fn failures_remain_honest_retryable_and_cannot_mix_progress_or_results() {
    let h = Harness::new("MSFT").await;
    let pool = h.review.execution_pool().unwrap();
    sqlx::query("CREATE TRIGGER fail_briefing BEFORE INSERT ON watchlist_briefings BEGIN SELECT RAISE(ABORT,'synthetic write failure'); END").execute(&pool).await.unwrap();
    let job = h.enqueue().await;
    h.run().await;
    let failed = h.jobs.get(job.id).await.unwrap();
    assert_eq!(failed.status, JobStatus::Failed);
    assert!(!failed.error.unwrap().contains("synthetic"));
    assert!(
        h.jobs
            .watchlist()
            .repository()
            .unwrap()
            .history()
            .await
            .unwrap()
            .is_empty()
    );
    sqlx::query("DROP TRIGGER fail_briefing")
        .execute(&pool)
        .await
        .unwrap();
    let repo = h.jobs.repository().unwrap();
    repo.retry(job.id).await.unwrap();
    let claimed = repo.claim().await.unwrap().unwrap();
    assert_eq!(
        repo.progress(job.id, claimed.attempt_count, JobStep::CalculatingReview)
            .await,
        Err(JobError::InvalidTransition)
    );
    assert_eq!(
        repo.succeed(job.id, claimed.attempt_count, 123).await,
        Err(JobError::InvalidTransition)
    );
    repo.fail(job.id, claimed.attempt_count, ExecutionError::Interrupted)
        .await
        .unwrap();
    let worker = h.start().await;
    h.jobs.retry(job.id).await.unwrap();
    assert_eq!(h.terminal(job.id).await.status, JobStatus::Succeeded);
    worker.shutdown().await;
    assert_eq!(
        h.jobs
            .watchlist()
            .repository()
            .unwrap()
            .history()
            .await
            .unwrap()
            .len(),
        1
    );
    // An unreadable research database is a failed job, not missing data.
    let bad_research =
        ResearchService::with_source(h.dir.path(), Err(ResearchError::SourceUnavailable)).await;
    let job = h.enqueue().await;
    runner::run_one(
        h.jobs.clone(),
        h.portfolio.clone(),
        bad_research,
        h.review.clone(),
    )
    .await
    .unwrap();
    assert_eq!(h.jobs.get(job.id).await.unwrap().status, JobStatus::Failed);
    assert_eq!(
        h.jobs
            .watchlist()
            .repository()
            .unwrap()
            .history()
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn empty_invalid_and_portfolio_independent_submission_are_explicit() {
    for value in ["", "AMD,,MSFT"] {
        let h = Harness::new(value).await;
        let app = h.app();
        assert_eq!(
            call(&app, "POST", "/api/watchlist/briefings", None).await.0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert!(h.jobs.recent().await.unwrap().is_empty());
    }
    let h = Harness::new("AMD").await;
    let worker = h.start().await;
    let app = h.app();
    assert_eq!(
        call(&app, "POST", "/api/reviews", None).await.0,
        StatusCode::CONFLICT
    );
    let (status, body) = call(&app, "POST", "/api/watchlist/briefings", None).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(
        h.terminal(body["job_id"].as_i64().unwrap()).await.status,
        JobStatus::Succeeded
    );
    worker.shutdown().await;
}

#[tokio::test]
async fn migration_preserves_stage_five_jobs_failures_and_keys() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("old.db");
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await
        .unwrap();
    // Apply the actual first two migrations, retaining their SQLx checksums.
    let all = sqlx::migrate!("./src/review/migrations");
    let old = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            all.iter().filter(|m| m.version <= 2).cloned().collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    old.run(&pool).await.unwrap();
    sqlx::query("INSERT INTO jobs(id,kind,status,created_at,queued_at,started_at,completed_at,attempt_count,step,error,idempotency_key) VALUES (41,'portfolio_review','failed','2025-01-01','2025-01-01','2025-01-01','2025-01-01',1,'loading_research','preserved failure','legacy-key')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO job_failures VALUES(41,1,'failed','loading_research','2025-01-01','2025-01-01','preserved failure')").execute(&pool).await.unwrap();
    pool.close().await;
    let review = ReviewService::open(&path).await;
    assert!(review.initialization_error().is_none());
    let jobs = JobService::new(&review);
    let repo = jobs.repository().unwrap();
    let old = repo.enqueue(Some("legacy-key")).await.unwrap();
    assert_eq!(old.id, 41);
    assert_eq!(old.input, JobInput::PortfolioReview);
    assert_eq!(old.previous_failures[0].error, "preserved failure");
    assert!(repo.enqueue(None).await.unwrap().id > 41);
    repo.retry(41).await.unwrap();
    let claimed = repo.claim().await.unwrap().unwrap();
    repo.fail(
        claimed.id,
        claimed.attempt_count,
        ExecutionError::Interrupted,
    )
    .await
    .unwrap();
    let violations: Vec<(String, i64, String, i64)> = sqlx::query_as("PRAGMA foreign_key_check")
        .fetch_all(&review.execution_pool().unwrap())
        .await
        .unwrap();
    assert!(violations.is_empty());
}
