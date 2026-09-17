#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    use epic_platform::server;
    use leptos::prelude::get_configuration;

    // Read cargo-leptos settings when launched directly from the project root.
    let options = get_configuration(Some("Cargo.toml"))?.leptos_options;
    let address = options.site_addr;
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("EPIC Platform: http://{address}");
    axum::serve(listener, server::router(options))
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
