use crate::error::ApiError;
use crate::state::{AppStateRef, OAuthFlowRecord};
use axum::{
    extract::{Path, Query, State},
    response::Html,
    Json,
};
use pebble_core::{new_id, now_timestamp, Account, OAuthTokens, ProviderType};
use pebble_oauth::{build_http_client, OAuthConfig, OAuthManager, OAuthNetworkConfig, PkceState};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const MICROSOFT_AUTH_URL: &str =
    "https://login.microsoftonline.com/common/oauth2/v2.0/authorize";
const MICROSOFT_TOKEN_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRequest {
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub account_label: Option<String>,
    pub sync_range_days: Option<u32>,
    pub proxy_host: Option<String>,
    pub proxy_port: Option<u16>,
}

#[derive(Serialize)]
pub struct StartResponse {
    pub authorization_url: String,
    pub flow_id: String,
}

#[derive(Serialize)]
pub struct StatusResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<Account>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

fn provider_config(provider: &str) -> Result<(OAuthConfig, String), ApiError> {
    let (id_key, secret_key, auth_url, token_url, scopes, callback_path) = match provider {
        "gmail" => (
            "PEBBLE_GOOGLE_CLIENT_ID",
            "PEBBLE_GOOGLE_CLIENT_SECRET",
            GOOGLE_AUTH_URL,
            GOOGLE_TOKEN_URL,
            vec![
                "openid",
                "email",
                "profile",
                "https://mail.google.com/",
            ],
            "/api/v1/oauth/gmail/callback",
        ),
        "outlook" => (
            "PEBBLE_MICROSOFT_CLIENT_ID",
            "PEBBLE_MICROSOFT_CLIENT_SECRET",
            MICROSOFT_AUTH_URL,
            MICROSOFT_TOKEN_URL,
            vec![
                "openid",
                "email",
                "offline_access",
                "User.Read",
                "Mail.ReadWrite",
                "Mail.Send",
            ],
            "/api/v1/oauth/outlook/callback",
        ),
        _ => return Err(ApiError::NotFound("Unsupported OAuth provider".into())),
    };
    let client_id = std::env::var(id_key).unwrap_or_default();
    let client_secret = std::env::var(secret_key).unwrap_or_default();
    if client_id.trim().is_empty() || client_secret.trim().is_empty() {
        return Err(ApiError::NotFound("OAuth provider is not configured".into()));
    }
    let public_base = std::env::var("PEBBLE_PUBLIC_BASE_URL")
        .map_err(|_| ApiError::Internal("PEBBLE_PUBLIC_BASE_URL is required for OAuth".into()))?;
    let base = reqwest::Url::parse(&public_base)
        .map_err(|_| ApiError::Internal("Invalid OAuth public base URL".into()))?;
    let local_http = base.scheme() == "http"
        && matches!(base.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if base.scheme() != "https" && !local_http {
        return Err(ApiError::Internal("OAuth public base URL must use HTTPS".into()));
    }
    let redirect = base.join(callback_path)
        .map_err(|_| ApiError::Internal("Invalid OAuth callback URL".into()))?
        .to_string();
    Ok((
        OAuthConfig {
            client_id,
            client_secret: Some(client_secret),
            auth_url: auth_url.into(),
            token_url: token_url.into(),
            scopes: scopes.into_iter().map(str::to_owned).collect(),
            redirect_port: 0,
        },
        redirect,
    ))
}

pub async fn providers() -> Json<Value> {
    Json(json!({
        "gmail": provider_config("gmail").is_ok(),
        "outlook": provider_config("outlook").is_ok(),
    }))
}

pub async fn start(
    State(state): State<AppStateRef>,
    Path(provider): Path<String>,
    Json(options): Json<StartRequest>,
) -> Result<Json<StartResponse>, ApiError> {
    let provider = provider.to_ascii_lowercase();
    pebble_store::accounts::validate_sync_range_days(options.sync_range_days)
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;
    if options.proxy_host.is_some() != options.proxy_port.is_some() {
        return Err(ApiError::BadRequest("Proxy host and port must be supplied together".into()));
    }
    let (config, redirect) = provider_config(&provider)?;
    let proxy = match (options.proxy_host, options.proxy_port) {
        (Some(host), Some(port)) => {
            let proxy = pebble_core::HttpProxyConfig { host, port };
            proxy.validate().map_err(ApiError::BadRequest)?;
            Some(proxy)
        }
        _ => None,
    };
    let network = OAuthNetworkConfig { proxy: proxy.clone() };
    let manager = OAuthManager::new_web(config, network, redirect);
    let (authorization_url, pkce) = manager.start_auth().await
        .map_err(|error| ApiError::Internal(format!("Could not start OAuth: {error}")))?;
    let (verifier, csrf) = pkce.into_secrets();
    let flow_id = new_id();
    let account_label = options.account_label.as_deref()
        .map(|label| pebble_store::accounts::normalize_account_label(Some(label)))
        .transpose()
        .map_err(|error| ApiError::BadRequest(error.to_string()))?
        .flatten();
    state.oauth_flows.lock().await.insert(flow_id.clone(), OAuthFlowRecord {
        provider,
        state: csrf,
        verifier,
        expires_at: now_timestamp() + 600,
        account_label,
        display_name_hint: options.display_name,
        sync_range_days: options.sync_range_days,
        proxy,
        consumed: false,
        status: "pending".into(),
        account: None,
        error: None,
    });
    Ok(Json(StartResponse { authorization_url, flow_id }))
}

pub async fn status(
    State(state): State<AppStateRef>,
    Path((provider, flow_id)): Path<(String, String)>,
) -> Result<Json<StatusResponse>, ApiError> {
    let flows = state.oauth_flows.lock().await;
    let flow = flows.get(&flow_id).ok_or_else(|| ApiError::NotFound("OAuth flow not found".into()))?;
    if flow.provider != provider.to_ascii_lowercase() || flow.expires_at < now_timestamp() {
        return Err(ApiError::NotFound("OAuth flow not found".into()));
    }
    Ok(Json(StatusResponse {
        status: flow.status.clone(),
        account: flow.account.clone(),
        error: flow.error.clone(),
    }))
}

fn safe_callback_page() -> Html<&'static str> {
    Html("<!doctype html><meta charset=\"utf-8\"><title>Pebble account setup</title><p>Authorization finished. You may close this window.</p>")
}

fn matches_state(left: &str, right: &str) -> bool {
    left.len() == right.len() && left.as_bytes().iter().zip(right.as_bytes()).fold(0u8, |diff, (a, b)| diff | (a ^ b)) == 0
}

async fn finish_callback(
    state: &AppStateRef,
    provider: &str,
    query: CallbackQuery,
) -> Result<(), String> {
    let supplied_state = query.state.as_deref().ok_or_else(|| "Invalid authorization response".to_string())?;
    let flow_id = {
        let mut flows = state.oauth_flows.lock().await;
        let candidate = flows.iter().find(|(_, flow)| {
            flow.provider == provider
                && flow.expires_at >= now_timestamp()
                && !flow.consumed
                && matches_state(&flow.state, supplied_state)
        }).map(|(id, _)| id.clone()).ok_or_else(|| "Authorization expired or was already used".to_string())?;
        let flow = flows.get_mut(&candidate).expect("candidate exists");
        flow.consumed = true;
        if query.error.is_some() {
            flow.status = "error".into();
            flow.error = Some("Authorization was not completed".into());
            return Ok(());
        }
        candidate
    };
    let code = query.code.ok_or_else(|| "Authorization response was incomplete".to_string())?;
    let (flow, config, redirect, proxy) = {
        let flows = state.oauth_flows.lock().await;
        let flow = flows.get(&flow_id).ok_or_else(|| "Authorization flow expired".to_string())?;
        let (config, redirect) = provider_config(provider).map_err(|_| "OAuth provider is unavailable".to_string())?;
        (flow.verifier.clone(), config, redirect, flow.proxy.clone())
    };
    let network = OAuthNetworkConfig { proxy: proxy.clone() };
    let manager = OAuthManager::new_web(config, network.clone(), redirect);
    let tokens = manager.complete_auth(&code, PkceState::from_secrets(flow, supplied_state.to_owned()))
        .await.map_err(|_| "Authorization could not be completed".to_string())?;
    let profile_url = if provider == "gmail" {
        "https://www.googleapis.com/oauth2/v2/userinfo"
    } else {
        "https://graph.microsoft.com/v1.0/me?$select=id,mail,displayName"
    };
    let client = build_http_client(&network).map_err(|_| "Mailbox verification failed".to_string())?;
    let profile: Value = client.get(profile_url)
        .bearer_auth(&tokens.access_token).send().await
        .map_err(|_| "Mailbox verification failed".to_string())?
        .error_for_status().map_err(|_| "Mailbox verification failed".to_string())?
        .json().await.map_err(|_| "Mailbox verification failed".to_string())?;
    let email = if provider == "gmail" {
        profile["email"].as_str()
    } else {
        profile["mail"].as_str()
    }.filter(|value| !value.trim().is_empty()).ok_or_else(|| "Provider did not confirm a mailbox address".to_string())?;
    let subject = profile["id"].as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "Provider did not confirm a stable mailbox identity".to_string())?;
    if provider == "gmail" && profile["verified_email"].as_bool() == Some(false) {
        return Err("Provider mailbox address is not verified".into());
    }
    let display_name = profile["name"].as_str()
        .or_else(|| profile["displayName"].as_str())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| state.oauth_flows.try_lock().ok()
            .and_then(|flows| flows.get(&flow_id).and_then(|flow| flow.display_name_hint.clone())))
        .unwrap_or_default();
    let account_id = new_id();
    let now = now_timestamp();
    let account = Account {
        account_label: None,
        provider_display_name: (!display_name.is_empty()).then(|| display_name.clone()),
        id: account_id.clone(),
        email: email.to_string(),
        display_name: if display_name.is_empty() { email.to_string() } else { display_name },
        color: None,
        provider: if provider == "gmail" { ProviderType::Gmail } else { ProviderType::Outlook },
        created_at: now,
        updated_at: now,
    };
    let account_label = {
        let flows = state.oauth_flows.lock().await;
        flows.get(&flow_id).and_then(|flow| flow.account_label.clone())
    };
    let account = Account { account_label, ..account };
    let token_data = OAuthTokens {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        expires_at: tokens.expires_at,
        scopes: tokens.scopes,
    };
    let plaintext = serde_json::to_vec(&token_data).map_err(|_| "Could not save authorization".to_string())?;
    let encrypted = state.crypto.encrypt_for("accounts.auth_data", &account_id, &plaintext)
        .map_err(|_| "Could not secure authorization".to_string())?;
    let (cutoff, sync_range_days, proxy) = {
        let flows = state.oauth_flows.lock().await;
        let flow = flows.get(&flow_id).ok_or_else(|| "Authorization flow expired".to_string())?;
        (
            pebble_store::accounts::sync_range_cutoff(flow.sync_range_days, now),
            flow.sync_range_days,
            flow.proxy.clone(),
        )
    };
    let account_label = account.account_label.clone();
    let provider_display_name = account.provider_display_name.clone();
    let email_value = account.email.clone();
    let display_value = account.display_name.clone();
    let color = account.color.clone();
    let provider_value = provider.to_string();
    let subject_value = format!("{provider}:{subject}");
    let sync_state = json!({
        "provider": provider,
        "sync_range_days": sync_range_days,
        "sync_since": cutoff,
        "proxy": proxy
    }).to_string();
    state.store.with_write_async(move |conn| {
        conn.execute(
            "INSERT INTO accounts (id, email, display_name, color, provider, created_at, updated_at, account_label, provider_display_name, oauth_subject, auth_data, sync_state)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            rusqlite::params![
                account_id, email_value, display_value, color, provider_value, now, now,
                account_label, provider_display_name, subject_value, encrypted, sync_state
            ],
        )?;
        Ok(())
    }).await.map_err(|_| "Could not save the authorized account".to_string())?;
    let mut flows = state.oauth_flows.lock().await;
    if let Some(flow) = flows.get_mut(&flow_id) {
        flow.status = "completed".into();
        flow.account = Some(account);
    }
    Ok(())
}

pub async fn callback(
    State(state): State<AppStateRef>,
    Path(provider): Path<String>,
    Query(query): Query<CallbackQuery>,
) -> Html<&'static str> {
    let provider = provider.to_ascii_lowercase();
    let callback_state = query.state.clone();
    match finish_callback(&state, &provider, query).await {
        Ok(()) => safe_callback_page(),
        Err(error) => {
            tracing::warn!("OAuth callback failed: {error}");
            if let Some(callback_state) = callback_state {
                let mut flows = state.oauth_flows.lock().await;
                if let Some(flow) = flows.values_mut().find(|flow| {
                    flow.provider == provider && matches_state(&flow.state, &callback_state)
                }) {
                    flow.status = "error".into();
                    flow.error = Some("Authorization could not be completed".into());
                }
            }
            safe_callback_page()
        }
    }
}
