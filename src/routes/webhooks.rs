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
    pub existing_endpoint_id: Option<String>,
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
    if existing.endpoint.provider == body.provider {
        let existing_config = decrypt_config(&state, &existing.encrypted_config)?;
        merge_missing_config_fields(&mut config, existing_config);
    }
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
    State(state): State<AppStateRef>,
    Json(body): Json<TestWebhookRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut config = parse_config(body.config)?;
    if let Some(id) = body.existing_endpoint_id.as_deref() {
        let existing = state
            .store
            .get_webhook_endpoint(id)
            .map_err(|e| ApiError::Internal(format!("Failed to load webhook: {e}")))?
            .ok_or_else(|| ApiError::NotFound("Webhook endpoint not found".to_string()))?;
        if existing.endpoint.provider == body.provider {
            let existing_config = decrypt_config(&state, &existing.encrypted_config)?;
            merge_missing_config_fields(&mut config, existing_config);
        }
    }
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
    if config.url.is_none() {
        config.url = existing.url;
    }
    if config.method.is_none() {
        config.method = existing.method;
    }
    if config.headers.is_empty() {
        config.headers = existing.headers;
    }
    if config.bot_token.is_none() {
        config.bot_token = existing.bot_token;
    }
    if config.chat_id.is_none() {
        config.chat_id = existing.chat_id;
    }
    if config.parse_mode.is_none() {
        config.parse_mode = existing.parse_mode;
    }
    if config.secret.is_none() {
        config.secret = existing.secret;
    }
    if config.server_url.is_none() {
        config.server_url = existing.server_url;
    }
    if config.topic.is_none() {
        config.topic = existing.topic;
    }
    if config.token.is_none() {
        config.token = existing.token;
    }
    if config.priority.is_none() {
        config.priority = existing.priority;
    }
}

fn validate_name(name: &str) -> Result<(), ApiError> {
    if name.trim().is_empty() {
        Err(ApiError::BadRequest("Webhook name is required".to_string()))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn existing_config() -> WebhookConfig {
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Bearer old".to_string());

        WebhookConfig {
            url: Some("https://example.com/webhook".to_string()),
            method: Some("PUT".to_string()),
            headers,
            bot_token: Some("old-bot-token".to_string()),
            chat_id: Some("old-chat-id".to_string()),
            parse_mode: Some("Markdown".to_string()),
            secret: Some("old-secret".to_string()),
            server_url: Some("https://ntfy.example.com".to_string()),
            topic: Some("old-topic".to_string()),
            token: Some("old-token".to_string()),
            priority: Some(4),
        }
    }

    #[test]
    fn merge_missing_config_fields_preserves_existing_values() {
        let mut config = WebhookConfig::default();

        merge_missing_config_fields(&mut config, existing_config());

        assert_eq!(config.url.as_deref(), Some("https://example.com/webhook"));
        assert_eq!(config.method.as_deref(), Some("PUT"));
        assert_eq!(
            config.headers.get("Authorization").map(String::as_str),
            Some("Bearer old")
        );
        assert_eq!(config.bot_token.as_deref(), Some("old-bot-token"));
        assert_eq!(config.chat_id.as_deref(), Some("old-chat-id"));
        assert_eq!(config.parse_mode.as_deref(), Some("Markdown"));
        assert_eq!(config.secret.as_deref(), Some("old-secret"));
        assert_eq!(
            config.server_url.as_deref(),
            Some("https://ntfy.example.com")
        );
        assert_eq!(config.topic.as_deref(), Some("old-topic"));
        assert_eq!(config.token.as_deref(), Some("old-token"));
        assert_eq!(config.priority, Some(4));
    }

    #[test]
    fn merge_missing_config_fields_keeps_provided_values() {
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Bearer new".to_string());
        let mut config = WebhookConfig {
            url: Some("https://new.example.com/webhook".to_string()),
            method: Some("PATCH".to_string()),
            headers,
            bot_token: Some("new-bot-token".to_string()),
            chat_id: Some("new-chat-id".to_string()),
            parse_mode: Some("HTML".to_string()),
            secret: Some("new-secret".to_string()),
            server_url: Some("https://new-ntfy.example.com".to_string()),
            topic: Some("new-topic".to_string()),
            token: Some("new-token".to_string()),
            priority: Some(2),
        };

        merge_missing_config_fields(&mut config, existing_config());

        assert_eq!(config.url.as_deref(), Some("https://new.example.com/webhook"));
        assert_eq!(config.method.as_deref(), Some("PATCH"));
        assert_eq!(
            config.headers.get("Authorization").map(String::as_str),
            Some("Bearer new")
        );
        assert_eq!(config.bot_token.as_deref(), Some("new-bot-token"));
        assert_eq!(config.chat_id.as_deref(), Some("new-chat-id"));
        assert_eq!(config.parse_mode.as_deref(), Some("HTML"));
        assert_eq!(config.secret.as_deref(), Some("new-secret"));
        assert_eq!(
            config.server_url.as_deref(),
            Some("https://new-ntfy.example.com")
        );
        assert_eq!(config.topic.as_deref(), Some("new-topic"));
        assert_eq!(config.token.as_deref(), Some("new-token"));
        assert_eq!(config.priority, Some(2));
    }
}
