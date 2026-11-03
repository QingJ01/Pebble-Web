use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use pebble_core::{OAuthTokens, ProviderType};
use pebble_crypto::CryptoService;
use pebble_mail::{
    ConnectionSecurity, GmailProvider, GmailSyncWorker, ImapConfig, ImapMailProvider,
    OutlookProvider, OutlookSyncWorker, Pop3Config, Pop3Provider, Pop3SyncWorker, ProxyConfig,
    SyncConfig, SyncError, SyncProgress, SyncRuntimeStatus, SyncTrigger, SyncWorker,
};
use pebble_search::TantivySearch;
use pebble_store::Store;
use tokio::sync::{broadcast, mpsc, watch, Mutex};
use tokio::task::JoinHandle;
use tokio::time::{timeout, Duration};
use tracing::{error, info, warn};

use crate::credentials::{
    decrypt_credentials, AccountCredentials, ImapCredentials, Pop3Credentials,
};

/// Handle for a running sync worker task.
pub struct SyncHandle {
    pub stop_tx: watch::Sender<bool>,
    pub trigger_tx: mpsc::UnboundedSender<SyncTrigger>,
    pub task: JoinHandle<()>,
}

enum WorkerTask {
    Imap(SyncWorker),
    Gmail(GmailSyncWorker),
    Outlook(OutlookSyncWorker),
    Pop3(Pop3SyncWorker),
}

fn oauth_token_refresher(
    store: Arc<Store>,
    crypto: Arc<CryptoService>,
    locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    account_id: String,
    provider: String,
) -> pebble_mail::gmail_sync::TokenRefresher {
    Box::new(move || {
        let store = store.clone();
        let crypto = crypto.clone();
        let locks = locks.clone();
        let account_id = account_id.clone();
        let provider = provider.clone();
        Box::pin(async move {
            let tokens = crate::routes::oauth::refresh_account_tokens(
                store, crypto, locks, account_id, provider,
            )
            .await
            .map_err(pebble_core::PebbleError::TokenRefreshFailed)?;
            Ok((tokens.access_token, tokens.expires_at))
        })
            as Pin<Box<dyn Future<Output = pebble_core::Result<(String, Option<i64>)>> + Send>>
    })
}

fn spawn_worker(
    worker: WorkerTask,
    config: SyncConfig,
    stop_rx: watch::Receiver<bool>,
    trigger_rx: mpsc::UnboundedReceiver<SyncTrigger>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        match worker {
            WorkerTask::Imap(worker) => worker.run(config, Some(trigger_rx)).await,
            WorkerTask::Gmail(worker) => worker.run(config, Some(trigger_rx)).await,
            WorkerTask::Outlook(worker) => worker.run(config, stop_rx, Some(trigger_rx)).await,
            WorkerTask::Pop3(worker) => worker.run(config, Some(trigger_rx)).await,
        }
    })
}

async fn stop_handle(mut handle: SyncHandle) {
    let _ = handle.stop_tx.send(true);
    if timeout(Duration::from_secs(5), &mut handle.task)
        .await
        .is_err()
    {
        handle.task.abort();
        let _ = handle.task.await;
    }
}

fn spawn_event_forwarders(
    account_id: String,
    mut error_rx: mpsc::UnboundedReceiver<SyncError>,
    mut message_rx: mpsc::UnboundedReceiver<pebble_mail::StoredMessage>,
    mut progress_rx: mpsc::UnboundedReceiver<SyncProgress>,
    mut status_rx: mpsc::UnboundedReceiver<SyncRuntimeStatus>,
    search: Arc<TantivySearch>,
    ws_tx: broadcast::Sender<String>,
) {
    let message_ws = ws_tx.clone();
    let message_account = account_id.clone();
    tokio::spawn(async move {
        while let Some(stored) = message_rx.recv().await {
            if let Err(error) = search.index_message(&stored.message, &stored.folder_ids) {
                warn!(
                    "Failed to index synced message {}: {error}",
                    stored.message.id
                );
            }
            if stored.notify {
                let payload = serde_json::json!({
                    "account_id": message_account,
                    "message_id": stored.message.id,
                });
                let _ = message_ws.send(
                    serde_json::json!({
                        "type": "new_mail", "payload": payload
                    })
                    .to_string(),
                );
                let _ = message_ws.send(
                    serde_json::json!({
                        "type": "mail:new", "payload": payload
                    })
                    .to_string(),
                );
            }
        }
    });

    let error_ws = ws_tx.clone();
    let error_account = account_id.clone();
    tokio::spawn(async move {
        while let Some(error) = error_rx.recv().await {
            let _ = error_ws.send(
                serde_json::json!({
                    "type": "sync_error", "account_id": error_account, "payload": error
                })
                .to_string(),
            );
        }
    });

    let progress_ws = ws_tx.clone();
    let progress_account = account_id.clone();
    tokio::spawn(async move {
        while let Some(progress) = progress_rx.recv().await {
            let value = serde_json::to_value(progress).unwrap_or(serde_json::Value::Null);
            let _ = progress_ws.send(
                serde_json::json!({
                    "type": "sync_progress", "account_id": progress_account, "payload": value
                })
                .to_string(),
            );
            if value["status"] == "completed" {
                let _ = progress_ws.send(
                    serde_json::json!({
                        "type": "sync_complete", "account_id": progress_account
                    })
                    .to_string(),
                );
                let _ = progress_ws.send(
                    serde_json::json!({
                        "type": "mail:folder-changed", "payload": { "account_id": progress_account }
                    })
                    .to_string(),
                );
            }
        }
    });

    let status_account = account_id;
    tokio::spawn(async move {
        while let Some(status) = status_rx.recv().await {
            let value = match status {
                SyncRuntimeStatus::ImapIdleAvailable => "imap_idle_available",
                SyncRuntimeStatus::ImapPollingFallback => "imap_polling_fallback",
            };
            let _ = ws_tx.send(
                serde_json::json!({
                    "type": "sync_status", "account_id": status_account, "payload": value
                })
                .to_string(),
            );
        }
    });
}

/// Manages provider-aware background sync workers for all configured accounts.
pub struct SyncManager {
    handles: Mutex<HashMap<String, SyncHandle>>,
    store: Arc<Store>,
    crypto: Arc<CryptoService>,
    search: Arc<TantivySearch>,
    oauth_refresh_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    attachments_dir: PathBuf,
    sync_interval_secs: Mutex<u64>,
    ws_tx: broadcast::Sender<String>,
}

impl SyncManager {
    pub fn new(
        store: Arc<Store>,
        crypto: Arc<CryptoService>,
        search: Arc<TantivySearch>,
        oauth_refresh_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
        attachments_dir: PathBuf,
        sync_interval_secs: u64,
        ws_tx: broadcast::Sender<String>,
    ) -> Self {
        Self {
            handles: Mutex::new(HashMap::new()),
            store,
            crypto,
            search,
            oauth_refresh_locks,
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
            if let Err(e) = self.start_account_sync(&account.id).await {
                error!("Failed to start sync for account {}: {e}", account.id);
            }
        }
    }

    /// Start sync for a single account by ID.
    pub async fn start_account_sync(&self, account_id: &str) -> Result<(), String> {
        let mut handles = self.handles.lock().await;

        if let Some(handle) = handles.remove(account_id) {
            stop_handle(handle).await;
        }

        let account = self
            .store
            .get_account(account_id)
            .map_err(|error| format!("Failed to get account: {error}"))?
            .ok_or_else(|| format!("Account {account_id} does not exist"))?;
        let sync_state_json = self
            .store
            .get_account_sync_state(account_id)
            .map_err(|e| format!("Failed to get sync state: {e}"))?
            .ok_or_else(|| "No sync state found for account".to_string())?;

        let sync_state: serde_json::Value = serde_json::from_str(&sync_state_json)
            .map_err(|e| format!("Invalid sync state JSON: {e}"))?;
        let global_proxy = load_global_proxy(&self.store, &self.crypto)?;

        let (stop_tx, stop_rx) = watch::channel(false);
        let (trigger_tx, trigger_rx) = mpsc::unbounded_channel();
        let (error_tx, error_rx) = mpsc::unbounded_channel();
        let (message_tx, message_rx) = mpsc::unbounded_channel();
        let (progress_tx, progress_rx) = mpsc::unbounded_channel();
        let (status_tx, status_rx) = mpsc::unbounded_channel();

        let poll_interval_secs = *self.sync_interval_secs.lock().await;
        let sync_config = SyncConfig {
            poll_interval_secs,
            ..SyncConfig::default()
        };
        let account_id_owned = account_id.to_string();
        let worker_store = self.store.clone();
        let attachments_dir = self.attachments_dir.clone();
        let crypto = self.crypto.clone();
        let provider_kind = account.provider;
        let worker = match provider_kind.clone() {
            ProviderType::Imap | ProviderType::Pop3 => {
                let encrypted_hex = sync_state["credentials"]
                    .as_str()
                    .ok_or_else(|| "No credentials in sync state".to_string())?;
                let creds = decrypt_credentials(&crypto, encrypted_hex)
                    .map_err(|error| format!("Failed to decrypt credentials: {error}"))?;
                match (provider_kind, creds) {
                    (ProviderType::Imap, AccountCredentials::Imap { imap, .. }) => {
                        let mut provider = ImapMailProvider::new(build_imap_config(
                            &imap,
                            &sync_state,
                            &account.email,
                            global_proxy.as_ref(),
                        )?);
                        provider.set_account_id(account_id.to_string());
                        let worker = SyncWorker::new(
                            account_id.to_string(),
                            Arc::new(provider),
                            worker_store,
                            stop_rx.clone(),
                            attachments_dir,
                        )
                        .with_error_tx(error_tx)
                        .with_message_tx(message_tx)
                        .with_progress_tx(progress_tx)
                        .with_runtime_status_tx(status_tx);
                        WorkerTask::Imap(worker)
                    }
                    (ProviderType::Pop3, AccountCredentials::Pop3 { pop3, smtp: _ }) => {
                        let worker = Pop3SyncWorker::new(
                            account_id.to_string(),
                            Arc::new(Pop3Provider::new(build_pop3_config(
                                &pop3,
                                &sync_state,
                                &account.email,
                                global_proxy.as_ref(),
                            )?)),
                            worker_store,
                            stop_rx.clone(),
                            attachments_dir,
                        )
                        .with_error_tx(error_tx)
                        .with_message_tx(message_tx)
                        .with_progress_tx(progress_tx);
                        WorkerTask::Pop3(worker)
                    }
                    _ => return Err("Account provider and stored credentials do not match".into()),
                }
            }
            ProviderType::Gmail | ProviderType::Outlook => {
                let lookup_account_id = account_id.to_string();
                let encrypted = self
                    .store
                    .with_read_async(move |conn| {
                        Ok(conn.query_row(
                            "SELECT auth_data FROM accounts WHERE id=?1",
                            rusqlite::params![lookup_account_id],
                            |row| row.get::<_, Option<Vec<u8>>>(0),
                        )?)
                    })
                    .await
                    .map_err(|error| format!("Failed to load OAuth credentials: {error}"))?
                    .ok_or_else(|| "OAuth credentials are missing".to_string())?;
                let plaintext = crypto
                    .decrypt_for("accounts.auth_data", account_id, &encrypted)
                    .map_err(|error| format!("Failed to decrypt OAuth credentials: {error}"))?;
                let tokens: OAuthTokens = serde_json::from_slice(&plaintext)
                    .map_err(|error| format!("Invalid OAuth credentials: {error}"))?;
                let proxy = oauth_proxy(&sync_state, global_proxy.as_ref())?;
                let expires_at = tokens.expires_at;
                let access_token = tokens.access_token;
                match provider_kind {
                    ProviderType::Gmail => {
                        let provider = Arc::new(
                            GmailProvider::new_with_proxy(access_token, proxy)
                                .map_err(|error| format!("Failed to configure Gmail: {error}"))?,
                        );
                        let refresher = oauth_token_refresher(
                            self.store.clone(),
                            self.crypto.clone(),
                            self.oauth_refresh_locks.clone(),
                            account_id.to_string(),
                            "gmail".into(),
                        );
                        WorkerTask::Gmail(
                            GmailSyncWorker::new(
                                account_id.to_string(),
                                provider,
                                worker_store,
                                stop_rx.clone(),
                                attachments_dir,
                            )
                            .with_error_tx(error_tx)
                            .with_message_tx(message_tx)
                            .with_progress_tx(progress_tx)
                            .with_token_refresher(refresher, expires_at),
                        )
                    }
                    ProviderType::Outlook => {
                        let provider = Arc::new(
                            OutlookProvider::new_with_proxy(
                                access_token,
                                account_id.to_string(),
                                proxy,
                            )
                            .map_err(|error| format!("Failed to configure Outlook: {error}"))?,
                        );
                        let refresher = oauth_token_refresher(
                            self.store.clone(),
                            self.crypto.clone(),
                            self.oauth_refresh_locks.clone(),
                            account_id.to_string(),
                            "outlook".into(),
                        );
                        WorkerTask::Outlook(
                            OutlookSyncWorker::new(
                                account_id.to_string(),
                                provider,
                                worker_store,
                                attachments_dir,
                            )
                            .with_error_tx(error_tx)
                            .with_message_tx(message_tx)
                            .with_progress_tx(progress_tx)
                            .with_token_refresher(refresher, expires_at),
                        )
                    }
                    _ => unreachable!(),
                }
            }
        };
        let task = spawn_worker(worker, sync_config, stop_rx, trigger_rx);
        spawn_event_forwarders(
            account_id_owned.clone(),
            error_rx,
            message_rx,
            progress_rx,
            status_rx,
            self.search.clone(),
            self.ws_tx.clone(),
        );

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
            stop_handle(handle).await;
        }
    }

    /// Trigger a manual sync for a specific account.
    pub async fn trigger_sync(&self, account_id: &str) -> Result<(), String> {
        let mut handles = self.handles.lock().await;
        if handles
            .get(account_id)
            .is_some_and(|handle| handle.task.is_finished())
        {
            if let Some(handle) = handles.remove(account_id) {
                stop_handle(handle).await;
            }
        }
        if let Some(handle) = handles.get(account_id) {
            handle
                .trigger_tx
                .send(SyncTrigger::Manual)
                .map_err(|_| "Sync worker channel closed".to_string())?;
            return Ok(());
        }
        drop(handles);
        self.start_account_sync(account_id).await
    }

    pub async fn is_account_running(&self, account_id: &str) -> bool {
        self.handles
            .lock()
            .await
            .get(account_id)
            .is_some_and(|handle| !handle.task.is_finished())
    }

    pub async fn restart_running_accounts(&self) -> Result<(), String> {
        let account_ids = self
            .handles
            .lock()
            .await
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for account_id in account_ids {
            self.start_account_sync(&account_id).await?;
        }
        Ok(())
    }

    pub async fn set_realtime_poll_interval(&self, interval_secs: u64) -> Result<(), String> {
        *self.sync_interval_secs.lock().await = interval_secs;
        let account_ids = self
            .handles
            .lock()
            .await
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for account_id in account_ids {
            self.start_account_sync(&account_id).await?;
        }
        Ok(())
    }
}

fn connection_security(value: &str, allow_plaintext: bool) -> Result<ConnectionSecurity, String> {
    match value {
        "tls" => Ok(ConnectionSecurity::Tls),
        "starttls" => Ok(ConnectionSecurity::StartTls),
        "plain" if allow_plaintext => Ok(ConnectionSecurity::Plain),
        "plain" => Err("Plaintext mail transport requires explicit consent".into()),
        _ => Err("Unsupported mail transport security".into()),
    }
}

fn account_proxy(
    sync_state: &serde_json::Value,
) -> Result<Option<pebble_core::HttpProxyConfig>, String> {
    let Some(host) = sync_state["proxy"]["host"].as_str() else {
        return Ok(None);
    };
    let port = sync_state["proxy"]["port"]
        .as_u64()
        .and_then(|value| u16::try_from(value).ok())
        .ok_or_else(|| "Invalid account proxy port".to_string())?;
    let proxy = pebble_core::HttpProxyConfig {
        host: host.trim().to_string(),
        port,
    };
    proxy.validate()?;
    Ok(Some(proxy))
}

pub(crate) fn resolve_proxy(
    sync_state: &serde_json::Value,
    global_proxy: Option<&pebble_core::HttpProxyConfig>,
) -> Result<Option<pebble_core::HttpProxyConfig>, String> {
    let proxy = account_proxy(sync_state)?;
    let mode = sync_state["proxy_mode"].as_str().unwrap_or("inherit");
    match mode {
        "disabled" | "none" => Ok(None),
        "custom" => proxy
            .ok_or_else(|| "Custom account proxy is missing".into())
            .map(Some),
        "inherit" if proxy.is_some() => Ok(proxy),
        "inherit" => Ok(global_proxy.cloned()),
        _ => Err("Unsupported account proxy mode".into()),
    }
}

pub(crate) fn build_imap_config(
    creds: &ImapCredentials,
    sync_state: &serde_json::Value,
    email: &str,
    global_proxy: Option<&pebble_core::HttpProxyConfig>,
) -> Result<ImapConfig, String> {
    let security = connection_security(&creds.security, creds.allow_plaintext)?;
    Ok(ImapConfig {
        host: creds.host.clone(),
        port: creds.port,
        username: if creds.username.trim().is_empty() {
            email.to_string()
        } else {
            creds.username.clone()
        },
        password: creds.password.clone(),
        security,
        accept_invalid_certs: creds.accept_invalid_certs,
        proxy: resolve_proxy(sync_state, global_proxy)?.map(|proxy| ProxyConfig {
            host: proxy.host,
            port: proxy.port,
        }),
    })
}

fn build_pop3_config(
    creds: &Pop3Credentials,
    sync_state: &serde_json::Value,
    email: &str,
    global_proxy: Option<&pebble_core::HttpProxyConfig>,
) -> Result<Pop3Config, String> {
    Ok(Pop3Config {
        host: creds.host.clone(),
        port: creds.port,
        username: if creds.username.trim().is_empty() {
            email.to_string()
        } else {
            creds.username.clone()
        },
        password: creds.password.clone(),
        security: connection_security(&creds.security, creds.allow_plaintext)?,
        accept_invalid_certs: creds.accept_invalid_certs,
        proxy: resolve_proxy(sync_state, global_proxy)?.map(|proxy| ProxyConfig {
            host: proxy.host,
            port: proxy.port,
        }),
    })
}

pub(crate) fn oauth_proxy(
    sync_state: &serde_json::Value,
    global_proxy: Option<&pebble_core::HttpProxyConfig>,
) -> Result<Option<pebble_core::HttpProxyConfig>, String> {
    resolve_proxy(sync_state, global_proxy)
}

pub(crate) fn load_global_proxy(
    store: &Store,
    crypto: &CryptoService,
) -> Result<Option<pebble_core::HttpProxyConfig>, String> {
    const KEY: &str = "global_network_proxy";
    let Some(ciphertext) = store
        .get_secure_user_data(KEY)
        .map_err(|error| format!("Failed to read global proxy: {error}"))?
    else {
        return Ok(None);
    };
    let plaintext = match crypto.decrypt_for("secure_user_data.value", KEY, &ciphertext) {
        Ok(plaintext) => plaintext,
        Err(_) => {
            let plaintext = crypto
                .decrypt(&ciphertext)
                .map_err(|error| format!("Failed to decrypt global proxy: {error}"))?;
            let proxy: pebble_core::HttpProxyConfig = serde_json::from_slice(&plaintext)
                .map_err(|error| format!("Invalid global proxy: {error}"))?;
            let encrypted = crypto
                .encrypt_for("secure_user_data.value", KEY, &plaintext)
                .map_err(|error| format!("Failed to migrate global proxy: {error}"))?;
            store
                .compare_exchange_secure_user_data(KEY, &ciphertext, &encrypted)
                .map_err(|error| format!("Failed to persist migrated global proxy: {error}"))?;
            proxy.validate()?;
            return Ok(Some(proxy));
        }
    };
    let proxy: pebble_core::HttpProxyConfig = serde_json::from_slice(&plaintext)
        .map_err(|error| format!("Invalid global proxy: {error}"))?;
    proxy.validate()?;
    Ok(Some(proxy))
}
