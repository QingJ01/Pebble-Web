use crate::error::ApiError;
use crate::state::AppStateRef;
use axum::{
    extract::{Path, State},
    Json,
};
use pebble_core::{
    new_id, now_timestamp, Attachment, EmailAddress, Folder, FolderRole, FolderType, Message,
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

fn address_list(values: Vec<String>) -> Vec<EmailAddress> {
    values
        .into_iter()
        .map(|value| EmailAddress {
            name: None,
            address: value,
        })
        .collect()
}

async fn copy_attachments_to_draft(
    root: &std::path::Path,
    draft_id: &str,
    paths: &[String],
) -> Result<Vec<Attachment>, ApiError> {
    if draft_id.len() != 36
        || !draft_id
            .chars()
            .all(|ch| ch.is_ascii_hexdigit() || ch == '-')
    {
        return Err(ApiError::BadRequest("Invalid draft identifier".into()));
    }
    let canonical_root = root
        .canonicalize()
        .map_err(|_| ApiError::BadRequest("Attachment storage is unavailable".into()))?;
    let drafts_root = canonical_root.join("drafts");
    tokio::fs::create_dir_all(&drafts_root)
        .await
        .map_err(|error| {
            ApiError::Internal(format!(
                "Failed to create draft attachment storage: {error}"
            ))
        })?;
    let canonical_drafts_root = drafts_root
        .canonicalize()
        .map_err(|error| ApiError::Internal(format!("Failed to resolve draft storage: {error}")))?;
    if !canonical_drafts_root.starts_with(&canonical_root) {
        return Err(ApiError::BadRequest(
            "Draft storage outside attachment storage".into(),
        ));
    }
    let draft_dir = canonical_drafts_root.join(draft_id);
    tokio::fs::create_dir_all(&draft_dir)
        .await
        .map_err(|error| {
            ApiError::Internal(format!(
                "Failed to create draft attachment directory: {error}"
            ))
        })?;
    let canonical_draft_dir = draft_dir.canonicalize().map_err(|error| {
        ApiError::Internal(format!("Failed to resolve draft directory: {error}"))
    })?;
    if !canonical_draft_dir.starts_with(&canonical_root) {
        return Err(ApiError::BadRequest(
            "Draft directory outside attachment storage".into(),
        ));
    }

    let mut attachments: Vec<Attachment> = Vec::with_capacity(paths.len());
    let copy_result: Result<(), ApiError> = async {
        let mut total_size = 0u64;
        for supplied in paths {
            let source = crate::routes::safe_files::resolve_attachment_path(root, supplied)?;
            let source_metadata = tokio::fs::metadata(&source).await.map_err(|error| {
                ApiError::BadRequest(format!("Attachment unavailable: {error}"))
            })?;
            total_size = total_size.saturating_add(source_metadata.len());
            if source_metadata.len() > 25 * 1024 * 1024 || total_size > 25 * 1024 * 1024 {
                return Err(ApiError::BadRequest(
                    "Attachments exceed the 25 MiB total size limit".into(),
                ));
            }
            let source_name = source
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("attachment");
            let filename = crate::routes::safe_files::safe_filename(source_name)?;
            let destination = canonical_draft_dir.join(format!("{}_{}", new_id(), filename));
            tokio::fs::copy(&source, &destination)
                .await
                .map_err(|error| {
                    ApiError::Internal(format!("Failed to copy draft attachment: {error}"))
                })?;
            let metadata = tokio::fs::metadata(&destination).await.map_err(|error| {
                ApiError::Internal(format!("Failed to inspect draft attachment: {error}"))
            })?;
            attachments.push(Attachment {
                id: new_id(),
                message_id: draft_id.to_string(),
                filename,
                mime_type: "application/octet-stream".to_string(),
                size: metadata.len() as i64,
                local_path: Some(destination.to_string_lossy().to_string()),
                content_id: None,
                is_inline: false,
            });
        }
        Ok(())
    }
    .await;
    if let Err(error) = copy_result {
        for attachment in &attachments {
            if let Some(path) = &attachment.local_path {
                let _ = tokio::fs::remove_file(path).await;
            }
        }
        return Err(error);
    }
    Ok(attachments)
}

pub async fn save_draft(
    State(state): State<AppStateRef>,
    Json(body): Json<SaveDraftRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = state.store.clone();
    let account_id = body.account_id.clone();
    let draft_id = body.existing_draft_id.clone().unwrap_or_else(new_id);
    let now = now_timestamp();
    let attachment_paths = body.attachment_paths.unwrap_or_default();

    let account = state
        .store
        .get_account(&body.account_id)
        .map_err(|error| ApiError::Internal(format!("Failed to load draft account: {error}")))?
        .ok_or_else(|| ApiError::BadRequest("Account not found".into()))?;
    let _ = account;
    if body.existing_draft_id.is_some() {
        let existing = state
            .store
            .get_message(&draft_id)
            .map_err(|error| ApiError::Internal(format!("Failed to load existing draft: {error}")))?
            .ok_or_else(|| ApiError::BadRequest("Draft not found".into()))?;
        if !existing.is_draft || existing.account_id != body.account_id {
            return Err(ApiError::BadRequest(
                "Draft does not belong to this account".into(),
            ));
        }
    }
    let old_attachments = state
        .store
        .list_attachments_by_message(&draft_id)
        .map_err(|error| {
            ApiError::Internal(format!("Failed to load old draft attachments: {error}"))
        })?;
    let attachments =
        copy_attachments_to_draft(&state.attachments_dir, &draft_id, &attachment_paths).await?;

    let folder_id = store
        .with_write_async(move |conn| {
            let existing: Option<String> = conn
                .query_row(
                    "SELECT id FROM folders WHERE account_id = ?1 AND role = 'drafts'",
                    rusqlite::params![account_id],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(id) = existing {
                return Ok(id);
            }

            let folder = Folder {
                id: new_id(),
                account_id,
                remote_id: "__local_drafts__".to_string(),
                name: "Drafts".to_string(),
                folder_type: FolderType::Folder,
                role: Some(FolderRole::Drafts),
                parent_id: None,
                color: None,
                is_system: true,
                sort_order: 2,
            };
            conn.execute(
                "INSERT INTO folders (id, account_id, remote_id, name, folder_type, role, parent_id, color, is_system, sort_order)
                 VALUES (?1, ?2, ?3, ?4, 'folder', 'drafts', NULL, NULL, 1, ?5)",
                rusqlite::params![folder.id, folder.account_id, folder.remote_id, folder.name, folder.sort_order],
            )?;
            Ok(folder.id)
        })
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to prepare drafts folder: {e}")))?;

    let message = Message {
        id: draft_id.clone(),
        account_id: body.account_id,
        remote_id: draft_id.clone(),
        message_id_header: None,
        in_reply_to: body.in_reply_to,
        references_header: None,
        thread_id: None,
        subject: body.subject,
        snippet: body.body_text.chars().take(180).collect(),
        from_address: String::new(),
        from_name: String::new(),
        to_list: address_list(body.to),
        cc_list: address_list(body.cc),
        bcc_list: address_list(body.bcc),
        body_text: body.body_text,
        body_html_raw: body.body_html.unwrap_or_default(),
        has_attachments: !attachment_paths.is_empty(),
        is_read: true,
        is_starred: false,
        is_draft: true,
        date: now,
        remote_version: None,
        is_deleted: false,
        deleted_at: None,
        created_at: now,
        updated_at: now,
    };
    if let Err(error) =
        state
            .store
            .replace_message_with_attachments(&message, &[folder_id], &attachments)
    {
        for attachment in &attachments {
            if let Some(path) = &attachment.local_path {
                let _ = tokio::fs::remove_file(path).await;
            }
        }
        return Err(ApiError::Internal(format!("Failed to save draft: {error}")));
    }

    for attachment in old_attachments {
        if let Some(path) = attachment.local_path {
            let referenced = state
                .store
                .is_attachment_local_path_referenced(&path)
                .map_err(|error| {
                    ApiError::Internal(format!("Failed to check attachment references: {error}"))
                })?;
            if !referenced {
                let _ = tokio::fs::remove_file(path).await;
            }
        }
    }

    Ok(Json(serde_json::json!({ "id": draft_id })))
}

pub async fn delete_draft(
    State(state): State<AppStateRef>,
    Path((account_id, draft_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let message = state
        .store
        .get_message(&draft_id)
        .map_err(|error| ApiError::Internal(format!("Failed to read draft: {error}")))?
        .ok_or_else(|| ApiError::BadRequest("Draft not found".into()))?;
    if !message.is_draft || message.account_id != account_id {
        return Err(ApiError::BadRequest(
            "Draft does not belong to this account".into(),
        ));
    }
    let attachments = state
        .store
        .list_attachments_by_message(&draft_id)
        .map_err(|error| {
            ApiError::Internal(format!("Failed to load draft attachments: {error}"))
        })?;
    state
        .store
        .with_write_async(move |conn| {
            conn.execute(
                "DELETE FROM messages WHERE id = ?1 AND account_id = ?2 AND is_draft = 1",
                rusqlite::params![draft_id, account_id],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to delete draft: {e}")))?;

    for attachment in attachments {
        if let Some(path) = attachment.local_path {
            let referenced = state
                .store
                .is_attachment_local_path_referenced(&path)
                .map_err(|error| {
                    ApiError::Internal(format!("Failed to check attachment references: {error}"))
                })?;
            if !referenced {
                let _ = tokio::fs::remove_file(path).await;
            }
        }
    }

    Ok(Json(serde_json::json!({ "ok": true })))
}

use rusqlite::OptionalExtension;
