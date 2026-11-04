use pebble_core::{Attachment, Result};
use rusqlite::{params, OptionalExtension};

use crate::Store;

impl Store {
    pub fn insert_attachment(&self, attachment: &Attachment) -> Result<()> {
        self.with_write(|conn| {
            conn.execute(
                "INSERT INTO attachments (id, message_id, filename, mime_type, size, local_path, content_id, is_inline)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    attachment.id,
                    attachment.message_id,
                    attachment.filename,
                    attachment.mime_type,
                    attachment.size,
                    attachment.local_path,
                    attachment.content_id,
                    attachment.is_inline,
                ],
            )?;
            Ok(())
        })
    }

    pub fn list_attachments_by_message(&self, message_id: &str) -> Result<Vec<Attachment>> {
        self.with_read(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT id, message_id, filename, mime_type, size, local_path, content_id, is_inline
                     FROM attachments WHERE message_id = ?1",
                )?;
            let rows = stmt
                .query_map(params![message_id], |row| {
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
    }

    pub fn get_attachment(&self, attachment_id: &str) -> Result<Option<Attachment>> {
        self.with_read(|conn| {
            let result = conn.query_row(
                "SELECT id, message_id, filename, mime_type, size, local_path, content_id, is_inline
                 FROM attachments WHERE id = ?1",
                params![attachment_id],
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
            .optional()?;
            Ok(result)
        })
    }

    pub fn is_attachment_local_path_referenced(&self, local_path: &str) -> Result<bool> {
        self.with_read(|conn| {
            let referenced = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM attachments WHERE local_path = ?1)",
                params![local_path],
                |row| row.get(0),
            )?;
            Ok(referenced)
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::Store;
    use pebble_core::{new_id, now_timestamp, Attachment};

    fn setup_store_with_message() -> (Store, String) {
        let store = Store::open_in_memory().unwrap();
        let now = now_timestamp();
        let account = pebble_core::Account {
            account_label: None,
            provider_display_name: None,
            id: new_id(),
            email: "test@example.com".to_string(),
            display_name: "Test".to_string(),
            color: None,
            provider: pebble_core::ProviderType::Imap,
            created_at: now,
            updated_at: now,
        };
        store.insert_account(&account).unwrap();
        let folder = pebble_core::Folder {
            id: new_id(),
            account_id: account.id.clone(),
            remote_id: "INBOX".to_string(),
            name: "Inbox".to_string(),
            folder_type: pebble_core::FolderType::Folder,
            role: Some(pebble_core::FolderRole::Inbox),
            parent_id: None,
            color: None,
            is_system: true,
            sort_order: 0,
        };
        store.insert_folder(&folder).unwrap();
        let msg_id = new_id();
        let msg = pebble_core::Message {
            id: msg_id.clone(),
            account_id: account.id.clone(),
            remote_id: "1".to_string(),
            message_id_header: None,
            in_reply_to: None,
            references_header: None,
            thread_id: None,
            subject: "Test".to_string(),
            snippet: "Test snippet".to_string(),
            from_address: "sender@example.com".to_string(),
            from_name: "Sender".to_string(),
            to_list: vec![],
            cc_list: vec![],
            bcc_list: vec![],
            body_text: "body".to_string(),
            body_html_raw: "<p>body</p>".to_string(),
            has_attachments: false,
            is_read: false,
            is_starred: false,
            is_draft: false,
            date: now,
            remote_version: None,
            is_deleted: false,
            deleted_at: None,
            created_at: now,
            updated_at: now,
        };
        store
            .insert_message(&msg, std::slice::from_ref(&folder.id))
            .unwrap();
        (store, msg_id)
    }

    #[test]
    fn test_insert_and_list_attachments() {
        let (store, msg_id) = setup_store_with_message();
        let att = Attachment {
            id: new_id(),
            message_id: msg_id.clone(),
            filename: "test.pdf".to_string(),
            mime_type: "application/pdf".to_string(),
            size: 1024,
            local_path: Some("/tmp/test.pdf".to_string()),
            content_id: None,
            is_inline: false,
        };
        store.insert_attachment(&att).unwrap();

        let list = store.list_attachments_by_message(&msg_id).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].filename, "test.pdf");
        assert_eq!(list[0].mime_type, "application/pdf");
        assert_eq!(list[0].size, 1024);
        assert_eq!(list[0].local_path.as_deref(), Some("/tmp/test.pdf"));
    }

    #[test]
    fn test_get_attachment() {
        let (store, msg_id) = setup_store_with_message();
        let att_id = new_id();
        let att = Attachment {
            id: att_id.clone(),
            message_id: msg_id,
            filename: "image.png".to_string(),
            mime_type: "image/png".to_string(),
            size: 2048,
            local_path: None,
            content_id: Some("cid:image001".to_string()),
            is_inline: true,
        };
        store.insert_attachment(&att).unwrap();

        let fetched = store.get_attachment(&att_id).unwrap();
        assert!(fetched.is_some());
        let fetched = fetched.unwrap();
        assert_eq!(fetched.filename, "image.png");
        assert_eq!(fetched.size, 2048);
        assert!(fetched.local_path.is_none());
    }

    #[test]
    fn test_get_attachment_not_found() {
        let (store, _) = setup_store_with_message();
        let fetched = store.get_attachment("nonexistent").unwrap();
        assert!(fetched.is_none());
    }

    #[test]
    fn test_list_attachments_empty() {
        let (store, msg_id) = setup_store_with_message();
        let list = store.list_attachments_by_message(&msg_id).unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn test_multiple_attachments() {
        let (store, msg_id) = setup_store_with_message();
        for i in 0..3 {
            let att = Attachment {
                id: new_id(),
                message_id: msg_id.clone(),
                filename: format!("file{i}.txt"),
                mime_type: "text/plain".to_string(),
                size: 100 * (i + 1),
                local_path: None,
                content_id: None,
                is_inline: false,
            };
            store.insert_attachment(&att).unwrap();
        }

        let list = store.list_attachments_by_message(&msg_id).unwrap();
        assert_eq!(list.len(), 3);
    }
}
