#![cfg(feature = "ssr")]

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use epic_platform::{
    config::AppConfig,
    jobs::{domain::*, runner, service::JobService},
    research::{domain::ResearchError, service::ResearchService},
    review::{
        domain::{ReviewError, ReviewStep},
        repository::ReviewRepository,
        service::ReviewService,
    },
    state::{AppState, PortfolioState},
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
    async fn new() -> Self {
        let dir = tempdir().unwrap();
        let portfolio = PortfolioState::new(AppConfig {
            schwab_data_dir: "tests/fixtures/research".into(),
            ..AppConfig::default()
        });
        portfolio.reload().await;
        let research = ResearchService::with_source(
            &dir.path().join("research.db"),
            Err(ResearchError::SourceUnavailable),
        )
        .await;
        let review = ReviewService::open(&dir.path().join("reviews.db")).await;
        let jobs = JobService::new(&review);
        Self {
            dir,
            portfolio,
            research,
            review,
            jobs,
        }
    }
    async fn run(&self) -> bool {
        runner::run_one(
            self.jobs.clone(),
            self.portfolio.clone(),
            self.research.clone(),
            self.review.clone(),
        )
        .await
        .unwrap()
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
        .expect("worker should finish local review")
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
    let bytes = to_bytes(response.into_body(), 4_000_000).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[test]
fn lifecycle_is_closed_and_explicit() {
    use JobStatus::*;
    for from in [Queued, Running, Succeeded, Failed, Interrupted] {
        for to in [Queued, Running, Succeeded, Failed, Interrupted] {
            let expected = matches!(
                (from, to),
                (Queued, Running)
                    | (Running, Succeeded | Failed | Interrupted)
                    | (Failed | Interrupted, Queued)
            );
            assert_eq!(from.allows(to), expected);
        }
    }
}

#[tokio::test]
async fn atomic_claiming_fifo_and_stale_attempt_guards() {
    let h = Harness::new().await;
    let repo = h.jobs.repository().unwrap();
    let first = repo.enqueue(None).await.unwrap();
    let second = repo.enqueue(None).await.unwrap();
    assert_eq!(repo.claim().await.unwrap().unwrap().id, first.id);
    let (a, b) = tokio::join!(repo.claim(), repo.claim());
    let claimed: Vec<_> = [a.unwrap(), b.unwrap()].into_iter().flatten().collect();
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].id, second.id);
    assert_eq!(claimed[0].attempt_count, 1);
    assert_eq!(
        repo.retry(first.id).await.unwrap_err(),
        JobError::InvalidTransition
    );
    repo.progress(first.id, 1, JobStep::LoadingResearch)
        .await
        .unwrap();
    assert_eq!(
        h.jobs.get(first.id).await.unwrap().step,
        JobStep::LoadingResearch
    );
    assert_eq!(
        repo.progress(first.id, 1, JobStep::SnapshottingPortfolio)
            .await
            .unwrap_err(),
        JobError::InvalidTransition
    );
    repo.fail(
        first.id,
        1,
        ExecutionError::Review(ReviewError::Calculation),
    )
    .await
    .unwrap();
    assert_eq!(
        repo.succeed(first.id, 1, 1).await.unwrap_err(),
        JobError::InvalidTransition
    );
    let retried = repo.retry(first.id).await.unwrap();
    assert_eq!(retried.attempt_count, 1);
    assert!(
        retried.started_at.is_none() && retried.completed_at.is_none() && retried.error.is_none()
    );
    assert_eq!(retried.previous_failures.len(), 1);
    assert_eq!(retried.previous_failures[0].step, JobStep::LoadingResearch);
    let again = repo.claim().await.unwrap().unwrap();
    assert_eq!(again.id, first.id);
    assert_eq!(again.attempt_count, 2);
    assert_eq!(
        repo.fail(first.id, 1, ExecutionError::Interrupted)
            .await
            .unwrap_err(),
        JobError::InvalidTransition
    );
}

#[tokio::test]
async fn enqueue_uniqueness_survives_concurrent_requests_and_restart() {
    let h = Harness::new().await;
    let repo = h.jobs.repository().unwrap();
    let (a, b) = tokio::join!(
        repo.enqueue(Some("same-key")),
        repo.enqueue(Some("same-key"))
    );
    let id = a.unwrap().id;
    assert_eq!(id, b.unwrap().id);
    let reopened = ReviewService::open(&h.dir.path().join("reviews.db")).await;
    let jobs = JobService::new(&reopened);
    assert_eq!(
        jobs.repository()
            .unwrap()
            .enqueue(Some("same-key"))
            .await
            .unwrap()
            .id,
        id
    );
    assert_eq!(jobs.get(id).await.unwrap().status, JobStatus::Queued);
    assert_eq!(jobs.recent().await.unwrap().len(), 1);
    assert!(h.run().await);
    assert_eq!(h.jobs.get(id).await.unwrap().status, JobStatus::Succeeded);
    assert_eq!(h.review.history().await.unwrap().len(), 1);
    assert!(!h.run().await);
}

#[tokio::test]
async fn successful_execution_uses_public_review_and_failure_does_not_block_next_job() {
    let h = Harness::new().await;
    let first = h.jobs.repository().unwrap().enqueue(None).await.unwrap();
    let pool = h.review.execution_pool().unwrap();
    sqlx::query("CREATE TRIGGER reject_review BEFORE INSERT ON reviews BEGIN SELECT RAISE(ABORT,'synthetic private failure'); END").execute(&pool).await.unwrap();
    assert!(h.run().await);
    let failed = h.jobs.get(first.id).await.unwrap();
    assert_eq!(failed.status, JobStatus::Failed);
    assert_eq!(failed.step, JobStep::PersistingReview);
    assert!(!failed.error.unwrap().contains("synthetic private"));
    assert!(h.review.history().await.unwrap().is_empty());
    sqlx::query("DROP TRIGGER reject_review")
        .execute(&pool)
        .await
        .unwrap();
    let second = h.jobs.repository().unwrap().enqueue(None).await.unwrap();
    assert!(h.run().await);
    assert_eq!(
        h.jobs.get(second.id).await.unwrap().status,
        JobStatus::Succeeded
    );
    h.jobs.repository().unwrap().retry(first.id).await.unwrap();
    assert!(h.run().await);
    let retried = h.jobs.get(first.id).await.unwrap();
    assert_eq!(retried.status, JobStatus::Succeeded);
    assert_eq!(retried.attempt_count, 2);
    assert_eq!(retried.previous_failures.len(), 1);
    assert_eq!(h.review.history().await.unwrap().len(), 2);
}

#[tokio::test]
async fn queued_jobs_execute_after_restart_but_abandoned_running_jobs_require_retry() {
    let h = Harness::new().await;
    let abandoned = h.jobs.repository().unwrap().enqueue(None).await.unwrap();
    h.jobs.repository().unwrap().claim().await.unwrap();
    let queued = h.jobs.repository().unwrap().enqueue(None).await.unwrap();
    let review = ReviewService::open(&h.dir.path().join("reviews.db")).await;
    let jobs = JobService::new(&review);
    let worker = runner::start(
        jobs.clone(),
        h.portfolio.clone(),
        h.research.clone(),
        review,
    )
    .await
    .unwrap();
    assert_eq!(
        jobs.get(abandoned.id).await.unwrap().status,
        JobStatus::Interrupted
    );
    assert_eq!(h.terminal(queued.id).await.status, JobStatus::Succeeded);
    let retried = jobs.retry(abandoned.id).await.unwrap();
    assert_eq!(retried.job_id, abandoned.id);
    assert_eq!(h.terminal(abandoned.id).await.attempt_count, 2);
    worker.shutdown().await;
}

#[tokio::test]
async fn crash_after_review_commit_recovers_success_without_new_review_or_new_inputs() {
    let h = Harness::new().await;
    let job = h.jobs.repository().unwrap().enqueue(None).await.unwrap();
    h.jobs.repository().unwrap().claim().await.unwrap();
    let saved = h
        .review
        .create_for_job(&h.portfolio, &h.research, Some(job.id), |_| async {
            Ok(())
        })
        .await
        .unwrap();
    // Simulate process death before jobs.succeed. Reopen without a loaded portfolio.
    let reopened = ReviewService::open(&h.dir.path().join("reviews.db")).await;
    let jobs = JobService::new(&reopened);
    runner::reconcile(&jobs, &reopened).await.unwrap();
    let recovered = jobs.get(job.id).await.unwrap();
    assert_eq!(recovered.status, JobStatus::Succeeded);
    assert_eq!(recovered.result, Some(JobResult::Review { id: saved.id }));
    let unloaded = PortfolioState::new(AppConfig {
        schwab_data_dir: h.dir.path().join("missing"),
        ..AppConfig::default()
    });
    let same = reopened
        .create_for_job(&unloaded, &h.research, Some(job.id), |_| async {
            panic!("must return existing result")
        })
        .await
        .unwrap();
    assert_eq!(same, saved);
    assert_eq!(reopened.history().await.unwrap().len(), 1);
    assert_eq!(
        jobs.repository().unwrap().retry(job.id).await.unwrap_err(),
        JobError::InvalidTransition
    );
}

#[tokio::test]
async fn origin_uniqueness_is_enforced_inside_review_transaction() {
    let h = Harness::new().await;
    let original = h
        .review
        .create_portfolio_review(&h.portfolio, &h.research)
        .await
        .unwrap();
    let repository = ReviewRepository::open(&h.dir.path().join("reviews.db"))
        .await
        .unwrap();
    let (a, b) = tokio::join!(
        repository.save_for_job(original.document.clone(), Some(123)),
        repository.save_for_job(original.document.clone(), Some(123))
    );
    assert_eq!(a.unwrap(), b.unwrap());
    assert_eq!(repository.history().await.unwrap().len(), 2);
}

#[tokio::test]
async fn progress_boundaries_are_real_and_release_portfolio_lock() {
    let h = Harness::new().await;
    let (send, mut receive) = tokio::sync::mpsc::unbounded_channel();
    h.review
        .create_for_job(&h.portfolio, &h.research, None, |step| {
            let send = send.clone();
            let portfolio = h.portfolio.clone();
            async move {
                send.send(step).unwrap();
                // Every progress callback can independently read/reload Portfolio.
                tokio::time::timeout(Duration::from_secs(2), portfolio.snapshot())
                    .await
                    .unwrap();
                Ok(())
            }
        })
        .await
        .unwrap();
    assert!(matches!(
        receive.recv().await.unwrap(),
        ReviewStep::SnapshottingPortfolio
    ));
    assert!(matches!(
        receive.recv().await.unwrap(),
        ReviewStep::LoadingResearch
    ));
    assert!(matches!(
        receive.recv().await.unwrap(),
        ReviewStep::CalculatingReview
    ));
    assert!(matches!(
        receive.recv().await.unwrap(),
        ReviewStep::PersistingReview
    ));
}

#[tokio::test]
async fn recent_history_is_bounded_and_newest_first() {
    let h = Harness::new().await;
    for _ in 0..35 {
        h.jobs.repository().unwrap().enqueue(None).await.unwrap();
    }
    let rows = h.jobs.recent().await.unwrap();
    assert_eq!(rows.len(), 30);
    assert_eq!(rows.first().unwrap().id, 35);
    assert_eq!(rows.last().unwrap().id, 6);
}

#[tokio::test]
async fn browser_duplicate_api_readiness_errors_and_shutdown() {
    let h = Harness::new().await;
    let app = h.app();
    assert_eq!(
        call(&app, "GET", "/ready", None).await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    let worker = h.start().await;
    assert!(
        runner::start(
            h.jobs.clone(),
            h.portfolio.clone(),
            h.research.clone(),
            h.review.clone()
        )
        .await
        .is_err()
    );
    assert_eq!(call(&app, "GET", "/ready", None).await.0, StatusCode::OK);
    let (a, b) = tokio::join!(
        call(&app, "POST", "/api/reviews", Some("browser-click-1")),
        call(&app, "POST", "/api/reviews", Some("browser-click-1"))
    );
    assert_eq!(a.0, StatusCode::ACCEPTED);
    assert_eq!(b.0, StatusCode::ACCEPTED);
    assert_eq!(a.1["job_id"], b.1["job_id"]);
    let id = a.1["job_id"].as_i64().unwrap();
    assert_eq!(h.terminal(id).await.status, JobStatus::Succeeded);
    assert_eq!(
        call(&app, "GET", "/api/jobs", None)
            .await
            .1
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(h.review.history().await.unwrap().len(), 1);
    assert_eq!(
        call(&app, "POST", &format!("/api/jobs/{id}/retry"), None)
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(&app, "GET", "/api/jobs/999", None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, "GET", "/api/jobs/zero", None).await.0,
        StatusCode::BAD_REQUEST
    );
    for key in ["", "bad key", "contains/separator"] {
        assert_eq!(
            call(&app, "POST", "/api/reviews", Some(key)).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    worker.shutdown().await;
    assert_eq!(
        call(&app, "GET", "/ready", None).await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        call(&app, "POST", "/api/reviews", Some("new-click"))
            .await
            .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        call(&app, "POST", "/api/reviews", Some("browser-click-1"))
            .await
            .1["job_id"],
        id
    );
}

#[tokio::test]
async fn stopping_before_first_claim_leaves_queued_work_durable() {
    let h = Harness::new().await;
    let queued = h.jobs.repository().unwrap().enqueue(None).await.unwrap();
    let worker = h.start().await;
    worker.request_stop(); // No yield between starting and stopping the task.
    worker.shutdown().await;
    assert_eq!(
        h.jobs.get(queued.id).await.unwrap().status,
        JobStatus::Queued
    );
    let worker = h.start().await;
    assert_eq!(h.terminal(queued.id).await.status, JobStatus::Succeeded);
    worker.shutdown().await;
}

#[tokio::test]
async fn failed_execution_without_portfolio_and_recovery_read_failure_are_honest() {
    let h = Harness::new().await;
    let job = h.jobs.repository().unwrap().enqueue(None).await.unwrap();
    let absent = PortfolioState::new(AppConfig {
        schwab_data_dir: h.dir.path().join("absent"),
        ..AppConfig::default()
    });
    runner::run_one(h.jobs.clone(), absent, h.research.clone(), h.review.clone())
        .await
        .unwrap();
    assert_eq!(h.jobs.get(job.id).await.unwrap().status, JobStatus::Failed);
    assert_eq!(
        h.jobs.get(job.id).await.unwrap().error,
        Some(ReviewError::NoPortfolio.to_string())
    );
    h.jobs.repository().unwrap().retry(job.id).await.unwrap();
    h.jobs.repository().unwrap().claim().await.unwrap();
    let broken = ReviewService::open(h.dir.path()).await;
    assert_eq!(
        runner::reconcile(&h.jobs, &broken).await.unwrap_err(),
        JobError::Repository
    );
    assert_eq!(h.jobs.get(job.id).await.unwrap().status, JobStatus::Running);
}

#[tokio::test]
async fn failed_job_api_retry_preserves_identity_and_counts_execution() {
    let h = Harness::new().await;
    let pool = h.review.execution_pool().unwrap();
    sqlx::query("CREATE TRIGGER reject_review BEFORE INSERT ON reviews BEGIN SELECT RAISE(ABORT,'synthetic failure'); END").execute(&pool).await.unwrap();
    let worker = h.start().await;
    let app = h.app();
    let accepted = call(&app, "POST", "/api/reviews", Some("retry-me")).await;
    let id = accepted.1["job_id"].as_i64().unwrap();
    assert_eq!(h.terminal(id).await.status, JobStatus::Failed);
    sqlx::query("DROP TRIGGER reject_review")
        .execute(&pool)
        .await
        .unwrap();
    let retry = call(&app, "POST", &format!("/api/jobs/{id}/retry"), None).await;
    assert_eq!(retry.0, StatusCode::ACCEPTED);
    assert_eq!(retry.1["job_id"], id);
    let complete = h.terminal(id).await;
    assert_eq!(complete.status, JobStatus::Succeeded);
    assert_eq!(complete.attempt_count, 2);
    assert_eq!(complete.previous_failures.len(), 1);
    assert_eq!(h.review.history().await.unwrap().len(), 1);
    worker.shutdown().await;
}

#[tokio::test]
async fn migration_upgrades_stage_four_database_and_keeps_existing_review() {
    let h = Harness::new().await;
    let original = h
        .review
        .create_portfolio_review(&h.portfolio, &h.research)
        .await
        .unwrap();
    let migrations = h.dir.path().join("old-migrations");
    std::fs::create_dir(&migrations).unwrap();
    std::fs::write(
        migrations.join("0001_reviews.sql"),
        include_str!("../src/review/migrations/0001_reviews.sql"),
    )
    .unwrap();
    let path = h.dir.path().join("stage-four.db");
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true),
    )
    .await
    .unwrap();
    sqlx::migrate::Migrator::new(migrations.as_path())
        .await
        .unwrap()
        .run(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO reviews(created_at,summary_json,document_json) VALUES (?,?,?)")
        .bind(&original.document.created_at)
        .bind(serde_json::to_string(&original.document.summary).unwrap())
        .bind(serde_json::to_string(&original.document).unwrap())
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let upgraded = ReviewService::open(&path).await;
    assert_eq!(
        upgraded.detail(1).await.unwrap().review.document,
        original.document
    );
    let jobs = JobService::new(&upgraded);
    assert!(jobs.usable().await);
    assert_eq!(
        jobs.repository().unwrap().enqueue(None).await.unwrap().id,
        1
    );
}

#[tokio::test]
async fn readiness_reports_storage_failure_without_breaking_liveness_or_portfolio() {
    let h = Harness::new().await;
    let broken = ReviewService::open(h.dir.path()).await;
    let jobs = JobService::new(&broken);
    let app = epic_platform::server::router(AppState {
        leptos_options: leptos::prelude::get_configuration(Some("Cargo.toml"))
            .unwrap()
            .leptos_options,
        portfolio: h.portfolio.clone(),
        research: h.research.clone(),
        review: broken,
        jobs,
    });
    let ready = call(&app, "GET", "/ready", None).await;
    assert_eq!(ready.0, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(ready.1["migrations_completed"], false);
    assert_eq!(ready.1["job_repository_usable"], false);
    assert_eq!(ready.1["worker_started"], false);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        call(&app, "GET", "/api/portfolio", None).await.0,
        StatusCode::OK
    );
}
