pub mod gmail;
pub mod imap_provider;
pub mod outlook;

use std::sync::Arc;

use pebble_core::traits::MailProvider;
use pebble_core::{HttpProxyConfig, PebbleError, ProviderType, Result};

pub(crate) fn http_client_with_proxy(proxy: Option<&HttpProxyConfig>) -> Result<reqwest::Client> {
    let mut builder = reqwest::ClientBuilder::new()
        .timeout(std::time::Duration::from_secs(60))
        .connect_timeout(std::time::Duration::from_secs(15));
    if let Some(proxy) = proxy {
        let uri = proxy.socks5h_uri().map_err(PebbleError::Network)?;
        let reqwest_proxy = reqwest::Proxy::all(&uri)
            .map_err(|e| PebbleError::Network(format!("Invalid proxy: {e}")))?;
        builder = builder.proxy(reqwest_proxy);
    }
    builder
        .build()
        .map_err(|e| PebbleError::Network(format!("Failed to build HTTP client: {e}")))
}

fn proxy_from_credentials(credentials: &serde_json::Value) -> Result<Option<HttpProxyConfig>> {
    credentials
        .get("proxy")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| PebbleError::Auth(format!("Invalid OAuth proxy config: {e}")))
}

/// Create a trait-based mail provider from the given provider type and credentials.
pub async fn create_provider(
    provider_type: &ProviderType,
    credentials: &serde_json::Value,
    account_id: &str,
) -> Result<Arc<dyn MailProvider>> {
    match provider_type {
        ProviderType::Imap => {
            let imap_config: crate::imap::ImapConfig = serde_json::from_value(credentials.clone())
                .map_err(|e| PebbleError::Auth(format!("Invalid IMAP config: {e}")))?;
            let provider = imap_provider::ImapMailProvider::new(imap_config);
            Ok(Arc::new(provider))
        }
        ProviderType::Gmail => {
            let token = credentials
                .get("access_token")
                .and_then(|v| v.as_str())
                .ok_or_else(|| PebbleError::Auth("Missing access_token for Gmail".to_string()))?
                .to_string();
            let provider =
                gmail::GmailProvider::new_with_proxy(token, proxy_from_credentials(credentials)?)?;
            Ok(Arc::new(provider))
        }
        ProviderType::Outlook => {
            let token = credentials
                .get("access_token")
                .and_then(|v| v.as_str())
                .ok_or_else(|| PebbleError::Auth("Missing access_token for Outlook".to_string()))?
                .to_string();
            let provider = outlook::OutlookProvider::new_with_proxy(
                token,
                account_id.to_string(),
                proxy_from_credentials(credentials)?,
            )?;
            Ok(Arc::new(provider))
        }
    }
}
