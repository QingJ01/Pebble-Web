use crate::auth::AuthSession;
use crate::error::ApiError;
use crate::state::{AppStateRef, OAuthFlowRecord};
use axum::{
    extract::{Extension, Path, Query, State},
    response::Html,
    Json,
};
use pebble_core::{
    new_id, now_timestamp, Account, OAuthMailboxIdentity, OAuthTokens, ProviderType,
};
use pebble_oauth::{build_http_client, OAuthConfig, OAuthManager, OAuthNetworkConfig, PkceState};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use zeroize::Zeroize;

const MAX_OAUTH_FLOWS: usize = 1024;

const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const MICROSOFT_AUTH_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/authorize";
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
    pub account_id: Option<String>,
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

pub(crate) fn provider_config(provider: &str) -> Result<(OAuthConfig, String), ApiError> {
    resolve_provider_config(provider, |key| std::env::var(key).ok())
}

fn resolve_provider_config(
    provider: &str,
    env: impl Fn(&str) -> Option<String>,
) -> Result<(OAuthConfig, String), ApiError> {
    // Microsoft supports public clients (no secret, PKCE only); Google web clients require a secret.
    let (id_key, secret_key, secret_required, auth_url, token_url, scopes, callback_path) =
        match provider {
            "gmail" => (
                "PEBBLE_GOOGLE_CLIENT_ID",
                "PEBBLE_GOOGLE_CLIENT_SECRET",
                true,
                GOOGLE_AUTH_URL,
                GOOGLE_TOKEN_URL,
                vec!["openid", "email", "profile", "https://mail.google.com/"],
                "/api/v1/oauth/gmail/callback",
            ),
            "outlook" => (
                "PEBBLE_MICROSOFT_CLIENT_ID",
                "PEBBLE_MICROSOFT_CLIENT_SECRET",
                false,
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
    let client_id = env(id_key)
        .map(|value| value.trim().to_string())
        .unwrap_or_default();
    let client_secret = env(secret_key)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if client_id.is_empty() || (secret_required && client_secret.is_none()) {
        return Err(ApiError::NotFound(
            "OAuth provider is not configured".into(),
        ));
    }
    let public_base = env("PEBBLE_PUBLIC_BASE_URL")
        .ok_or_else(|| ApiError::Internal("PEBBLE_PUBLIC_BASE_URL is required for OAuth".into()))?;
    let base = reqwest::Url::parse(&public_base)
        .map_err(|_| ApiError::Internal("Invalid OAuth public base URL".into()))?;
    if !base.username().is_empty()
        || base.password().is_some()
        || base.query().is_some()
        || base.fragment().is_some()
    {
        return Err(ApiError::Internal(
            "OAuth public base URL must not contain user information, query, or fragment".into(),
        ));
    }
    let local_http = base.scheme() == "http"
        && matches!(base.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if base.scheme() != "https" && !local_http {
        return Err(ApiError::Internal(
            "OAuth public base URL must use HTTPS".into(),
        ));
    }
    let redirect = base
        .join(callback_path)
        .map_err(|_| ApiError::Internal("Invalid OAuth callback URL".into()))?
        .to_string();
    Ok((
        OAuthConfig {
            client_id,
            client_secret,
            auth_url: auth_url.into(),
            token_url: token_url.into(),
            scopes: scopes.into_iter().map(str::to_owned).collect(),
            redirect_port: 0,
        },
        redirect,
    ))
}

pub(crate) async fn refresh_account_tokens(
    store: Arc<pebble_store::Store>,
    crypto: Arc<pebble_crypto::CryptoService>,
    locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    account_id: String,
    provider: String,
) -> Result<pebble_core::OAuthTokens, String> {
    let lock = {
        let mut locks = locks.lock().await;
        locks
            .entry(account_id.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    };
    let _guard = lock.lock().await;
    let (encrypted, sync_state): (Option<Vec<u8>>, Option<String>) = store
        .with_read_async({
            let account_id = account_id.clone();
            let provider = provider.clone();
            move |conn| {
                Ok(conn.query_row(
                    "SELECT auth_data, sync_state FROM accounts WHERE id=?1 AND provider=?2",
                    rusqlite::params![account_id, provider],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?)
            }
        })
        .await
        .map_err(|error| format!("Failed to load account tokens: {error}"))?;
    let encrypted = encrypted.ok_or_else(|| "Account tokens are missing".to_string())?;
    let plaintext = crypto
        .decrypt_for("accounts.auth_data", &account_id, &encrypted)
        .map_err(|error| format!("Failed to decrypt account tokens: {error}"))?;
    let mut tokens: pebble_core::OAuthTokens = serde_json::from_slice(&plaintext)
        .map_err(|error| format!("Invalid account tokens: {error}"))?;
    let now = now_timestamp();
    if !tokens
        .expires_at
        .is_some_and(|expires_at| expires_at - now < 300)
    {
        return Ok(tokens);
    }
    let refresh_token = tokens
        .refresh_token
        .clone()
        .ok_or_else(|| "Account has no refresh token; reauthorization is required".to_string())?;
    let (config, redirect) =
        provider_config(&provider).map_err(|_| "OAuth provider is unavailable".to_string())?;
    let sync_state = sync_state
        .and_then(|value| serde_json::from_str::<Value>(&value).ok())
        .unwrap_or_else(|| json!({}));
    let global_proxy = crate::sync::load_global_proxy(&store, &crypto)?;
    let proxy = crate::sync::oauth_proxy(&sync_state, global_proxy.as_ref())?;
    let manager = OAuthManager::new_web(config, OAuthNetworkConfig { proxy }, redirect);
    let refreshed = manager
        .refresh_token(&refresh_token)
        .await
        .map_err(|error| format!("OAuth token refresh failed: {error}"))?;
    tokens.access_token = refreshed.access_token;
    tokens.refresh_token = retain_refresh_token(refreshed.refresh_token, tokens.refresh_token);
    tokens.expires_at = refreshed.expires_at;
    if !refreshed.scopes.is_empty() {
        tokens.scopes = refreshed.scopes;
    }
    let plaintext = serde_json::to_vec(&tokens)
        .map_err(|error| format!("Failed to serialize refreshed tokens: {error}"))?;
    let encrypted = crypto
        .encrypt_for("accounts.auth_data", &account_id, &plaintext)
        .map_err(|error| format!("Failed to encrypt refreshed tokens: {error}"))?;
    store
        .with_write_async(move |conn| {
            let changed = conn.execute(
                "UPDATE accounts SET auth_data=?1 WHERE id=?2",
                rusqlite::params![encrypted, account_id],
            )?;
            if changed != 1 {
                return Err(rusqlite::Error::QueryReturnedNoRows.into());
            }
            Ok(())
        })
        .await
        .map_err(|error| format!("Failed to persist refreshed tokens: {error}"))?;
    Ok(tokens)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthIdentityPreview {
    pub account_id: String,
    pub previous_email: String,
    pub identity: OAuthMailboxIdentity,
}

async fn inspect_account_identity(
    state: &AppStateRef,
    account_id: &str,
    expected: Option<&OAuthIdentityPreview>,
) -> Result<OAuthIdentityPreview, String> {
    let account = state
        .store
        .get_account(account_id)
        .map_err(|error| format!("Failed to load account: {error}"))?
        .ok_or_else(|| "Account not found".to_string())?;
    let provider = match account.provider {
        ProviderType::Gmail => "gmail",
        ProviderType::Outlook => "outlook",
        _ => return Err("OAuth identity repair requires a Gmail or Outlook account".into()),
    };
    refresh_account_tokens(
        state.store.clone(),
        state.crypto.clone(),
        state.oauth_refresh_locks.clone(),
        account_id.to_string(),
        provider.to_string(),
    )
    .await?;
    let lock = {
        let mut locks = state.oauth_refresh_locks.lock().await;
        locks
            .entry(account_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    };
    let _guard = lock.lock().await;
    let lookup_id = account_id.to_string();
    let (encrypted, sync_state) = state
        .store
        .with_read_async(move |conn| {
            Ok(conn.query_row(
                "SELECT auth_data, sync_state FROM accounts WHERE id=?1",
                rusqlite::params![lookup_id],
                |row| {
                    Ok((
                        row.get::<_, Option<Vec<u8>>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                    ))
                },
            )?)
        })
        .await
        .map_err(|error| format!("Failed to load account authorization: {error}"))?;
    let encrypted = encrypted.ok_or_else(|| "Account authorization is missing".to_string())?;
    let plaintext = state
        .crypto
        .decrypt_for("accounts.auth_data", account_id, &encrypted)
        .map_err(|error| format!("Failed to decrypt account authorization: {error}"))?;
    let tokens: OAuthTokens = serde_json::from_slice(&plaintext)
        .map_err(|error| format!("Account authorization is invalid: {error}"))?;
    let sync_state = sync_state
        .and_then(|value| serde_json::from_str::<Value>(&value).ok())
        .unwrap_or_else(|| json!({}));
    let global_proxy = crate::sync::load_global_proxy(&state.store, &state.crypto)?;
    let proxy = crate::sync::oauth_proxy(&sync_state, global_proxy.as_ref())?;
    let network = OAuthNetworkConfig { proxy };
    let client = build_http_client(&network)
        .map_err(|error| format!("Mailbox verification failed: {error}"))?;
    let url = if provider == "gmail" {
        "https://www.googleapis.com/oauth2/v2/userinfo"
    } else {
        "https://graph.microsoft.com/v1.0/me?$select=id,mail,userPrincipalName,displayName"
    };
    let profile: Value = client
        .get(url)
        .bearer_auth(&tokens.access_token)
        .send()
        .await
        .map_err(|_| "Mailbox verification failed".to_string())?
        .error_for_status()
        .map_err(|_| "Mailbox verification failed".to_string())?
        .json()
        .await
        .map_err(|_| "Mailbox profile was invalid".to_string())?;
    if provider == "gmail" && !gmail_email_is_verified(&profile) {
        return Err("Provider mailbox address is not verified".into());
    }
    let email = if provider == "gmail" {
        profile["email"].as_str()
    } else {
        profile["mail"].as_str()
    }
    .filter(|value| !value.trim().is_empty())
    .ok_or_else(|| "Provider could not confirm the actual mailbox address".to_string())?;
    let subject = profile["id"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "Provider did not return a stable mailbox identity".to_string())?;
    let display_name = profile["name"]
        .as_str()
        .or_else(|| profile["displayName"].as_str())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned);
    let identity = OAuthMailboxIdentity {
        subject: format!("{provider}:{subject}"),
        email: email.to_string(),
        display_name,
    };
    pebble_mail::sender::sender_mailbox(&pebble_core::EmailAddress {
        name: identity.display_name.clone(),
        address: identity.email.clone(),
    })
    .map_err(|error| format!("Provider mailbox address is invalid: {error}"))?;
    let preview = OAuthIdentityPreview {
        account_id: account_id.to_string(),
        previous_email: account.email,
        identity,
    };
    if let Some(expected) = expected {
        if expected.account_id != preview.account_id
            || expected.previous_email != preview.previous_email
            || expected.identity.subject != preview.identity.subject
            || expected.identity.email != preview.identity.email
        {
            return Err(
                "Mailbox details changed since the preview. Verify the mailbox again.".into(),
            );
        }
        state
            .store
            .apply_verified_oauth_identity(
                account_id,
                &preview.previous_email,
                &preview.identity,
                true,
            )
            .map_err(|error| format!("Could not apply verified mailbox identity: {error}"))?;
    }
    Ok(preview)
}

pub async fn preview_account_identity(
    state: &AppStateRef,
    account_id: &str,
) -> Result<OAuthIdentityPreview, ApiError> {
    inspect_account_identity(state, account_id, None)
        .await
        .map_err(ApiError::BadRequest)
}

pub async fn apply_account_identity(
    state: &AppStateRef,
    preview: OAuthIdentityPreview,
) -> Result<Account, ApiError> {
    inspect_account_identity(state, &preview.account_id, Some(&preview))
        .await
        .map_err(ApiError::BadRequest)?;
    state
        .store
        .get_account(&preview.account_id)
        .map_err(|error| ApiError::Internal(format!("Failed to load updated account: {error}")))?
        .ok_or_else(|| ApiError::NotFound("Account not found".into()))
}

fn retain_refresh_token(new_token: Option<String>, old_token: Option<String>) -> Option<String> {
    new_token.or(old_token)
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
    Extension(session): Extension<AuthSession>,
    Json(options): Json<StartRequest>,
) -> Result<Json<StartResponse>, ApiError> {
    let _email_hint = options.email.as_deref();
    let provider = provider.to_ascii_lowercase();
    if session.expires_at <= now_timestamp() {
        return Err(ApiError::Unauthorized("Authentication expired".into()));
    }
    pebble_store::accounts::validate_sync_range_days(options.sync_range_days)
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;
    if options.proxy_host.is_some() != options.proxy_port.is_some() {
        return Err(ApiError::BadRequest(
            "Proxy host and port must be supplied together".into(),
        ));
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
    let network = OAuthNetworkConfig {
        proxy: proxy.clone(),
    };
    let manager = OAuthManager::new_web(config, network, redirect);
    let (authorization_url, pkce) = manager
        .start_auth()
        .await
        .map_err(|error| ApiError::Internal(format!("Could not start OAuth: {error}")))?;
    let (verifier, csrf) = pkce.into_secrets();
    let flow_id = new_id();
    let account_label = options
        .account_label
        .as_deref()
        .map(|label| pebble_store::accounts::normalize_account_label(Some(label)))
        .transpose()
        .map_err(|error| ApiError::BadRequest(error.to_string()))?
        .flatten();
    if let Some(account_id) = options.account_id.as_deref() {
        let account_id = account_id.to_string();
        let account_provider = state
            .store
            .with_read_async(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT provider FROM accounts WHERE id=?1",
                        rusqlite::params![account_id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?)
            })
            .await
            .map_err(|error| {
                ApiError::Internal(format!("Failed to validate OAuth account: {error}"))
            })?
            .ok_or_else(|| ApiError::NotFound("Account not found".into()))?;
        if account_provider != provider {
            return Err(ApiError::BadRequest(
                "OAuth provider does not match account".into(),
            ));
        }
    }
    let expires_at = (now_timestamp() + 600).min(session.expires_at);
    if expires_at <= now_timestamp() {
        return Err(ApiError::Unauthorized("Authentication expired".into()));
    }
    let mut flows = state.oauth_flows.lock().await;
    let now = now_timestamp();
    flows.retain(|_, flow| {
        if flow.expires_at < now {
            flow.state.zeroize();
            flow.verifier.zeroize();
            false
        } else {
            true
        }
    });
    if flows.len() >= MAX_OAUTH_FLOWS {
        return Err(ApiError::BadRequest("Too many active OAuth flows".into()));
    }
    flows.insert(
        flow_id.clone(),
        OAuthFlowRecord {
            provider,
            owner_token_digest: session.token_digest,
            state: csrf,
            verifier,
            expires_at,
            account_id: options.account_id,
            account_label,
            display_name_hint: options.display_name,
            sync_range_days: options.sync_range_days,
            proxy,
            consumed: false,
            status: "pending".into(),
            account: None,
            error: None,
        },
    );
    Ok(Json(StartResponse {
        authorization_url,
        flow_id,
    }))
}

pub async fn status(
    State(state): State<AppStateRef>,
    Path((provider, flow_id)): Path<(String, String)>,
    Extension(session): Extension<AuthSession>,
) -> Result<Json<StatusResponse>, ApiError> {
    let flows = state.oauth_flows.lock().await;
    let flow = flows
        .get(&flow_id)
        .ok_or_else(|| ApiError::NotFound("OAuth flow not found".into()))?;
    if session.expires_at <= now_timestamp()
        || !status_visible(flow, &provider, &session.token_digest, now_timestamp())
    {
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
    left.len() == right.len()
        && left
            .as_bytes()
            .iter()
            .zip(right.as_bytes())
            .fold(0u8, |diff, (a, b)| diff | (a ^ b))
            == 0
}

fn status_visible(flow: &OAuthFlowRecord, provider: &str, owner_digest: &str, now: i64) -> bool {
    flow.provider == provider.to_ascii_lowercase()
        && flow.expires_at >= now
        && matches_state(&flow.owner_token_digest, owner_digest)
}

fn gmail_email_is_verified(profile: &Value) -> bool {
    profile["verified_email"].as_bool() == Some(true)
}

struct OAuthCallbackClaim {
    flow_id: String,
    verifier: String,
    account_id: Option<String>,
    account_label: Option<String>,
    display_name_hint: Option<String>,
    sync_range_days: Option<u32>,
    proxy: Option<pebble_core::HttpProxyConfig>,
}

fn claim_callback_flow(
    flows: &mut HashMap<String, OAuthFlowRecord>,
    provider: &str,
    supplied_state: &str,
    now: i64,
) -> Result<OAuthCallbackClaim, String> {
    let candidate = flows
        .iter()
        .find(|(_, flow)| {
            flow.provider == provider
                && flow.expires_at >= now
                && !flow.consumed
                && matches_state(&flow.state, supplied_state)
        })
        .map(|(id, _)| id.clone())
        .ok_or_else(|| "Authorization expired or was already used".to_string())?;
    let flow = flows.get_mut(&candidate).expect("candidate exists");
    flow.consumed = true;
    let claim = OAuthCallbackClaim {
        flow_id: candidate,
        verifier: std::mem::take(&mut flow.verifier),
        account_id: flow.account_id.clone(),
        account_label: flow.account_label.clone(),
        display_name_hint: flow.display_name_hint.clone(),
        sync_range_days: flow.sync_range_days,
        proxy: flow.proxy.clone(),
    };
    flow.state.zeroize();
    Ok(claim)
}

struct ExistingOAuthAccount {
    id: String,
    email: String,
    display_name: String,
    color: Option<String>,
    account_label: Option<String>,
    provider_display_name: Option<String>,
    oauth_subject: Option<String>,
    auth_data: Option<Vec<u8>>,
    sync_state: Option<String>,
    created_at: i64,
}

async fn finish_claimed_callback(
    state: &AppStateRef,
    provider: &str,
    supplied_state: &str,
    code: &str,
    claim: &mut OAuthCallbackClaim,
) -> Result<Account, String> {
    let (config, redirect) =
        provider_config(provider).map_err(|_| "OAuth provider is unavailable".to_string())?;
    let network = OAuthNetworkConfig {
        proxy: claim.proxy.clone(),
    };
    let manager = OAuthManager::new_web(config, network.clone(), redirect);
    let pkce = PkceState::from_secrets(claim.verifier.clone(), supplied_state.to_owned());
    claim.verifier.zeroize();
    let tokens = manager
        .complete_auth(code, pkce)
        .await
        .map_err(|_| "Authorization could not be completed".to_string())?;
    let profile_url = if provider == "gmail" {
        "https://www.googleapis.com/oauth2/v2/userinfo"
    } else {
        "https://graph.microsoft.com/v1.0/me?$select=id,mail,displayName"
    };
    let client =
        build_http_client(&network).map_err(|_| "Mailbox verification failed".to_string())?;
    let profile: Value = client
        .get(profile_url)
        .bearer_auth(&tokens.access_token)
        .send()
        .await
        .map_err(|_| "Mailbox verification failed".to_string())?
        .error_for_status()
        .map_err(|_| "Mailbox verification failed".to_string())?
        .json()
        .await
        .map_err(|_| "Mailbox verification failed".to_string())?;
    let email = if provider == "gmail" {
        profile["email"].as_str()
    } else {
        profile["mail"].as_str()
    }
    .filter(|value| !value.trim().is_empty())
    .ok_or_else(|| "Provider did not confirm a mailbox address".to_string())?;
    let subject = profile["id"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "Provider did not confirm a stable mailbox identity".to_string())?;
    if provider == "gmail" && !gmail_email_is_verified(&profile) {
        return Err("Provider mailbox address is not verified".into());
    }
    let provider_display_name = profile["name"]
        .as_str()
        .or_else(|| profile["displayName"].as_str())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_default();
    let subject_value = format!("{provider}:{subject}");
    let provider_value = provider.to_string();
    let subject_lock_key = format!("identity:{subject_value}");
    let subject_lock = {
        let mut locks = state.oauth_refresh_locks.lock().await;
        locks
            .entry(subject_lock_key)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    };
    let _subject_guard = subject_lock.lock().await;
    let email_lock_key = format!("email:{}:{}", provider, email.to_ascii_lowercase());
    let email_lock = {
        let mut locks = state.oauth_refresh_locks.lock().await;
        locks
            .entry(email_lock_key)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    };
    let _email_guard = email_lock.lock().await;
    let lookup_id = claim.account_id.clone();
    let lookup_subject = subject_value.clone();
    let lookup_provider = provider_value.clone();
    let existing = state
        .store
        .with_read_async(move |conn| {
            let row =
                if let Some(account_id) = lookup_id {
                    conn.query_row(
                "SELECT id, email, display_name, color, account_label, provider_display_name,
                        oauth_subject, auth_data, sync_state, created_at, provider
                 FROM accounts WHERE id=?1",
                rusqlite::params![account_id],
                |row| {
                    Ok((
                        ExistingOAuthAccount {
                            id: row.get(0)?,
                            email: row.get(1)?,
                            display_name: row.get(2)?,
                            color: row.get(3)?,
                            account_label: row.get(4)?,
                            provider_display_name: row.get(5)?,
                            oauth_subject: row.get(6)?,
                            auth_data: row.get(7)?,
                            sync_state: row.get(8)?,
                            created_at: row.get(9)?,
                        },
                        row.get::<_, String>(10)?,
                    ))
                },
            ).optional()?
                } else {
                    conn.query_row(
                "SELECT id, email, display_name, color, account_label, provider_display_name,
                        oauth_subject, auth_data, sync_state, created_at, provider
                 FROM accounts WHERE provider=?1 AND oauth_subject=?2",
                rusqlite::params![lookup_provider, lookup_subject],
                |row| {
                    Ok((
                        ExistingOAuthAccount {
                            id: row.get(0)?,
                            email: row.get(1)?,
                            display_name: row.get(2)?,
                            color: row.get(3)?,
                            account_label: row.get(4)?,
                            provider_display_name: row.get(5)?,
                            oauth_subject: row.get(6)?,
                            auth_data: row.get(7)?,
                            sync_state: row.get(8)?,
                            created_at: row.get(9)?,
                        },
                        row.get::<_, String>(10)?,
                    ))
                },
            ).optional()?
                };
            Ok(row)
        })
        .await
        .map_err(|error| format!("Could not check mailbox identity: {error}"))?;

    if claim.account_id.is_some() && existing.is_none() {
        return Err("The requested account no longer exists".into());
    }
    if let Some((existing, existing_provider)) = &existing {
        if existing_provider != provider
            || existing.oauth_subject.as_deref() != Some(&subject_value)
        {
            return Err("Mailbox identity does not match the existing account".into());
        }
        if !existing.email.eq_ignore_ascii_case(email) {
            return Err("The verified mailbox address differs from the existing account".into());
        }
    } else {
        let colliding_email = state
            .store
            .with_read_async({
                let provider_value = provider_value.clone();
                let email = email.to_string();
                move |conn| {
                    Ok(conn
                        .query_row(
                            "SELECT 1 FROM accounts WHERE provider=?1 AND lower(email)=lower(?2) LIMIT 1",
                            rusqlite::params![provider_value, email],
                            |_| Ok(()),
                        )
                        .optional()?)
                }
            })
            .await
            .map_err(|error| format!("Could not verify mailbox identity: {error}"))?;
        if colliding_email.is_some() {
            return Err("An account with this email already exists; verify its provider identity before reauthorization".into());
        }
    }

    let account_id = existing
        .as_ref()
        .map(|(account, _)| account.id.clone())
        .unwrap_or_else(new_id);
    let account_lock = {
        let mut locks = state.oauth_refresh_locks.lock().await;
        locks
            .entry(account_id.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    };
    let _account_guard = account_lock.lock().await;
    let was_running = state.sync_manager.is_account_running(&account_id).await;
    if was_running {
        state.sync_manager.stop_account_sync(&account_id).await;
    }
    let now = now_timestamp();
    let display_name = existing
        .as_ref()
        .map(|(account, _)| account.display_name.clone())
        .or_else(|| claim.display_name_hint.clone())
        .filter(|name| !name.trim().is_empty())
        .or_else(|| (!provider_display_name.is_empty()).then(|| provider_display_name.clone()))
        .unwrap_or_else(|| email.to_string());
    let account_label = claim.account_label.clone().or_else(|| {
        existing
            .as_ref()
            .and_then(|(account, _)| account.account_label.clone())
    });
    let provider_display_name_value = (!provider_display_name.is_empty())
        .then_some(provider_display_name.clone())
        .or_else(|| {
            existing
                .as_ref()
                .and_then(|(account, _)| account.provider_display_name.clone())
        });
    let color = existing
        .as_ref()
        .and_then(|(account, _)| account.color.clone());
    let account = Account {
        account_label: account_label.clone(),
        provider_display_name: provider_display_name_value.clone(),
        id: account_id.clone(),
        email: email.to_string(),
        display_name,
        color: color.clone(),
        provider: if provider == "gmail" {
            ProviderType::Gmail
        } else {
            ProviderType::Outlook
        },
        created_at: existing
            .as_ref()
            .map(|(account, _)| account.created_at)
            .unwrap_or(now),
        updated_at: now,
    };
    let mut token_data = OAuthTokens {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        expires_at: tokens.expires_at,
        scopes: tokens.scopes,
    };
    if token_data.refresh_token.is_none() {
        if let Some(previous) = existing
            .as_ref()
            .and_then(|(account, _)| account.auth_data.as_ref())
        {
            let plaintext = state
                .crypto
                .decrypt_for("accounts.auth_data", &account_id, previous)
                .map_err(|_| "Could not read existing mailbox authorization".to_string())?;
            let previous: OAuthTokens = serde_json::from_slice(&plaintext)
                .map_err(|_| "Existing mailbox authorization is invalid".to_string())?;
            token_data.refresh_token =
                retain_refresh_token(token_data.refresh_token.take(), previous.refresh_token);
        }
    }
    let plaintext =
        serde_json::to_vec(&token_data).map_err(|_| "Could not save authorization".to_string())?;
    let encrypted = state
        .crypto
        .encrypt_for("accounts.auth_data", &account_id, &plaintext)
        .map_err(|_| "Could not secure authorization".to_string())?;
    let cutoff = pebble_store::accounts::sync_range_cutoff(claim.sync_range_days, now);
    let account_label = account.account_label.clone();
    let provider_display_name = account.provider_display_name.clone();
    let email_value = account.email.clone();
    let display_value = account.display_name.clone();
    let color = account.color.clone();
    let provider_value = provider.to_string();
    let subject_value = format!("{provider}:{subject}");
    let mut sync_state = existing
        .as_ref()
        .and_then(|(account, _)| account.sync_state.as_deref())
        .map(serde_json::from_str::<Value>)
        .transpose()
        .map_err(|_| "Existing account sync settings are invalid".to_string())?
        .unwrap_or_else(|| json!({}));
    if let Some(sync_state) = sync_state.as_object_mut() {
        sync_state.insert("provider".into(), json!(provider));
        if let Some(days) = claim.sync_range_days {
            sync_state.insert("sync_range_days".into(), json!(days));
            sync_state.insert("sync_since".into(), json!(cutoff));
        }
        if claim.proxy.is_some() {
            sync_state.insert("proxy".into(), json!(claim.proxy));
            sync_state.insert("proxy_mode".into(), json!("custom"));
        }
    }
    let sync_state = sync_state.to_string();
    let existing_id = existing.as_ref().map(|(account, _)| account.id.clone());
    let write_account_id = account_id.clone();
    state.store.with_write_async(move |conn| {
        if existing_id.is_some() {
            conn.execute(
                "UPDATE accounts SET email=?1, display_name=?2, color=?3, updated_at=?4,
                        account_label=?5, provider_display_name=?6, oauth_subject=?7,
                        auth_data=?8, sync_state=?9 WHERE id=?10",
                rusqlite::params![
                    email_value, display_value, color, now, account_label, provider_display_name,
                    subject_value, encrypted, sync_state, write_account_id
                ],
            )?;
        } else {
            conn.execute(
                "INSERT INTO accounts (id, email, display_name, color, provider, created_at, updated_at, account_label, provider_display_name, oauth_subject, auth_data, sync_state)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                rusqlite::params![
                    write_account_id, email_value, display_value, color, provider_value, now, now,
                    account_label, provider_display_name, subject_value, encrypted, sync_state
                ],
            )?;
        }
        Ok(())
    }).await.map_err(|_| "Could not save the authorized account".to_string())?;
    if was_running || existing.is_none() {
        if let Err(error) = state.sync_manager.start_account_sync(&account_id).await {
            tracing::error!("Authorized account {account_id} could not start sync: {error}");
        }
    }
    Ok(account)
}

async fn finish_callback(
    state: &AppStateRef,
    provider: &str,
    query: CallbackQuery,
) -> Result<(), String> {
    let supplied_state = query
        .state
        .as_deref()
        .ok_or_else(|| "Invalid authorization response".to_string())?
        .to_owned();
    let mut claim = {
        let mut flows = state.oauth_flows.lock().await;
        claim_callback_flow(&mut flows, provider, &supplied_state, now_timestamp())?
    };

    let result = if query.error.is_some() {
        Err("Authorization was not completed".to_string())
    } else if let Some(code) = query.code.as_deref() {
        finish_claimed_callback(state, provider, &supplied_state, code, &mut claim)
            .await
            .map(|account| {
                let account_id = account.id.clone();
                (account_id, account)
            })
    } else {
        Err("Authorization response was incomplete".to_string())
    };

    let mut flows = state.oauth_flows.lock().await;
    if let Some(flow) = flows.get_mut(&claim.flow_id) {
        flow.verifier.zeroize();
        flow.state.zeroize();
        match result {
            Ok((_account_id, account)) => {
                flow.status = "completed".into();
                flow.account = Some(account);
                flow.error = None;
                return Ok(());
            }
            Err(_) => {
                flow.status = "error".into();
                flow.error = Some("Authorization could not be completed".into());
            }
        }
    }
    Err("Authorization could not be completed".into())
}

pub async fn callback(
    State(state): State<AppStateRef>,
    Path(provider): Path<String>,
    Query(query): Query<CallbackQuery>,
) -> Html<&'static str> {
    let provider = provider.to_ascii_lowercase();
    match finish_callback(&state, &provider, query).await {
        Ok(()) => safe_callback_page(),
        Err(error) => {
            tracing::warn!("OAuth callback failed: {error}");
            safe_callback_page()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_from(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |key| map.get(key).cloned()
    }

    #[test]
    fn outlook_public_client_needs_only_client_id() {
        let env = env_from(&[
            ("PEBBLE_MICROSOFT_CLIENT_ID", " outlook-client "),
            ("PEBBLE_PUBLIC_BASE_URL", "https://mail.example.com"),
        ]);
        let (config, redirect) = resolve_provider_config("outlook", env).unwrap();
        assert_eq!(config.client_id, "outlook-client");
        assert!(config.client_secret.is_none());
        assert_eq!(
            redirect,
            "https://mail.example.com/api/v1/oauth/outlook/callback"
        );
    }

    #[test]
    fn outlook_blank_secret_is_treated_as_public_client() {
        let env = env_from(&[
            ("PEBBLE_MICROSOFT_CLIENT_ID", "outlook-client"),
            ("PEBBLE_MICROSOFT_CLIENT_SECRET", "   "),
            ("PEBBLE_PUBLIC_BASE_URL", "https://mail.example.com"),
        ]);
        let (config, _) = resolve_provider_config("outlook", env).unwrap();
        assert!(config.client_secret.is_none());
    }

    #[test]
    fn outlook_confidential_client_keeps_secret() {
        let env = env_from(&[
            ("PEBBLE_MICROSOFT_CLIENT_ID", "outlook-client"),
            ("PEBBLE_MICROSOFT_CLIENT_SECRET", "outlook-secret"),
            ("PEBBLE_PUBLIC_BASE_URL", "https://mail.example.com"),
        ]);
        let (config, _) = resolve_provider_config("outlook", env).unwrap();
        assert_eq!(config.client_secret.as_deref(), Some("outlook-secret"));
    }

    #[test]
    fn gmail_still_requires_client_secret() {
        let env = env_from(&[
            ("PEBBLE_GOOGLE_CLIENT_ID", "google-client"),
            ("PEBBLE_PUBLIC_BASE_URL", "https://mail.example.com"),
        ]);
        assert!(matches!(
            resolve_provider_config("gmail", env),
            Err(ApiError::NotFound(_))
        ));
    }

    #[test]
    fn missing_client_id_is_not_configured() {
        let env = env_from(&[
            ("PEBBLE_MICROSOFT_CLIENT_SECRET", "outlook-secret"),
            ("PEBBLE_PUBLIC_BASE_URL", "https://mail.example.com"),
        ]);
        assert!(matches!(
            resolve_provider_config("outlook", env),
            Err(ApiError::NotFound(_))
        ));
    }

    fn test_flow(provider: &str, state: &str, expires_at: i64) -> OAuthFlowRecord {
        OAuthFlowRecord {
            provider: provider.to_string(),
            owner_token_digest: "owner-digest".to_string(),
            state: state.to_string(),
            verifier: "pkce-verifier-secret".to_string(),
            expires_at,
            account_id: None,
            account_label: None,
            display_name_hint: None,
            sync_range_days: None,
            proxy: None,
            consumed: false,
            status: "pending".to_string(),
            account: None,
            error: None,
        }
    }

    #[test]
    fn oauth_status_requires_matching_provider_owner_and_unexpired_flow() {
        let flow = test_flow("gmail", "state-secret", 120);
        assert!(status_visible(&flow, "gmail", "owner-digest", 100));
        assert!(!status_visible(&flow, "outlook", "owner-digest", 100));
        assert!(!status_visible(&flow, "gmail", "other-digest", 100));
        assert!(!status_visible(&flow, "gmail", "owner-digest", 121));
    }

    #[test]
    fn callback_claim_is_single_use_while_pending_and_after_success() {
        let mut flows = HashMap::from([(
            "flow-id".to_string(),
            test_flow("gmail", "state-secret", 120),
        )]);
        let claim = claim_callback_flow(&mut flows, "gmail", "state-secret", 100).unwrap();
        assert_eq!(claim.flow_id, "flow-id");
        assert_eq!(claim.verifier, "pkce-verifier-secret");
        let flow = flows.get("flow-id").unwrap();
        assert!(flow.consumed);
        assert!(flow.verifier.is_empty());
        assert!(flow.state.is_empty());
        assert_eq!(flow.status, "pending");
        assert!(claim_callback_flow(&mut flows, "gmail", "state-secret", 100).is_err());
        assert_eq!(flows["flow-id"].status, "pending");
        flows.get_mut("flow-id").unwrap().status = "completed".into();
        assert!(claim_callback_flow(&mut flows, "gmail", "state-secret", 100).is_err());
        assert_eq!(flows["flow-id"].status, "completed");
    }

    #[test]
    fn callback_claim_rejects_wrong_provider_and_expired_state() {
        let mut flows = HashMap::from([(
            "flow-id".to_string(),
            test_flow("gmail", "state-secret", 100),
        )]);
        assert!(claim_callback_flow(&mut flows, "outlook", "state-secret", 50).is_err());
        assert!(claim_callback_flow(&mut flows, "gmail", "state-secret", 101).is_err());
        assert!(!flows["flow-id"].consumed);
    }

    #[test]
    fn gmail_requires_positive_verified_email_signal() {
        assert!(!gmail_email_is_verified(&json!({"verified_email": false})));
        assert!(!gmail_email_is_verified(&json!({})));
        assert!(gmail_email_is_verified(&json!({"verified_email": true})));
    }

    #[test]
    fn refresh_rotation_without_new_refresh_token_keeps_the_previous_token() {
        assert_eq!(
            retain_refresh_token(None, Some("old-refresh-token".into())),
            Some("old-refresh-token".into())
        );
        assert_eq!(
            retain_refresh_token(
                Some("new-refresh-token".into()),
                Some("old-refresh-token".into())
            ),
            Some("new-refresh-token".into())
        );
    }

    #[test]
    fn oauth_status_response_never_serializes_flow_or_token_secrets() {
        let response = StatusResponse {
            status: "pending".into(),
            account: None,
            error: None,
        };
        let value = serde_json::to_string(&response).unwrap();
        for secret_name in ["state", "verifier", "access_token", "refresh_token"] {
            assert!(!value.contains(secret_name));
        }
    }
}
