use crate::config::Config;
use crate::sync::SyncManager;
use pebble_crypto::CryptoService;
use pebble_search::TantivySearch;
use pebble_store::Store;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, Notify, OwnedMutexGuard};

pub struct OAuthFlowRecord {
    pub provider: String,
    pub owner_token_digest: String,
    pub state: String,
    pub verifier: String,
    pub expires_at: i64,
    pub account_id: Option<String>,
    pub account_label: Option<String>,
    pub display_name_hint: Option<String>,
    pub sync_range_days: Option<u32>,
    pub proxy: Option<pebble_core::HttpProxyConfig>,
    pub consumed: bool,
    pub status: String,
    pub account: Option<pebble_core::Account>,
    pub error: Option<String>,
}

pub type AppStateRef = Arc<AppState>;

pub struct AppState {
    pub config: Config,
    pub store: Arc<Store>,
    pub search: Arc<TantivySearch>,
    pub crypto: Arc<CryptoService>,
    pub attachments_dir: PathBuf,
    pub sync_manager: Arc<SyncManager>,
    pub ws_broadcast: broadcast::Sender<String>,
    pub oauth_flows: Mutex<HashMap<String, OAuthFlowRecord>>,
    pub oauth_refresh_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    pub auto_backup_wake: Notify,
    secure_settings_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl AppState {
    pub async fn secure_setting_guard(&self, key: &str) -> OwnedMutexGuard<()> {
        let lock = {
            let mut locks = self.secure_settings_locks.lock().await;
            locks
                .entry(key.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        lock.lock_owned().await
    }

    pub fn init(config: Config) -> Result<Self, String> {
        std::fs::create_dir_all(&config.data_dir)
            .map_err(|e| format!("Failed to create data dir: {e}"))?;
        std::fs::create_dir_all(config.attachments_dir())
            .map_err(|e| format!("Failed to create attachments dir: {e}"))?;

        let store =
            Store::open(&config.db_path()).map_err(|e| format!("Failed to open store: {e}"))?;

        let search = TantivySearch::open(&config.index_dir())
            .map_err(|e| format!("Failed to open search index: {e}"))?;

        let key_file = config.key_file_path();
        let crypto = CryptoService::init(Some(&key_file))
            .map_err(|e| format!("Failed to init crypto: {e}"))?;

        let attachments_dir = config.attachments_dir();

        let store = Arc::new(store);
        let crypto = Arc::new(crypto);
        let search = Arc::new(search);

        let (ws_broadcast, _) = broadcast::channel(100);
        let oauth_refresh_locks = Arc::new(Mutex::new(HashMap::new()));

        let mut realtime_preference = config.realtime_preference.clone();
        if let Some(ciphertext) = store
            .get_secure_user_data("realtime_preference")
            .map_err(|error| format!("Failed to load realtime preference: {error}"))?
        {
            let plaintext = crypto
                .decrypt_for("secure_user_data.value", "realtime_preference", &ciphertext)
                .map_err(|error| format!("Failed to decrypt realtime preference: {error}"))?;
            realtime_preference = serde_json::from_slice::<String>(&plaintext)
                .map_err(|error| format!("Invalid realtime preference: {error}"))?;
            if CryptoService::ciphertext_needs_migration(&ciphertext) {
                let encrypted = crypto
                    .encrypt_for("secure_user_data.value", "realtime_preference", &plaintext)
                    .map_err(|error| format!("Failed to migrate realtime preference: {error}"))?;
                if !store
                    .compare_exchange_secure_user_data(
                        "realtime_preference",
                        &ciphertext,
                        &encrypted,
                    )
                    .map_err(|error| {
                        format!("Failed to persist realtime preference migration: {error}")
                    })?
                {
                    return Err("Realtime preference changed during migration".into());
                }
            }
        } else {
            let plaintext = serde_json::to_vec(&realtime_preference)
                .map_err(|error| format!("Failed to serialize realtime preference: {error}"))?;
            let ciphertext = crypto
                .encrypt_for("secure_user_data.value", "realtime_preference", &plaintext)
                .map_err(|error| format!("Failed to encrypt realtime preference: {error}"))?;
            store
                .set_secure_user_data("realtime_preference", &ciphertext)
                .map_err(|error| format!("Failed to persist realtime preference: {error}"))?;
        }
        let interval = crate::config::realtime_poll_interval(&realtime_preference)
            .ok_or_else(|| "Invalid persisted realtime preference".to_string())?;
        let sync_manager = Arc::new(SyncManager::new(
            store.clone(),
            crypto.clone(),
            search.clone(),
            oauth_refresh_locks.clone(),
            attachments_dir.clone(),
            interval,
            ws_broadcast.clone(),
        ));

        Ok(Self {
            config,
            store,
            search,
            crypto,
            attachments_dir,
            sync_manager,
            ws_broadcast,
            oauth_flows: Mutex::new(HashMap::new()),
            oauth_refresh_locks,
            auto_backup_wake: Notify::new(),
            secure_settings_locks: Mutex::new(HashMap::new()),
        })
    }
}
