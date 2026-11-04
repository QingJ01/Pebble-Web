use crate::error::ApiError;
use crate::state::AppStateRef;
use axum::{extract::State, Json};
use pebble_core::{now_timestamp, TranslateConfig};
use pebble_translate::types::TranslateProviderConfig;
use pebble_translate::TranslateService;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct TranslateRequest {
    pub text: String,
    #[serde(alias = "to_lang")]
    pub target_lang: String,
    #[serde(alias = "from_lang")]
    pub source_lang: Option<String>,
}

#[derive(Serialize)]
pub struct TranslateResponse {
    pub translated: String,
}

pub async fn translate(
    State(state): State<AppStateRef>,
    Json(body): Json<TranslateRequest>,
) -> Result<Json<TranslateResponse>, ApiError> {
    let store = state.store.clone();

    // Load translation config from the store
    let config = store
        .get_translate_config()
        .map_err(|e| ApiError::Internal(format!("Failed to load translate config: {e}")))?;

    let config =
        config.ok_or_else(|| ApiError::BadRequest("Translation is not configured".to_string()))?;

    if !config.is_enabled {
        return Err(ApiError::BadRequest(
            "Translation is currently disabled".to_string(),
        ));
    }

    // Parse the provider config from the stored JSON blob
    let provider_config: TranslateProviderConfig = serde_json::from_str(&config.config)
        .map_err(|e| ApiError::Internal(format!("Invalid translate provider config: {e}")))?;

    let source_lang = body.source_lang.as_deref().unwrap_or("auto");

    let result =
        TranslateService::translate(&provider_config, &body.text, source_lang, &body.target_lang)
            .await
            .map_err(|e| ApiError::Internal(format!("Translation failed: {e}")))?;

    Ok(Json(TranslateResponse {
        translated: result.translated,
    }))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslateConfigResponse {
    pub provider_type: String,
    pub config: String,
    pub is_enabled: bool,
}

pub async fn get_translate_config(
    State(state): State<AppStateRef>,
) -> Result<Json<Option<TranslateConfigResponse>>, ApiError> {
    let config = state
        .store
        .get_translate_config()
        .map_err(|e| ApiError::Internal(format!("Failed to load translate config: {e}")))?;

    Ok(Json(config.map(|c| TranslateConfigResponse {
        provider_type: c.provider_type,
        config: c.config,
        is_enabled: c.is_enabled,
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveTranslateConfigRequest {
    pub provider_type: String,
    pub config: String,
    pub is_enabled: bool,
}

pub async fn save_translate_config(
    State(state): State<AppStateRef>,
    Json(body): Json<SaveTranslateConfigRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let now = now_timestamp();
    let config = TranslateConfig {
        id: "active".to_string(),
        provider_type: body.provider_type,
        config: body.config,
        is_enabled: body.is_enabled,
        created_at: now,
        updated_at: now,
    };

    state
        .store
        .save_translate_config(&config)
        .map_err(|e| ApiError::Internal(format!("Failed to save translate config: {e}")))?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn test_translate_connection(
    Json(body): Json<SaveTranslateConfigRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let provider_config: TranslateProviderConfig = serde_json::from_str(&body.config)
        .map_err(|e| ApiError::BadRequest(format!("Invalid provider config: {e}")))?;

    let result = TranslateService::translate(&provider_config, "Hello", "auto", "zh")
        .await
        .map_err(|e| ApiError::BadRequest(format!("Connection test failed: {e}")))?;

    Ok(Json(
        serde_json::json!({ "ok": true, "result": result.translated }),
    ))
}
