use crate::error::ApiError;
use crate::state::AppStateRef;
use axum::{extract::State, Json};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerSyncRequest {
    pub account_id: String,
}

pub async fn trigger_sync(
    State(state): State<AppStateRef>,
    Json(body): Json<TriggerSyncRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    match state.sync_manager.trigger_sync(&body.account_id).await {
        Ok(()) => Ok(Json(serde_json::json!({ "ok": true }))),
        Err(e) if e.contains("No sync worker running") => {
            state
                .sync_manager
                .start_account_sync(&body.account_id)
                .await
                .map_err(|err| {
                    ApiError::Internal(format!("Failed to start sync for account: {err}"))
                })?;
            // Worker starts with an initial sync pass; no extra manual trigger needed.
            Ok(Json(serde_json::json!({ "ok": true, "started": true })))
        }
        Err(e) => Err(ApiError::Internal(format!("Failed to trigger sync: {e}"))),
    }
}
