pub mod deepl;
pub mod deeplx;
pub mod generic;
pub mod llm;
pub mod types;

use pebble_core::{HttpProxyConfig, PebbleError, Result};
use types::{TranslateProviderConfig, TranslateResult};

pub struct TranslateService;

impl TranslateService {
    pub fn http_client_with_proxy(proxy: Option<&HttpProxyConfig>) -> Result<reqwest::Client> {
        let mut builder = reqwest::ClientBuilder::new().no_proxy();
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

    pub async fn translate(
        config: &TranslateProviderConfig,
        text: &str,
        from: &str,
        to: &str,
    ) -> Result<TranslateResult> {
        Self::translate_with_proxy(config, None, text, from, to).await
    }

    pub async fn translate_with_proxy(
        config: &TranslateProviderConfig,
        proxy: Option<&HttpProxyConfig>,
        text: &str,
        from: &str,
        to: &str,
    ) -> Result<TranslateResult> {
        let client = Self::http_client_with_proxy(proxy)?;

        match config {
            TranslateProviderConfig::DeepLX { endpoint } => {
                deeplx::translate(&client, endpoint, text, from, to).await
            }
            TranslateProviderConfig::DeepL {
                api_key,
                use_free_api,
            } => deepl::translate(&client, api_key, *use_free_api, text, from, to).await,
            TranslateProviderConfig::GenericApi {
                endpoint,
                api_key,
                source_lang_param,
                target_lang_param,
                text_param,
                result_path,
            } => {
                generic::translate(
                    &client,
                    endpoint,
                    api_key.as_deref(),
                    source_lang_param,
                    target_lang_param,
                    text_param,
                    result_path,
                    text,
                    from,
                    to,
                )
                .await
            }
            TranslateProviderConfig::LLM {
                endpoint,
                api_key,
                model,
                mode,
            } => llm::translate(&client, endpoint, api_key, model, mode, text, from, to).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pebble_core::HttpProxyConfig;
    use std::ffi::OsString;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Mutex;

    static PROXY_ENV_LOCK: Mutex<()> = Mutex::new(());

    const PROXY_ENV_KEYS: [&str; 8] = [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "NO_PROXY",
        "no_proxy",
    ];

    struct ProxyEnvironmentGuard(Vec<(&'static str, Option<OsString>)>);

    impl ProxyEnvironmentGuard {
        fn install(proxy_url: &str) -> Self {
            let previous = PROXY_ENV_KEYS
                .into_iter()
                .map(|key| (key, std::env::var_os(key)))
                .collect();
            for key in PROXY_ENV_KEYS {
                if key.eq_ignore_ascii_case("ALL_PROXY") {
                    std::env::set_var(key, proxy_url);
                } else {
                    std::env::remove_var(key);
                }
            }
            Self(previous)
        }
    }

    impl Drop for ProxyEnvironmentGuard {
        fn drop(&mut self) {
            for (key, value) in self.0.drain(..) {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    fn start_http_origin() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            stream
                .write_all(
                    b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
        });
        format!("http://{address}/health")
    }

    fn unused_http_proxy_url() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        format!("http://{address}")
    }

    #[tokio::test]
    async fn translate_service_without_explicit_proxy_ignores_all_proxy() {
        let origin_url = start_http_origin();
        let client = {
            let _lock = PROXY_ENV_LOCK.lock().unwrap();
            let _proxy_environment = ProxyEnvironmentGuard::install(&unused_http_proxy_url());
            TranslateService::http_client_with_proxy(None).unwrap()
        };

        let response = client
            .get(origin_url)
            .timeout(std::time::Duration::from_secs(2))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::NO_CONTENT);
    }

    #[test]
    fn translate_service_accepts_socks5_proxy_config() {
        let proxy = HttpProxyConfig {
            host: "127.0.0.1".to_string(),
            port: 7890,
        };

        let client = TranslateService::http_client_with_proxy(Some(&proxy));

        assert!(client.is_ok());
    }

    #[test]
    fn translate_service_rejects_invalid_proxy_config() {
        let proxy = HttpProxyConfig {
            host: " ".to_string(),
            port: 7890,
        };

        let err = TranslateService::http_client_with_proxy(Some(&proxy)).unwrap_err();

        assert!(err.to_string().contains("Proxy host"));
    }

    #[tokio::test]
    async fn translate_service_validates_proxy_before_translation_request() {
        let config = TranslateProviderConfig::DeepLX {
            endpoint: "http://localhost:1188/translate".to_string(),
        };
        let proxy = HttpProxyConfig {
            host: " ".to_string(),
            port: 7890,
        };

        let err =
            TranslateService::translate_with_proxy(&config, Some(&proxy), "Hello", "en", "zh")
                .await
                .unwrap_err();

        assert!(err.to_string().contains("Proxy host"));
    }
}
