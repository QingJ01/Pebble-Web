use crate::error::ApiError;
use crate::state::AppStateRef;
use axum::{
    extract::{Path, State},
    Json,
};
use pebble_core::{now_timestamp, SnoozedMessage};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnoozeRequest {
    pub until: i64,
    pub return_to: String,
}

pub async fn snooze_message(
    State(state): State<AppStateRef>,
    Path(message_id): Path<String>,
    Json(body): Json<SnoozeRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if body.until <= now_timestamp() {
        return Err(ApiError::BadRequest(
            "Snooze time must be in the future".to_string(),
        ));
    }

    state
        .store
        .snooze_message(&SnoozedMessage {
            message_id,
            snoozed_at: now_timestamp(),
            unsnoozed_at: body.until,
            return_to: body.return_to,
        })
        .map_err(|e| ApiError::Internal(format!("Failed to snooze message: {e}")))?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn unsnooze_message(
    State(state): State<AppStateRef>,
    Path(message_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    state
        .store
        .unsnooze_message(&message_id)
        .map_err(|e| ApiError::Internal(format!("Failed to unsnooze message: {e}")))?;
    let _ = state.ws_broadcast.send(
        serde_json::json!({
            "type": "mail:unsnoozed",
            "payload": { "message_id": message_id }
        })
        .to_string(),
    );

    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn list_snoozed(
    State(state): State<AppStateRef>,
) -> Result<Json<Vec<SnoozedMessage>>, ApiError> {
    let snoozed = state
        .store
        .list_snoozed_messages()
        .map_err(|e| ApiError::Internal(format!("Failed to list snoozed messages: {e}")))?;

    Ok(Json(snoozed))
}
