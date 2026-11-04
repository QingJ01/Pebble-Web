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
}

pub async fn list_messages_by_folder(
    State(state): State<AppStateRef>,
    Path(folder_id): Path<String>,
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
                 JOIN message_folders mf ON m.id = mf.message_id \
                 WHERE mf.folder_id = ?1 AND m.is_deleted = 0 \
                 ORDER BY m.date DESC \
                 LIMIT ?2 OFFSET ?3";
            let mut stmt = conn.prepare(sql)?;
            let rows = stmt.query_map(rusqlite::params![folder_id, limit, offset], |row| {
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
    crate::mail_service::flags::update_message_flags(
        &state,
        message_id,
        body.is_read,
        body.is_starred,
    )
    .await?;
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
    crate::mail_service::lifecycle::move_to_folder(&state, message_id, body.folder_id).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn delete_message(
    State(state): State<AppStateRef>,
    Path(message_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    crate::mail_service::lifecycle::delete_message(&state, message_id).await?;
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
    let action = crate::mail_service::lifecycle::archive_message(&state, message_id).await?;
    Ok(Json(serde_json::json!({ "ok": true, "action": action })))
}

pub async fn restore_message(
    State(state): State<AppStateRef>,
    Path(message_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    crate::mail_service::lifecycle::restore_message(&state, message_id).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
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
    let count = crate::mail_service::lifecycle::empty_trash(&state, account_id).await?;
    Ok(Json(serde_json::json!({ "ok": true, "count": count })))
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
    let count = crate::mail_service::batch::batch_archive(&state, body.message_ids).await?;
    Ok(Json(serde_json::json!({ "ok": true, "count": count })))
}

pub async fn batch_delete(
    State(state): State<AppStateRef>,
    Json(body): Json<BatchMessageIds>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let count = crate::mail_service::batch::batch_delete(&state, body.message_ids).await?;
    Ok(Json(serde_json::json!({ "ok": true, "count": count })))
}

pub async fn batch_mark_read(
    State(state): State<AppStateRef>,
    Json(body): Json<BatchMarkReadRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let count =
        crate::mail_service::batch::batch_mark_read(&state, body.message_ids, body.is_read).await?;
    Ok(Json(serde_json::json!({ "ok": true, "count": count })))
}

pub async fn batch_star(
    State(state): State<AppStateRef>,
    Json(body): Json<BatchStarRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let count =
        crate::mail_service::batch::batch_star(&state, body.message_ids, body.starred).await?;
    Ok(Json(serde_json::json!({ "ok": true, "count": count })))
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
