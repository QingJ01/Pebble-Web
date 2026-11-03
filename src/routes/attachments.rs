use crate::error::ApiError;
use crate::state::AppStateRef;
use axum::{
    body::Body,
    extract::{Path, State},
    http::header,
    response::Response,
    Json,
};
use pebble_core::Attachment;
use tokio_util::io::ReaderStream;

pub async fn list_attachments(
    State(state): State<AppStateRef>,
    Path(message_id): Path<String>,
) -> Result<Json<Vec<Attachment>>, ApiError> {
    let store = state.store.clone();

    let attachments = store
        .with_read_async(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, message_id, filename, mime_type, size, local_path, content_id, is_inline
                 FROM attachments WHERE message_id = ?1",
            )?;
            let rows = stmt.query_map(rusqlite::params![message_id], |row| {
                Ok(Attachment {
                    id: row.get(0)?,
                    message_id: row.get(1)?,
                    filename: row.get(2)?,
                    mime_type: row.get(3)?,
                    size: row.get(4)?,
                    local_path: row.get(5)?,
                    content_id: row.get(6)?,
                    is_inline: row.get(7)?,
                })
            })?;
            let mut results = Vec::new();
            for row in rows {
                results.push(row?);
            }
            Ok(results)
        })
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to list attachments: {e}")))?;

    Ok(Json(attachments))
}

pub async fn download_attachment(
    State(state): State<AppStateRef>,
    Path(attachment_id): Path<String>,
) -> Result<Response, ApiError> {
    let store = state.store.clone();
    let attachments_dir = state.attachments_dir.clone();

    let attachment = store
        .with_read_async(move |conn| {
            let result = conn
                .query_row(
                    "SELECT id, message_id, filename, mime_type, size, local_path, content_id, is_inline
                     FROM attachments WHERE id = ?1",
                    rusqlite::params![attachment_id],
                    |row| {
                        Ok(Attachment {
                            id: row.get(0)?,
                            message_id: row.get(1)?,
                            filename: row.get(2)?,
                            mime_type: row.get(3)?,
                            size: row.get(4)?,
                            local_path: row.get(5)?,
                            content_id: row.get(6)?,
                            is_inline: row.get(7)?,
                        })
                    },
                )
                .optional();
            match result {
                Ok(att) => Ok(att),
                Err(e) => Err(pebble_core::PebbleError::Storage(e.to_string())),
            }
        })
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to get attachment: {e}")))?;

    let attachment =
        attachment.ok_or_else(|| ApiError::BadRequest("Attachment not found".to_string()))?;

    let file_path = match &attachment.local_path {
        Some(path) => std::path::PathBuf::from(path),
        None => {
            // Sanitize filename by stripping path separators
            let safe_filename = std::path::Path::new(&attachment.filename)
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or("attachment")
                .to_string();
            attachments_dir
                .join(&attachment.message_id)
                .join(&safe_filename)
        }
    };

    // Validate resolved path is within the allowed attachments directory
    let allowed_dir = attachments_dir
        .canonicalize()
        .unwrap_or_else(|_| attachments_dir.clone());
    let canonical_path = file_path
        .canonicalize()
        .map_err(|_| ApiError::NotFound("Attachment file not found".to_string()))?;
    if !canonical_path.starts_with(&allowed_dir) {
        return Err(ApiError::BadRequest(
            "Attachment path outside allowed directory".to_string(),
        ));
    }

    let file = tokio::fs::File::open(&canonical_path)
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to open file: {e}")))?;

    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    let content_type = attachment.mime_type.clone();
    let filename = attachment.filename.clone();

    let response = Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", filename),
        )
        .body(body)
        .map_err(|e| ApiError::Internal(format!("Failed to build response: {e}")))?;

    Ok(response)
}

use rusqlite::OptionalExtension;
