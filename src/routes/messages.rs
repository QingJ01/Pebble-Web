use crate::error::ApiError;
use crate::state::AppStateRef;
use axum::{
    extract::{Path, Query, State},
    Json,
};
use lol_html::{element, HtmlRewriter, Settings};
use pebble_core::{Message, MessageSummary, PrivacyMode, RenderedHtml};
use rusqlite::OptionalExtension;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
pub struct ListMessagesParams {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    /// Comma-separated folder IDs for "all accounts" aggregated views.
    pub folder_ids: Option<String>,
}

fn parse_folder_ids(folder_id: &str, folder_ids: Option<&str>) -> Vec<String> {
    let mut ids: Vec<String> = folder_ids
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    if ids.is_empty() {
        ids.push(folder_id.to_string());
    }
    ids
}

pub async fn list_messages_by_folder(
    State(state): State<AppStateRef>,
    Path(folder_id): Path<String>,
    Query(params): Query<ListMessagesParams>,
) -> Result<Json<Vec<MessageSummary>>, ApiError> {
    let limit = params.limit.unwrap_or(50);
    let offset = params.offset.unwrap_or(0);
    let folder_ids = parse_folder_ids(&folder_id, params.folder_ids.as_deref());
    let store = state.store.clone();

    let messages = tokio::task::spawn_blocking(move || {
        store.list_messages_by_folders(&folder_ids, limit, offset)
    })
    .await
    .map_err(|e| ApiError::Internal(format!("Failed to list messages: {e}")))?
    .map_err(|e| ApiError::Internal(format!("Failed to list messages: {e}")))?;

    Ok(Json(messages))
}

pub async fn get_message(
    State(state): State<AppStateRef>,
    Path(message_id): Path<String>,
) -> Result<Json<Message>, ApiError> {
    let store = state.store.clone();

    let message = store
        .with_read_async(move |conn| {
            let sql = "SELECT id, account_id, remote_id, message_id_header, in_reply_to, \
                 references_header, thread_id, subject, snippet, from_address, \
                 from_name, to_list, cc_list, bcc_list, \
                 body_text, body_html_raw, \
                 has_attachments, is_read, is_starred, is_draft, \
                 date, remote_version, is_deleted, deleted_at, created_at, updated_at \
                 FROM messages WHERE id = ?1";
            let result = conn
                .query_row(sql, rusqlite::params![message_id], |row| {
                    let to_json: String = row.get(11)?;
                    let cc_json: String = row.get(12)?;
                    let bcc_json: String = row.get(13)?;
                    let has_attachments: i32 = row.get(16)?;
                    let is_read: i32 = row.get(17)?;
                    let is_starred: i32 = row.get(18)?;
                    let is_draft: i32 = row.get(19)?;
                    let is_deleted: i32 = row.get(22)?;
                    Ok(Message {
                        id: row.get(0)?,
                        account_id: row.get(1)?,
                        remote_id: row.get(2)?,
                        message_id_header: row.get(3)?,
                        in_reply_to: row.get(4)?,
                        references_header: row.get(5)?,
                        thread_id: row.get(6)?,
                        subject: row.get(7)?,
                        snippet: row.get(8)?,
                        from_address: row.get(9)?,
                        from_name: row.get(10)?,
                        to_list: serde_json::from_str(&to_json).unwrap_or_default(),
                        cc_list: serde_json::from_str(&cc_json).unwrap_or_default(),
                        bcc_list: serde_json::from_str(&bcc_json).unwrap_or_default(),
                        body_text: row.get(14)?,
                        body_html_raw: row.get(15)?,
                        has_attachments: has_attachments != 0,
                        is_read: is_read != 0,
                        is_starred: is_starred != 0,
                        is_draft: is_draft != 0,
                        date: row.get(20)?,
                        remote_version: row.get(21)?,
                        is_deleted: is_deleted != 0,
                        deleted_at: row.get(23)?,
                        created_at: row.get(24)?,
                        updated_at: row.get(25)?,
                    })
                })
                .optional();
            match result {
                Ok(Some(msg)) => Ok(Some(msg)),
                Ok(None) => Ok(None),
                Err(e) => Err(pebble_core::PebbleError::Storage(e.to_string())),
            }
        })
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to get message: {e}")))?;

    match message {
        Some(msg) => Ok(Json(msg)),
        None => Err(ApiError::NotFound("Message not found".to_string())),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateFlagsRequest {
    pub is_read: Option<bool>,
    pub is_starred: Option<bool>,
}

pub async fn update_message_flags(
    State(state): State<AppStateRef>,
    Path(message_id): Path<String>,
    Json(body): Json<UpdateFlagsRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = state.store.clone();
    let is_read = body.is_read;
    let is_starred = body.is_starred;

    store
        .with_write_async(move |conn| {
            let mut sets = Vec::new();
            let mut values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

            if let Some(read) = is_read {
                sets.push(format!("is_read = ?{}", values.len() + 1));
                values.push(Box::new(read as i32));
            }
            if let Some(starred) = is_starred {
                sets.push(format!("is_starred = ?{}", values.len() + 1));
                values.push(Box::new(starred as i32));
            }

            if sets.is_empty() {
                return Ok(());
            }

            let now = pebble_core::now_timestamp();
            sets.push(format!("updated_at = ?{}", values.len() + 1));
            values.push(Box::new(now));

            let id_idx = values.len() + 1;
            values.push(Box::new(message_id));

            let sql = format!(
                "UPDATE messages SET {} WHERE id = ?{}",
                sets.join(", "),
                id_idx
            );
            let params: Vec<&dyn rusqlite::types::ToSql> =
                values.iter().map(|v| v.as_ref()).collect();
            conn.execute(&sql, params.as_slice())?;
            Ok(())
        })
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to update flags: {e}")))?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveMessageRequest {
    pub folder_id: String,
}

pub async fn move_message(
    State(state): State<AppStateRef>,
    Path(message_id): Path<String>,
    Json(body): Json<MoveMessageRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = state.store.clone();
    let folder_id = body.folder_id;

    store
        .with_write_async(move |conn| {
            let now = pebble_core::now_timestamp();
            let tx = conn.unchecked_transaction()?;

            tx.execute(
                "DELETE FROM message_folders WHERE message_id = ?1",
                rusqlite::params![message_id],
            )?;

            tx.execute(
                "INSERT INTO message_folders (message_id, folder_id) VALUES (?1, ?2)",
                rusqlite::params![message_id, folder_id],
            )?;

            tx.execute(
                "UPDATE messages SET is_deleted = 0, deleted_at = NULL, updated_at = ?1 WHERE id = ?2",
                rusqlite::params![now, message_id],
            )?;

            tx.commit()?;
            Ok(())
        })
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to move message: {e}")))?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn delete_message(
    State(state): State<AppStateRef>,
    Path(message_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = state.store.clone();

    store
        .with_write_async(move |conn| {
            let now = pebble_core::now_timestamp();
            conn.execute(
                "UPDATE messages SET is_deleted = 1, deleted_at = ?1, updated_at = ?1 WHERE id = ?2",
                rusqlite::params![now, message_id],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to delete message: {e}")))?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

// --- New handlers ---

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyModeRequest {
    pub privacy_mode: Option<PrivacyMode>,
}

fn should_load_remote_content(mode: &PrivacyMode, sender: Option<&str>) -> bool {
    match mode {
        PrivacyMode::Off | PrivacyMode::LoadOnce => true,
        PrivacyMode::Strict => false,
        PrivacyMode::TrustSender(trusted) => sender
            .map(|sender| sender.eq_ignore_ascii_case(trusted))
            .unwrap_or(false),
    }
}

fn is_remote_resource_url(url: &str) -> bool {
    let trimmed = url.trim().to_ascii_lowercase();
    trimmed.starts_with("http://") || trimmed.starts_with("https://") || trimmed.starts_with("//")
}

fn render_email_html_for_privacy(
    html: &str,
    mode: PrivacyMode,
    sender: Option<&str>,
) -> Result<RenderedHtml, String> {
    if should_load_remote_content(&mode, sender) {
        return Ok(RenderedHtml {
            html: html.to_string(),
            trackers_blocked: Vec::new(),
            images_blocked: 0,
        });
    }

    let mut output = Vec::with_capacity(html.len());
    let mut images_blocked = 0u32;
    let mut rewriter = HtmlRewriter::new(
        Settings {
            element_content_handlers: vec![element!("img", |el| {
                let mut blocked_src = None;
                if let Some(src) = el.get_attribute("src") {
                    if is_remote_resource_url(&src) {
                        blocked_src = Some(src);
                        el.remove_attribute("src");
                    }
                }
                if let Some(srcset) = el.get_attribute("srcset") {
                    if srcset.split(',').any(is_remote_resource_url) {
                        if blocked_src.is_none() {
                            blocked_src = Some(srcset);
                        }
                        el.remove_attribute("srcset");
                    }
                }
                if let Some(src) = blocked_src {
                    images_blocked += 1;
                    el.set_attribute("data-pebble-blocked-src", &src)?;
                    el.set_attribute("class", "blocked-image")?;
                    el.set_attribute("alt", "Remote image blocked")?;
                }
                Ok(())
            })],
            ..Settings::default()
        },
        |chunk: &[u8]| output.extend_from_slice(chunk),
    );

    rewriter
        .write(html.as_bytes())
        .map_err(|e| format!("Failed to rewrite HTML: {e}"))?;
    rewriter
        .end()
        .map_err(|e| format!("Failed to finish HTML rewrite: {e}"))?;

    Ok(RenderedHtml {
        html: String::from_utf8(output)
            .map_err(|e| format!("Rewritten HTML was not valid UTF-8: {e}"))?,
        trackers_blocked: Vec::new(),
        images_blocked,
    })
}

#[cfg(test)]
mod privacy_tests {
    use super::{render_email_html_for_privacy, PrivacyModeRequest};
    use pebble_core::PrivacyMode;

    #[test]
    fn strict_privacy_blocks_remote_images() {
        let rendered = render_email_html_for_privacy(
            r#"<p>hi</p><img src="https://tracker.example/p.gif"><img src="cid:inline-1">"#,
            PrivacyMode::Strict,
            None,
        )
        .unwrap();

        assert_eq!(rendered.images_blocked, 1);
        assert!(!rendered
            .html
            .contains(r#" src="https://tracker.example/p.gif""#));
        assert!(rendered.html.contains("cid:inline-1"));
        assert!(rendered.html.contains("data-pebble-blocked-src"));
    }

    #[test]
    fn off_privacy_allows_remote_images() {
        let rendered = render_email_html_for_privacy(
            r#"<img src="https://cdn.example/image.png">"#,
            PrivacyMode::Off,
            None,
        )
        .unwrap();

        assert_eq!(rendered.images_blocked, 0);
        assert!(rendered.html.contains("https://cdn.example/image.png"));
    }

    #[test]
    fn trust_sender_allows_matching_sender() {
        let rendered = render_email_html_for_privacy(
            r#"<img src="https://cdn.example/image.png">"#,
            PrivacyMode::TrustSender("sender@example.com".to_string()),
            Some("sender@example.com"),
        )
        .unwrap();

        assert_eq!(rendered.images_blocked, 0);
        assert!(rendered.html.contains("https://cdn.example/image.png"));
    }

    #[test]
    fn privacy_mode_request_accepts_camel_case_body() {
        let request: PrivacyModeRequest =
            serde_json::from_str(r#"{"privacyMode":{"TrustSender":"sender@example.com"}}"#)
                .unwrap();

        assert!(matches!(
            request.privacy_mode,
            Some(PrivacyMode::TrustSender(sender)) if sender == "sender@example.com"
        ));
    }
}

pub async fn get_message_with_html(
    State(state): State<AppStateRef>,
    Path(message_id): Path<String>,
    Json(body): Json<PrivacyModeRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = state.store.clone();

    let result = store
        .with_read_async(move |conn| {
            let sql = "SELECT id, account_id, remote_id, message_id_header, in_reply_to, \
                 references_header, thread_id, subject, snippet, from_address, \
                 from_name, to_list, cc_list, bcc_list, \
                 body_text, body_html_raw, \
                 has_attachments, is_read, is_starred, is_draft, \
                 date, remote_version, is_deleted, deleted_at, created_at, updated_at \
                 FROM messages WHERE id = ?1";
            let row_result = conn
                .query_row(sql, rusqlite::params![message_id], |row| {
                    let to_json: String = row.get(11)?;
                    let cc_json: String = row.get(12)?;
                    let bcc_json: String = row.get(13)?;
                    let body_html_raw: String = row.get(15)?;
                    let has_attachments: i32 = row.get(16)?;
                    let is_read: i32 = row.get(17)?;
                    let is_starred: i32 = row.get(18)?;
                    let is_draft: i32 = row.get(19)?;
                    let is_deleted: i32 = row.get(22)?;
                    let msg = Message {
                        id: row.get(0)?,
                        account_id: row.get(1)?,
                        remote_id: row.get(2)?,
                        message_id_header: row.get(3)?,
                        in_reply_to: row.get(4)?,
                        references_header: row.get(5)?,
                        thread_id: row.get(6)?,
                        subject: row.get(7)?,
                        snippet: row.get(8)?,
                        from_address: row.get(9)?,
                        from_name: row.get(10)?,
                        to_list: serde_json::from_str(&to_json).unwrap_or_default(),
                        cc_list: serde_json::from_str(&cc_json).unwrap_or_default(),
                        bcc_list: serde_json::from_str(&bcc_json).unwrap_or_default(),
                        body_text: row.get(14)?,
                        body_html_raw: body_html_raw.clone(),
                        has_attachments: has_attachments != 0,
                        is_read: is_read != 0,
                        is_starred: is_starred != 0,
                        is_draft: is_draft != 0,
                        date: row.get(20)?,
                        remote_version: row.get(21)?,
                        is_deleted: is_deleted != 0,
                        deleted_at: row.get(23)?,
                        created_at: row.get(24)?,
                        updated_at: row.get(25)?,
                    };
                    Ok((msg, body_html_raw))
                })
                .optional();
            match row_result {
                Ok(Some(data)) => Ok(Some(data)),
                Ok(None) => Ok(None),
                Err(e) => Err(pebble_core::PebbleError::Storage(e.to_string())),
            }
        })
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to get message with html: {e}")))?;

    match result {
        Some((msg, html_raw)) => {
            let mut privacy_mode = body.privacy_mode.unwrap_or(PrivacyMode::Strict);
            if matches!(privacy_mode, PrivacyMode::Strict) {
                if let Ok(Some(_trust)) = state
                    .store
                    .is_trusted_sender(&msg.account_id, &msg.from_address)
                {
                    privacy_mode = PrivacyMode::LoadOnce;
                }
            }
            let rendered =
                render_email_html_for_privacy(&html_raw, privacy_mode, Some(&msg.from_address))
                    .map_err(|e| ApiError::Internal(format!("Failed to render html: {e}")))?;
            let msg_json = serde_json::to_value(msg)
                .map_err(|e| ApiError::Internal(format!("Failed to serialize message: {e}")))?;
            let html_part = json!({
                "html": rendered.html,
                "loadedRemoteContent": false,
                "trackers_blocked": rendered.trackers_blocked,
                "images_blocked": rendered.images_blocked
            });
            Ok(Json(json!([msg_json, html_part])))
        }
        None => Err(ApiError::NotFound("Message not found".to_string())),
    }
}

pub async fn render_html(
    State(state): State<AppStateRef>,
    Path(message_id): Path<String>,
    Json(body): Json<PrivacyModeRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = state.store.clone();

    let result = store
        .with_read_async(move |conn| {
            let sql = "SELECT account_id, from_address, body_html_raw FROM messages WHERE id = ?1";
            let row_result = conn
                .query_row(sql, rusqlite::params![message_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })
                .optional();
            match row_result {
                Ok(Some(data)) => Ok(Some(data)),
                Ok(None) => Ok(None),
                Err(e) => Err(pebble_core::PebbleError::Storage(e.to_string())),
            }
        })
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to render html: {e}")))?;

    match result {
        Some((account_id, from_address, html)) => {
            let mut privacy_mode = body.privacy_mode.unwrap_or(PrivacyMode::Strict);
            if matches!(privacy_mode, PrivacyMode::Strict) {
                if let Ok(Some(_trust)) = state.store.is_trusted_sender(&account_id, &from_address)
                {
                    privacy_mode = PrivacyMode::LoadOnce;
                }
            }
            let rendered = render_email_html_for_privacy(&html, privacy_mode, Some(&from_address))
                .map_err(|e| ApiError::Internal(format!("Failed to render html: {e}")))?;
            Ok(Json(json!({
                "html": rendered.html,
                "loadedRemoteContent": false,
                "trackers_blocked": rendered.trackers_blocked,
                "images_blocked": rendered.images_blocked
            })))
        }
        None => Err(ApiError::NotFound("Message not found".to_string())),
    }
}

pub async fn archive_message(
    State(state): State<AppStateRef>,
    Path(message_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = state.store.clone();

    store
        .with_write_async(move |conn| {
            let now = pebble_core::now_timestamp();
            let tx = conn.unchecked_transaction()?;

            // Find the account_id for this message
            let account_id: String = tx.query_row(
                "SELECT account_id FROM messages WHERE id = ?1",
                rusqlite::params![message_id],
                |row| row.get(0),
            ).map_err(|_| pebble_core::PebbleError::Storage("Message not found".to_string()))?;

            // Find the archive folder for this account
            let archive_folder_id: Option<String> = tx.query_row(
                "SELECT id FROM folders WHERE account_id = ?1 AND role = 'archive'",
                rusqlite::params![account_id],
                |row| row.get(0),
            ).optional()
            .map_err(|e| pebble_core::PebbleError::Storage(e.to_string()))?;

            if let Some(folder_id) = archive_folder_id {
                tx.execute(
                    "DELETE FROM message_folders WHERE message_id = ?1",
                    rusqlite::params![message_id],
                )?;
                tx.execute(
                    "INSERT INTO message_folders (message_id, folder_id) VALUES (?1, ?2)",
                    rusqlite::params![message_id, folder_id],
                )?;
                tx.execute(
                    "UPDATE messages SET is_deleted = 0, deleted_at = NULL, updated_at = ?1 WHERE id = ?2",
                    rusqlite::params![now, message_id],
                )?;
            } else {
                // No archive folder — soft-delete as fallback
                tx.execute(
                    "UPDATE messages SET is_deleted = 1, deleted_at = ?1, updated_at = ?1 WHERE id = ?2",
                    rusqlite::params![now, message_id],
                )?;
            }

            tx.commit()?;
            Ok(())
        })
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to archive message: {e}")))?;

    Ok(Json(json!("archived")))
}

pub async fn restore_message(
    State(state): State<AppStateRef>,
    Path(message_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = state.store.clone();

    store
        .with_write_async(move |conn| {
            let now = pebble_core::now_timestamp();
            let tx = conn.unchecked_transaction()?;

            // Clear soft-delete flags
            tx.execute(
                "UPDATE messages SET is_deleted = 0, deleted_at = NULL, updated_at = ?1 WHERE id = ?2",
                rusqlite::params![now, message_id],
            )?;

            // Find the account_id for this message
            let account_id: String = tx.query_row(
                "SELECT account_id FROM messages WHERE id = ?1",
                rusqlite::params![message_id],
                |row| row.get(0),
            ).map_err(|_| pebble_core::PebbleError::Storage("Message not found".to_string()))?;

            // Find the inbox folder for this account
            let inbox_folder_id: Option<String> = tx.query_row(
                "SELECT id FROM folders WHERE account_id = ?1 AND role = 'inbox'",
                rusqlite::params![account_id],
                |row| row.get(0),
            ).optional()
            .map_err(|e| pebble_core::PebbleError::Storage(e.to_string()))?;

            // Move message to inbox if inbox folder exists
            if let Some(folder_id) = inbox_folder_id {
                tx.execute(
                    "DELETE FROM message_folders WHERE message_id = ?1",
                    rusqlite::params![message_id],
                )?;
                tx.execute(
                    "INSERT INTO message_folders (message_id, folder_id) VALUES (?1, ?2)",
                    rusqlite::params![message_id, folder_id],
                )?;
            }

            tx.commit()?;
            Ok(())
        })
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to restore message: {e}")))?;

    Ok(Json(json!({ "ok": true })))
}

pub async fn list_starred_messages(
    State(state): State<AppStateRef>,
    Path(account_id): Path<String>,
    Query(params): Query<ListMessagesParams>,
) -> Result<Json<Vec<MessageSummary>>, ApiError> {
    let limit = params.limit.unwrap_or(50);
    let offset = params.offset.unwrap_or(0);
    let store = state.store.clone();

    let messages = store
        .with_read_async(move |conn| {
            let sql =
                "SELECT m.id, m.account_id, m.remote_id, m.message_id_header, m.in_reply_to, \
                 m.references_header, m.thread_id, m.subject, m.snippet, m.from_address, \
                 m.from_name, m.to_list, m.cc_list, m.bcc_list, \
                 m.has_attachments, m.is_read, m.is_starred, m.is_draft, \
                 m.date, m.remote_version, m.is_deleted, m.deleted_at, m.created_at, m.updated_at \
                 FROM messages m \
                 WHERE m.account_id = ?1 AND m.is_starred = 1 AND m.is_deleted = 0 \
                 ORDER BY m.date DESC \
                 LIMIT ?2 OFFSET ?3";
            let mut stmt = conn.prepare(sql)?;
            let rows = stmt.query_map(rusqlite::params![account_id, limit, offset], |row| {
                let to_json: String = row.get(11)?;
                let cc_json: String = row.get(12)?;
                let bcc_json: String = row.get(13)?;
                let has_attachments: i32 = row.get(14)?;
                let is_read: i32 = row.get(15)?;
                let is_starred: i32 = row.get(16)?;
                let is_draft: i32 = row.get(17)?;
                let is_deleted: i32 = row.get(20)?;
                Ok(MessageSummary {
                    id: row.get(0)?,
                    account_id: row.get(1)?,
                    remote_id: row.get(2)?,
                    message_id_header: row.get(3)?,
                    in_reply_to: row.get(4)?,
                    references_header: row.get(5)?,
                    thread_id: row.get(6)?,
                    subject: row.get(7)?,
                    snippet: row.get(8)?,
                    from_address: row.get(9)?,
                    from_name: row.get(10)?,
                    to_list: serde_json::from_str(&to_json).unwrap_or_default(),
                    cc_list: serde_json::from_str(&cc_json).unwrap_or_default(),
                    bcc_list: serde_json::from_str(&bcc_json).unwrap_or_default(),
                    has_attachments: has_attachments != 0,
                    is_read: is_read != 0,
                    is_starred: is_starred != 0,
                    is_draft: is_draft != 0,
                    date: row.get(18)?,
                    remote_version: row.get(19)?,
                    is_deleted: is_deleted != 0,
                    deleted_at: row.get(21)?,
                    created_at: row.get(22)?,
                    updated_at: row.get(23)?,
                })
            })?;
            let mut messages = Vec::new();
            for row in rows {
                messages.push(row?);
            }
            Ok(messages)
        })
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to list starred messages: {e}")))?;

    Ok(Json(messages))
}

pub async fn empty_trash(
    State(state): State<AppStateRef>,
    Path(account_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = state.store.clone();

    let count = store
        .with_write_async(move |conn| {
            let tx = conn.unchecked_transaction()?;

            // Delete from message_folders for all deleted messages of this account
            tx.execute(
                "DELETE FROM message_folders WHERE message_id IN \
                 (SELECT id FROM messages WHERE account_id = ?1 AND is_deleted = 1)",
                rusqlite::params![account_id],
            )?;

            // Permanently delete messages
            let deleted = tx.execute(
                "DELETE FROM messages WHERE account_id = ?1 AND is_deleted = 1",
                rusqlite::params![account_id],
            )?;

            tx.commit()?;
            Ok(deleted)
        })
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to empty trash: {e}")))?;

    Ok(Json(json!(count)))
}

// --- Batch operations ---

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchMessageIds {
    pub message_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchMarkReadRequest {
    pub message_ids: Vec<String>,
    pub is_read: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchStarRequest {
    pub message_ids: Vec<String>,
    pub starred: bool,
}

pub async fn batch_archive(
    State(state): State<AppStateRef>,
    Json(body): Json<BatchMessageIds>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = state.store.clone();
    let message_ids = body.message_ids;

    let count = store
        .with_write_async(move |conn| {
            let now = pebble_core::now_timestamp();
            let tx = conn.unchecked_transaction()?;
            let mut total = 0usize;
            for id in &message_ids {
                // Find account_id for this message
                let account_id: Option<String> = tx.query_row(
                    "SELECT account_id FROM messages WHERE id = ?1",
                    rusqlite::params![id],
                    |row| row.get(0),
                ).optional()?;

                let Some(account_id) = account_id else { continue };

                // Find archive folder
                let archive_folder_id: Option<String> = tx.query_row(
                    "SELECT id FROM folders WHERE account_id = ?1 AND role = 'archive'",
                    rusqlite::params![account_id],
                    |row| row.get(0),
                ).optional()?;

                if let Some(folder_id) = archive_folder_id {
                    tx.execute(
                        "DELETE FROM message_folders WHERE message_id = ?1",
                        rusqlite::params![id],
                    )?;
                    tx.execute(
                        "INSERT INTO message_folders (message_id, folder_id) VALUES (?1, ?2)",
                        rusqlite::params![id, folder_id],
                    )?;
                    tx.execute(
                        "UPDATE messages SET is_deleted = 0, deleted_at = NULL, updated_at = ?1 WHERE id = ?2",
                        rusqlite::params![now, id],
                    )?;
                } else {
                    tx.execute(
                        "UPDATE messages SET is_deleted = 1, deleted_at = ?1, updated_at = ?1 WHERE id = ?2",
                        rusqlite::params![now, id],
                    )?;
                }
                total += 1;
            }
            tx.commit()?;
            Ok(total)
        })
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to batch archive: {e}")))?;

    Ok(Json(json!(count)))
}

pub async fn batch_delete(
    State(state): State<AppStateRef>,
    Json(body): Json<BatchMessageIds>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = state.store.clone();
    let message_ids = body.message_ids;

    let count = store
        .with_write_async(move |conn| {
            let tx = conn.unchecked_transaction()?;
            let mut total = 0usize;
            for id in &message_ids {
                tx.execute(
                    "DELETE FROM message_folders WHERE message_id = ?1",
                    rusqlite::params![id],
                )?;
                let affected =
                    tx.execute("DELETE FROM messages WHERE id = ?1", rusqlite::params![id])?;
                total += affected;
            }
            tx.commit()?;
            Ok(total)
        })
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to batch delete: {e}")))?;

    Ok(Json(json!(count)))
}

pub async fn batch_mark_read(
    State(state): State<AppStateRef>,
    Json(body): Json<BatchMarkReadRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = state.store.clone();
    let message_ids = body.message_ids;
    let is_read = body.is_read;

    let count = store
        .with_write_async(move |conn| {
            let now = pebble_core::now_timestamp();
            let read_val = is_read as i32;
            let mut total = 0usize;
            for id in &message_ids {
                let affected = conn.execute(
                    "UPDATE messages SET is_read = ?1, updated_at = ?2 WHERE id = ?3",
                    rusqlite::params![read_val, now, id],
                )?;
                total += affected;
            }
            Ok(total)
        })
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to batch mark read: {e}")))?;

    Ok(Json(json!(count)))
}

pub async fn batch_star(
    State(state): State<AppStateRef>,
    Json(body): Json<BatchStarRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = state.store.clone();
    let message_ids = body.message_ids;
    let starred = body.starred;

    let count = store
        .with_write_async(move |conn| {
            let now = pebble_core::now_timestamp();
            let starred_val = starred as i32;
            let mut total = 0usize;
            for id in &message_ids {
                let affected = conn.execute(
                    "UPDATE messages SET is_starred = ?1, updated_at = ?2 WHERE id = ?3",
                    rusqlite::params![starred_val, now, id],
                )?;
                total += affected;
            }
            Ok(total)
        })
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to batch star: {e}")))?;

    Ok(Json(json!(count)))
}

pub async fn get_messages_batch(
    State(state): State<AppStateRef>,
    Json(body): Json<BatchMessageIds>,
) -> Result<Json<Vec<Message>>, ApiError> {
    let store = state.store.clone();
    let message_ids = body.message_ids;

    let messages = store
        .with_read_async(move |conn| {
            let mut results = Vec::new();
            for id in &message_ids {
                let sql = "SELECT id, account_id, remote_id, message_id_header, in_reply_to, \
                     references_header, thread_id, subject, snippet, from_address, \
                     from_name, to_list, cc_list, bcc_list, \
                     body_text, body_html_raw, \
                     has_attachments, is_read, is_starred, is_draft, \
                     date, remote_version, is_deleted, deleted_at, created_at, updated_at \
                     FROM messages WHERE id = ?1";
                let row = conn
                    .query_row(sql, rusqlite::params![id], |row| {
                        let to_json: String = row.get(11)?;
                        let cc_json: String = row.get(12)?;
                        let bcc_json: String = row.get(13)?;
                        let has_attachments: i32 = row.get(16)?;
                        let is_read: i32 = row.get(17)?;
                        let is_starred: i32 = row.get(18)?;
                        let is_draft: i32 = row.get(19)?;
                        let is_deleted: i32 = row.get(22)?;
                        Ok(Message {
                            id: row.get(0)?,
                            account_id: row.get(1)?,
                            remote_id: row.get(2)?,
                            message_id_header: row.get(3)?,
                            in_reply_to: row.get(4)?,
                            references_header: row.get(5)?,
                            thread_id: row.get(6)?,
                            subject: row.get(7)?,
                            snippet: row.get(8)?,
                            from_address: row.get(9)?,
                            from_name: row.get(10)?,
                            to_list: serde_json::from_str(&to_json).unwrap_or_default(),
                            cc_list: serde_json::from_str(&cc_json).unwrap_or_default(),
                            bcc_list: serde_json::from_str(&bcc_json).unwrap_or_default(),
                            body_text: row.get(14)?,
                            body_html_raw: row.get(15)?,
                            has_attachments: has_attachments != 0,
                            is_read: is_read != 0,
                            is_starred: is_starred != 0,
                            is_draft: is_draft != 0,
                            date: row.get(20)?,
                            remote_version: row.get(21)?,
                            is_deleted: is_deleted != 0,
                            deleted_at: row.get(23)?,
                            created_at: row.get(24)?,
                            updated_at: row.get(25)?,
                        })
                    })
                    .optional()?;
                if let Some(msg) = row {
                    results.push(msg);
                }
            }
            Ok(results)
        })
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to get messages batch: {e}")))?;

    Ok(Json(messages))
}
