mod auth;
mod config;
mod credentials;
mod error;
mod oauth;
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
            EnvFilter::from_default_env()
                .add_directive("pebble_web=info".parse().unwrap())
                .add_directive("pebble_mail=info".parse().unwrap())
                .add_directive("pebble_oauth=info".parse().unwrap()),
        )
        .init();

    let config = Config::from_env().expect("Failed to load config");
    let port = config.port;

    if config.microsoft_oauth.is_some() {
        let has_secret = config
            .microsoft_oauth
            .as_ref()
            .and_then(|c| c.client_secret.as_ref())
            .is_some();
        info!(
            "Outlook OAuth enabled (client_secret configured: {has_secret}, redirect: {})",
            config.oauth_callback_url()
        );
        if !has_secret {
            tracing::warn!(
                "MICROSOFT_CLIENT_SECRET is empty; Web app registrations usually require it or token exchange will fail with AADSTS70002"
            );
        }
    }
    if config.google_oauth.is_some() {
        let has_secret = config
            .google_oauth
            .as_ref()
            .and_then(|c| c.client_secret.as_ref())
            .is_some();
        info!(
            "Gmail OAuth enabled (client_secret configured: {has_secret}, redirect: {})",
            config.oauth_callback_url()
        );
    }

    let state = AppState::init(config).expect("Failed to initialize app state");
    let state: AppStateRef = Arc::new(state);

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
