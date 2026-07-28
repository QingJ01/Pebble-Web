use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use pebble_core::ProviderType;
use pebble_crypto::CryptoService;
use pebble_mail::{
    ConnectionSecurity, GmailProvider, GmailSyncWorker, ImapConfig, ImapMailProvider,
    OutlookProvider, OutlookSyncWorker, StoredMessage, SyncConfig, SyncProgress, SyncTrigger,
    SyncWorker,
};
use pebble_store::Store;
use tokio::sync::{broadcast, mpsc, watch, Mutex};
use tokio::task::JoinHandle;
use tracing::{error, info};

use crate::credentials::{decrypt_credentials, AccountCredentials, ImapCredentials};
use crate::oauth::{
    build_oauth_token_refresher, config_for_provider, read_oauth_tokens, OAuthProviderCredentials,
};

/// Handle for a running sync worker task.
pub struct SyncHandle {
    pub stop_tx: watch::Sender<bool>,
    pub trigger_tx: mpsc::UnboundedSender<SyncTrigger>,
    pub task: JoinHandle<()>,
}

/// Manages background mail sync workers for all configured accounts.
pub struct SyncManager {
    handles: Mutex<HashMap<String, SyncHandle>>,
    store: Arc<Store>,
    crypto: Arc<CryptoService>,
    attachments_dir: PathBuf,
    sync_interval_secs: u64,
    google_oauth: Option<OAuthProviderCredentials>,
    microsoft_oauth: Option<OAuthProviderCredentials>,
    oauth_redirect_url: String,
    ws_tx: broadcast::Sender<String>,
}

impl SyncManager {
    pub fn new(
        store: Arc<Store>,
        crypto: Arc<CryptoService>,
        attachments_dir: PathBuf,
        sync_interval_secs: u64,
        google_oauth: Option<OAuthProviderCredentials>,
        microsoft_oauth: Option<OAuthProviderCredentials>,
        oauth_redirect_url: String,
        ws_tx: broadcast::Sender<String>,
    ) -> Self {
        Self {
            handles: Mutex::new(HashMap::new()),
            store,
            crypto,
            attachments_dir,
            sync_interval_secs,
            google_oauth,
            microsoft_oauth,
            oauth_redirect_url,
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
            if let Err(e) = self.start_account_sync(&account.id).await {
                error!("Failed to start sync for account {}: {e}", account.id);
            }
        }
    }

    /// Start sync for a single account by ID.
    pub async fn start_account_sync(&self, account_id: &str) -> Result<(), String> {
        let mut handles = self.handles.lock().await;

        if let Some(handle) = handles.remove(account_id) {
            let _ = handle.stop_tx.send(true);
            handle.task.abort();
        }

        let account = self
            .store
            .get_account(account_id)
            .map_err(|e| format!("Failed to get account: {e}"))?
            .ok_or_else(|| format!("Account not found: {account_id}"))?;

        let (stop_tx, stop_rx) = watch::channel(false);
        let (trigger_tx, trigger_rx) = mpsc::unbounded_channel();
        let sync_config = SyncConfig {
            poll_interval_secs: self.sync_interval_secs,
            ..SyncConfig::default()
        };

        let account_id_owned = account_id.to_string();
        let ws_tx = self.ws_tx.clone();
        let (progress_tx, progress_rx) = mpsc::unbounded_channel::<SyncProgress>();
        let (message_tx, message_rx) = mpsc::unbounded_channel::<StoredMessage>();
        spawn_sync_ws_bridges(
            ws_tx.clone(),
            account_id_owned.clone(),
            progress_rx,
            message_rx,
        );

        let task = match account.provider {
            ProviderType::Imap => {
                let provider = self.build_imap_provider(account_id)?;
                let worker = SyncWorker::new(
                    account_id.to_string(),
                    provider,
                    self.store.clone(),
                    stop_rx,
                    self.attachments_dir.clone(),
                )
                .with_progress_tx(progress_tx)
                .with_message_tx(message_tx);
                tokio::spawn(async move {
                    emit_sync_started(&ws_tx, &account_id_owned);
                    info!("IMAP sync worker started for account {}", account_id_owned);
                    worker.run(sync_config, Some(trigger_rx)).await;
                    emit_sync_complete(&ws_tx, &account_id_owned);
                    info!("IMAP sync worker stopped for account {}", account_id_owned);
                })
            }
            ProviderType::Gmail => {
                let (provider, refresher, expires_at) =
                    self.build_gmail_provider(account_id).await?;
                let worker = GmailSyncWorker::new(
                    account_id.to_string(),
                    provider,
                    self.store.clone(),
                    stop_rx,
                    self.attachments_dir.clone(),
                )
                .with_token_refresher(refresher, expires_at)
                .with_progress_tx(progress_tx)
                .with_message_tx(message_tx);
                tokio::spawn(async move {
                    emit_sync_started(&ws_tx, &account_id_owned);
                    info!("Gmail sync worker started for account {}", account_id_owned);
                    worker.run(sync_config, Some(trigger_rx)).await;
                    emit_sync_complete(&ws_tx, &account_id_owned);
                    info!("Gmail sync worker stopped for account {}", account_id_owned);
                })
            }
            ProviderType::Outlook => {
                let (provider, refresher, expires_at) =
                    self.build_outlook_provider(account_id).await?;
                let worker = OutlookSyncWorker::new(
                    account_id.to_string(),
                    provider,
                    self.store.clone(),
                    self.attachments_dir.clone(),
                )
                .with_token_refresher(refresher, expires_at)
                .with_progress_tx(progress_tx)
                .with_message_tx(message_tx);
                tokio::spawn(async move {
                    emit_sync_started(&ws_tx, &account_id_owned);
                    info!(
                        "Outlook sync worker started for account {}",
                        account_id_owned
                    );
                    worker
                        .run(sync_config, stop_rx, Some(trigger_rx))
                        .await;
                    emit_sync_complete(&ws_tx, &account_id_owned);
                    info!(
                        "Outlook sync worker stopped for account {}",
                        account_id_owned
                    );
                })
            }
        };

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

    fn build_imap_provider(&self, account_id: &str) -> Result<Arc<ImapMailProvider>, String> {
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

        Ok(Arc::new(ImapMailProvider::new(build_imap_config(
            &imap_creds,
        ))))
    }

    async fn build_gmail_provider(
        &self,
        account_id: &str,
    ) -> Result<
        (
            Arc<GmailProvider>,
            pebble_mail::gmail_sync::TokenRefresher,
            Option<i64>,
        ),
        String,
    > {
        let stored = read_oauth_tokens(&self.crypto, &self.store, account_id)?;
        let oauth_config = config_for_provider(
            "gmail",
            self.google_oauth.as_ref(),
            self.microsoft_oauth.as_ref(),
            self.oauth_redirect_url.clone(),
        )?;
        let provider = Arc::new(
            GmailProvider::new_with_proxy(stored.access_token.clone(), None)
                .map_err(|e| format!("Failed to create Gmail provider: {e}"))?,
        );
        let refresher = build_oauth_token_refresher(
            oauth_config,
            stored.refresh_token.clone(),
            stored.access_token.clone(),
            self.crypto.clone(),
            self.store.clone(),
            account_id.to_string(),
        );
        Ok((provider, refresher, stored.expires_at))
    }

    async fn build_outlook_provider(
        &self,
        account_id: &str,
    ) -> Result<
        (
            Arc<OutlookProvider>,
            pebble_mail::gmail_sync::TokenRefresher,
            Option<i64>,
        ),
        String,
    > {
        let stored = read_oauth_tokens(&self.crypto, &self.store, account_id)?;
        let oauth_config = config_for_provider(
            "outlook",
            self.google_oauth.as_ref(),
            self.microsoft_oauth.as_ref(),
            self.oauth_redirect_url.clone(),
        )?;
        let provider = Arc::new(
            OutlookProvider::new_with_proxy(
                stored.access_token.clone(),
                account_id.to_string(),
                None,
            )
            .map_err(|e| format!("Failed to create Outlook provider: {e}"))?,
        );
        let refresher = build_oauth_token_refresher(
            oauth_config,
            stored.refresh_token.clone(),
            stored.access_token.clone(),
            self.crypto.clone(),
            self.store.clone(),
            account_id.to_string(),
        );
        Ok((provider, refresher, stored.expires_at))
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

        emit_sync_started(&self.ws_tx, account_id);
        Ok(())
    }
}

fn emit_sync_started(ws_tx: &broadcast::Sender<String>, account_id: &str) {
    let _ = ws_tx.send(
        serde_json::json!({
            "type": "sync_started",
            "account_id": account_id,
        })
        .to_string(),
    );
}

fn emit_sync_complete(ws_tx: &broadcast::Sender<String>, account_id: &str) {
    let _ = ws_tx.send(
        serde_json::json!({
            "type": "sync_complete",
            "account_id": account_id,
        })
        .to_string(),
    );
}

fn emit_new_mail(ws_tx: &broadcast::Sender<String>, account_id: &str, message_id: &str) {
    let _ = ws_tx.send(
        serde_json::json!({
            "type": "new_mail",
            "account_id": account_id,
            "message_id": message_id,
        })
        .to_string(),
    );
}

/// Forward per-poll sync progress / new messages to the frontend WebSocket.
/// Without this, `sync_complete` only fires when the worker task exits, so the
/// UI keeps a stale empty folders/messages cache during continuous Outlook sync.
fn spawn_sync_ws_bridges(
    ws_tx: broadcast::Sender<String>,
    account_id: String,
    mut progress_rx: mpsc::UnboundedReceiver<SyncProgress>,
    mut message_rx: mpsc::UnboundedReceiver<StoredMessage>,
) {
    let progress_ws = ws_tx.clone();
    let progress_account_id = account_id.clone();
    tokio::spawn(async move {
        while let Some(progress) = progress_rx.recv().await {
            if progress.status == "completed" {
                emit_sync_complete(&progress_ws, &progress_account_id);
            }
        }
    });

    tokio::spawn(async move {
        while let Some(stored) = message_rx.recv().await {
            if stored.notify {
                emit_new_mail(&ws_tx, &account_id, &stored.message.id);
            }
        }
    });
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
