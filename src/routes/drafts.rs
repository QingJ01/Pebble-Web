use crate::error::ApiError;
use crate::state::{AppState, AppStateRef};
use axum::{
    extract::{Path, State},
    Json,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveDraftRequest {
    pub account_id: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body_text: String,
    pub body_html: Option<String>,
    pub in_reply_to: Option<String>,
    pub existing_draft_id: Option<String>,
    pub attachment_paths: Option<Vec<String>>,
}

pub async fn save_with_service(
    state: &AppState,
    body: SaveDraftRequest,
) -> Result<String, ApiError> {
    Ok(crate::mail_service::drafts::save_draft(
        state,
        body.account_id,
        body.to,
        body.cc,
        body.bcc,
        body.subject,
        body.body_text,
        body.body_html,
        body.in_reply_to,
        body.attachment_paths,
        body.existing_draft_id,
    )
    .await?)
}

pub async fn save_draft(
    State(state): State<AppStateRef>,
    Json(body): Json<SaveDraftRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let id = save_with_service(&state, body).await?;
    Ok(Json(serde_json::json!({ "id": id })))
}

pub async fn delete_draft(
    State(state): State<AppStateRef>,
    Path((account_id, draft_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    crate::mail_service::drafts::delete_draft(&state, account_id, draft_id).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
