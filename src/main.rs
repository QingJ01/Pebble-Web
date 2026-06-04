mod auth;
mod config;
mod credentials;
mod error;
mod notifications;
mod routes;
mod rules_engine;
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
            EnvFilter::from_default_env()
                .add_directive("pebble_web=info".parse().unwrap()),
        )
        .init();

    let config = Config::from_env().expect("Failed to load config");
    let port = config.port;

    let state = AppState::init(config).expect("Failed to initialize app state");
    let state: AppStateRef = Arc::new(state);

    if let Some(mut message_rx) = state.sync_manager.take_message_rx().await {
        let notification_state = state.clone();
        tokio::spawn(async move {
            while let Some(stored) = message_rx.recv().await {
                if stored.notify {
                    crate::rules_engine::handle_new_message(
                        notification_state.clone(),
                        stored.message,
                    )
                    .await;
                }
            }
        });
    }

    // Spawn background sync for all configured accounts.
    let sync_manager = state.sync_manager.clone();
    tokio::spawn(async move {
        sync_manager.start_all().await;
    });

    let static_dir = std::env::var("PEBBLE_STATIC_DIR")
        .unwrap_or_else(|_| "/usr/local/share/pebble-web/static".to_string());

    let app = routes::build_router(state, &static_dir);

    let addr = format!("0.0.0.0:{port}");
    info!("Pebble Web listening on {addr}");
    let listener = TcpListener::bind(&addr).await.expect("Failed to bind");
    axum::serve(listener, app).await.expect("Server error");
}
