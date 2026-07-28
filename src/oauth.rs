//! Web OAuth2 (Authorization Code + PKCE) helpers for Gmail and Outlook.
//!
//! Flow:
//! 1. Authenticated client calls `POST /api/v1/oauth/start`
//! 2. Server stores PKCE/CSRF state and returns the provider authorization URL
//! 3. Browser is redirected to the provider, then back to `GET /api/v1/oauth/callback`
//! 4. Server exchanges the code, creates the account, and redirects to the SPA

use crate::state::AppState;
use pebble_core::{new_id, now_timestamp, Account, OAuthTokens, ProviderType};
use pebble_crypto::CryptoService;
use pebble_mail::gmail_sync::TokenRefresher;
use pebble_oauth::{OAuthConfig, OAuthError, OAuthManager, OAuthNetworkConfig, PkceState};
use pebble_store::Store;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::debug;

const SESSION_TTL: Duration = Duration::from_secs(600);

#[derive(Debug, Clone)]
pub struct OAuthProviderCredentials {
    pub client_id: String,
    pub client_secret: Option<String>,
}

#[derive(Debug)]
pub struct PendingOAuthSession {
    pub provider: String,
    pub code_verifier: String,
    pub csrf_token: String,
    pub created_at: Instant,
}

#[derive(Debug, Default)]
pub struct OAuthSessionStore {
    inner: Mutex<HashMap<String, PendingOAuthSession>>,
}

impl OAuthSessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn insert(&self, state: String, session: PendingOAuthSession) {
        let mut guard = self.inner.lock().await;
        Self::purge_expired_locked(&mut guard);
        guard.insert(state, session);
    }

    pub async fn take(&self, state: &str) -> Option<PendingOAuthSession> {
        let mut guard = self.inner.lock().await;
        Self::purge_expired_locked(&mut guard);
        guard.remove(state)
    }

    fn purge_expired_locked(map: &mut HashMap<String, PendingOAuthSession>) {
        let now = Instant::now();
        map.retain(|_, session| now.duration_since(session.created_at) < SESSION_TTL);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredOAuthAuthData {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    #[serde(default)]
    pub scopes: Vec<String>,
}

impl StoredOAuthAuthData {
    pub fn from_tokens(tokens: OAuthTokens) -> Self {
        Self {
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            expires_at: tokens.expires_at,
            scopes: tokens.scopes,
        }
    }
}

fn is_placeholder(value: &str) -> bool {
    let v = value.trim();
    v.is_empty()
        || v.eq_ignore_ascii_case("YOUR_CLIENT_ID")
        || v.eq_ignore_ascii_case("YOUR_CLIENT_SECRET")
        || v.ends_with("_PLACEHOLDER")
        || v.starts_with("your-")
}

pub fn optional_oauth_credentials(
    client_id: Option<String>,
    client_secret: Option<String>,
) -> Option<OAuthProviderCredentials> {
    let client_id = client_id?.trim().to_string();
    if is_placeholder(&client_id) {
        return None;
    }
    let client_secret = client_secret
        .map(|s| s.trim().to_string())
        .filter(|s| !is_placeholder(s));
    Some(OAuthProviderCredentials {
        client_id,
        client_secret,
    })
}

pub fn gmail_oauth_config(
    creds: &OAuthProviderCredentials,
    redirect_url: String,
) -> OAuthConfig {
    OAuthConfig {
        client_id: creds.client_id.clone(),
        client_secret: creds.client_secret.clone(),
        auth_url: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
        token_url: "https://oauth2.googleapis.com/token".to_string(),
        scopes: vec![
            "https://mail.google.com/".to_string(),
            "https://www.googleapis.com/auth/userinfo.email".to_string(),
            "https://www.googleapis.com/auth/userinfo.profile".to_string(),
        ],
        redirect_port: 0,
        redirect_url: Some(redirect_url),
        extra_auth_params: vec![
            ("access_type".to_string(), "offline".to_string()),
            ("prompt".to_string(), "consent".to_string()),
        ],
    }
}

pub fn outlook_oauth_config(
    creds: &OAuthProviderCredentials,
    redirect_url: String,
) -> OAuthConfig {
    OAuthConfig {
        client_id: creds.client_id.clone(),
        client_secret: creds.client_secret.clone(),
        auth_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize".to_string(),
        token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token".to_string(),
        scopes: vec![
            "https://graph.microsoft.com/Mail.ReadWrite".to_string(),
            "https://graph.microsoft.com/Mail.Send".to_string(),
            "https://graph.microsoft.com/User.Read".to_string(),
            "offline_access".to_string(),
        ],
        redirect_port: 0,
        redirect_url: Some(redirect_url),
        extra_auth_params: vec![],
    }
}

pub fn config_for_provider(
    provider: &str,
    google: Option<&OAuthProviderCredentials>,
    microsoft: Option<&OAuthProviderCredentials>,
    redirect_url: String,
) -> Result<OAuthConfig, String> {
    match provider.to_lowercase().as_str() {
        "gmail" => {
            let creds = google.ok_or_else(|| {
                "Gmail OAuth is not configured. Set GOOGLE_CLIENT_ID (and optionally GOOGLE_CLIENT_SECRET).".to_string()
            })?;
            Ok(gmail_oauth_config(creds, redirect_url))
        }
        "outlook" => {
            let creds = microsoft.ok_or_else(|| {
                "Outlook OAuth is not configured. Set MICROSOFT_CLIENT_ID (and optionally MICROSOFT_CLIENT_SECRET).".to_string()
            })?;
            Ok(outlook_oauth_config(creds, redirect_url))
        }
        other => Err(format!("Unknown OAuth provider: {other}")),
    }
}

pub fn provider_type(provider: &str) -> Result<ProviderType, String> {
    match provider.to_lowercase().as_str() {
        "gmail" => Ok(ProviderType::Gmail),
        "outlook" => Ok(ProviderType::Outlook),
        other => Err(format!("Unknown OAuth provider: {other}")),
    }
}

pub fn provider_slug(provider: &ProviderType) -> &'static str {
    match provider {
        ProviderType::Imap => "imap",
        ProviderType::Gmail => "gmail",
        ProviderType::Outlook => "outlook",
    }
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in left.as_bytes().iter().zip(right.as_bytes()) {
        diff |= a ^ b;
    }
    diff == 0
}

pub fn token_exchange_error_message(provider: &str, error: &OAuthError) -> String {
    let detail = match error {
        OAuthError::TokenExchange(message) => message.as_str(),
        _ => return format!("Token exchange failed: {error}"),
    };

    if provider.eq_ignore_ascii_case("outlook")
        && detail
            .to_ascii_lowercase()
            .contains("client_secret is missing")
    {
        return "Token exchange failed: Microsoft rejected this app registration as a confidential client. Configure it as a public client, or set MICROSOFT_CLIENT_SECRET.".to_string();
    }

    if provider.eq_ignore_ascii_case("gmail")
        && detail
            .to_ascii_lowercase()
            .contains("client_secret is missing")
    {
        return "Token exchange failed: Google requires a client secret for this OAuth client. Set GOOGLE_CLIENT_SECRET.".to_string();
    }

    format!("Token exchange failed: {detail}")
}

pub async fn fetch_userinfo(
    provider: &str,
    access_token: &str,
) -> Result<(String, String), String> {
    let url = match provider.to_lowercase().as_str() {
        "gmail" => "https://www.googleapis.com/oauth2/v2/userinfo",
        "outlook" => "https://graph.microsoft.com/v1.0/me",
        _ => return Err(format!("Unsupported provider: {provider}")),
    };

    let client = pebble_oauth::build_http_client(&OAuthNetworkConfig::default())
        .map_err(|e| format!("Userinfo HTTP client failed: {e}"))?;
    let resp: serde_json::Value = client
        .get(url)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("Userinfo request failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Userinfo request failed: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Userinfo parse failed: {e}"))?;

    let email = resp["email"]
        .as_str()
        .or_else(|| resp["mail"].as_str())
        .or_else(|| resp["userPrincipalName"].as_str())
        .unwrap_or("")
        .to_string();

    let name = resp["name"]
        .as_str()
        .or_else(|| resp["displayName"].as_str())
        .unwrap_or("")
        .to_string();

    debug!("Fetched userinfo from OAuth provider");
    Ok((email, name))
}

pub fn persist_oauth_tokens(
    crypto: &CryptoService,
    store: &Store,
    account_id: &str,
    tokens: &OAuthTokens,
) -> Result<(), String> {
    let stored = StoredOAuthAuthData::from_tokens(tokens.clone());
    let bytes = serde_json::to_vec(&stored).map_err(|e| e.to_string())?;
    let encrypted = crypto.encrypt(&bytes).map_err(|e| e.to_string())?;
    store
        .set_auth_data(account_id, &encrypted)
        .map_err(|e| e.to_string())
}

pub fn read_oauth_tokens(
    crypto: &CryptoService,
    store: &Store,
    account_id: &str,
) -> Result<StoredOAuthAuthData, String> {
    let encrypted = store
        .get_auth_data(account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("No OAuth auth data for account {account_id}"))?;
    let decrypted = crypto.decrypt(&encrypted).map_err(|e| e.to_string())?;
    serde_json::from_slice(&decrypted).map_err(|e| format!("Invalid OAuth auth data: {e}"))
}

pub fn build_oauth_token_refresher(
    oauth_config: OAuthConfig,
    refresh_token: Option<String>,
    fallback_access_token: String,
    crypto: Arc<CryptoService>,
    store: Arc<Store>,
    account_id: String,
) -> TokenRefresher {
    match refresh_token {
        Some(initial_rt) => Box::new(move || {
            let config = oauth_config.clone();
            let crypto = Arc::clone(&crypto);
            let store = Arc::clone(&store);
            let account_id = account_id.clone();
            let initial_rt = initial_rt.clone();
            Box::pin(async move {
                let rt = match read_oauth_tokens(&crypto, &store, &account_id) {
                    Ok(stored) => stored.refresh_token.unwrap_or(initial_rt),
                    Err(_) => initial_rt,
                };

                let manager = OAuthManager::new(config);
                let token_pair = manager
                    .refresh_token(&rt)
                    .await
                    .map_err(|e| pebble_core::PebbleError::OAuth(format!("Token refresh failed: {e}")))?;

                let tokens = OAuthTokens {
                    access_token: token_pair.access_token.clone(),
                    refresh_token: token_pair.refresh_token.clone().or(Some(rt)),
                    expires_at: token_pair.expires_at,
                    scopes: token_pair.scopes.clone(),
                };
                persist_oauth_tokens(&crypto, &store, &account_id, &tokens)
                    .map_err(pebble_core::PebbleError::OAuth)?;
                Ok((token_pair.access_token, token_pair.expires_at))
            })
        }),
        None => Box::new(move || {
            let token = fallback_access_token.clone();
            Box::pin(async move { Ok((token, None)) })
        }),
    }
}

pub fn pkce_from_session(session: &PendingOAuthSession) -> PkceState {
    PkceState::from_secrets(session.code_verifier.clone(), session.csrf_token.clone())
}

pub fn verify_csrf(session: &PendingOAuthSession, state: &str) -> bool {
    constant_time_eq(&session.csrf_token, state)
}

pub fn create_oauth_account(
    state: &AppState,
    provider: &str,
    email: String,
    display_name: String,
    tokens: OAuthTokens,
) -> Result<Account, String> {
    let provider_ty = provider_type(provider)?;
    let now = now_timestamp();
    let existing = state
        .store
        .list_accounts()
        .map_err(|e| format!("Failed to list accounts: {e}"))?;

    if existing.iter().any(|a| a.email.eq_ignore_ascii_case(&email)) {
        return Err(format!("An account for {email} already exists"));
    }

    let account = Account {
        id: new_id(),
        email,
        display_name,
        color: None,
        provider: provider_ty,
        created_at: now,
        updated_at: now,
    };

    state
        .store
        .insert_account(&account)
        .map_err(|e| format!("Failed to create account: {e}"))?;

    if let Err(e) = (|| -> Result<(), String> {
        persist_oauth_tokens(&state.crypto, &state.store, &account.id, &tokens)?;
        let slug = provider_slug(&account.provider).to_string();
        state
            .store
            .update_sync_state(&account.id, |s| {
                s.provider = Some(slug);
            })
            .map_err(|e| e.to_string())?;
        Ok(())
    })() {
        let _ = state.store.delete_account(&account.id);
        return Err(e);
    }

    Ok(account)
}

pub fn oauth_callback_redirect_base(public_url: &str) -> String {
    format!("{}/api/v1/oauth/callback", public_url.trim_end_matches('/'))
}
