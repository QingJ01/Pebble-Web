use pebble_core::{PebbleError, Result, StoredWebhookEndpoint, WebhookEndpoint, WebhookProvider};
use rusqlite::{params, OptionalExtension};

use crate::Store;

fn provider_to_str(provider: &WebhookProvider) -> &'static str {
    match provider {
        WebhookProvider::Generic => "generic",
        WebhookProvider::Slack => "slack",
        WebhookProvider::Discord => "discord",
        WebhookProvider::Telegram => "telegram",
        WebhookProvider::Feishu => "feishu",
        WebhookProvider::Dingtalk => "dingtalk",
        WebhookProvider::Wecom => "wecom",
        WebhookProvider::Ntfy => "ntfy",
        WebhookProvider::Gotify => "gotify",
    }
}

fn str_to_provider(value: &str) -> Result<WebhookProvider> {
    match value {
        "generic" => Ok(WebhookProvider::Generic),
        "slack" => Ok(WebhookProvider::Slack),
        "discord" => Ok(WebhookProvider::Discord),
        "telegram" => Ok(WebhookProvider::Telegram),
        "feishu" => Ok(WebhookProvider::Feishu),
        "dingtalk" => Ok(WebhookProvider::Dingtalk),
        "wecom" => Ok(WebhookProvider::Wecom),
        "ntfy" => Ok(WebhookProvider::Ntfy),
        "gotify" => Ok(WebhookProvider::Gotify),
        _ => Err(PebbleError::Validation(format!(
            "Unknown webhook provider: {value}"
        ))),
    }
}

fn row_to_stored(row: &rusqlite::Row) -> rusqlite::Result<StoredWebhookEndpoint> {
    let provider: String = row.get(2)?;
    let is_enabled: i32 = row.get(4)?;
    let notify_on_new_mail: i32 = row.get(5)?;
    let provider = str_to_provider(&provider).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            2,
            rusqlite::types::Type::Text,
            Box::new(e),
        )
    })?;

    Ok(StoredWebhookEndpoint {
        endpoint: WebhookEndpoint {
            id: row.get(0)?,
            name: row.get(1)?,
            provider,
            is_enabled: is_enabled != 0,
            notify_on_new_mail: notify_on_new_mail != 0,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
        },
        encrypted_config: row.get(3)?,
    })
}

impl Store {
    pub fn insert_webhook_endpoint(&self, endpoint: &StoredWebhookEndpoint) -> Result<()> {
        self.with_write(|conn| {
            conn.execute(
                "INSERT INTO webhook_endpoints
                 (id, name, provider, encrypted_config, is_enabled, notify_on_new_mail, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    endpoint.endpoint.id,
                    endpoint.endpoint.name,
                    provider_to_str(&endpoint.endpoint.provider),
                    endpoint.encrypted_config,
                    endpoint.endpoint.is_enabled as i32,
                    endpoint.endpoint.notify_on_new_mail as i32,
                    endpoint.endpoint.created_at,
                    endpoint.endpoint.updated_at,
                ],
            )?;
            Ok(())
        })
    }

    pub fn list_webhook_endpoints(&self) -> Result<Vec<WebhookEndpoint>> {
        self.with_read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, provider, encrypted_config, is_enabled, notify_on_new_mail, created_at, updated_at
                 FROM webhook_endpoints ORDER BY name COLLATE NOCASE ASC",
            )?;
            let rows = stmt.query_map([], row_to_stored)?;
            let mut endpoints = Vec::new();
            for row in rows {
                endpoints.push(row?.endpoint);
            }
            Ok(endpoints)
        })
    }

    pub fn list_enabled_webhook_endpoints_for_new_mail(&self) -> Result<Vec<StoredWebhookEndpoint>> {
        self.with_read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, provider, encrypted_config, is_enabled, notify_on_new_mail, created_at, updated_at
                 FROM webhook_endpoints
                 WHERE is_enabled = 1 AND notify_on_new_mail = 1
                 ORDER BY name COLLATE NOCASE ASC",
            )?;
            let rows = stmt.query_map([], row_to_stored)?;
            let mut endpoints = Vec::new();
            for row in rows {
                endpoints.push(row?);
            }
            Ok(endpoints)
        })
    }

    pub fn get_webhook_endpoint(&self, id: &str) -> Result<Option<StoredWebhookEndpoint>> {
        self.with_read(|conn| {
            conn.query_row(
                "SELECT id, name, provider, encrypted_config, is_enabled, notify_on_new_mail, created_at, updated_at
                 FROM webhook_endpoints WHERE id = ?1",
                params![id],
                row_to_stored,
            )
            .optional()
            .map_err(Into::into)
        })
    }

    pub fn update_webhook_endpoint(&self, endpoint: &StoredWebhookEndpoint) -> Result<()> {
        self.with_write(|conn| {
            let changed = conn.execute(
                "UPDATE webhook_endpoints
                 SET name = ?1, provider = ?2, encrypted_config = ?3, is_enabled = ?4,
                     notify_on_new_mail = ?5, updated_at = ?6
                 WHERE id = ?7",
                params![
                    endpoint.endpoint.name,
                    provider_to_str(&endpoint.endpoint.provider),
                    endpoint.encrypted_config,
                    endpoint.endpoint.is_enabled as i32,
                    endpoint.endpoint.notify_on_new_mail as i32,
                    endpoint.endpoint.updated_at,
                    endpoint.endpoint.id,
                ],
            )?;
            if changed == 0 {
                return Err(PebbleError::Storage(format!(
                    "Webhook endpoint not found: {}",
                    endpoint.endpoint.id
                )));
            }
            Ok(())
        })
    }

    pub fn delete_webhook_endpoint(&self, id: &str) -> Result<()> {
        self.with_write(|conn| {
            conn.execute("DELETE FROM webhook_endpoints WHERE id = ?1", params![id])?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;

    fn endpoint(name: &str) -> StoredWebhookEndpoint {
        let now = pebble_core::now_timestamp();
        StoredWebhookEndpoint {
            endpoint: WebhookEndpoint {
                id: pebble_core::new_id(),
                name: name.to_string(),
                provider: WebhookProvider::Slack,
                is_enabled: true,
                notify_on_new_mail: false,
                created_at: now,
                updated_at: now,
            },
            encrypted_config: b"encrypted webhook config".to_vec(),
        }
    }

    #[test]
    fn webhook_endpoint_crud_round_trips_encrypted_config() {
        let store = Store::open_in_memory().unwrap();
        let mut item = endpoint("Alerts");
        let id = item.endpoint.id.clone();

        store.insert_webhook_endpoint(&item).unwrap();
        let listed = store.list_webhook_endpoints().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "Alerts");

        let loaded = store.get_webhook_endpoint(&id).unwrap().unwrap();
        assert_eq!(loaded.encrypted_config, b"encrypted webhook config");

        item.endpoint.name = "Critical alerts".to_string();
        item.endpoint.notify_on_new_mail = true;
        item.encrypted_config = b"updated encrypted config".to_vec();
        item.endpoint.updated_at += 1;
        store.update_webhook_endpoint(&item).unwrap();

        let enabled = store.list_enabled_webhook_endpoints_for_new_mail().unwrap();
        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0].endpoint.name, "Critical alerts");
        assert_eq!(enabled[0].encrypted_config, b"updated encrypted config");

        store.delete_webhook_endpoint(&id).unwrap();
        assert!(store.get_webhook_endpoint(&id).unwrap().is_none());
    }
}
