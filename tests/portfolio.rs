#![cfg(feature = "ssr")]

use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use epic_platform::{
    config::AppConfig,
    portfolio::{LoadStatus, PortfolioSnapshot, loader::load_portfolio},
    state::{AppState, PortfolioState},
};
use rust_decimal::Decimal;
use std::{fs, path::PathBuf};
use tempfile::tempdir;
use tower::ServiceExt;

const CSV: &str = include_str!("fixtures/schwab/Example-Positions.csv");
fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/schwab")
}

#[test]
fn loads_synthetic_directory_with_exact_signed_total_and_cash() {
    let portfolio = load_portfolio(&fixture()).unwrap();
    assert_eq!(portfolio.summary.position_count, 4);
    assert_eq!(
        portfolio.summary.total_market_value,
        Decimal::new(155275, 2)
    );
    assert_eq!(portfolio.positions[2].market_value, Decimal::new(-50, 0));
    assert_eq!(portfolio.positions[3].quantity, None);
    assert_eq!(portfolio.positions[1].cost_basis, None);
}

#[test]
fn missing_directory_is_distinct_from_missing_required_data() {
    let directory = tempdir().unwrap();
    assert_eq!(
        load_portfolio(&directory.path().join("missing"))
            .unwrap_err()
            .code,
        "directory_missing"
    );
    fs::write(directory.path().join("Balances.csv"), "Account Value,100").unwrap();
    assert_eq!(
        load_portfolio(directory.path()).unwrap_err().code,
        "positions_missing"
    );
}

#[test]
fn rejects_empty_malformed_missing_values_and_bad_totals() {
    let directory = tempdir().unwrap();
    for content in [
        String::new(),
        "not a positions export".into(),
        CSV.replace("$125.25", "not money"),
        CSV.replace("$125.25", "--"),
        CSV.replace("$1,552.75", "$1,000.00"),
        CSV.replace(
            "\"DEMO-B\",\"Example Index Fund\"",
            "\"DEMO-A\",\"Example Index Fund\"",
        ),
        "Symbol,Description,Qty,Price,Mkt Val\nX,Example,1,2\n".into(),
        "Symbol,Description,Qty,Price,Mkt Val\n".into(),
    ] {
        fs::write(directory.path().join("Positions.csv"), content).unwrap();
        assert_eq!(
            load_portfolio(directory.path()).unwrap_err().code,
            "positions_invalid"
        );
    }
}

#[test]
fn selects_newest_case_insensitive_csv_and_does_not_fall_back_on_failure() {
    let directory = tempdir().unwrap();
    let old = directory.path().join("Old-Positions.csv");
    fs::write(&old, CSV).unwrap();
    fs::File::open(&old)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH)
        .unwrap();
    fs::write(directory.path().join("New-POSITIONS.CSV"), CSV).unwrap();
    assert_eq!(
        load_portfolio(directory.path()).unwrap().source_file,
        "New-POSITIONS.CSV"
    );
    fs::write(directory.path().join("New-POSITIONS.CSV"), "").unwrap();
    assert!(load_portfolio(directory.path()).is_err());
}

#[test]
fn directory_must_be_a_directory_and_symlinks_are_not_sources() {
    let directory = tempdir().unwrap();
    let file = directory.path().join("file");
    fs::write(&file, CSV).unwrap();
    assert_eq!(load_portfolio(&file).unwrap_err().code, "directory_invalid");
    std::os::unix::fs::symlink(&file, directory.path().join("Positions.csv")).unwrap();
    assert_eq!(
        load_portfolio(directory.path()).unwrap_err().code,
        "positions_missing"
    );
}

#[test]
fn rejects_malformed_quoting_and_supports_reordered_columns() {
    let directory = tempdir().unwrap();
    let file = directory.path().join("Positions.csv");
    fs::write(
        &file,
        "Symbol,Description,Qty,Price,Mkt Val,Asset Type\nX,Example,1,2,2,\"unclosed",
    )
    .unwrap();
    assert!(
        load_portfolio(directory.path())
            .unwrap_err()
            .message
            .contains("unterminated")
    );
    fs::write(&file, "\u{feff}Symbol,Mkt Val,Price,Qty,Description\r\nX,2.00,2.00,1,\"Example \"\"quoted\"\" name\"\r\n").unwrap();
    assert_eq!(
        load_portfolio(directory.path())
            .unwrap()
            .summary
            .total_market_value,
        Decimal::new(2, 0)
    );
}

#[test]
fn unreadable_directory_and_file_report_errors() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let directory = tempdir().unwrap();
    // Root bypasses POSIX permissions, so this assertion only applies to normal users.
    if fs::metadata(directory.path()).unwrap().uid() == 0 {
        return;
    }
    let original = fs::metadata(directory.path()).unwrap().permissions();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o000)).unwrap();
    let result = load_portfolio(directory.path());
    fs::set_permissions(directory.path(), original).unwrap();
    assert_eq!(result.unwrap_err().code, "directory_unreadable");
    let file = directory.path().join("Positions.csv");
    fs::write(&file, CSV).unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).unwrap();
    assert_eq!(
        load_portfolio(directory.path()).unwrap_err().code,
        "file_unreadable"
    );
}

#[tokio::test]
async fn failed_reload_preserves_snapshot_and_can_recover() {
    let directory = tempdir().unwrap();
    let file = directory.path().join("Positions.csv");
    fs::write(&file, CSV).unwrap();
    let state = PortfolioState::new(AppConfig {
        schwab_data_dir: directory.path().into(),
        ..AppConfig::default()
    });
    let before = state.reload().await;
    assert_eq!(before.status, LoadStatus::Loaded);
    fs::write(&file, "malformed").unwrap();
    let after = state.reload().await;
    assert_eq!(after.status, LoadStatus::Failed);
    assert_eq!(after.portfolio, before.portfolio);
    assert_eq!(after.last_successful_load, before.last_successful_load);
    assert!(after.error.is_some());
    fs::write(&file, CSV).unwrap();
    assert_eq!(state.reload().await.status, LoadStatus::Loaded);
    assert!(state.snapshot().await.error.is_none());
}

async fn api(directory: PathBuf) -> axum::Router {
    let portfolio = PortfolioState::new(AppConfig {
        schwab_data_dir: directory,
        ..AppConfig::default()
    });
    portfolio.reload().await;
    let options = leptos::prelude::get_configuration(Some("Cargo.toml"))
        .unwrap()
        .leptos_options;
    let research_dir = tempdir().unwrap();
    let research = epic_platform::research::service::ResearchService::open(
        &research_dir.path().join("research.db"),
    )
    .await;
    let review = epic_platform::review::service::ReviewService::open(
        &research_dir.path().join("reviews.db"),
    )
    .await;
    let jobs = epic_platform::jobs::service::JobService::new(&review);
    epic_platform::server::router(AppState {
        leptos_options: options,
        portfolio,
        research,
        review,
        jobs,
    })
}

#[tokio::test]
async fn startup_and_http_reload_return_the_snapshot() {
    let app = api(fixture()).await;
    for (method, url) in [("GET", "/api/portfolio"), ("POST", "/api/portfolio/reload")] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(url)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let bytes = to_bytes(response.into_body(), 1_000_000).await.unwrap();
        let snapshot: PortfolioSnapshot = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(snapshot.status, LoadStatus::Loaded);
        assert_eq!(snapshot.portfolio.unwrap().summary.position_count, 4);
    }
}

#[tokio::test]
async fn bad_startup_still_serves_health_and_status() {
    let directory = tempdir().unwrap();
    let app = api(directory.path().join("missing")).await;
    for (method, url, status) in [
        ("GET", "/health", 200),
        ("GET", "/api/portfolio", 200),
        ("POST", "/api/portfolio/reload", 422),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(url)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        if url != "/health" {
            let bytes = to_bytes(response.into_body(), 1_000_000).await.unwrap();
            let snapshot: PortfolioSnapshot = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(snapshot.status, LoadStatus::Failed);
            assert!(snapshot.portfolio.is_none());
            assert_eq!(snapshot.error.unwrap().code, "directory_missing");
        }
    }
}
