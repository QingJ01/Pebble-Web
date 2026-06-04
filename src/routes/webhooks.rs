use crate::error::ApiError;
use crate::notifications::{send_notification, NotificationMessage, WebhookConfig};
use crate::state::AppStateRef;
use axum::{
    extract::{Path, State},
    Json,
};
use pebble_core::{new_id, now_timestamp, StoredWebhookEndpoint, WebhookEndpoint, WebhookProvider};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveWebhookEndpointRequest {
    pub name: String,
    pub provider: WebhookProvider,
    pub config: Value,
    pub is_enabled: bool,
    pub notify_on_new_mail: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestWebhookRequest {
    pub provider: WebhookProvider,
    pub config: Value,
}

pub async fn list_webhooks(
    State(state): State<AppStateRef>,
) -> Result<Json<Vec<WebhookEndpoint>>, ApiError> {
    let endpoints = state
        .store
        .list_webhook_endpoints()
        .map_err(|e| ApiError::Internal(format!("Failed to list webhooks: {e}")))?;
    Ok(Json(endpoints))
}

pub async fn create_webhook(
    State(state): State<AppStateRef>,
    Json(body): Json<SaveWebhookEndpointRequest>,
) -> Result<Json<WebhookEndpoint>, ApiError> {
    validate_name(&body.name)?;
    let config = parse_config(body.config)?;
    crate::notifications::validate_config(&body.provider, &config).map_err(ApiError::BadRequest)?;

    let encrypted_config = encrypt_config(&state, &config)?;
    let now = now_timestamp();
    let endpoint = WebhookEndpoint {
        id: new_id(),
        name: body.name.trim().to_string(),
        provider: body.provider,
        is_enabled: body.is_enabled,
        notify_on_new_mail: body.notify_on_new_mail,
        created_at: now,
        updated_at: now,
    };

    state
        .store
        .insert_webhook_endpoint(&StoredWebhookEndpoint {
            endpoint: endpoint.clone(),
            encrypted_config,
        })
        .map_err(|e| ApiError::Internal(format!("Failed to create webhook: {e}")))?;

    Ok(Json(endpoint))
}

pub async fn update_webhook(
    State(state): State<AppStateRef>,
    Path(id): Path<String>,
    Json(body): Json<SaveWebhookEndpointRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    validate_name(&body.name)?;
    let existing = state
        .store
        .get_webhook_endpoint(&id)
        .map_err(|e| ApiError::Internal(format!("Failed to load webhook: {e}")))?
        .ok_or_else(|| ApiError::NotFound("Webhook endpoint not found".to_string()))?;

    let mut config = parse_config(body.config)?;
    let existing_config = decrypt_config(&state, &existing.encrypted_config)?;
    merge_missing_config_fields(&mut config, existing_config);
    crate::notifications::validate_config(&body.provider, &config).map_err(ApiError::BadRequest)?;
    let encrypted_config = encrypt_config(&state, &config)?;

    state
        .store
        .update_webhook_endpoint(&StoredWebhookEndpoint {
            endpoint: WebhookEndpoint {
                id,
                name: body.name.trim().to_string(),
                provider: body.provider,
                is_enabled: body.is_enabled,
                notify_on_new_mail: body.notify_on_new_mail,
                created_at: existing.endpoint.created_at,
                updated_at: now_timestamp(),
            },
            encrypted_config,
        })
        .map_err(|e| ApiError::Internal(format!("Failed to update webhook: {e}")))?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn delete_webhook(
    State(state): State<AppStateRef>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    state
        .store
        .delete_webhook_endpoint(&id)
        .map_err(|e| ApiError::Internal(format!("Failed to delete webhook: {e}")))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn test_webhook(
    Json(body): Json<TestWebhookRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config = parse_config(body.config)?;
    send_notification(&body.provider, &config, &NotificationMessage::test())
        .await
        .map_err(ApiError::BadRequest)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn test_saved_webhook(
    State(state): State<AppStateRef>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let stored = state
        .store
        .get_webhook_endpoint(&id)
        .map_err(|e| ApiError::Internal(format!("Failed to load webhook: {e}")))?
        .ok_or_else(|| ApiError::NotFound("Webhook endpoint not found".to_string()))?;
    let config = decrypt_config(&state, &stored.encrypted_config)?;
    send_notification(&stored.endpoint.provider, &config, &NotificationMessage::test())
        .await
        .map_err(ApiError::BadRequest)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub(crate) fn decrypt_config(state: &AppStateRef, encrypted: &[u8]) -> Result<WebhookConfig, ApiError> {
    let bytes = state
        .crypto
        .decrypt(encrypted)
        .map_err(|e| ApiError::Internal(format!("Failed to decrypt webhook config: {e}")))?;
    serde_json::from_slice(&bytes)
        .map_err(|e| ApiError::Internal(format!("Invalid webhook config: {e}")))
}

fn encrypt_config(state: &AppStateRef, config: &WebhookConfig) -> Result<Vec<u8>, ApiError> {
    let bytes = serde_json::to_vec(config)
        .map_err(|e| ApiError::Internal(format!("Failed to serialize webhook config: {e}")))?;
    state
        .crypto
        .encrypt(&bytes)
        .map_err(|e| ApiError::Internal(format!("Failed to encrypt webhook config: {e}")))
}

fn parse_config(value: Value) -> Result<WebhookConfig, ApiError> {
    serde_json::from_value(value).map_err(|e| ApiError::BadRequest(format!("Invalid webhook config: {e}")))
}

fn merge_missing_config_fields(config: &mut WebhookConfig, existing: WebhookConfig) {
    if config.url.as_deref().unwrap_or_default().trim().is_empty() {
        config.url = existing.url;
    }
    if config.bot_token.as_deref().unwrap_or_default().trim().is_empty() {
        config.bot_token = existing.bot_token;
    }
    if config.chat_id.as_deref().unwrap_or_default().trim().is_empty() {
        config.chat_id = existing.chat_id;
    }
    if config.parse_mode.as_deref().unwrap_or_default().trim().is_empty() {
        config.parse_mode = existing.parse_mode;
    }
    if config.secret.as_deref().unwrap_or_default().trim().is_empty() {
        config.secret = existing.secret;
    }
    if config.server_url.as_deref().unwrap_or_default().trim().is_empty() {
        config.server_url = existing.server_url;
    }
    if config.topic.as_deref().unwrap_or_default().trim().is_empty() {
        config.topic = existing.topic;
    }
    if config.token.as_deref().unwrap_or_default().trim().is_empty() {
        config.token = existing.token;
    }
}

fn validate_name(name: &str) -> Result<(), ApiError> {
    if name.trim().is_empty() {
        Err(ApiError::BadRequest("Webhook name is required".to_string()))
    } else {
        Ok(())
    }
}
