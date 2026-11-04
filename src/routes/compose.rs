use crate::error::ApiError;
use crate::state::{AppState, AppStateRef};
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeRequest {
    pub account_id: String,
    pub to: Vec<String>,
    pub cc: Option<Vec<String>>,
    pub bcc: Option<Vec<String>>,
    pub subject: String,
    pub body_text: String,
    pub body_html: Option<String>,
    pub in_reply_to: Option<String>,
    pub attachment_paths: Option<Vec<String>>,
}

pub async fn send_with_service(state: &AppState, body: ComposeRequest) -> Result<(), ApiError> {
    crate::mail_service::compose::send_email(
        state,
        body.account_id,
        body.to,
        body.cc.unwrap_or_default(),
        body.bcc.unwrap_or_default(),
        body.subject,
        body.body_text,
        body.body_html,
        body.in_reply_to,
        body.attachment_paths,
    )
    .await?;
    Ok(())
}

pub async fn send_email(
    State(state): State<AppStateRef>,
    Json(body): Json<ComposeRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    send_with_service(&state, body).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageAttachmentRequest {
    pub filename: String,
    pub data: String, // base64-encoded file content
}

#[derive(Serialize)]
pub struct StageAttachmentResponse {
    pub path: String,
}

pub async fn stage_attachment(
    State(state): State<AppStateRef>,
    Json(body): Json<StageAttachmentRequest>,
) -> Result<Json<StageAttachmentResponse>, ApiError> {
    use base64::Engine;

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&body.data)
        .map_err(|e| ApiError::BadRequest(format!("Invalid base64 data: {e}")))?;
    let path =
        crate::routes::safe_files::stage_bytes(&state.attachments_dir, &body.filename, &bytes)
            .await?;
    Ok(Json(StageAttachmentResponse { path }))
}
