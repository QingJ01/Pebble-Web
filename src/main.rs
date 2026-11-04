mod auth;
mod auto_backup;
mod config;
mod credentials;
mod error;
mod mail_service;
mod maintenance;
mod routes;
mod state;
mod sync;
mod ws;

use crate::config::Config;
use crate::state::{AppState, AppStateRef};
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("pebble_web=info".parse().unwrap()),
        )
        .init();

    let config = Config::from_env().expect("Failed to load config");
    let port = config.port;

    let state = AppState::init(config).expect("Failed to initialize app state");
    let state: AppStateRef = Arc::new(state);
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    // Spawn background sync for all configured accounts.
    let sync_manager = state.sync_manager.clone();
    tokio::spawn(async move {
        sync_manager.start_all().await;
    });
    let backup_state = state.clone();
    let backup_shutdown = shutdown_rx.clone();
    let backup_task = tokio::spawn(async move {
        auto_backup::run_scheduler(backup_state, backup_shutdown).await;
    });
    let pending_state = state.clone();
    let pending_shutdown = shutdown_rx.clone();
    let pending_task = tokio::spawn(async move {
        mail_service::pending_ops::run_pending_mail_ops_worker(pending_state, pending_shutdown)
            .await;
    });
    let maintenance_state = state.clone();
    let maintenance_shutdown = shutdown_rx.clone();
    let maintenance_task = tokio::spawn(async move {
        maintenance::run(maintenance_state, maintenance_shutdown).await;
    });

    let static_dir = std::env::var("PEBBLE_STATIC_DIR")
        .unwrap_or_else(|_| "/usr/local/share/pebble-web/static".to_string());

    let app = routes::build_router(state.clone(), &static_dir);

    let addr = format!("0.0.0.0:{port}");
    info!("Pebble Web listening on {addr}");
    let listener = TcpListener::bind(&addr).await.expect("Failed to bind");
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            let _ = shutdown_tx.send(true);
        })
        .await
        .expect("Server error");
    for (name, task) in [
        ("auto-backup", backup_task),
        ("pending-ops", pending_task),
        ("maintenance", maintenance_task),
    ] {
        match tokio::time::timeout(std::time::Duration::from_secs(30), task).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::warn!("{name} worker stopped abnormally: {error}"),
            Err(_) => tracing::warn!("{name} worker did not stop within 30 seconds"),
        }
    }
    state.sync_manager.stop_all().await;
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
    info!("Shutdown signal received");
}
