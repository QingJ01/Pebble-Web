use crate::error::ApiError;
use crate::state::AppStateRef;
use axum::{
    extract::{Path, State},
    Json,
};
use pebble_core::traits::FolderProvider;
use pebble_core::{Folder, FolderRole, ProviderType};
use pebble_mail::ImapMailProvider;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RealtimePreferenceArgs {
    mode: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct AccountArgs {
    account_id: String,
}

#[derive(Deserialize)]
struct AddAccountArgs {
    request: crate::routes::accounts::CreateAccountRequest,
}

#[derive(Deserialize)]
struct ConnectionTestArgs {
    request: crate::routes::accounts::TestImapRequest,
}

#[derive(Deserialize)]
struct Pop3ConnectionTestArgs {
    request: crate::routes::accounts::TestPop3Request,
}

#[derive(Deserialize)]
struct OAuthIdentityPreviewArgs {
    preview: crate::routes::oauth::OAuthIdentityPreview,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct StartSyncArgs {
    account_id: String,
    poll_interval_secs: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct UpdateSyncRangeArgs {
    account_id: String,
    days: Option<u32>,
    poll_interval_secs: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct TriggerSyncArgs {
    account_id: String,
    reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct UpdateImapSyncFoldersArgs {
    account_id: String,
    selected_remote_ids: Vec<String>,
}

#[derive(Serialize)]
struct ImapSyncFolderSettings {
    folders: Vec<Folder>,
    selected_remote_ids: Vec<String>,
}

async fn discover_imap_folders(
    state: &AppStateRef,
    account_id: &str,
) -> Result<Vec<Folder>, ApiError> {
    let account = state
        .store
        .get_account(account_id)
        .map_err(|error| ApiError::Internal(format!("Failed to load account: {error}")))?
        .ok_or_else(|| ApiError::NotFound("Account not found".into()))?;
    if account.provider != ProviderType::Imap {
        return Err(ApiError::BadRequest(
            "Folder selection is currently available for IMAP accounts".into(),
        ));
    }
    let sync_state = state
        .store
        .get_account_sync_state(account_id)
        .map_err(|error| ApiError::Internal(format!("Failed to load sync state: {error}")))?
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .unwrap_or_else(|| json!({}));
    let encrypted = sync_state["credentials"]
        .as_str()
        .ok_or_else(|| ApiError::Internal("Account credentials are missing".into()))?;
    let credentials = crate::credentials::decrypt_credentials(&state.crypto, encrypted)
        .map_err(|error| ApiError::Internal(format!("Failed to decrypt credentials: {error}")))?;
    let crate::credentials::AccountCredentials::Imap { imap, .. } = credentials else {
        return Err(ApiError::Internal(
            "Account credentials do not match IMAP provider".into(),
        ));
    };
    let global_proxy =
        read_encrypted_json::<pebble_core::HttpProxyConfig>(state, "global_network_proxy")?;
    let config =
        crate::sync::build_imap_config(&imap, &sync_state, &account.email, global_proxy.as_ref())
            .map_err(ApiError::BadRequest)?;
    let mut provider = ImapMailProvider::new(config);
    provider.set_account_id(account_id.to_string());
    provider
        .connect()
        .await
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;
    let result = provider
        .list_folders()
        .await
        .map_err(|error| ApiError::BadRequest(error.to_string()));
    if let Err(error) = provider.disconnect().await {
        tracing::debug!("Failed to disconnect folder discovery for {account_id}: {error}");
    }
    result
}

fn selected_imap_remote_ids(folders: &[Folder], configured: Option<Vec<String>>) -> Vec<String> {
    let configured = configured.map(|ids| ids.into_iter().collect::<HashSet<_>>());
    folders
        .iter()
        .filter(|folder| {
            folder.role == Some(FolderRole::Inbox)
                || folder.remote_id.eq_ignore_ascii_case("INBOX")
                || configured
                    .as_ref()
                    .is_none_or(|ids| ids.contains(&folder.remote_id))
        })
        .map(|folder| folder.remote_id.clone())
        .collect()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct SignatureArgs {
    account_id: String,
    signature: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmailTemplate {
    id: Option<String>,
    name: String,
    subject: String,
    body: String,
    #[serde(rename = "createdAt")]
    created_at: Option<i64>,
    #[serde(default, rename = "deduplicateByContent")]
    deduplicate_by_content: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TemplateIdArgs {
    id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TemplateSaveArgs {
    template: EmailTemplate,
}

fn parse_args<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T, ApiError> {
    serde_json::from_value(value)
        .map_err(|error| ApiError::BadRequest(format!("Invalid command arguments: {error}")))
}

fn secure_user_json<T: for<'de> Deserialize<'de> + Serialize>(
    state: &AppStateRef,
    key: &str,
    default: T,
) -> Result<T, ApiError> {
    let Some(ciphertext) = state
        .store
        .get_secure_user_data(key)
        .map_err(|error| ApiError::Internal(format!("Failed to read {key}: {error}")))?
    else {
        return Ok(default);
    };
    let plaintext = match state
        .crypto
        .decrypt_for("secure_user_data.value", key, &ciphertext)
    {
        Ok(plaintext) => plaintext,
        Err(_) => {
            let _: T = serde_json::from_slice(&ciphertext)
                .map_err(|error| ApiError::Internal(format!("Invalid legacy {key}: {error}")))?;
            let encrypted = state
                .crypto
                .encrypt_for("secure_user_data.value", key, &ciphertext)
                .map_err(|error| ApiError::Internal(format!("Failed to migrate {key}: {error}")))?;
            if !state
                .store
                .compare_exchange_secure_user_data(key, &ciphertext, &encrypted)
                .map_err(|error| {
                    ApiError::Internal(format!("Failed to persist migrated {key}: {error}"))
                })?
            {
                return secure_user_json(state, key, default);
            }
            ciphertext
        }
    };
    serde_json::from_slice(&plaintext)
        .map_err(|error| ApiError::Internal(format!("Invalid {key}: {error}")))
}

fn set_secure_user_json<T: Serialize>(
    state: &AppStateRef,
    key: &str,
    value: &T,
) -> Result<(), ApiError> {
    let plaintext = serde_json::to_vec(value)
        .map_err(|error| ApiError::Internal(format!("Failed to encode {key}: {error}")))?;
    let encrypted = state
        .crypto
        .encrypt_for("secure_user_data.value", key, &plaintext)
        .map_err(|error| ApiError::Internal(format!("Failed to encrypt {key}: {error}")))?;
    state
        .store
        .set_secure_user_data(key, &encrypted)
        .map_err(|error| ApiError::Internal(format!("Failed to persist {key}: {error}")))
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AutoBackupStatus {
    #[serde(default)]
    last_success_at: Option<i64>,
    #[serde(default)]
    last_error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum AccountProxyMode {
    Inherit,
    Disabled,
    Custom,
}

impl AccountProxyMode {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Inherit => "inherit",
            Self::Disabled => "disabled",
            Self::Custom => "custom",
        }
    }
}

#[derive(Serialize)]
struct AccountProxySetting {
    mode: AccountProxyMode,
    proxy: Option<pebble_core::HttpProxyConfig>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProxyArgs {
    account_id: Option<String>,
    mode: Option<AccountProxyMode>,
    proxy_host: Option<String>,
    proxy_port: Option<u16>,
}

fn proxy_from_args(args: &ProxyArgs) -> Result<Option<pebble_core::HttpProxyConfig>, ApiError> {
    match (args.proxy_host.as_deref(), args.proxy_port) {
        (None, None) => Ok(None),
        (Some(host), Some(port)) if !host.trim().is_empty() && port != 0 => {
            let proxy = pebble_core::HttpProxyConfig {
                host: host.trim().to_string(),
                port,
            };
            proxy.validate().map_err(ApiError::BadRequest)?;
            Ok(Some(proxy))
        }
        _ => Err(ApiError::BadRequest(
            "Proxy host and port must be supplied together".into(),
        )),
    }
}

fn account_proxy_setting(
    state: &AppStateRef,
    account_id: &str,
) -> Result<AccountProxySetting, ApiError> {
    state
        .store
        .get_account(account_id)
        .map_err(|error| ApiError::Internal(format!("Failed to load account: {error}")))?
        .ok_or_else(|| ApiError::NotFound("Account not found".into()))?;
    let sync_state = state
        .store
        .get_account_sync_state(account_id)
        .map_err(|error| ApiError::Internal(format!("Failed to load proxy settings: {error}")))?
        .map(|value| serde_json::from_str::<Value>(&value))
        .transpose()
        .map_err(|error| ApiError::Internal(format!("Invalid account sync state: {error}")))?
        .unwrap_or_else(|| json!({}));
    let mode = match sync_state["proxy_mode"].as_str() {
        Some("disabled" | "none") => AccountProxyMode::Disabled,
        Some("custom") => AccountProxyMode::Custom,
        _ if sync_state["proxy"].is_object() => AccountProxyMode::Custom,
        _ => AccountProxyMode::Inherit,
    };
    let proxy = if matches!(mode, AccountProxyMode::Custom) {
        Some(
            serde_json::from_value(sync_state["proxy"].clone())
                .map_err(|error| ApiError::Internal(format!("Invalid account proxy: {error}")))?,
        )
    } else {
        None
    };
    Ok(AccountProxySetting { mode, proxy })
}

async fn save_account_proxy_setting(
    state: &AppStateRef,
    account_id: String,
    mode: AccountProxyMode,
    proxy: Option<pebble_core::HttpProxyConfig>,
) -> Result<(), ApiError> {
    let proxy = match mode {
        AccountProxyMode::Custom => Some(proxy.ok_or_else(|| {
            ApiError::BadRequest("Custom proxy settings require a host and port".into())
        })?),
        _ => None,
    };
    let was_running = state.sync_manager.is_account_running(&account_id).await;
    state.sync_manager.stop_account_sync(&account_id).await;
    let stored_mode = mode.as_str().to_string();
    state
        .store
        .with_write_async({
            let account_id = account_id.clone();
            move |conn| {
                let current: Option<Option<String>> = conn
                    .query_row(
                        "SELECT sync_state FROM accounts WHERE id=?1",
                        rusqlite::params![account_id],
                        |row| row.get(0),
                    )
                    .optional()?;
                let Some(current) = current else {
                    return Err(rusqlite::Error::QueryReturnedNoRows.into());
                };
                let mut value = current
                    .map(|state| serde_json::from_str::<Value>(&state))
                    .transpose()
                    .map_err(|_| rusqlite::Error::InvalidQuery)?
                    .unwrap_or_else(|| json!({}));
                let object = value.as_object_mut().ok_or(rusqlite::Error::InvalidQuery)?;
                object.insert("proxy_mode".into(), json!(stored_mode));
                match proxy {
                    Some(proxy) => {
                        object.insert("proxy".into(), json!(proxy));
                    }
                    None => {
                        object.remove("proxy");
                    }
                }
                conn.execute(
                    "UPDATE accounts SET sync_state=?1 WHERE id=?2",
                    rusqlite::params![value.to_string(), account_id],
                )?;
                Ok(())
            }
        })
        .await
        .map_err(|error| ApiError::Internal(format!("Failed to save proxy settings: {error}")))?;
    if was_running {
        state
            .sync_manager
            .start_account_sync(&account_id)
            .await
            .map_err(|error| {
                ApiError::Internal(format!("Failed to restart account sync: {error}"))
            })?;
    }
    Ok(())
}

fn read_encrypted_json<T: for<'de> Deserialize<'de>>(
    state: &AppStateRef,
    key: &str,
) -> Result<Option<T>, ApiError> {
    let Some(value) = state
        .store
        .get_secure_user_data(key)
        .map_err(|error| ApiError::Internal(format!("Failed to read {key}: {error}")))?
    else {
        return Ok(None);
    };
    let plaintext = match state
        .crypto
        .decrypt_for("secure_user_data.value", key, &value)
    {
        Ok(plaintext) => plaintext,
        Err(_) => {
            let plaintext = state
                .crypto
                .decrypt(&value)
                .map_err(|error| ApiError::Internal(format!("Failed to decrypt {key}: {error}")))?;
            let _: T = serde_json::from_slice(&plaintext)
                .map_err(|error| ApiError::Internal(format!("Invalid legacy {key}: {error}")))?;
            let encrypted = state
                .crypto
                .encrypt_for("secure_user_data.value", key, &plaintext)
                .map_err(|error| ApiError::Internal(format!("Failed to migrate {key}: {error}")))?;
            if !state
                .store
                .compare_exchange_secure_user_data(key, &value, &encrypted)
                .map_err(|error| {
                    ApiError::Internal(format!("Failed to persist migrated {key}: {error}"))
                })?
            {
                return read_encrypted_json(state, key);
            }
            plaintext
        }
    };
    serde_json::from_slice(&plaintext)
        .map(Some)
        .map_err(|error| ApiError::Internal(format!("Invalid {key}: {error}")))
}

fn write_encrypted_json<T: Serialize>(
    state: &AppStateRef,
    key: &str,
    value: &T,
) -> Result<(), ApiError> {
    let plaintext = serde_json::to_vec(value)
        .map_err(|error| ApiError::Internal(format!("Failed to encode {key}: {error}")))?;
    let encrypted = state
        .crypto
        .encrypt_for("secure_user_data.value", key, &plaintext)
        .map_err(|error| ApiError::Internal(format!("Failed to encrypt {key}: {error}")))?;
    state
        .store
        .set_secure_user_data(key, &encrypted)
        .map_err(|error| ApiError::Internal(format!("Failed to persist {key}: {error}")))
}

pub async fn invoke(
    State(state): State<AppStateRef>,
    Path(command): Path<String>,
    Json(args): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    match command.as_str() {
        "health_check" => Ok(Json(json!("ok"))),
        "get_server_info" => Ok(Json(json!({ "version": env!("CARGO_PKG_VERSION") }))),
        "preview_oauth_identity" => {
            let args: AccountArgs = parse_args(args)?;
            let preview =
                crate::routes::oauth::preview_account_identity(&state, &args.account_id).await?;
            Ok(Json(json!(preview)))
        }
        "apply_oauth_identity" => {
            let args: OAuthIdentityPreviewArgs = parse_args(args)?;
            let account =
                crate::routes::oauth::apply_account_identity(&state, args.preview).await?;
            Ok(Json(json!(account)))
        }
        "add_account" => {
            let args: AddAccountArgs = parse_args(args)?;
            let Json(account) =
                crate::routes::accounts::create_account(State(state.clone()), Json(args.request))
                    .await?;
            Ok(Json(json!(account)))
        }
        "create_account" => {
            let args: crate::routes::accounts::CreateAccountRequest = parse_args(args)?;
            let Json(account) =
                crate::routes::accounts::create_account(State(state.clone()), Json(args)).await?;
            Ok(Json(json!(account)))
        }
        "update_account" => {
            let account_id = args["accountId"]
                .as_str()
                .ok_or_else(|| ApiError::BadRequest("accountId is required".into()))?
                .to_string();
            let update: crate::routes::accounts::UpdateAccountRequest = parse_args(args)?;
            let Json(account) = crate::routes::accounts::update_account(
                State(state.clone()),
                Path(account_id),
                Json(update),
            )
            .await?;
            let _ = account;
            Ok(Json(Value::Null))
        }
        "delete_account" => {
            let args: AccountArgs = parse_args(args)?;
            let result = crate::routes::accounts::delete_account(
                State(state.clone()),
                Path(args.account_id),
            )
            .await?;
            Ok(result)
        }
        "test_account_connection" => {
            let args: AccountArgs = parse_args(args)?;
            let result = crate::routes::accounts::test_account_connection(
                State(state.clone()),
                Path(args.account_id),
            )
            .await?;
            Ok(Json(json!(result.0["report"]
                .as_str()
                .unwrap_or("Connection successful"))))
        }
        "test_imap_connection" => {
            let args: ConnectionTestArgs = parse_args(args)?;
            let result = crate::routes::accounts::test_imap_connection(
                State(state.clone()),
                Json(args.request),
            )
            .await?;
            Ok(Json(json!(result.0["report"]
                .as_str()
                .unwrap_or("Connection successful"))))
        }
        "test_pop3_connection" => {
            let args: Pop3ConnectionTestArgs = parse_args(args)?;
            let result = crate::routes::accounts::test_pop3_connection(
                State(state.clone()),
                Json(args.request),
            )
            .await?;
            Ok(Json(json!(result.0["report"]
                .as_str()
                .unwrap_or("Connection successful"))))
        }
        "get_global_proxy" => {
            let proxy = read_encrypted_json::<pebble_core::HttpProxyConfig>(
                &state,
                "global_network_proxy",
            )?;
            Ok(Json(json!(proxy)))
        }
        "update_global_proxy" => {
            let args: ProxyArgs = parse_args(args)?;
            let proxy = proxy_from_args(&args)?;
            if let Some(proxy) = proxy {
                write_encrypted_json(&state, "global_network_proxy", &proxy)?;
            } else {
                state
                    .store
                    .delete_secure_user_data("global_network_proxy")
                    .map_err(|error| {
                        ApiError::Internal(format!("Failed to clear global proxy: {error}"))
                    })?;
            }
            state
                .sync_manager
                .restart_running_accounts()
                .await
                .map_err(|error| {
                    ApiError::Internal(format!("Failed to apply global proxy: {error}"))
                })?;
            Ok(Json(Value::Null))
        }
        "get_account_proxy" | "get_oauth_account_proxy" => {
            let args: AccountArgs = parse_args(args)?;
            let setting = account_proxy_setting(&state, &args.account_id)?;
            let proxy = match setting.mode {
                AccountProxyMode::Disabled => None,
                AccountProxyMode::Custom => setting.proxy,
                AccountProxyMode::Inherit => read_encrypted_json::<pebble_core::HttpProxyConfig>(
                    &state,
                    "global_network_proxy",
                )?,
            };
            Ok(Json(json!(proxy)))
        }
        "get_account_proxy_setting" | "get_oauth_account_proxy_setting" => {
            let args: AccountArgs = parse_args(args)?;
            Ok(Json(json!(account_proxy_setting(
                &state,
                &args.account_id
            )?)))
        }
        "update_account_proxy" | "update_oauth_account_proxy" => {
            let args: ProxyArgs = parse_args(args)?;
            let account_id = args
                .account_id
                .clone()
                .ok_or_else(|| ApiError::BadRequest("accountId is required".into()))?;
            let proxy = proxy_from_args(&args)?;
            let mode = if proxy.is_some() {
                AccountProxyMode::Custom
            } else {
                AccountProxyMode::Inherit
            };
            save_account_proxy_setting(&state, account_id, mode, proxy).await?;
            Ok(Json(Value::Null))
        }
        "update_account_proxy_setting" | "update_oauth_account_proxy_setting" => {
            let args: ProxyArgs = parse_args(args)?;
            let account_id = args
                .account_id
                .clone()
                .ok_or_else(|| ApiError::BadRequest("accountId is required".into()))?;
            let mode = args
                .mode
                .ok_or_else(|| ApiError::BadRequest("Proxy mode is required".into()))?;
            let proxy = proxy_from_args(&args)?;
            save_account_proxy_setting(&state, account_id, mode, proxy).await?;
            Ok(Json(Value::Null))
        }
        "list_accounts" => {
            let accounts = state
                .store
                .list_accounts()
                .map_err(|error| ApiError::Internal(format!("Failed to list accounts: {error}")))?;
            Ok(Json(json!(accounts)))
        }
        "get_imap_sync_folders" => {
            let args: AccountArgs = parse_args(args)?;
            let folders = discover_imap_folders(&state, &args.account_id).await?;
            let configured = state
                .store
                .get_sync_state(&args.account_id)
                .map_err(|error| {
                    ApiError::Internal(format!("Failed to load folder selection: {error}"))
                })?
                .and_then(|sync_state| sync_state.selected_imap_folder_remote_ids);
            Ok(Json(json!(ImapSyncFolderSettings {
                selected_remote_ids: selected_imap_remote_ids(&folders, configured),
                folders,
            })))
        }
        "update_imap_sync_folders" => {
            let args: UpdateImapSyncFoldersArgs = parse_args(args)?;
            let folders = discover_imap_folders(&state, &args.account_id).await?;
            let available: HashSet<&str> = folders
                .iter()
                .map(|folder| folder.remote_id.as_str())
                .collect();
            let unknown: Vec<&str> = args
                .selected_remote_ids
                .iter()
                .map(String::as_str)
                .filter(|remote_id| !available.contains(remote_id))
                .collect();
            if !unknown.is_empty() {
                return Err(ApiError::BadRequest(format!(
                    "Unknown IMAP folders: {}",
                    unknown.join(", ")
                )));
            }
            if !folders.iter().any(|folder| {
                folder.role == Some(FolderRole::Inbox)
                    || folder.remote_id.eq_ignore_ascii_case("INBOX")
            }) {
                return Err(ApiError::BadRequest(
                    "The IMAP server did not return an Inbox folder".into(),
                ));
            }
            let requested: HashSet<&str> = args
                .selected_remote_ids
                .iter()
                .map(String::as_str)
                .collect();
            let selected_remote_ids: Vec<String> = folders
                .iter()
                .filter(|folder| {
                    folder.role == Some(FolderRole::Inbox)
                        || folder.remote_id.eq_ignore_ascii_case("INBOX")
                        || requested.contains(folder.remote_id.as_str())
                })
                .map(|folder| folder.remote_id.clone())
                .collect();
            let was_running = state
                .sync_manager
                .is_account_running(&args.account_id)
                .await;
            if was_running {
                state.sync_manager.stop_account_sync(&args.account_id).await;
            }
            let store = state.store.clone();
            let aid = args.account_id.clone();
            let remote_folders = folders.clone();
            let selected = selected_remote_ids.clone();
            let change = match tokio::task::spawn_blocking(move || {
                store.apply_imap_folder_selection(&aid, &remote_folders, &selected)
            })
            .await
            {
                Ok(Ok(change)) => change,
                Ok(Err(error)) => {
                    if was_running {
                        let _ = state
                            .sync_manager
                            .start_account_sync(&args.account_id)
                            .await;
                    }
                    return Err(ApiError::Internal(format!(
                        "Failed to update IMAP folders: {error}"
                    )));
                }
                Err(error) => {
                    if was_running {
                        let _ = state
                            .sync_manager
                            .start_account_sync(&args.account_id)
                            .await;
                    }
                    return Err(ApiError::Internal(format!(
                        "Folder update task failed: {error}"
                    )));
                }
            };
            for message_id in &change.deleted_message_ids {
                let _ = state.search.remove_message(message_id);
            }
            if let Ok(root) = state.attachments_dir.canonicalize() {
                for message_id in &change.deleted_message_ids {
                    let path = root.join(message_id);
                    if path
                        .file_name()
                        .is_some_and(|name| name == message_id.as_str())
                        && path
                            .canonicalize()
                            .is_ok_and(|canonical| canonical.starts_with(&root))
                    {
                        if let Err(error) = std::fs::remove_dir_all(&path) {
                            tracing::warn!(
                                "Failed to remove attachments for message {message_id}: {error}"
                            );
                        }
                    }
                }
            }
            if was_running {
                state
                    .sync_manager
                    .start_account_sync(&args.account_id)
                    .await
                    .map_err(|error| {
                        ApiError::Internal(format!("Failed to restart sync: {error}"))
                    })?;
            }
            Ok(Json(json!(ImapSyncFolderSettings {
                folders,
                selected_remote_ids
            })))
        }
        "get_account_sync_range" => {
            let args: AccountArgs = parse_args(args)?;
            state
                .store
                .get_account(&args.account_id)
                .map_err(|error| ApiError::Internal(format!("Failed to load account: {error}")))?
                .ok_or_else(|| ApiError::NotFound("Account not found".into()))?;
            let days = state
                .store
                .get_sync_state(&args.account_id)
                .map_err(|error| ApiError::Internal(format!("Failed to read sync range: {error}")))?
                .and_then(|sync_state| sync_state.sync_range_days);
            Ok(Json(json!(days)))
        }
        "update_account_sync_range" => {
            let args: UpdateSyncRangeArgs = parse_args(args)?;
            pebble_store::accounts::validate_sync_range_days(args.days)
                .map_err(|error| ApiError::BadRequest(error.to_string()))?;
            let account = state
                .store
                .get_account(&args.account_id)
                .map_err(|error| ApiError::Internal(format!("Failed to load account: {error}")))?
                .ok_or_else(|| ApiError::NotFound("Account not found".into()))?;
            if account.provider == pebble_core::ProviderType::Pop3 && args.days.is_some() {
                return Err(ApiError::BadRequest(
                    "POP3 accounts do not support bounded sync ranges".into(),
                ));
            }
            let current = state
                .store
                .get_sync_state(&args.account_id)
                .map_err(|error| ApiError::Internal(format!("Failed to read sync range: {error}")))?
                .and_then(|sync_state| sync_state.sync_range_days);
            if current == args.days {
                return Ok(Json(Value::Null));
            }
            let was_running = state
                .sync_manager
                .is_account_running(&args.account_id)
                .await;
            if was_running {
                state.sync_manager.stop_account_sync(&args.account_id).await;
            }
            let cutoff =
                pebble_store::accounts::sync_range_cutoff(args.days, pebble_core::now_timestamp());
            if let Err(error) = state
                .store
                .update_sync_state(&args.account_id, |sync_state| {
                    sync_state.sync_range_days = args.days;
                    sync_state.sync_since = cutoff;
                })
            {
                if was_running {
                    let _ = state
                        .sync_manager
                        .start_account_sync(&args.account_id)
                        .await;
                }
                return Err(ApiError::Internal(format!(
                    "Failed to update sync range: {error}"
                )));
            }
            if was_running {
                state
                    .sync_manager
                    .start_account_sync(&args.account_id)
                    .await
                    .map_err(|error| {
                        ApiError::Internal(format!("Failed to restart account sync: {error}"))
                    })?;
            }
            Ok(Json(Value::Null))
        }
        "list_folders" => {
            let args: AccountArgs = parse_args(args)?;
            let folders = state
                .store
                .list_folders(&args.account_id)
                .map_err(|error| ApiError::Internal(format!("Failed to list folders: {error}")))?;
            Ok(Json(json!(folders)))
        }
        "list_labels" => {
            let labels = state
                .store
                .list_labels()
                .map_err(|error| ApiError::Internal(format!("Failed to list labels: {error}")))?;
            Ok(Json(json!(labels)))
        }
        "list_rules" => {
            let rules = state
                .store
                .list_rules()
                .map_err(|error| ApiError::Internal(format!("Failed to list rules: {error}")))?;
            Ok(Json(json!(rules)))
        }
        "list_snoozed" => {
            let snoozed = state.store.list_snoozed_messages().map_err(|error| {
                ApiError::Internal(format!("Failed to list snoozed messages: {error}"))
            })?;
            Ok(Json(json!(snoozed)))
        }
        "start_sync" => {
            let args: StartSyncArgs = parse_args(args)?;
            let _browser_poll_interval = args.poll_interval_secs;
            state
                .sync_manager
                .start_account_sync(&args.account_id)
                .await
                .map_err(|error| ApiError::Internal(format!("Failed to start sync: {error}")))?;
            Ok(Json(json!("Sync started")))
        }
        "trigger_sync" => {
            let args: TriggerSyncArgs = parse_args(args)?;
            let _reason = args.reason;
            state
                .sync_manager
                .trigger_sync(&args.account_id)
                .await
                .map_err(|error| ApiError::Internal(format!("Failed to trigger sync: {error}")))?;
            Ok(Json(Value::Null))
        }
        "stop_sync" => {
            let args: AccountArgs = parse_args(args)?;
            state.sync_manager.stop_account_sync(&args.account_id).await;
            Ok(Json(Value::Null))
        }
        "get_realtime_preference" => {
            let mode = read_encrypted_json::<String>(&state, "realtime_preference")?
                .unwrap_or_else(|| "balanced".to_string());
            Ok(Json(json!(mode)))
        }
        "set_realtime_preference" => {
            let args: RealtimePreferenceArgs = serde_json::from_value(args).map_err(|error| {
                ApiError::BadRequest(format!("Invalid command arguments: {error}"))
            })?;
            let interval = crate::config::realtime_poll_interval(&args.mode)
                .ok_or_else(|| ApiError::BadRequest("Invalid realtime preference".into()))?;
            write_encrypted_json(&state, "realtime_preference", &args.mode)?;
            state
                .sync_manager
                .set_realtime_poll_interval(interval)
                .await
                .map_err(|error| {
                    ApiError::Internal(format!("Failed to restart sync workers: {error}"))
                })?;
            let _ = state.ws_broadcast.send(
                json!({
                    "type": "app:realtime-preference",
                    "payload": { "mode": args.mode }
                })
                .to_string(),
            );
            Ok(Json(Value::Null))
        }
        "get_auto_backup_status" => {
            let status = read_encrypted_json::<AutoBackupStatus>(&state, "auto_backup_status")?
                .unwrap_or(AutoBackupStatus {
                    last_success_at: None,
                    last_error: None,
                });
            Ok(Json(json!(status)))
        }
        "get_email_signature" => {
            let args: SignatureArgs = parse_args(args)?;
            let signature = secure_user_json(
                &state,
                &format!("signature:{}", args.account_id),
                String::new(),
            )?;
            Ok(Json(json!(signature)))
        }
        "set_email_signature" | "migrate_email_signature_if_absent" => {
            let args: SignatureArgs = parse_args(args)?;
            let key = format!("signature:{}", args.account_id);
            let signature = args.signature.unwrap_or_default();
            if command == "migrate_email_signature_if_absent" {
                let current: String = secure_user_json(&state, &key, String::new())?;
                if !current.is_empty() {
                    return Ok(Json(json!(current)));
                }
            }
            set_secure_user_json(&state, &key, &signature)?;
            if command == "migrate_email_signature_if_absent" {
                Ok(Json(json!(signature)))
            } else {
                Ok(Json(Value::Null))
            }
        }
        "list_email_templates" => {
            let templates: Vec<Value> = secure_user_json(&state, "email_templates", Vec::new())?;
            Ok(Json(json!(templates)))
        }
        "save_email_template" => {
            let args: TemplateSaveArgs = parse_args(args)?;
            let mut templates: Vec<Value> =
                secure_user_json(&state, "email_templates", Vec::new())?;
            let template = args.template;
            if template.name.trim().is_empty() {
                return Err(ApiError::BadRequest("Template name is required".into()));
            }
            if template.deduplicate_by_content {
                if let Some(existing) = templates.iter().find(|item| {
                    item["name"] == template.name
                        && item["subject"] == template.subject
                        && item["body"] == template.body
                }) {
                    return Ok(Json(existing.clone()));
                }
            }
            let saved = json!({
                "id": template.id.unwrap_or_else(pebble_core::new_id),
                "name": template.name,
                "subject": template.subject,
                "body": template.body,
                "createdAt": template.created_at.unwrap_or_else(pebble_core::now_timestamp),
            });
            templates.push(saved.clone());
            set_secure_user_json(&state, "email_templates", &templates)?;
            Ok(Json(saved))
        }
        "delete_email_template" => {
            let args: TemplateIdArgs = parse_args(args)?;
            let mut templates: Vec<Value> =
                secure_user_json(&state, "email_templates", Vec::new())?;
            templates.retain(|template| template["id"] != args.id);
            set_secure_user_json(&state, "email_templates", &templates)?;
            Ok(Json(Value::Null))
        }
        _ => Err(ApiError::NotFound(format!(
            "Unsupported command: {command}"
        ))),
    }
}
