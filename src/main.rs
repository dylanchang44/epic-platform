#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "epic_platform=info".into()),
        )
        .init();
    use epic_platform::server;
    use epic_platform::{
        config::AppConfig,
        state::{AppState, PortfolioState},
    };
    use leptos::prelude::get_configuration;

    // Read cargo-leptos settings when launched directly from the project root.
    let options = get_configuration(Some("Cargo.toml"))?.leptos_options;
    let address = options.site_addr;
    let config = AppConfig::from_env();
    let research =
        epic_platform::research::service::ResearchService::open(&config.research_db_path).await;
    if let Some(error) = research.initialization_error() {
        eprintln!("Research unavailable: {error}");
    }
    let review = epic_platform::review::service::ReviewService::open(&config.review_db_path).await;
    if let Some(error) = review.initialization_error() {
        eprintln!("Review unavailable: {error}");
    }
    let portfolio = PortfolioState::new(config);
    let initial = portfolio.reload().await;
    if let Some(error) = initial.error {
        eprintln!("Portfolio unavailable: {}", error.message);
    }
    let jobs = epic_platform::jobs::service::JobService::new(&review);
    // Bind before starting a worker: a second process on this address must not
    // reconcile the live process's running job.
    let listener = tokio::net::TcpListener::bind(address).await?;
    let worker = match epic_platform::jobs::runner::start(
        jobs.clone(),
        portfolio.clone(),
        research.clone(),
        review.clone(),
    )
    .await
    {
        Ok(worker) => Some(worker),
        Err(error) => {
            tracing::error!(%error, "review worker unavailable");
            None
        }
    };
    let state = AppState {
        leptos_options: options,
        portfolio,
        research,
        review,
        jobs,
    };
    println!("EPIC Platform: http://{address}");
    axum::serve(listener, server::router(state))
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            if let Some(worker) = worker {
                worker.shutdown().await;
            }
        })
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("install SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {},
        _ = terminate.recv() => {},
    }
}
