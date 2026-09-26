#![cfg(feature = "ssr")]

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    routing::get,
};
use epic_platform::{
    config::AppConfig,
    portfolio::Position,
    research::{
        domain::*,
        repository::ResearchRepository,
        service::ResearchService,
        source::{ResearchSource, normalize_response},
    },
    state::{AppState, PortfolioState},
    symbol::StockSymbol,
};
use rust_decimal::Decimal;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tempfile::tempdir;
use tokio::sync::Notify;
use tower::ServiceExt;

const FORECAST: &str = include_str!("fixtures/research/forecast.json");
const RETRIEVED: &str = "2025-08-02T00:00:00Z";

fn symbol(value: &str) -> StockSymbol {
    StockSymbol::parse(value).unwrap()
}
fn snapshot() -> ResearchSnapshot {
    normalize_response(
        &Company::find(&symbol("NVDA")).unwrap(),
        FORECAST.as_bytes(),
        RETRIEVED,
    )
    .unwrap()
}
fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/research")
}

struct Mock {
    url: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Mock {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn mock(router: Router) -> Mock {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    Mock { url, task }
}
async fn response_mock(status: StatusCode, body: &'static str) -> Mock {
    mock(Router::new().route(
        "/stocks/nvda/forecast/__data.json",
        get(move || async move { (status, body) }),
    ))
    .await
}
async fn service(path: &Path, upstream: &Mock) -> Arc<ResearchService> {
    ResearchService::with_source(
        path,
        ResearchSource::with_base_url(&upstream.url, Duration::from_secs(2)),
    )
    .await
}
async fn portfolio(directory: PathBuf) -> Arc<PortfolioState> {
    let state = PortfolioState::new(AppConfig {
        schwab_data_dir: directory,
        ..AppConfig::default()
    });
    state.reload().await;
    state
}
async fn app(portfolio: Arc<PortfolioState>, research: Arc<ResearchService>) -> Router {
    let directory = tempdir().unwrap();
    let review =
        epic_platform::review::service::ReviewService::open(&directory.path().join("reviews.db"))
            .await;
    let jobs = epic_platform::jobs::service::JobService::new(&review);
    epic_platform::server::router(AppState {
        leptos_options: leptos::prelude::get_configuration(Some("Cargo.toml"))
            .unwrap()
            .leptos_options,
        portfolio,
        research,
        review,
        jobs,
    })
}
async fn call(app: &Router, method: &str, path: &str) -> (StatusCode, serde_json::Value) {
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
    assert_eq!(response.headers()["cache-control"], "no-store");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 2_000_000).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[test]
fn normalizes_symbols_without_inference_or_aliases() {
    assert_eq!(symbol(" nvda "), symbol("NVDA"));
    assert_ne!(symbol("GOOG"), symbol("GOOGL"));
    assert_ne!(symbol("BRK.B"), symbol("BRK-B"));
    for value in [
        "",
        "  ",
        "NVDA 12/18/2026 100 C",
        "NVDA261218C00100000",
        "../../db",
        "BRK..B",
        "股票",
        "BRK-",
    ] {
        assert!(StockSymbol::parse(value).is_err(), "{value}");
    }
    assert!(serde_json::from_str::<StockSymbol>("\"../db\"").is_err());
    assert!(Company::find(&symbol("ZZZZ")).is_none());
    let mut position = Position {
        symbol: " nvda ".into(),
        description: "Synthetic".into(),
        quantity: Some(Decimal::ONE),
        price: Some(Decimal::ONE),
        market_value: Decimal::ONE,
        cost_basis: None,
        asset_type: "Equity".into(),
    };
    assert_eq!(position.stock_symbol(), Some(symbol("NVDA")));
    for asset in ["ETF", "Option", "Cash", "Mutual Fund", "Bond", "Unknown"] {
        position.asset_type = asset.into();
        assert_eq!(position.stock_symbol(), None);
    }
    position.asset_type.clear();
    position.symbol = "Cash".into();
    assert_eq!(position.stock_symbol(), None);
}

#[test]
fn decodes_actual_source_contract_and_rejects_incomplete_data() {
    let parsed = snapshot();
    assert_eq!(parsed.period.label, "FY2025 Q2");
    assert_eq!(parsed.period.ended_on, "2025-06-30");
    assert_eq!(parsed.revenue, Decimal::new(1_250_000_000, 0));
    assert_eq!(parsed.diluted_eps, Decimal::new(25, 1));
    assert_eq!(parsed.retrieved_at, RETRIEVED);
    assert_eq!(
        parsed.sources[0].url,
        "https://stockanalysis.com/stocks/nvda/forecast/"
    );
    for invalid in [
        "not JSON".into(),
        "{}".into(),
        r#"{"nodes":[{"data":[{"cycle":0}]}]}"#.into(),
        r#"{"nodes":[{"data":[{"bad":999}]}]}"#.into(),
        FORECAST.replace("1250000000", "null"),
        FORECAST.replace("1250000000", "\"[PRO]\""),
        FORECAST.replace("2025-06-30", "2999-06-30"),
        FORECAST.replace("\"USD\"", "\"EUR\""),
        FORECAST.replace("1754006400000", "null"),
        FORECAST.replace("\"Q2\"", "\"Q9\""),
    ] {
        assert_eq!(
            normalize_response(&parsed.company, invalid.as_bytes(), RETRIEVED),
            Err(ResearchError::MalformedSource)
        );
    }
}

#[tokio::test]
async fn migrations_history_immutability_and_restart() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("nested/research.db");
    let repository = ResearchRepository::open(&path).await.unwrap();
    assert!(path.exists());
    assert!(repository.latest(&symbol("NVDA")).await.unwrap().is_none());
    let original = snapshot();
    assert!(repository.save(&original).await.unwrap());
    let mut revision = original.clone();
    revision.revenue = Decimal::ONE;
    revision.retrieved_at = "2025-08-03T00:00:00Z".into();
    assert!(!repository.save(&revision).await.unwrap());
    assert_eq!(
        repository.latest(&symbol("NVDA")).await.unwrap(),
        Some(original.clone())
    );
    let mut newer = original.clone();
    newer.period = EarningsPeriod {
        label: "FY2025 Q3".into(),
        ended_on: "2025-09-30".into(),
    };
    assert!(repository.save(&newer).await.unwrap());
    let mut older = original.clone();
    older.period = EarningsPeriod {
        label: "FY2025 Q1".into(),
        ended_on: "2025-03-31".into(),
    };
    assert!(repository.save(&older).await.unwrap());
    assert_eq!(
        repository.latest(&symbol("NVDA")).await.unwrap(),
        Some(newer.clone())
    );
    assert_eq!(
        repository.history(&symbol("NVDA")).await.unwrap(),
        vec![newer.clone(), original, older]
    );
    drop(repository);
    let reopened = ResearchRepository::open(&path).await.unwrap();
    assert_eq!(reopened.latest(&symbol("NVDA")).await.unwrap(), Some(newer));
    assert_eq!(reopened.history(&symbol("NVDA")).await.unwrap().len(), 3);
    let pool =
        sqlx::SqlitePool::connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(&path))
            .await
            .unwrap();
    let versions: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM _sqlx_migrations WHERE success = 1")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(versions, vec![1]);
}

#[tokio::test]
async fn successful_refresh_exact_holdings_match_and_restart() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("research.db");
    let upstream = response_mock(StatusCode::OK, FORECAST).await;
    let research = service(&path, &upstream).await;
    let holdings = portfolio(fixture()).await;
    let before = holdings.snapshot().await;
    let initial = research.holdings(&holdings).await;
    assert_eq!(
        initial.holdings[0].research.status,
        ResearchStatus::NeverRefreshed
    );
    let refreshed = research.refresh(" nvda ").await;
    assert_eq!(refreshed.outcome, RefreshOutcome::Saved);
    assert!(refreshed.company.snapshot.is_some());
    assert_eq!(
        research.refresh("NVDA").await.outcome,
        RefreshOutcome::Unchanged
    );
    let result = research.holdings(&holdings).await;
    assert_eq!(result.holdings.len(), 6);
    assert_eq!(
        result.holdings[0].research.status,
        ResearchStatus::Available
    );
    assert_eq!(
        result.holdings[0].holding.market_value,
        Decimal::new(200, 0)
    );
    assert_eq!(
        result.holdings[1].research.status,
        ResearchStatus::NeverRefreshed
    );
    assert_eq!(
        result.holdings[2].research.status,
        ResearchStatus::Unmatched
    );
    assert!(
        result.holdings[3..]
            .iter()
            .all(|row| row.research.status == ResearchStatus::Unsupported)
    );
    assert_eq!(holdings.snapshot().await, before);
    drop(research);
    drop(upstream);
    let reopened = ResearchService::open(&path).await;
    let saved = reopened.company("NVDA").await;
    assert_eq!(saved.snapshot, refreshed.company.snapshot);
    assert!(saved.last_refresh_at.is_none()); // attempts are operational memory, not facts
}

#[tokio::test]
async fn failed_refresh_preserves_snapshot_and_reports_error_separately() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("research.db");
    let repository = ResearchRepository::open(&path).await.unwrap();
    repository.save(&snapshot()).await.unwrap();
    for (status, body, error) in [
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "down",
            ResearchError::SourceHttp { status: 503 },
        ),
        (StatusCode::OK, "{wrong", ResearchError::MalformedSource),
    ] {
        let upstream = response_mock(status, body).await;
        let research = service(&path, &upstream).await;
        let result = research.refresh("NVDA").await;
        assert_eq!(result.outcome, RefreshOutcome::Failed);
        assert_eq!(result.company.refresh_error, Some(error.clone()));
        assert_eq!(result.company.snapshot, Some(snapshot()));
        assert_eq!(research.company("NVDA").await.refresh_error, Some(error));
    }
    assert_eq!(repository.history(&symbol("NVDA")).await.unwrap().len(), 1);
}

#[tokio::test]
async fn connection_failure_and_timeout_are_typed_and_bounded() {
    let directory = tempdir().unwrap();
    let upstream = response_mock(StatusCode::OK, FORECAST).await;
    let source = ResearchSource::with_base_url(&upstream.url, Duration::from_millis(100));
    upstream.task.abort();
    // Join guarantees the listener was dropped; no fixed port or internet dependency.
    while !upstream.task.is_finished() {
        tokio::task::yield_now().await;
    }
    let research =
        ResearchService::with_source(&directory.path().join("connection.db"), source).await;
    assert_eq!(
        research.refresh("NVDA").await.company.refresh_error,
        Some(ResearchError::SourceUnavailable)
    );
    let upstream = mock(Router::new().route(
        "/stocks/nvda/forecast/__data.json",
        get(|| async {
            std::future::pending::<()>().await;
            FORECAST
        }),
    ))
    .await;
    let research = ResearchService::with_source(
        &directory.path().join("timeout.db"),
        ResearchSource::with_base_url(&upstream.url, Duration::from_millis(50)),
    )
    .await;
    let result = tokio::time::timeout(Duration::from_secs(2), research.refresh("NVDA"))
        .await
        .unwrap();
    assert_eq!(result.company.refresh_error, Some(ResearchError::Timeout));
}

#[tokio::test]
async fn slow_source_holds_no_portfolio_lock_and_second_refresh_is_rejected() {
    let directory = tempdir().unwrap();
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let upstream = mock(Router::new().route(
        "/stocks/nvda/forecast/__data.json",
        get({
            let started = started.clone();
            let release = release.clone();
            move || {
                let started = started.clone();
                let release = release.clone();
                async move {
                    started.notify_one();
                    release.notified().await;
                    FORECAST
                }
            }
        }),
    ))
    .await;
    let research = service(&directory.path().join("research.db"), &upstream).await;
    let holdings = portfolio(fixture()).await;
    let refresh = tokio::spawn({
        let research = research.clone();
        async move { research.refresh("NVDA").await }
    });
    tokio::time::timeout(Duration::from_secs(2), started.notified())
        .await
        .unwrap();
    // Both read and write acquisition succeed while the external request is paused.
    let read = tokio::time::timeout(Duration::from_secs(1), holdings.snapshot())
        .await
        .unwrap();
    let reload = tokio::time::timeout(Duration::from_secs(1), holdings.reload())
        .await
        .unwrap();
    assert_eq!(read.portfolio, reload.portfolio);
    assert_eq!(
        research.refresh("MSFT").await.company.refresh_error,
        Some(ResearchError::Busy)
    );
    let saved_read = tokio::time::timeout(Duration::from_secs(1), research.holdings(&holdings))
        .await
        .unwrap();
    assert_eq!(
        saved_read.holdings[0].research.status,
        ResearchStatus::NeverRefreshed
    );
    release.notify_one();
    assert_eq!(refresh.await.unwrap().outcome, RefreshOutcome::Saved);
}

#[tokio::test]
async fn no_portfolio_and_database_initialization_failure_leave_portfolio_usable() {
    let directory = tempdir().unwrap();
    let holdings = portfolio(directory.path().join("missing")).await;
    // A directory cannot be opened as a SQLite file.
    let research = ResearchService::open(directory.path()).await;
    assert_eq!(
        research.initialization_error(),
        Some(ResearchError::Initialization)
    );
    let result = research.holdings(&holdings).await;
    assert!(!result.portfolio_available);
    assert!(result.holdings.is_empty());
    let holdings = portfolio(fixture()).await;
    let router = app(holdings, research).await;
    assert_eq!(
        call(&router, "GET", "/api/portfolio").await.1["status"],
        "loaded"
    );
    let result = call(&router, "GET", "/api/research/holdings").await;
    assert_eq!(result.0, StatusCode::OK);
    assert_eq!(
        result.1["holdings"][0]["research"]["status"],
        "repository_failure"
    );
    assert_eq!(
        call(&router, "POST", "/api/research/companies/NVDA/refresh")
            .await
            .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
async fn migration_failure_is_distinct_from_open_failure() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("conflict.db");
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true),
    )
    .await
    .unwrap();
    sqlx::query("CREATE TABLE research_snapshots (wrong TEXT)")
        .execute(&pool)
        .await
        .unwrap();
    let research = ResearchService::open(&path).await;
    assert_eq!(
        research.initialization_error(),
        Some(ResearchError::Migration)
    );
    assert_eq!(
        research.company("NVDA").await.error,
        Some(ResearchError::Migration)
    );
}

#[tokio::test]
async fn database_write_failure_keeps_history_and_saved_view() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("research.db");
    let repository = ResearchRepository::open(&path).await.unwrap();
    repository.save(&snapshot()).await.unwrap();
    let pool =
        sqlx::SqlitePool::connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(&path))
            .await
            .unwrap();
    sqlx::query("CREATE TRIGGER fail_writes BEFORE INSERT ON research_snapshots BEGIN SELECT RAISE(FAIL, 'synthetic disk failure'); END;")
        .execute(&pool).await.unwrap();
    let upstream = response_mock(StatusCode::OK, FORECAST).await;
    let research = service(&path, &upstream).await;
    let result = research.refresh("NVDA").await;
    assert_eq!(result.outcome, RefreshOutcome::Failed);
    assert_eq!(
        result.company.refresh_error,
        Some(ResearchError::Repository)
    );
    assert_eq!(result.company.snapshot, Some(snapshot()));
    assert_eq!(repository.history(&symbol("NVDA")).await.unwrap().len(), 1);
}

#[tokio::test]
async fn http_contract_success_missing_unsupported_and_source_failure() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("research.db");
    let upstream = response_mock(StatusCode::OK, FORECAST).await;
    let router = app(portfolio(fixture()).await, service(&path, &upstream).await).await;
    let never = call(&router, "GET", "/api/research/companies/NVDA").await;
    assert_eq!(never.0, StatusCode::OK);
    assert_eq!(never.1["status"], "never_refreshed");
    assert_eq!(
        call(&router, "POST", "/api/research/companies/nvda/refresh")
            .await
            .1["outcome"],
        "saved"
    );
    assert_eq!(
        call(&router, "GET", "/api/research/companies/NVDA").await.1["snapshot"]["company"]["symbol"],
        "NVDA"
    );
    assert_eq!(
        call(&router, "POST", "/api/research/companies/ZZZZ/refresh")
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&router, "POST", "/api/research/companies/NVDA123/refresh")
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    drop(upstream);
    let upstream = response_mock(StatusCode::BAD_GATEWAY, "bad gateway").await;
    let router = app(portfolio(fixture()).await, service(&path, &upstream).await).await;
    let failed = call(&router, "POST", "/api/research/companies/NVDA/refresh").await;
    assert_eq!(failed.0, StatusCode::BAD_GATEWAY);
    assert_eq!(failed.1["outcome"], "failed");
    assert!(failed.1["company"]["snapshot"].is_object());
    assert_eq!(
        call(&router, "POST", "/api/portfolio/reload").await.1["status"],
        "loaded"
    );
}
