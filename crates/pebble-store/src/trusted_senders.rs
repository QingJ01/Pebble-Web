use pebble_core::{Result, TrustType, TrustedSender};
use rusqlite::{params, OptionalExtension};

use crate::Store;

fn trust_type_to_str(t: &TrustType) -> &'static str {
    match t {
        TrustType::Images => "images",
        TrustType::All => "all",
    }
}

fn str_to_trust_type(s: &str) -> TrustType {
    match s {
        "all" => TrustType::All,
        _ => TrustType::Images,
    }
}

fn row_to_trusted_sender(row: &rusqlite::Row) -> rusqlite::Result<TrustedSender> {
    Ok(TrustedSender {
        account_id: row.get(0)?,
        email: row.get(1)?,
        trust_type: str_to_trust_type(&row.get::<_, String>(2)?),
        created_at: row.get(3)?,
    })
}

impl Store {
    pub fn trust_sender(&self, sender: &TrustedSender) -> Result<()> {
        self.with_write(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO trusted_senders (account_id, email, trust_type, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    sender.account_id,
                    sender.email,
                    trust_type_to_str(&sender.trust_type),
                    sender.created_at,
                ],
            )?;
            Ok(())
        })
    }

    pub fn is_trusted_sender(&self, account_id: &str, email: &str) -> Result<Option<TrustType>> {
        self.with_read(|conn| {
            let result = conn
                .query_row(
                    "SELECT trust_type FROM trusted_senders WHERE account_id = ?1 AND email = ?2",
                    params![account_id, email],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            Ok(result.map(|s| str_to_trust_type(&s)))
        })
    }

    pub fn list_trusted_senders(&self, account_id: &str) -> Result<Vec<TrustedSender>> {
        self.with_read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT account_id, email, trust_type, created_at
                     FROM trusted_senders WHERE account_id = ?1",
            )?;
            let rows = stmt.query_map(params![account_id], row_to_trusted_sender)?;
            let mut senders = Vec::new();
            for row in rows {
                senders.push(row?);
            }
            Ok(senders)
        })
    }

    pub fn remove_trusted_sender(&self, account_id: &str, email: &str) -> Result<()> {
        self.with_write(|conn| {
            conn.execute(
                "DELETE FROM trusted_senders WHERE account_id = ?1 AND email = ?2",
                params![account_id, email],
            )?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;

    #[test]
    fn test_trust_sender_crud() {
        let store = Store::open_in_memory().unwrap();
        let now = pebble_core::now_timestamp();
        let account = pebble_core::Account {
            account_label: None,
            provider_display_name: None,
            id: pebble_core::new_id(),
            email: "me@example.com".to_string(),
            display_name: "Me".to_string(),
            color: None,
            provider: pebble_core::ProviderType::Imap,
            created_at: now,
            updated_at: now,
        };
        store.insert_account(&account).unwrap();

        let sender = TrustedSender {
            account_id: account.id.clone(),
            email: "trusted@example.com".to_string(),
            trust_type: TrustType::Images,
            created_at: now,
        };
        store.trust_sender(&sender).unwrap();

        // Check trust
        let trust = store
            .is_trusted_sender(&account.id, "trusted@example.com")
            .unwrap();
        assert_eq!(trust, Some(TrustType::Images));

        // Unknown sender
        let trust = store
            .is_trusted_sender(&account.id, "unknown@example.com")
            .unwrap();
        assert_eq!(trust, None);

        // List
        let senders = store.list_trusted_senders(&account.id).unwrap();
        assert_eq!(senders.len(), 1);
        assert_eq!(senders[0].email, "trusted@example.com");

        // Upgrade trust
        let sender2 = TrustedSender {
            account_id: account.id.clone(),
            email: "trusted@example.com".to_string(),
            trust_type: TrustType::All,
            created_at: now,
        };
        store.trust_sender(&sender2).unwrap();
        let trust = store
            .is_trusted_sender(&account.id, "trusted@example.com")
            .unwrap();
        assert_eq!(trust, Some(TrustType::All));

        // Remove
        store
            .remove_trusted_sender(&account.id, "trusted@example.com")
            .unwrap();
        let senders = store.list_trusted_senders(&account.id).unwrap();
        assert_eq!(senders.len(), 0);
    }
}
