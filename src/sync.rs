use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use pebble_core::ProviderType;
use pebble_crypto::CryptoService;
use pebble_mail::{
    ConnectionSecurity, ImapConfig, ImapMailProvider, SyncConfig, SyncTrigger, SyncWorker,
};
use pebble_store::Store;
use tokio::sync::{broadcast, mpsc, watch, Mutex};
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::credentials::{decrypt_credentials, AccountCredentials, ImapCredentials};

/// Handle for a running sync worker task.
pub struct SyncHandle {
    pub stop_tx: watch::Sender<bool>,
    pub trigger_tx: mpsc::UnboundedSender<SyncTrigger>,
    pub task: JoinHandle<()>,
}

/// Manages background IMAP sync workers for all configured accounts.
pub struct SyncManager {
    handles: Mutex<HashMap<String, SyncHandle>>,
    store: Arc<Store>,
    crypto: Arc<CryptoService>,
    attachments_dir: PathBuf,
    sync_interval_secs: Mutex<u64>,
    ws_tx: broadcast::Sender<String>,
}

impl SyncManager {
    pub fn new(
        store: Arc<Store>,
        crypto: Arc<CryptoService>,
        attachments_dir: PathBuf,
        sync_interval_secs: u64,
        ws_tx: broadcast::Sender<String>,
    ) -> Self {
        Self {
            handles: Mutex::new(HashMap::new()),
            store,
            crypto,
            attachments_dir,
            sync_interval_secs: Mutex::new(sync_interval_secs),
            ws_tx,
        }
    }

    /// Start sync workers for all configured accounts.
    pub async fn start_all(&self) {
        let accounts = match self.store.list_accounts() {
            Ok(accounts) => accounts,
            Err(e) => {
                error!("Failed to list accounts for sync startup: {e}");
                return;
            }
        };

        for account in accounts {
            if account.provider != ProviderType::Imap {
                warn!(
                    "Skipping sync for non-IMAP account {} (provider: {:?})",
                    account.id, account.provider
                );
                continue;
            }

            if let Err(e) = self.start_account_sync(&account.id).await {
                error!("Failed to start sync for account {}: {e}", account.id);
            }
        }
    }

    /// Start sync for a single account by ID.
    pub async fn start_account_sync(&self, account_id: &str) -> Result<(), String> {
        let mut handles = self.handles.lock().await;

        // If already running, stop the existing worker first.
        if let Some(handle) = handles.remove(account_id) {
            let _ = handle.stop_tx.send(true);
            handle.task.abort();
        }

        // Get credentials from sync_state.
        let sync_state_json = self
            .store
            .get_account_sync_state(account_id)
            .map_err(|e| format!("Failed to get sync state: {e}"))?
            .ok_or_else(|| "No sync state found for account".to_string())?;

        let sync_state: serde_json::Value = serde_json::from_str(&sync_state_json)
            .map_err(|e| format!("Invalid sync state JSON: {e}"))?;

        let encrypted_hex = sync_state["credentials"]
            .as_str()
            .ok_or_else(|| "No credentials in sync state".to_string())?;

        let creds = decrypt_credentials(&self.crypto, encrypted_hex)
            .map_err(|e| format!("Failed to decrypt credentials: {e}"))?;

        let imap_creds = match creds {
            AccountCredentials::Imap { ref imap, .. } => imap.clone(),
        };

        let imap_config = build_imap_config(&imap_creds);
        let provider = Arc::new(ImapMailProvider::new(imap_config));

        let (stop_tx, stop_rx) = watch::channel(false);
        let (trigger_tx, trigger_rx) = mpsc::unbounded_channel();

        let worker = SyncWorker::new(
            account_id.to_string(),
            provider,
            self.store.clone(),
            stop_rx,
            self.attachments_dir.clone(),
        );

        let poll_interval_secs = *self.sync_interval_secs.lock().await;
        let sync_config = SyncConfig {
            poll_interval_secs,
            ..SyncConfig::default()
        };

        let account_id_owned = account_id.to_string();
        let ws_tx = self.ws_tx.clone();
        let task = tokio::spawn(async move {
            info!("Sync worker started for account {}", account_id_owned);
            let _ = ws_tx.send(
                serde_json::json!({
                    "type": "sync_started",
                    "account_id": account_id_owned,
                })
                .to_string(),
            );
            worker.run(sync_config, Some(trigger_rx)).await;
            let _ = ws_tx.send(
                serde_json::json!({
                    "type": "sync_complete",
                    "account_id": account_id_owned,
                })
                .to_string(),
            );
            info!("Sync worker stopped for account {}", account_id_owned);
        });

        handles.insert(
            account_id.to_string(),
            SyncHandle {
                stop_tx,
                trigger_tx,
                task,
            },
        );

        info!("Started sync for account {}", account_id);
        Ok(())
    }

    /// Stop sync for a single account.
    pub async fn stop_account_sync(&self, account_id: &str) {
        let mut handles = self.handles.lock().await;
        if let Some(handle) = handles.remove(account_id) {
            info!("Stopping sync for account {}", account_id);
            let _ = handle.stop_tx.send(true);
            handle.task.abort();
        }
    }

    /// Trigger a manual sync for a specific account.
    pub async fn trigger_sync(&self, account_id: &str) -> Result<(), String> {
        let handles = self.handles.lock().await;
        let handle = handles
            .get(account_id)
            .ok_or_else(|| format!("No sync worker running for account {account_id}"))?;

        handle
            .trigger_tx
            .send(SyncTrigger::Manual)
            .map_err(|_| "Sync worker channel closed".to_string())?;

        let _ = self.ws_tx.send(
            serde_json::json!({
                "type": "sync_started",
                "account_id": account_id,
            })
            .to_string(),
        );

        Ok(())
    }

    pub async fn set_realtime_poll_interval(&self, interval_secs: u64) -> Result<(), String> {
        *self.sync_interval_secs.lock().await = interval_secs;
        let account_ids = self.handles.lock().await.keys().cloned().collect::<Vec<_>>();
        for account_id in account_ids {
            self.start_account_sync(&account_id).await?;
        }
        Ok(())
    }
}

/// Convert ImapCredentials to ImapConfig for pebble-mail.
fn build_imap_config(creds: &ImapCredentials) -> ImapConfig {
    let security = match creds.security.as_str() {
        "starttls" => ConnectionSecurity::StartTls,
        "plain" => ConnectionSecurity::Plain,
        _ => ConnectionSecurity::Tls,
    };

    ImapConfig {
        host: creds.host.clone(),
        port: creds.port,
        username: creds.username.clone(),
        password: creds.password.clone(),
        security,
        proxy: None,
    }
}
