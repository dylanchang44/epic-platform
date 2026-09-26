#![cfg(feature = "ssr")]

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use epic_platform::{
    config::AppConfig,
    portfolio::LoadStatus,
    research::{
        domain::{Company, ResearchError, ResearchSnapshot},
        repository::ResearchRepository,
        service::ResearchService,
        source::{ResearchSource, normalize_response},
    },
    review::{calculations, domain::*, repository::ReviewRepository, service::ReviewService},
    state::{AppState, PortfolioState},
    symbol::StockSymbol,
};
use rust_decimal::Decimal;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tempfile::{TempDir, tempdir};
use tower::ServiceExt;

const CSV: &str = "Symbol,Description,Qty,Price,Mkt Val,Asset Type\nNVDA,Synthetic NVIDIA,1,100,100,Equity\nMSFT,Synthetic Microsoft,3,100,300,Equity\nSPY,Synthetic fund,6,100,600,ETF\n";
const CHANGED: &str = "Symbol,Description,Qty,Price,Mkt Val,Asset Type\nNVDA,Synthetic NVIDIA,2,100,200,Equity\nAMD,Synthetic AMD,4,100,400,Equity\n";

fn research_snapshot(symbol: &str) -> ResearchSnapshot {
    normalize_response(
        &Company::find(&StockSymbol::parse(symbol).unwrap()).unwrap(),
        include_bytes!("fixtures/research/forecast.json"),
        "2025-08-02T00:00:00Z",
    )
    .unwrap()
}

struct Harness {
    directory: TempDir,
    portfolio: Arc<PortfolioState>,
    research: Arc<ResearchService>,
    reviews: Arc<ReviewService>,
}
impl Harness {
    async fn new() -> Self {
        let directory = tempdir().unwrap();
        let holdings = directory.path().join("holdings");
        std::fs::create_dir(&holdings).unwrap();
        std::fs::write(holdings.join("Positions.csv"), CSV).unwrap();
        let portfolio = PortfolioState::new(AppConfig {
            schwab_data_dir: holdings,
            ..AppConfig::default()
        });
        assert_eq!(portfolio.reload().await.status, LoadStatus::Loaded);
        let research = ResearchService::with_source(
            &directory.path().join("research.db"),
            Err(ResearchError::SourceUnavailable),
        )
        .await;
        let reviews = ReviewService::open(&directory.path().join("reviews.db")).await;
        Self {
            directory,
            portfolio,
            research,
            reviews,
        }
    }
    fn research_path(&self) -> PathBuf {
        self.directory.path().join("research.db")
    }
    fn review_path(&self) -> PathBuf {
        self.directory.path().join("reviews.db")
    }
    async fn seed(&self, symbol: &str) {
        ResearchRepository::open(&self.research_path())
            .await
            .unwrap()
            .save(&research_snapshot(symbol))
            .await
            .unwrap();
    }
    async fn create(&self) -> SavedReview {
        self.reviews
            .create_portfolio_review(&self.portfolio, &self.research)
            .await
            .unwrap()
    }
    fn app(&self, jobs: Arc<epic_platform::jobs::service::JobService>) -> axum::Router {
        epic_platform::server::router(AppState {
            leptos_options: leptos::prelude::get_configuration(Some("Cargo.toml"))
                .unwrap()
                .leptos_options,
            portfolio: self.portfolio.clone(),
            research: self.research.clone(),
            review: self.reviews.clone(),
            jobs,
        })
    }
}

fn position(symbol: &str, value: i64, coverage: CoverageStatus) -> ReviewPosition {
    ReviewPosition {
        symbol: symbol.into(),
        stock_symbol: StockSymbol::parse(symbol).ok(),
        description: "Synthetic".into(),
        asset_type: "Equity".into(),
        quantity: Some(Decimal::ONE),
        market_value: Decimal::from(value),
        weight_percent: None,
        coverage,
        research: None,
    }
}

#[test]
fn deterministic_concentration_and_coverage() {
    let mut positions = vec![
        position("NVDA", 400, CoverageStatus::Available),
        position("MSFT", 300, CoverageStatus::Missing),
        position("SPY", 200, CoverageStatus::Unsupported),
        position("ZZZZ", 100, CoverageStatus::Unmatched),
    ];
    let result = calculations::summarize(&mut positions).unwrap();
    assert_eq!(result.total_market_value, Decimal::from(1000));
    assert_eq!(result.largest_position.symbol, "NVDA");
    assert_eq!(
        result.top_three_concentration_percent,
        Some(Decimal::from(90))
    );
    assert_eq!(positions[0].weight_percent, Some(Decimal::from(40)));
    assert_eq!(result.supported_position_count, 2);
    assert_eq!(result.covered_position_count, 1);
    assert_eq!(result.coverage_count_percent, Some(Decimal::from(50)));
    assert_eq!(
        result.coverage_value_percent,
        Some(Decimal::new(57142857, 6))
    );
    assert_eq!(result.unsupported_position_count, 1);
    assert_eq!(result.unmatched_position_count, 1);
    positions.reverse();
    assert_eq!(calculations::summarize(&mut positions).unwrap(), result);
}

#[test]
fn shorts_zero_totals_ties_and_numeric_limits_are_explicit() {
    let mut positions = vec![
        position("NVDA", 100, CoverageStatus::Available),
        position("AMD", -100, CoverageStatus::Missing),
    ];
    let result = calculations::summarize(&mut positions).unwrap();
    assert_eq!(result.total_market_value, Decimal::ZERO);
    assert_eq!(result.gross_market_value, Decimal::from(200));
    assert_eq!(result.largest_position.symbol, "AMD");
    assert_eq!(result.coverage_value_percent, Some(Decimal::from(50)));
    assert!(positions.iter().all(|p| p.weight_percent.is_none()));
    let mut zero = vec![position("SPY", 0, CoverageStatus::Unsupported)];
    let result = calculations::summarize(&mut zero).unwrap();
    assert!(result.coverage_count_percent.is_none());
    assert!(result.coverage_value_percent.is_none());
    assert!(result.top_three_concentration_percent.is_none());
    assert_eq!(
        calculations::summarize(&mut []),
        Err(ReviewError::InvalidPortfolio)
    );
    positions[0].market_value = Decimal::MAX;
    positions[1].market_value = Decimal::MAX;
    assert_eq!(
        calculations::summarize(&mut positions),
        Err(ReviewError::Calculation)
    );
}

#[test]
fn exact_age_has_no_freshness_threshold() {
    assert_eq!(
        calculations::age_seconds("2025-08-03T00:00:01Z", "2025-08-02T00:00:00Z"),
        Ok(86401)
    );
    assert_eq!(
        calculations::age_seconds("2025-08-02T00:00:00Z", "2025-08-03T00:00:00Z"),
        Ok(-86400)
    );
    assert_eq!(
        calculations::age_seconds("invalid", "2025-08-02T00:00:00Z"),
        Err(ReviewError::Calculation)
    );
}

#[tokio::test]
async fn full_partial_and_zero_coverage_are_saved() {
    let h = Harness::new().await;
    let zero = h.create().await;
    assert_eq!(
        zero.document.summary.coverage_count_percent,
        Some(Decimal::ZERO)
    );
    assert_eq!(zero.document.summary.missing_position_count, 2);
    h.seed("NVDA").await;
    let partial = h.create().await;
    assert_eq!(
        partial.document.summary.coverage_count_percent,
        Some(Decimal::from(50))
    );
    assert_eq!(
        partial.document.summary.coverage_value_percent,
        Some(Decimal::from(25))
    );
    assert_eq!(
        partial.document.positions[0]
            .research
            .as_ref()
            .unwrap()
            .snapshot,
        research_snapshot("NVDA")
    );
    assert!(
        partial.document.positions[0]
            .research
            .as_ref()
            .unwrap()
            .snapshot_id
            > 0
    );
    h.seed("MSFT").await;
    let full = h.create().await;
    assert_eq!(
        full.document.summary.coverage_count_percent,
        Some(Decimal::from(100))
    );
    assert_eq!(
        full.document.summary.coverage_value_percent,
        Some(Decimal::from(100))
    );
    assert_eq!(full.document.summary.unsupported_position_count, 1);
    assert_eq!(h.reviews.detail(zero.id).await.unwrap().review, zero);
}

#[tokio::test]
async fn captured_inputs_survive_portfolio_and_research_changes_and_restart() {
    let h = Harness::new().await;
    h.seed("NVDA").await;
    let first = h.create().await;
    let original_research = first.document.positions[0].research.clone();
    std::fs::write(h.directory.path().join("holdings/Positions.csv"), CHANGED).unwrap();
    h.portfolio.reload().await;
    let mut newer = research_snapshot("NVDA");
    newer.period.label = "FY2025 Q3".into();
    newer.period.ended_on = "2025-09-30".into();
    newer.retrieved_at = "2025-10-30T00:00:00Z".into();
    newer.revenue = Decimal::from(999);
    ResearchRepository::open(&h.research_path())
        .await
        .unwrap()
        .save(&newer)
        .await
        .unwrap();
    h.seed("AMD").await;
    let second = h.create().await;
    assert_eq!(h.reviews.detail(first.id).await.unwrap().review, first);
    assert_eq!(first.document.positions[0].research, original_research);
    assert_eq!(
        second.document.positions[0]
            .research
            .as_ref()
            .unwrap()
            .snapshot,
        newer
    );
    assert_ne!(
        second.document.positions[0]
            .research
            .as_ref()
            .unwrap()
            .snapshot_id,
        original_research.unwrap().snapshot_id
    );
    let comparison = h
        .reviews
        .detail(second.id)
        .await
        .unwrap()
        .comparison
        .unwrap();
    assert_eq!(comparison.previous_id, first.id);
    assert_eq!(comparison.total_value_change, Decimal::from(-400));
    assert_eq!(comparison.position_count_change, -1);
    assert_eq!(comparison.previous_largest_symbol, "SPY");
    assert_eq!(comparison.current_largest_symbol, "AMD");
    assert_eq!(comparison.added_symbols, vec!["AMD"]);
    assert_eq!(comparison.removed_symbols, vec!["MSFT", "SPY"]);
    assert_eq!(
        comparison.coverage_count_change_points,
        Some(Decimal::from(50))
    );
    let reopened = ReviewService::open(&h.review_path()).await;
    assert_eq!(reopened.detail(first.id).await.unwrap().review, first);
    assert_eq!(reopened.detail(second.id).await.unwrap().review, second);
    assert_eq!(
        reopened
            .history()
            .await
            .unwrap()
            .iter()
            .map(|r| r.id)
            .collect::<Vec<_>>(),
        vec![second.id, first.id]
    );
    assert!(
        reopened
            .detail(first.id)
            .await
            .unwrap()
            .comparison
            .is_none()
    );
}

#[tokio::test]
async fn history_uses_save_order_even_when_timestamps_match() {
    let h = Harness::new().await;
    let first = h.create().await;
    let repository = ReviewRepository::open(&h.review_path()).await.unwrap();
    let second = repository.save(first.document.clone()).await.unwrap();
    let history = repository.history().await.unwrap();
    assert_eq!(history[0].id, second.id);
    assert_eq!(history[1].id, first.id);
    assert_eq!(repository.previous(second.id).await.unwrap(), Some(first));
}

#[tokio::test]
async fn rollback_and_database_immutability_constraints() {
    let h = Harness::new().await;
    let first = h.create().await;
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new().filename(h.review_path()),
    )
    .await
    .unwrap();
    // AFTER INSERT fails after the row was written within the transaction.
    sqlx::query("CREATE TRIGGER fail_review AFTER INSERT ON reviews BEGIN SELECT RAISE(FAIL, 'synthetic persistence failure'); END;")
        .execute(&pool).await.unwrap();
    assert_eq!(
        h.reviews
            .create_portfolio_review(&h.portfolio, &h.research)
            .await,
        Err(ReviewError::Repository)
    );
    assert_eq!(h.reviews.history().await.unwrap().len(), 1);
    assert_eq!(h.reviews.detail(first.id).await.unwrap().review, first);
    assert!(
        sqlx::query("UPDATE reviews SET created_at = 'changed'")
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM reviews")
            .execute(&pool)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn research_failure_is_recorded_and_saved_reviews_need_no_current_inputs() {
    let h = Harness::new().await;
    let unavailable = ResearchService::open(h.directory.path()).await;
    let review = h
        .reviews
        .create_portfolio_review(&h.portfolio, &unavailable)
        .await
        .unwrap();
    assert_eq!(
        review.document.research_error,
        Some(ResearchError::Initialization)
    );
    assert_eq!(
        review.document.positions[0].coverage,
        CoverageStatus::RepositoryFailure
    );
    assert_eq!(review.document.summary.covered_position_count, 0);
    let no_portfolio = PortfolioState::new(AppConfig {
        schwab_data_dir: h.directory.path().join("missing"),
        ..AppConfig::default()
    });
    assert_eq!(
        h.reviews
            .create_portfolio_review(&no_portfolio, &unavailable)
            .await,
        Err(ReviewError::NoPortfolio)
    );
    assert_eq!(h.reviews.detail(review.id).await.unwrap().review, review);
    // Failed reload still leaves a usable prior snapshot, explicitly labeled.
    std::fs::write(h.directory.path().join("holdings/Positions.csv"), "broken").unwrap();
    h.portfolio.reload().await;
    assert_eq!(
        h.create().await.document.portfolio_load_status,
        LoadStatus::Failed
    );
}

#[tokio::test]
async fn creation_never_calls_source_or_reloads_files() {
    let h = Harness::new().await;
    let count = Arc::new(AtomicUsize::new(0));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let app = axum::Router::new().fallback({
        let count = count.clone();
        move || {
            let count = count.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                "unexpected request"
            }
        }
    });
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let research = ResearchService::with_source(
        &h.research_path(),
        ResearchSource::with_base_url(&url, Duration::from_secs(1)),
    )
    .await;
    std::fs::write(
        h.directory.path().join("holdings/Positions.csv"),
        "broken on disk",
    )
    .unwrap();
    let before = h.portfolio.snapshot().await;
    let created = h
        .reviews
        .create_portfolio_review(&h.portfolio, &research)
        .await
        .unwrap();
    assert_eq!(created.document.summary.position_count, 3);
    assert_eq!(h.portfolio.snapshot().await, before);
    assert_eq!(count.load(Ordering::SeqCst), 0);
    server.abort();
}

#[tokio::test]
async fn owned_portfolio_snapshot_does_not_block_reload_during_database_wait() {
    let h = Harness::new().await;
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new().filename(h.review_path()),
    )
    .await
    .unwrap();
    let mut connection = pool.acquire().await.unwrap();
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *connection)
        .await
        .unwrap();
    let creating = tokio::spawn({
        let reviews = h.reviews.clone();
        let portfolio = h.portfolio.clone();
        let research = h.research.clone();
        async move { reviews.create_portfolio_review(&portfolio, &research).await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!creating.is_finished());
    let reloaded = tokio::time::timeout(Duration::from_secs(1), h.portfolio.reload())
        .await
        .unwrap();
    assert_eq!(reloaded.status, LoadStatus::Loaded);
    sqlx::query("ROLLBACK")
        .execute(&mut *connection)
        .await
        .unwrap();
    assert!(creating.await.unwrap().is_ok());
}

async fn call(app: &axum::Router, method: &str, path: &str) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    assert_eq!(response.headers()["cache-control"], "no-store");
    if status == StatusCode::ACCEPTED {
        assert!(
            response.headers()["location"]
                .to_str()
                .unwrap()
                .starts_with("/api/jobs/")
        );
    }
    let bytes = to_bytes(response.into_body(), 2_000_000).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn review_api_create_history_detail_and_structured_errors() {
    let h = Harness::new().await;
    let jobs = epic_platform::jobs::service::JobService::new(&h.reviews);
    let worker = epic_platform::jobs::runner::start(
        jobs.clone(),
        h.portfolio.clone(),
        h.research.clone(),
        h.reviews.clone(),
    )
    .await
    .unwrap();
    let app = h.app(jobs.clone());
    assert_eq!(
        call(&app, "GET", "/api/reviews").await.1,
        serde_json::json!([])
    );
    let (status, submitted) = call(&app, "POST", "/api/reviews").await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let job_id = submitted["job_id"].as_i64().unwrap();
    let id = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(epic_platform::jobs::domain::JobResult::Review { id }) =
                jobs.get(job_id).await.unwrap().result
            {
                break id;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let created = serde_json::to_value(h.reviews.detail(id).await.unwrap().review).unwrap();
    assert_eq!(
        call(&app, "GET", &format!("/api/reviews/{id}")).await.1["review"],
        created
    );
    let list = call(&app, "GET", "/api/reviews").await.1;
    assert_eq!(list[0]["id"], id);
    assert!(list[0].get("positions").is_none());
    assert_eq!(
        call(&app, "GET", "/api/reviews/9999").await.0,
        StatusCode::NOT_FOUND
    );
    for id in ["0", "-1", "abc"] {
        assert_eq!(
            call(&app, "GET", &format!("/api/reviews/{id}")).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    let unloaded = PortfolioState::new(AppConfig {
        schwab_data_dir: h.directory.path().join("absent"),
        ..AppConfig::default()
    });
    let app = epic_platform::server::router(AppState {
        leptos_options: leptos::prelude::get_configuration(Some("Cargo.toml"))
            .unwrap()
            .leptos_options,
        portfolio: unloaded,
        research: h.research.clone(),
        review: h.reviews.clone(),
        jobs,
    });
    let error = call(&app, "POST", "/api/reviews").await;
    assert_eq!(error.0, StatusCode::CONFLICT);
    assert_eq!(error.1["error"]["kind"], "no_portfolio");
    assert_eq!(
        call(&app, "GET", &format!("/api/reviews/{id}")).await.0,
        StatusCode::OK
    );
    worker.shutdown().await;
}

#[tokio::test]
async fn migration_and_initialization_failure_are_isolated() {
    let directory = tempdir().unwrap();
    let bad = ReviewService::open(directory.path()).await;
    assert_eq!(
        bad.initialization_error(),
        Some(ReviewError::Initialization)
    );
    let path = directory.path().join("conflict.db");
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true),
    )
    .await
    .unwrap();
    sqlx::query("CREATE TABLE reviews(wrong TEXT)")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        ReviewService::open(&path).await.initialization_error(),
        Some(ReviewError::Migration)
    );
}
