use crate::config::Config;
use crate::sync::SyncManager;
use pebble_crypto::CryptoService;
use pebble_search::TantivySearch;
use pebble_store::Store;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

pub struct OAuthFlowRecord {
    pub provider: String,
    pub state: String,
    pub verifier: String,
    pub expires_at: i64,
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
}

impl AppState {
    pub fn init(config: Config) -> Result<Self, String> {
        std::fs::create_dir_all(&config.data_dir)
            .map_err(|e| format!("Failed to create data dir: {e}"))?;
        std::fs::create_dir_all(config.attachments_dir())
            .map_err(|e| format!("Failed to create attachments dir: {e}"))?;

        let store = Store::open(&config.db_path())
            .map_err(|e| format!("Failed to open store: {e}"))?;

        let search = TantivySearch::open(&config.index_dir())
            .map_err(|e| format!("Failed to open search index: {e}"))?;

        let key_file = config.key_file_path();
        let crypto = CryptoService::init(Some(&key_file))
            .map_err(|e| format!("Failed to init crypto: {e}"))?;

        let attachments_dir = config.attachments_dir();

        let store = Arc::new(store);
        let crypto = Arc::new(crypto);

        let (ws_broadcast, _) = broadcast::channel(100);

        let sync_manager = Arc::new(SyncManager::new(
            store.clone(),
            crypto.clone(),
            attachments_dir.clone(),
            config.sync_interval_secs,
            ws_broadcast.clone(),
        ));

        Ok(Self {
            config,
            store,
            search: Arc::new(search),
            crypto,
            attachments_dir,
            sync_manager,
            ws_broadcast,
            oauth_flows: Mutex::new(HashMap::new()),
        })
    }
}
