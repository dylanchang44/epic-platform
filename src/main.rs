#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    use epic_platform::server;
    use epic_platform::{
        config::AppConfig,
        state::{AppState, PortfolioState},
    };
    use leptos::prelude::get_configuration;

    // Read cargo-leptos settings when launched directly from the project root.
    let options = get_configuration(Some("Cargo.toml"))?.leptos_options;
    let address = options.site_addr;
    let portfolio = PortfolioState::new(AppConfig::from_env());
    let initial = portfolio.reload().await;
    if let Some(error) = initial.error {
        eprintln!("Portfolio unavailable: {}", error.message);
    }
    let state = AppState {
        leptos_options: options,
        portfolio,
    };
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("EPIC Platform: http://{address}");
    axum::serve(listener, server::router(state))
        .with_graceful_shutdown(shutdown_signal())
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
