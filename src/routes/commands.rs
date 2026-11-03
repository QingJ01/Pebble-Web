use crate::error::ApiError;
use crate::state::AppStateRef;
use axum::{extract::{Path, State}, Json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

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
    let Some(ciphertext) = state.store.get_secure_user_data(key)
        .map_err(|error| ApiError::Internal(format!("Failed to read {key}: {error}")))?
    else {
        return Ok(default);
    };
    let plaintext = match state.crypto.decrypt_for("secure_user_data.value", key, &ciphertext) {
        Ok(plaintext) => plaintext,
        Err(_) => {
            let _: T = serde_json::from_slice(&ciphertext)
                .map_err(|error| ApiError::Internal(format!("Invalid legacy {key}: {error}")))?;
            let encrypted = state.crypto.encrypt_for("secure_user_data.value", key, &ciphertext)
                .map_err(|error| ApiError::Internal(format!("Failed to migrate {key}: {error}")))?;
            if !state.store.compare_exchange_secure_user_data(key, &ciphertext, &encrypted)
                .map_err(|error| ApiError::Internal(format!("Failed to persist migrated {key}: {error}")))?
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
    let encrypted = state.crypto.encrypt_for("secure_user_data.value", key, &plaintext)
        .map_err(|error| ApiError::Internal(format!("Failed to encrypt {key}: {error}")))?;
    state.store.set_secure_user_data(key, &encrypted)
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

fn read_encrypted_json<T: for<'de> Deserialize<'de>>(
    state: &AppStateRef,
    key: &str,
) -> Result<Option<T>, ApiError> {
    let Some(value) = state.store.get_secure_user_data(key)
        .map_err(|error| ApiError::Internal(format!("Failed to read {key}: {error}")))?
    else {
        return Ok(None);
    };
    let plaintext = state.crypto.decrypt(&value)
        .map_err(|error| ApiError::Internal(format!("Failed to decrypt {key}: {error}")))?;
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
    let encrypted = state.crypto.encrypt(&plaintext)
        .map_err(|error| ApiError::Internal(format!("Failed to encrypt {key}: {error}")))?;
    state.store.set_secure_user_data(key, &encrypted)
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
        "list_accounts" => {
            let accounts = state.store.list_accounts()
                .map_err(|error| ApiError::Internal(format!("Failed to list accounts: {error}")))?;
            Ok(Json(json!(accounts)))
        }
        "get_account_sync_range" => {
            let args: AccountArgs = parse_args(args)?;
            let days = state.store.get_sync_state(&args.account_id)
                .map_err(|error| ApiError::Internal(format!("Failed to read sync range: {error}")))?
                .and_then(|sync_state| sync_state.sync_range_days);
            Ok(Json(json!(days)))
        }
        "update_account_sync_range" => {
            let args: UpdateSyncRangeArgs = parse_args(args)?;
            pebble_store::accounts::validate_sync_range_days(args.days)
                .map_err(|error| ApiError::BadRequest(error.to_string()))?;
            let cutoff = pebble_store::accounts::sync_range_cutoff(args.days, pebble_core::now_timestamp());
            state.store.update_sync_state(&args.account_id, |sync_state| {
                sync_state.sync_range_days = args.days;
                sync_state.sync_since = cutoff;
                Ok(())
            }).map_err(|error| ApiError::Internal(format!("Failed to update sync range: {error}")))?;
            if let Some(interval) = args.poll_interval_secs {
                state.sync_manager.set_realtime_poll_interval(interval).await
                    .map_err(|error| ApiError::Internal(format!("Failed to restart sync workers: {error}")))?;
            } else {
                state.sync_manager.start_account_sync(&args.account_id).await
                    .map_err(|error| ApiError::Internal(format!("Failed to restart account sync: {error}")))?;
            }
            Ok(Json(Value::Null))
        }
        "list_folders" => {
            let args: AccountArgs = parse_args(args)?;
            let folders = state.store.list_folders(&args.account_id)
                .map_err(|error| ApiError::Internal(format!("Failed to list folders: {error}")))?;
            Ok(Json(json!(folders)))
        }
        "list_labels" => {
            let labels = state.store.list_labels()
                .map_err(|error| ApiError::Internal(format!("Failed to list labels: {error}")))?;
            Ok(Json(json!(labels)))
        }
        "list_rules" => {
            let rules = state.store.list_rules()
                .map_err(|error| ApiError::Internal(format!("Failed to list rules: {error}")))?;
            Ok(Json(json!(rules)))
        }
        "list_snoozed" => {
            let snoozed = state.store.list_snoozed_messages()
                .map_err(|error| ApiError::Internal(format!("Failed to list snoozed messages: {error}")))?;
            Ok(Json(json!(snoozed)))
        }
        "start_sync" => {
            let args: AccountArgs = parse_args(args)?;
            state.sync_manager.start_account_sync(&args.account_id).await
                .map_err(|error| ApiError::Internal(format!("Failed to start sync: {error}")))?;
            Ok(Json(json!("Sync started")))
        }
        "trigger_sync" => {
            let args: TriggerSyncArgs = parse_args(args)?;
            let _reason = args.reason;
            state.sync_manager.trigger_sync(&args.account_id).await
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
            let args: RealtimePreferenceArgs = serde_json::from_value(args)
                .map_err(|error| ApiError::BadRequest(format!("Invalid command arguments: {error}")))?;
            let interval = match args.mode.as_str() {
                "realtime" => 15,
                "balanced" => state.config.sync_interval_secs,
                "battery" => 300,
                "manual" => 0,
                _ => return Err(ApiError::BadRequest("Invalid realtime preference".into())),
            };
            write_encrypted_json(&state, "realtime_preference", &args.mode)?;
            state.sync_manager.set_realtime_poll_interval(interval).await
                .map_err(|error| ApiError::Internal(format!("Failed to restart sync workers: {error}")))?;
            let _ = state.ws_broadcast.send(json!({
                "type": "app:realtime-preference",
                "payload": { "mode": args.mode }
            }).to_string());
            Ok(Json(Value::Null))
        }
        "get_auto_backup_status" => {
            let status = read_encrypted_json::<AutoBackupStatus>(&state, "auto_backup_status")?
                .unwrap_or(AutoBackupStatus { last_success_at: None, last_error: None });
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
            let templates: Vec<Value> =
                secure_user_json(&state, "email_templates", Vec::new())?;
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
        _ => Err(ApiError::NotFound(format!("Unsupported command: {command}"))),
    }
}
