use crate::config::Config;
use crate::oauth::OAuthSessionStore;
use crate::sync::SyncManager;
use pebble_crypto::CryptoService;
use pebble_search::TantivySearch;
use pebble_store::Store;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::broadcast;

pub type AppStateRef = Arc<AppState>;

pub struct AppState {
    pub config: Config,
    pub store: Arc<Store>,
    pub search: Arc<TantivySearch>,
    pub crypto: Arc<CryptoService>,
    pub attachments_dir: PathBuf,
    pub sync_manager: Arc<SyncManager>,
    pub ws_broadcast: broadcast::Sender<String>,
    pub oauth_sessions: OAuthSessionStore,
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
            config.google_oauth.clone(),
            config.microsoft_oauth.clone(),
            config.oauth_callback_url(),
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
            oauth_sessions: OAuthSessionStore::new(),
        })
    }
}
