use crate::credentials::{
    encrypt_credentials, AccountCredentials, ImapCredentials, Pop3Credentials, SmtpCredentials,
};
use crate::error::ApiError;
use crate::state::AppStateRef;
use axum::{
    extract::{Path, State},
    Json,
};
use pebble_core::{new_id, now_timestamp, Account, ProviderType};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct AccountResponse {
    pub account_label: Option<String>,
    pub provider_display_name: Option<String>,
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub color: Option<String>,
    pub provider: String,
    pub created_at: i64,
    pub updated_at: i64,
}

impl From<Account> for AccountResponse {
    fn from(a: Account) -> Self {
        let provider = match a.provider {
            ProviderType::Imap => "imap",
            ProviderType::Pop3 => "pop3",
            ProviderType::Gmail => "gmail",
            ProviderType::Outlook => "outlook",
        };
        Self {
            account_label: a.account_label,
            provider_display_name: a.provider_display_name,
            id: a.id,
            email: a.email,
            display_name: a.display_name,
            color: a.color,
            provider: provider.to_string(),
            created_at: a.created_at,
            updated_at: a.updated_at,
        }
    }
}

#[derive(Deserialize)]
pub struct CreateAccountRequest {
    pub email: String,
    pub display_name: String,
    pub color: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub imap_host: Option<String>,
    #[serde(default)]
    pub imap_port: Option<u16>,
    #[serde(default)]
    pub pop3_host: Option<String>,
    #[serde(default)]
    pub pop3_port: Option<u16>,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub username: String,
    pub password: String,
    pub imap_security: Option<String>,
    pub smtp_security: Option<String>,
    pub pop3_security: Option<String>,
    #[serde(default)]
    pub accept_invalid_certs: bool,
    #[serde(default)]
    pub allow_plaintext: bool,
    pub proxy_host: Option<String>,
    pub proxy_port: Option<u16>,
    pub sync_range_days: Option<u32>,
    pub account_label: Option<String>,
}

impl CreateAccountRequest {
    fn into_credentials(&self) -> Result<(ProviderType, AccountCredentials), ApiError> {
        let provider = match self
            .provider
            .as_deref()
            .unwrap_or("imap")
            .to_ascii_lowercase()
            .as_str()
        {
            "imap" => ProviderType::Imap,
            "pop3" => ProviderType::Pop3,
            _ => return Err(ApiError::BadRequest("Unsupported account provider".into())),
        };
        let username = if self.username.trim().is_empty() {
            self.email.trim().to_string()
        } else {
            self.username.trim().to_string()
        };
        let imap_security = self
            .imap_security
            .clone()
            .unwrap_or_else(|| "tls".to_string());
        let pop3_security = self
            .pop3_security
            .clone()
            .unwrap_or_else(|| "tls".to_string());
        let smtp_security = self
            .smtp_security
            .clone()
            .unwrap_or_else(|| "tls".to_string());
        if self.smtp_host.trim().is_empty() {
            return Err(ApiError::BadRequest("SMTP host is required".into()));
        }
        if self.smtp_port == 0 {
            return Err(ApiError::BadRequest(
                "SMTP port must be greater than zero".into(),
            ));
        }
        connection_security(&smtp_security, self.allow_plaintext)?;
        let smtp = SmtpCredentials {
            host: self.smtp_host.clone(),
            port: self.smtp_port,
            username: username.clone(),
            password: self.password.clone(),
            security: smtp_security,
            accept_invalid_certs: self.accept_invalid_certs,
            allow_plaintext: self.allow_plaintext,
        };
        let creds = match provider {
            ProviderType::Imap => {
                connection_security(&imap_security, self.allow_plaintext)?;
                let port = self.imap_port.unwrap_or(993);
                if port == 0 {
                    return Err(ApiError::BadRequest(
                        "IMAP port must be greater than zero".into(),
                    ));
                }
                AccountCredentials::Imap {
                    imap: ImapCredentials {
                        host: self
                            .imap_host
                            .as_deref()
                            .filter(|host| !host.trim().is_empty())
                            .map(str::to_owned)
                            .ok_or_else(|| ApiError::BadRequest("IMAP host is required".into()))?,
                        port,
                        username: username.clone(),
                        password: self.password.clone(),
                        security: imap_security,
                        accept_invalid_certs: self.accept_invalid_certs,
                        allow_plaintext: self.allow_plaintext,
                    },
                    smtp,
                }
            }
            ProviderType::Pop3 => {
                connection_security(&pop3_security, self.allow_plaintext)?;
                let port = self.pop3_port.or(self.imap_port).unwrap_or(995);
                if port == 0 {
                    return Err(ApiError::BadRequest(
                        "POP3 port must be greater than zero".into(),
                    ));
                }
                AccountCredentials::Pop3 {
                    pop3: Pop3Credentials {
                        host: self
                            .pop3_host
                            .as_deref()
                            .or(self.imap_host.as_deref())
                            .filter(|host| !host.trim().is_empty())
                            .map(str::to_owned)
                            .ok_or_else(|| ApiError::BadRequest("POP3 host is required".into()))?,
                        port,
                        username,
                        password: smtp.password.clone(),
                        security: pop3_security,
                        accept_invalid_certs: self.accept_invalid_certs,
                        allow_plaintext: self.allow_plaintext,
                    },
                    smtp,
                }
            }
            ProviderType::Gmail | ProviderType::Outlook => {
                return Err(ApiError::BadRequest(
                    "OAuth accounts cannot use password credentials".into(),
                ));
            }
        };
        Ok((provider, creds))
    }
}

pub async fn list_accounts(
    State(state): State<AppStateRef>,
) -> Result<Json<Vec<AccountResponse>>, ApiError> {
    let store = state.store.clone();
    let accounts = store
        .with_read_async(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, email, display_name, color, provider, created_at, updated_at, account_label, provider_display_name
                 FROM accounts ORDER BY created_at ASC",
            )?;
            let rows = stmt.query_map([], |row| {
                let provider_str: String = row.get(4)?;
                let provider = match provider_str.as_str() {
                    "gmail" => ProviderType::Gmail,
                    "outlook" => ProviderType::Outlook,
                    "pop3" => ProviderType::Pop3,
                    _ => ProviderType::Imap,
                };
                Ok(Account {
                    account_label: row.get(7)?,
                    provider_display_name: row.get(8)?,
                    id: row.get(0)?,
                    email: row.get(1)?,
                    display_name: row.get(2)?,
                    color: row.get(3)?,
                    provider,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            })?;
            let mut accounts = Vec::new();
            for row in rows {
                accounts.push(row?);
            }
            Ok(accounts)
        })
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to list accounts: {e}")))?;

    let response: Vec<AccountResponse> = accounts.into_iter().map(AccountResponse::from).collect();
    Ok(Json(response))
}

pub async fn create_account(
    State(state): State<AppStateRef>,
    Json(body): Json<CreateAccountRequest>,
) -> Result<Json<AccountResponse>, ApiError> {
    let (provider, credentials) = body.into_credentials()?;
    pebble_store::accounts::validate_sync_range_days(body.sync_range_days)
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;
    if provider == ProviderType::Pop3 && body.sync_range_days.is_some() {
        return Err(ApiError::BadRequest(
            "POP3 accounts do not support bounded sync ranges".into(),
        ));
    }
    if body.proxy_host.is_some() != body.proxy_port.is_some() {
        return Err(ApiError::BadRequest(
            "Proxy host and port must be supplied together".into(),
        ));
    }
    let proxy = match (body.proxy_host.as_deref(), body.proxy_port) {
        (Some(host), Some(port)) if !host.trim().is_empty() && port != 0 => {
            let proxy = pebble_core::HttpProxyConfig {
                host: host.trim().to_string(),
                port,
            };
            proxy.validate().map_err(ApiError::BadRequest)?;
            Some(proxy)
        }
        (Some(_), Some(_)) => {
            return Err(ApiError::BadRequest(
                "Proxy host and port are invalid".into(),
            ));
        }
        (None, None) => None,
        _ => unreachable!(),
    };

    let encrypted = encrypt_credentials(&state.crypto, &credentials)
        .map_err(|e| ApiError::Internal(format!("Encryption failed: {e}")))?;

    let now = now_timestamp();
    let email = body.email.trim().to_string();
    if email.is_empty() {
        return Err(ApiError::BadRequest("Account email is required".into()));
    }
    let sync_since = pebble_store::accounts::sync_range_cutoff(body.sync_range_days, now);
    let mut sync_state = serde_json::json!({
        "credentials": encrypted,
        "sync_range_days": body.sync_range_days,
        "sync_since": sync_since,
        "proxy_mode": if proxy.is_some() { "custom" } else { "inherit" },
    });
    if let Some(proxy) = proxy {
        sync_state["proxy"] = serde_json::to_value(proxy)
            .map_err(|error| ApiError::Internal(format!("Failed to encode proxy: {error}")))?;
    }
    let account = Account {
        account_label: pebble_store::accounts::normalize_account_label(
            body.account_label.as_deref(),
        )
        .map_err(|error| ApiError::BadRequest(error.to_string()))?,
        provider_display_name: None,
        id: new_id(),
        email,
        display_name: body.display_name.trim().to_string(),
        color: body.color.clone(),
        provider,
        created_at: now,
        updated_at: now,
    };

    let account_clone = account.clone();
    let store = state.store.clone();
    let serialized_sync_state = sync_state.to_string();
    store
        .with_write_async(move |conn| {
            conn.execute(
                "INSERT INTO accounts (id, email, display_name, color, provider, created_at, updated_at, account_label, sync_state)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                rusqlite::params![
                    account_clone.id,
                    account_clone.email,
                    account_clone.display_name,
                    account_clone.color.as_deref(),
                    match account_clone.provider {
                        ProviderType::Imap => "imap",
                        ProviderType::Pop3 => "pop3",
                        ProviderType::Gmail => "gmail",
                        ProviderType::Outlook => "outlook",
                    },
                    account_clone.created_at,
                    account_clone.updated_at,
                    account_clone.account_label.as_deref(),
                    serialized_sync_state,
                ],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to create account: {e}")))?;

    // Auto-start sync worker for the new account
    let account_id = account.id.clone();
    let sync_manager = state.sync_manager.clone();
    tokio::spawn(async move {
        if let Err(e) = sync_manager.start_account_sync(&account_id).await {
            tracing::warn!("Failed to auto-start sync for new account {account_id}: {e}");
        }
    });

    Ok(Json(AccountResponse::from(account)))
}

pub async fn delete_account(
    State(state): State<AppStateRef>,
    Path(account_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Stop sync worker for this account
    state.sync_manager.stop_account_sync(&account_id).await;

    let store = state.store.clone();
    store
        .with_write_async(move |conn| {
            let tx = conn.unchecked_transaction()?;
            // Remove messages associated with this account
            tx.execute(
                "DELETE FROM message_folders WHERE message_id IN (SELECT id FROM messages WHERE account_id = ?1)",
                rusqlite::params![account_id],
            )?;
            tx.execute(
                "DELETE FROM messages WHERE account_id = ?1",
                rusqlite::params![account_id],
            )?;
            // Remove folders for this account
            tx.execute(
                "DELETE FROM folders WHERE account_id = ?1",
                rusqlite::params![account_id],
            )?;
            // Remove the account itself
            tx.execute(
                "DELETE FROM accounts WHERE id = ?1",
                rusqlite::params![account_id],
            )?;
            tx.commit()?;
            Ok(())
        })
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to delete account: {e}")))?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAccountRequest {
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub color: Option<String>,
    pub account_color: Option<String>,
    pub account_label: Option<String>,
    pub password: Option<String>,
    pub imap_host: Option<String>,
    pub imap_port: Option<u16>,
    pub pop3_host: Option<String>,
    pub pop3_port: Option<u16>,
    pub smtp_host: Option<String>,
    pub smtp_port: Option<u16>,
    pub imap_security: Option<String>,
    pub pop3_security: Option<String>,
    pub smtp_security: Option<String>,
    pub accept_invalid_certs: Option<bool>,
    pub allow_plaintext: Option<bool>,
}

fn updated_credentials_sync_state(
    crypto: &pebble_crypto::CryptoService,
    current: &Account,
    body: &UpdateAccountRequest,
    sync_state_json: Option<String>,
) -> Result<String, ApiError> {
    let sync_state_json = sync_state_json
        .ok_or_else(|| ApiError::BadRequest("Account credentials are missing".into()))?;
    let mut sync_state: serde_json::Value = serde_json::from_str(&sync_state_json)
        .map_err(|error| ApiError::Internal(format!("Invalid sync state: {error}")))?;
    let encrypted_hex = sync_state["credentials"]
        .as_str()
        .ok_or_else(|| ApiError::BadRequest("Account credentials are missing".into()))?;
    let mut credentials = crate::credentials::decrypt_credentials(crypto, encrypted_hex)
        .map_err(|error| ApiError::Internal(format!("Decrypt failed: {error}")))?;
    let next_email = body
        .email
        .as_deref()
        .map(str::trim)
        .unwrap_or(&current.email);
    match &mut credentials {
        AccountCredentials::Imap { imap, smtp } => {
            if imap.username.trim().is_empty() || imap.username.eq_ignore_ascii_case(&current.email)
            {
                imap.username = next_email.to_string();
            }
            if smtp.username.trim().is_empty() || smtp.username.eq_ignore_ascii_case(&current.email)
            {
                smtp.username = next_email.to_string();
            }
            if let Some(password) = &body.password {
                imap.password = password.clone();
                smtp.password = password.clone();
            }
            if let Some(host) = &body.imap_host {
                imap.host = host.trim().to_string();
            }
            if let Some(port) = body.imap_port {
                imap.port = port;
            }
            if let Some(security) = &body.imap_security {
                imap.security = security.clone();
            }
            if let Some(value) = body.accept_invalid_certs {
                imap.accept_invalid_certs = value;
            }
            if let Some(value) = body.allow_plaintext {
                imap.allow_plaintext = value;
            }
            if let Some(host) = &body.smtp_host {
                smtp.host = host.trim().to_string();
            }
            if let Some(port) = body.smtp_port {
                smtp.port = port;
            }
            if let Some(security) = &body.smtp_security {
                smtp.security = security.clone();
            }
            if let Some(value) = body.accept_invalid_certs {
                smtp.accept_invalid_certs = value;
            }
            if let Some(value) = body.allow_plaintext {
                smtp.allow_plaintext = value;
            }
            connection_security(&imap.security, imap.allow_plaintext)?;
            connection_security(&smtp.security, smtp.allow_plaintext)?;
            if imap.host.trim().is_empty()
                || imap.port == 0
                || smtp.host.trim().is_empty()
                || smtp.port == 0
            {
                return Err(ApiError::BadRequest(
                    "Mail server host and port are required".into(),
                ));
            }
        }
        AccountCredentials::Pop3 { pop3, smtp } => {
            if pop3.username.trim().is_empty() || pop3.username.eq_ignore_ascii_case(&current.email)
            {
                pop3.username = next_email.to_string();
            }
            if smtp.username.trim().is_empty() || smtp.username.eq_ignore_ascii_case(&current.email)
            {
                smtp.username = next_email.to_string();
            }
            if let Some(password) = &body.password {
                pop3.password = password.clone();
                smtp.password = password.clone();
            }
            if let Some(host) = body.pop3_host.as_ref().or(body.imap_host.as_ref()) {
                pop3.host = host.trim().to_string();
            }
            if let Some(port) = body.pop3_port.or(body.imap_port) {
                pop3.port = port;
            }
            if let Some(security) = body.pop3_security.as_ref().or(body.imap_security.as_ref()) {
                pop3.security = security.clone();
            }
            if let Some(value) = body.accept_invalid_certs {
                pop3.accept_invalid_certs = value;
            }
            if let Some(value) = body.allow_plaintext {
                pop3.allow_plaintext = value;
            }
            if let Some(host) = &body.smtp_host {
                smtp.host = host.trim().to_string();
            }
            if let Some(port) = body.smtp_port {
                smtp.port = port;
            }
            if let Some(security) = &body.smtp_security {
                smtp.security = security.clone();
            }
            if let Some(value) = body.accept_invalid_certs {
                smtp.accept_invalid_certs = value;
            }
            if let Some(value) = body.allow_plaintext {
                smtp.allow_plaintext = value;
            }
            connection_security(&pop3.security, pop3.allow_plaintext)?;
            connection_security(&smtp.security, smtp.allow_plaintext)?;
            if pop3.host.trim().is_empty()
                || pop3.port == 0
                || smtp.host.trim().is_empty()
                || smtp.port == 0
            {
                return Err(ApiError::BadRequest(
                    "Mail server host and port are required".into(),
                ));
            }
        }
    }
    let encrypted = encrypt_credentials(crypto, &credentials)
        .map_err(|error| ApiError::Internal(format!("Encrypt failed: {error}")))?;
    sync_state["credentials"] = serde_json::Value::String(encrypted);
    Ok(sync_state.to_string())
}

pub async fn update_account(
    State(state): State<AppStateRef>,
    Path(account_id): Path<String>,
    Json(body): Json<UpdateAccountRequest>,
) -> Result<Json<AccountResponse>, ApiError> {
    let current = state
        .store
        .get_account(&account_id)
        .map_err(|error| ApiError::Internal(format!("Failed to load account: {error}")))?
        .ok_or_else(|| ApiError::NotFound("Account not found".into()))?;
    if let Some(email) = body.email.as_deref() {
        if matches!(
            current.provider,
            ProviderType::Gmail | ProviderType::Outlook
        ) && !current.email.eq_ignore_ascii_case(email.trim())
        {
            return Err(ApiError::BadRequest(
                "OAuth mailbox addresses cannot be changed without mailbox verification".into(),
            ));
        }
    }
    let email_changed = body
        .email
        .as_deref()
        .is_some_and(|email| !current.email.eq_ignore_ascii_case(email.trim()));
    let has_cred_changes = body.password.is_some()
        || body.imap_host.is_some()
        || body.imap_port.is_some()
        || body.smtp_host.is_some()
        || body.smtp_port.is_some()
        || body.imap_security.is_some()
        || body.smtp_security.is_some()
        || body.pop3_host.is_some()
        || body.pop3_port.is_some()
        || body.pop3_security.is_some()
        || body.accept_invalid_certs.is_some()
        || body.allow_plaintext.is_some()
        || (email_changed && matches!(current.provider, ProviderType::Imap | ProviderType::Pop3));
    if has_cred_changes
        && matches!(
            current.provider,
            ProviderType::Gmail | ProviderType::Outlook
        )
    {
        return Err(ApiError::BadRequest(
            "OAuth credentials must be changed through verified reauthorization".into(),
        ));
    }
    if body
        .email
        .as_deref()
        .is_some_and(|email| email.trim().is_empty())
    {
        return Err(ApiError::BadRequest("Account email is required".into()));
    }
    let was_running = has_cred_changes && state.sync_manager.is_account_running(&account_id).await;
    if has_cred_changes {
        state.sync_manager.stop_account_sync(&account_id).await;
    }
    let updated_sync_state = if has_cred_changes {
        let lookup_id = account_id.clone();
        let sync_state_json = state
            .store
            .with_read_async(move |conn| {
                let result: Option<Option<String>> = conn
                    .query_row(
                        "SELECT sync_state FROM accounts WHERE id=?1",
                        rusqlite::params![lookup_id],
                        |row| row.get(0),
                    )
                    .optional()?;
                Ok(result.flatten())
            })
            .await
            .map_err(|error| ApiError::Internal(format!("Failed to read account: {error}")))?;
        let result =
            updated_credentials_sync_state(&state.crypto, &current, &body, sync_state_json);
        match result {
            Ok(sync_state) => Some(sync_state),
            Err(error) => {
                if was_running {
                    let _ = state.sync_manager.start_account_sync(&account_id).await;
                }
                return Err(error);
            }
        }
    } else {
        None
    };
    let normalized_account_label = body
        .account_label
        .as_deref()
        .map(|label| pebble_store::accounts::normalize_account_label(Some(label)))
        .transpose()
        .map_err(|error| ApiError::BadRequest(error.to_string()))?
        .flatten();
    let store = state.store.clone();
    let now = now_timestamp();
    let aid = account_id.clone();

    // Update basic account fields
    let mut sets = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::types::ToSql + Send>> = Vec::new();

    if let Some(email) = body
        .email
        .as_deref()
        .filter(|_| matches!(current.provider, ProviderType::Imap | ProviderType::Pop3))
    {
        sets.push(format!("email = ?{}", params.len() + 1));
        params.push(Box::new(email.trim().to_string()));
    }
    if let Some(ref display_name) = body.display_name {
        sets.push(format!("display_name = ?{}", params.len() + 1));
        params.push(Box::new(display_name.clone()));
    }
    let account_color = body.account_color.clone().or_else(|| body.color.clone());
    if let Some(color) = account_color {
        sets.push(format!("color = ?{}", params.len() + 1));
        params.push(Box::new(color));
    }
    if body.account_label.is_some() {
        sets.push(format!("account_label = ?{}", params.len() + 1));
        params.push(Box::new(normalized_account_label.clone()));
    }
    if let Some(sync_state) = &updated_sync_state {
        sets.push(format!("sync_state = ?{}", params.len() + 1));
        params.push(Box::new(sync_state.clone()));
    }

    sets.push(format!("updated_at = ?{}", params.len() + 1));
    params.push(Box::new(now));

    let aid_param_idx = params.len() + 1;
    params.push(Box::new(aid.clone()));

    let sql = format!(
        "UPDATE accounts SET {} WHERE id = ?{}",
        sets.join(", "),
        aid_param_idx
    );

    if let Err(error) = store
        .with_write_async(move |conn| {
            let param_refs: Vec<&dyn rusqlite::types::ToSql> = params
                .iter()
                .map(|p| p.as_ref() as &dyn rusqlite::types::ToSql)
                .collect();
            conn.execute(&sql, param_refs.as_slice())?;
            Ok(())
        })
        .await
    {
        if was_running {
            let _ = state.sync_manager.start_account_sync(&account_id).await;
        }
        return Err(ApiError::Internal(format!(
            "Failed to update account: {error}"
        )));
    }

    if was_running {
        state
            .sync_manager
            .start_account_sync(&account_id)
            .await
            .map_err(|error| ApiError::Internal(format!("Failed to restart sync: {error}")))?;
    }

    // Return updated account
    let store_final = state.store.clone();
    let aid_final = account_id.clone();
    let account = store_final
        .with_read_async(move |conn| {
            let row = conn.query_row(
                "SELECT id, email, display_name, color, provider, created_at, updated_at, account_label, provider_display_name FROM accounts WHERE id = ?1",
                rusqlite::params![aid_final],
                |row| {
                    let provider_str: String = row.get(4)?;
                    let provider = match provider_str.as_str() {
                        "gmail" => ProviderType::Gmail,
                        "outlook" => ProviderType::Outlook,
                        "pop3" => ProviderType::Pop3,
                        _ => ProviderType::Imap,
                    };
                    Ok(Account {
                        account_label: row.get(7)?,
                        provider_display_name: row.get(8)?,
                        id: row.get(0)?,
                        email: row.get(1)?,
                        display_name: row.get(2)?,
                        color: row.get(3)?,
                        provider,
                        created_at: row.get(5)?,
                        updated_at: row.get(6)?,
                    })
                },
            )?;
            Ok(row)
        })
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to read updated account: {e}")))?;

    Ok(Json(AccountResponse::from(account)))
}

pub async fn test_account_connection(
    State(state): State<AppStateRef>,
    Path(account_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let account = state
        .store
        .get_account(&account_id)
        .map_err(|error| ApiError::Internal(format!("Failed to load account: {error}")))?
        .ok_or_else(|| ApiError::NotFound("Account not found".into()))?;
    if matches!(
        account.provider,
        ProviderType::Gmail | ProviderType::Outlook
    ) {
        let preview = crate::routes::oauth::preview_account_identity(&state, &account_id).await?;
        return Ok(Json(serde_json::json!({
            "ok": true,
            "report": format!("Verified mailbox {}", preview.identity.email),
        })));
    }
    let store = state.store.clone();
    let lookup_id = account_id.clone();

    let sync_state_json = store
        .with_read_async(move |conn| {
            let result: Option<Option<String>> = conn
                .query_row(
                    "SELECT sync_state FROM accounts WHERE id = ?1",
                    rusqlite::params![lookup_id],
                    |row| row.get(0),
                )
                .optional()?;
            Ok(result.flatten())
        })
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to get account: {e}")))?
        .ok_or_else(|| ApiError::NotFound("Account not found or has no credentials".to_string()))?;

    let sync_state: serde_json::Value = serde_json::from_str(&sync_state_json)
        .map_err(|e| ApiError::Internal(format!("Invalid sync state: {e}")))?;

    let encrypted_hex = sync_state["credentials"]
        .as_str()
        .ok_or_else(|| ApiError::BadRequest("No credentials in account".to_string()))?;

    let creds = crate::credentials::decrypt_credentials(&state.crypto, encrypted_hex)
        .map_err(|e| ApiError::Internal(format!("Failed to decrypt credentials: {e}")))?;

    let global_proxy =
        crate::sync::load_global_proxy(&state.store, &state.crypto).map_err(ApiError::Internal)?;
    let proxy = crate::sync::resolve_proxy(&sync_state, global_proxy.as_ref())
        .map_err(ApiError::BadRequest)?
        .map(|proxy| pebble_mail::ProxyConfig {
            host: proxy.host,
            port: proxy.port,
        });
    let result = match &creds {
        crate::credentials::AccountCredentials::Imap { imap, .. } => {
            let security = connection_security(&imap.security, imap.allow_plaintext)?;
            let config = pebble_mail::ImapConfig {
                host: imap.host.clone(),
                port: imap.port,
                username: if imap.username.trim().is_empty() {
                    account.email
                } else {
                    imap.username.clone()
                },
                password: imap.password.clone(),
                security,
                accept_invalid_certs: imap.accept_invalid_certs,
                proxy,
            };
            pebble_mail::ImapProvider::test_connection_with_login(&config).await
        }
        crate::credentials::AccountCredentials::Pop3 { pop3, .. } => {
            let config = pebble_mail::Pop3Config {
                host: pop3.host.clone(),
                port: pop3.port,
                username: if pop3.username.trim().is_empty() {
                    account.email
                } else {
                    pop3.username.clone()
                },
                password: pop3.password.clone(),
                security: connection_security(&pop3.security, pop3.allow_plaintext)?,
                accept_invalid_certs: pop3.accept_invalid_certs,
                proxy,
            };
            pebble_mail::Pop3Provider::test_connection(&config).await
        }
    };
    match result {
        Ok(report) => Ok(Json(serde_json::json!({ "ok": true, "report": report }))),
        Err(e) => Err(ApiError::BadRequest(format!("Connection failed: {e}"))),
    }
}

fn connection_security(
    value: &str,
    allow_plaintext: bool,
) -> Result<pebble_mail::ConnectionSecurity, ApiError> {
    match value {
        "tls" => Ok(pebble_mail::ConnectionSecurity::Tls),
        "starttls" => Ok(pebble_mail::ConnectionSecurity::StartTls),
        "plain" if allow_plaintext => Ok(pebble_mail::ConnectionSecurity::Plain),
        "plain" => Err(ApiError::BadRequest(
            "Plaintext mail transport requires explicit consent".into(),
        )),
        _ => Err(ApiError::BadRequest(
            "Unsupported mail transport security".into(),
        )),
    }
}

#[derive(Deserialize)]
pub struct TestImapRequest {
    pub imap_host: String,
    pub imap_port: u16,
    pub imap_security: Option<String>,
    pub proxy_host: Option<String>,
    pub proxy_port: Option<u16>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub email: Option<String>,
    #[serde(default)]
    pub accept_invalid_certs: bool,
    #[serde(default)]
    pub allow_plaintext: bool,
}

#[derive(Deserialize)]
pub struct TestPop3Request {
    pub pop3_host: String,
    pub pop3_port: u16,
    pub pop3_security: Option<String>,
    pub accept_invalid_certs: Option<bool>,
    pub allow_plaintext: Option<bool>,
    pub proxy_host: Option<String>,
    pub proxy_port: Option<u16>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub email: Option<String>,
}

pub async fn test_imap_connection(
    State(state): State<AppStateRef>,
    Json(body): Json<TestImapRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let security = match body.imap_security.as_deref() {
        None | Some("tls") => pebble_mail::ConnectionSecurity::Tls,
        Some("starttls") => pebble_mail::ConnectionSecurity::StartTls,
        Some("plain") if body.allow_plaintext => pebble_mail::ConnectionSecurity::Plain,
        Some("plain") => {
            return Err(ApiError::BadRequest(
                "Plaintext mail transport requires explicit consent".into(),
            ))
        }
        Some(_) => {
            return Err(ApiError::BadRequest(
                "Unsupported mail transport security".into(),
            ))
        }
    };
    if body.proxy_host.is_some() != body.proxy_port.is_some() {
        return Err(ApiError::BadRequest(
            "Proxy host and port must be supplied together".into(),
        ));
    }
    let proxy = match (body.proxy_host.as_deref(), body.proxy_port) {
        (Some(host), Some(port)) if !host.trim().is_empty() && port != 0 => {
            let proxy = pebble_core::HttpProxyConfig {
                host: host.trim().to_string(),
                port,
            };
            proxy.validate().map_err(ApiError::BadRequest)?;
            Some(proxy)
        }
        (None, None) => None,
        _ => return Err(ApiError::BadRequest("Invalid proxy configuration".into())),
    };
    let proxy = match proxy {
        Some(proxy) => Some(proxy),
        None => crate::sync::load_global_proxy(&state.store, &state.crypto)
            .map_err(ApiError::Internal)?,
    };
    let username = body
        .username
        .filter(|value| !value.trim().is_empty())
        .or(body.email)
        .ok_or_else(|| ApiError::BadRequest("Username or email is required".into()))?;
    let password = body
        .password
        .ok_or_else(|| ApiError::BadRequest("Password is required".into()))?;

    let config = pebble_mail::ImapConfig {
        host: body.imap_host,
        port: body.imap_port,
        username,
        password,
        security,
        accept_invalid_certs: body.accept_invalid_certs,
        proxy: proxy.map(|proxy| pebble_mail::ProxyConfig {
            host: proxy.host,
            port: proxy.port,
        }),
    };

    match pebble_mail::ImapProvider::test_connection_with_login(&config).await {
        Ok(report) => Ok(Json(serde_json::json!({ "ok": true, "report": report }))),
        Err(e) => Err(ApiError::BadRequest(format!("Connection failed: {e}"))),
    }
}

pub async fn test_pop3_connection(
    State(state): State<AppStateRef>,
    Json(body): Json<TestPop3Request>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if body.proxy_host.is_some() != body.proxy_port.is_some() {
        return Err(ApiError::BadRequest(
            "Proxy host and port must be supplied together".into(),
        ));
    }
    let proxy = match (body.proxy_host.as_deref(), body.proxy_port) {
        (Some(host), Some(port)) if !host.trim().is_empty() && port != 0 => {
            let proxy = pebble_core::HttpProxyConfig {
                host: host.trim().to_string(),
                port,
            };
            proxy.validate().map_err(ApiError::BadRequest)?;
            Some(proxy)
        }
        (None, None) => None,
        _ => return Err(ApiError::BadRequest("Invalid proxy configuration".into())),
    };
    let proxy = match proxy {
        Some(proxy) => Some(proxy),
        None => crate::sync::load_global_proxy(&state.store, &state.crypto)
            .map_err(ApiError::Internal)?,
    };
    let security = connection_security(
        body.pop3_security.as_deref().unwrap_or("tls"),
        body.allow_plaintext.unwrap_or(false),
    )?;
    let username = body
        .username
        .filter(|value| !value.trim().is_empty())
        .or(body.email)
        .ok_or_else(|| ApiError::BadRequest("Username or email is required".into()))?;
    let password = body
        .password
        .ok_or_else(|| ApiError::BadRequest("Password is required".into()))?;
    let config = pebble_mail::Pop3Config {
        host: body.pop3_host,
        port: body.pop3_port,
        username,
        password,
        security,
        accept_invalid_certs: body.accept_invalid_certs.unwrap_or(false),
        proxy: proxy.map(|proxy| pebble_mail::ProxyConfig {
            host: proxy.host,
            port: proxy.port,
        }),
    };
    match pebble_mail::Pop3Provider::test_connection(&config).await {
        Ok(report) => Ok(Json(serde_json::json!({ "ok": true, "report": report }))),
        Err(error) => Err(ApiError::BadRequest(format!("Connection failed: {error}"))),
    }
}

use rusqlite::OptionalExtension;
