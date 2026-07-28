use crate::error::ApiError;
use crate::oauth::{
    config_for_provider, create_oauth_account, fetch_userinfo, pkce_from_session,
    token_exchange_error_message, verify_csrf, PendingOAuthSession,
};
use crate::state::AppStateRef;
use axum::{
    extract::{Query, State},
    response::{IntoResponse, Redirect},
    Json,
};
use pebble_core::OAuthTokens;
use pebble_oauth::OAuthManager;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthProvidersResponse {
    pub providers: Vec<String>,
    pub redirect_uri: String,
}

pub async fn list_providers(
    State(state): State<AppStateRef>,
) -> Result<Json<OAuthProvidersResponse>, ApiError> {
    let mut providers = Vec::new();
    if state.config.google_oauth.is_some() {
        providers.push("gmail".to_string());
    }
    if state.config.microsoft_oauth.is_some() {
        providers.push("outlook".to_string());
    }
    Ok(Json(OAuthProvidersResponse {
        providers,
        redirect_uri: state.config.oauth_callback_url(),
    }))
}

#[derive(Deserialize)]
pub struct StartOAuthRequest {
    pub provider: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartOAuthResponse {
    pub authorization_url: String,
    pub provider: String,
}

pub async fn start_oauth(
    State(state): State<AppStateRef>,
    Json(body): Json<StartOAuthRequest>,
) -> Result<Json<StartOAuthResponse>, ApiError> {
    let provider = body.provider.trim().to_lowercase();
    let redirect_url = state.config.oauth_callback_url();
    let config = config_for_provider(
        &provider,
        state.config.google_oauth.as_ref(),
        state.config.microsoft_oauth.as_ref(),
        redirect_url,
    )
    .map_err(ApiError::BadRequest)?;

    let manager = OAuthManager::new(config);
    let (authorization_url, pkce_state) = manager
        .start_auth()
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to start OAuth flow: {e}")))?;

    let csrf = pkce_state.csrf_token.secret().to_string();
    state
        .oauth_sessions
        .insert(
            csrf.clone(),
            PendingOAuthSession {
                provider: provider.clone(),
                code_verifier: pkce_state.verifier.secret().clone(),
                csrf_token: csrf,
                created_at: std::time::Instant::now(),
            },
        )
        .await;

    Ok(Json(StartOAuthResponse {
        authorization_url,
        provider,
    }))
}

#[derive(Deserialize)]
pub struct OAuthCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

fn urlencoding_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn spa_redirect(public_url: &str, params: &[(&str, &str)]) -> Redirect {
    let mut url = format!(
        "{}/settings?tab=accounts",
        public_url.trim_end_matches('/')
    );
    for (key, value) in params {
        url.push('&');
        url.push_str(key);
        url.push('=');
        url.push_str(&urlencoding_encode(value));
    }
    Redirect::temporary(&url)
}

pub async fn oauth_callback(
    State(state): State<AppStateRef>,
    Query(query): Query<OAuthCallbackQuery>,
) -> impl IntoResponse {
    let public_url = state.config.public_url.clone();

    if let Some(error) = query.error.as_deref() {
        let detail = query
            .error_description
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(error);
        return spa_redirect(&public_url, &[("oauth", "error"), ("message", detail)])
            .into_response();
    }

    let (code, callback_state) = match (query.code.as_deref(), query.state.as_deref()) {
        (Some(code), Some(state)) if !code.is_empty() && !state.is_empty() => (code, state),
        _ => {
            return spa_redirect(
                &public_url,
                &[
                    ("oauth", "error"),
                    ("message", "Missing authorization code or state"),
                ],
            )
            .into_response();
        }
    };

    let session = match state.oauth_sessions.take(callback_state).await {
        Some(session) => session,
        None => {
            return spa_redirect(
                &public_url,
                &[
                    ("oauth", "error"),
                    (
                        "message",
                        "OAuth session expired or invalid. Please try again.",
                    ),
                ],
            )
            .into_response();
        }
    };

    if !verify_csrf(&session, callback_state) {
        return spa_redirect(
            &public_url,
            &[("oauth", "error"), ("message", "OAuth state mismatch")],
        )
        .into_response();
    }

    let redirect_url = state.config.oauth_callback_url();
    let config = match config_for_provider(
        &session.provider,
        state.config.google_oauth.as_ref(),
        state.config.microsoft_oauth.as_ref(),
        redirect_url,
    ) {
        Ok(cfg) => cfg,
        Err(e) => {
            return spa_redirect(&public_url, &[("oauth", "error"), ("message", &e)]).into_response();
        }
    };

    let manager = OAuthManager::new(config);
    let pkce = pkce_from_session(&session);
    let token_pair = match manager.complete_auth(code, pkce).await {
        Ok(tokens) => tokens,
        Err(e) => {
            let message = token_exchange_error_message(&session.provider, &e);
            return spa_redirect(&public_url, &[("oauth", "error"), ("message", &message)])
                .into_response();
        }
    };

    let (email, name) = match fetch_userinfo(&session.provider, &token_pair.access_token).await {
        Ok(info) => info,
        Err(e) => {
            return spa_redirect(&public_url, &[("oauth", "error"), ("message", &e)]).into_response();
        }
    };

    if email.is_empty() {
        return spa_redirect(
            &public_url,
            &[
                ("oauth", "error"),
                ("message", "Provider did not return an email address"),
            ],
        )
        .into_response();
    }

    let display_name = if name.is_empty() {
        email.clone()
    } else {
        name
    };

    let tokens = OAuthTokens {
        access_token: token_pair.access_token,
        refresh_token: token_pair.refresh_token,
        expires_at: token_pair.expires_at,
        scopes: token_pair.scopes,
    };

    let account = match create_oauth_account(&state, &session.provider, email, display_name, tokens)
    {
        Ok(account) => account,
        Err(e) => {
            return spa_redirect(&public_url, &[("oauth", "error"), ("message", &e)]).into_response();
        }
    };

    let account_id = account.id.clone();
    let sync_manager = state.sync_manager.clone();
    tokio::spawn(async move {
        if let Err(e) = sync_manager.start_account_sync(&account_id).await {
            tracing::warn!("Failed to auto-start sync for OAuth account {account_id}: {e}");
        }
    });

    spa_redirect(
        &public_url,
        &[
            ("oauth", "success"),
            ("accountId", &account.id),
            ("email", &account.email),
        ],
    )
    .into_response()
}
