//! Rift daemon entry point.

use rift_ipc::default_socket_path;
use riftd::config::default_config_path;
use riftd::server::Server;
use tracing::error;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let socket_path = default_socket_path();
    let config_path = default_config_path();
    let server = match Server::bind(socket_path, config_path) {
        Ok(s) => s,
        Err(e) => {
            error!(error = %e, "failed to start");
            return Err(e);
        }
    };

    server.serve(shutdown_signal()).await;
    Ok(())
}

/// Resolve when the process receives SIGINT or SIGTERM.
async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};

    let mut sigint = signal(SignalKind::interrupt()).expect("install SIGINT handler");
    let mut sigterm = signal(SignalKind::terminate()).expect("install SIGTERM handler");

    tokio::select! {
        _ = sigint.recv() => {}
        _ = sigterm.recv() => {}
    }
}
