//! Outbound webhook notifications for new mail and rule actions.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use base64::{engine::general_purpose, Engine as _};
use hmac::{Hmac, Mac};
use pebble_core::{Message, StoredWebhookEndpoint, WebhookProvider};
use pebble_crypto::CryptoService;
use pebble_store::Store;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Sha256;
use tokio::sync::Semaphore;
use tracing::warn;

const DEFAULT_TIMEOUT_SECS: u64 = 10;
const DEFAULT_GOTIFY_PRIORITY: i32 = 5;
const MAX_CONCURRENT_DELIVERIES: usize = 4;
const CONFIG_PURPOSE: &str = "webhook_config";

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WebhookConfig {
    pub url: Option<String>,
    pub method: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    pub bot_token: Option<String>,
    pub chat_id: Option<String>,
    pub parse_mode: Option<String>,
    pub secret: Option<String>,
    pub server_url: Option<String>,
    pub topic: Option<String>,
    pub token: Option<String>,
    pub priority: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationMessage {
    pub title: String,
    pub body: String,
    pub subject: String,
    pub from_name: String,
    pub from_address: String,
    pub account_id: String,
    pub message_id: String,
    pub snippet: String,
    pub date: i64,
}

impl NotificationMessage {
    pub fn from_message(message: &Message) -> Self {
        let sender = if message.from_name.trim().is_empty() {
            message.from_address.clone()
        } else {
            format!("{} <{}>", message.from_name, message.from_address)
        };
        let subject = if message.subject.trim().is_empty() {
            "(no subject)".to_string()
        } else {
            message.subject.clone()
        };
        let body = format!("From: {sender}\nSubject: {subject}\n{}", message.snippet);

        Self {
            title: format!("New mail: {subject}"),
            body,
            subject,
            from_name: message.from_name.clone(),
            from_address: message.from_address.clone(),
            account_id: message.account_id.clone(),
            message_id: message.id.clone(),
            snippet: message.snippet.clone(),
            date: message.date,
        }
    }

    pub fn test() -> Self {
        Self {
            title: "Pebble Web test notification".to_string(),
            body: "This is a test notification from Pebble Web.".to_string(),
            subject: "Test notification".to_string(),
            from_name: "Pebble Web".to_string(),
            from_address: "pebble@example.com".to_string(),
            account_id: "test-account".to_string(),
            message_id: "test-message".to_string(),
            snippet: "If you can see this message, your webhook endpoint is configured correctly."
                .to_string(),
            date: pebble_core::now_timestamp(),
        }
    }
}

pub async fn send_notification(
    provider: &WebhookProvider,
    config: &WebhookConfig,
    message: &NotificationMessage,
) -> Result<(), String> {
    validate_config(provider, config)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))?;

    match provider {
        WebhookProvider::Generic => send_generic(&client, config, message).await,
        WebhookProvider::Slack => send_slack(&client, config, message).await,
        WebhookProvider::Discord => send_discord(&client, config, message).await,
        WebhookProvider::Telegram => send_telegram(&client, config, message).await,
        WebhookProvider::Feishu => send_feishu(&client, config, message).await,
        WebhookProvider::Dingtalk => send_dingtalk(&client, config, message).await,
        WebhookProvider::Wecom => send_wecom(&client, config, message).await,
        WebhookProvider::Ntfy => send_ntfy(&client, config, message).await,
        WebhookProvider::Gotify => send_gotify(&client, config, message).await,
    }
}

pub fn validate_config(provider: &WebhookProvider, config: &WebhookConfig) -> Result<(), String> {
    match provider {
        WebhookProvider::Generic
        | WebhookProvider::Slack
        | WebhookProvider::Discord
        | WebhookProvider::Feishu
        | WebhookProvider::Dingtalk
        | WebhookProvider::Wecom => {
            require_url(config.url.as_deref())?;
            validate_headers(&config.headers)
        }
        WebhookProvider::Telegram => {
            require_non_empty(config.bot_token.as_deref(), "Bot token")?;
            require_non_empty(config.chat_id.as_deref(), "Chat ID")?;
            Ok(())
        }
        WebhookProvider::Ntfy => {
            if config
                .url
                .as_deref()
                .is_some_and(|url| !url.trim().is_empty())
            {
                require_url(config.url.as_deref())
            } else {
                require_url(config.server_url.as_deref())?;
                require_non_empty(config.topic.as_deref(), "Topic")?;
                Ok(())
            }
        }
        WebhookProvider::Gotify => {
            require_url(config.server_url.as_deref())?;
            require_non_empty(config.token.as_deref(), "App token")?;
            Ok(())
        }
    }
}

fn validate_headers(headers: &HashMap<String, String>) -> Result<(), String> {
    for (name, value) in headers {
        reqwest::header::HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| format!("Invalid header name: {name}"))?;
        reqwest::header::HeaderValue::from_str(value)
            .map_err(|_| format!("Invalid value for header {name}"))?;
    }
    Ok(())
}

fn require_url(value: Option<&str>) -> Result<(), String> {
    let url = require_non_empty(value, "Webhook URL")?;
    if url.starts_with("http://") || url.starts_with("https://") {
        Ok(())
    } else {
        Err("Webhook URL must start with http:// or https://".to_string())
    }
}

fn require_non_empty<'a>(value: Option<&'a str>, field: &str) -> Result<&'a str, String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{field} is required"))
}

async fn send_generic(
    client: &reqwest::Client,
    config: &WebhookConfig,
    message: &NotificationMessage,
) -> Result<(), String> {
    let url = require_non_empty(config.url.as_deref(), "Webhook URL")?;
    let mut request = match config
        .method
        .as_deref()
        .unwrap_or("POST")
        .to_ascii_uppercase()
        .as_str()
    {
        "PUT" => client.put(url),
        "PATCH" => client.patch(url),
        _ => client.post(url),
    };
    for (key, value) in &config.headers {
        request = request.header(key.as_str(), value.as_str());
    }
    send_json(
        request,
        &json!({
            "title": message.title,
            "body": message.body,
            "subject": message.subject,
            "fromName": message.from_name,
            "fromAddress": message.from_address,
            "accountId": message.account_id,
            "messageId": message.message_id,
            "snippet": message.snippet,
            "date": message.date,
        }),
    )
    .await
}

async fn send_slack(
    client: &reqwest::Client,
    config: &WebhookConfig,
    message: &NotificationMessage,
) -> Result<(), String> {
    send_json(client.post(require_non_empty(config.url.as_deref(), "Webhook URL")?), &json!({
        "text": message.body,
        "blocks": [{
            "type": "section",
            "text": { "type": "mrkdwn", "text": format!("*{}*\n{}", message.title, message.body) }
        }]
    }))
    .await
}

async fn send_discord(
    client: &reqwest::Client,
    config: &WebhookConfig,
    message: &NotificationMessage,
) -> Result<(), String> {
    send_json(
        client.post(require_non_empty(config.url.as_deref(), "Webhook URL")?),
        &json!({
            "content": message.title,
            "embeds": [{
                "title": message.subject,
                "description": message.snippet,
                "fields": [
                    { "name": "From", "value": display_sender(message), "inline": false },
                    { "name": "Message ID", "value": message.message_id, "inline": false }
                ]
            }]
        }),
    )
    .await
}

async fn send_telegram(
    client: &reqwest::Client,
    config: &WebhookConfig,
    message: &NotificationMessage,
) -> Result<(), String> {
    let token = require_non_empty(config.bot_token.as_deref(), "Bot token")?;
    let chat_id = require_non_empty(config.chat_id.as_deref(), "Chat ID")?;
    let url = format!("https://api.telegram.org/bot{token}/sendMessage");
    let mut payload = json!({
        "chat_id": chat_id,
        "text": message.body,
        "disable_web_page_preview": true,
    });
    if let Some(parse_mode) = config
        .parse_mode
        .as_deref()
        .filter(|v| !v.trim().is_empty())
    {
        payload["parse_mode"] = Value::String(parse_mode.to_string());
    }
    send_json(client.post(url), &payload).await
}

async fn send_feishu(
    client: &reqwest::Client,
    config: &WebhookConfig,
    message: &NotificationMessage,
) -> Result<(), String> {
    let mut payload = json!({
        "msg_type": "text",
        "content": { "text": message.body }
    });
    if let Some(secret) = config.secret.as_deref().filter(|v| !v.trim().is_empty()) {
        let timestamp = pebble_core::now_timestamp().to_string();
        payload["timestamp"] = Value::String(timestamp.clone());
        payload["sign"] = Value::String(feishu_sign(&timestamp, secret)?);
    }
    send_json(
        client.post(require_non_empty(config.url.as_deref(), "Webhook URL")?),
        &payload,
    )
    .await
}

async fn send_dingtalk(
    client: &reqwest::Client,
    config: &WebhookConfig,
    message: &NotificationMessage,
) -> Result<(), String> {
    let mut url = require_non_empty(config.url.as_deref(), "Webhook URL")?.to_string();
    if let Some(secret) = config.secret.as_deref().filter(|v| !v.trim().is_empty()) {
        let timestamp = (pebble_core::now_timestamp() * 1000).to_string();
        let sign = urlencoding::encode(&dingtalk_sign(&timestamp, secret)?).into_owned();
        let separator = if url.contains('?') { '&' } else { '?' };
        url.push_str(&format!("{separator}timestamp={timestamp}&sign={sign}"));
    }
    send_json(
        client.post(url),
        &json!({
            "msgtype": "markdown",
            "markdown": { "title": message.title, "text": message.body }
        }),
    )
    .await
}

async fn send_wecom(
    client: &reqwest::Client,
    config: &WebhookConfig,
    message: &NotificationMessage,
) -> Result<(), String> {
    send_json(
        client.post(require_non_empty(config.url.as_deref(), "Webhook URL")?),
        &json!({
            "msgtype": "markdown",
            "markdown": { "content": message.body }
        }),
    )
    .await
}

async fn send_ntfy(
    client: &reqwest::Client,
    config: &WebhookConfig,
    message: &NotificationMessage,
) -> Result<(), String> {
    let url = if let Some(url) = config.url.as_deref().filter(|v| !v.trim().is_empty()) {
        url.trim().to_string()
    } else {
        let base =
            require_non_empty(config.server_url.as_deref(), "Server URL")?.trim_end_matches('/');
        let topic = require_non_empty(config.topic.as_deref(), "Topic")?.trim_start_matches('/');
        format!("{base}/{topic}")
    };
    let mut request = client
        .post(url)
        .header("Title", message.title.as_str())
        .header(
            "Priority",
            config
                .priority
                .unwrap_or(DEFAULT_GOTIFY_PRIORITY)
                .to_string(),
        );
    if let Some(token) = config.token.as_deref().filter(|v| !v.trim().is_empty()) {
        request = request.bearer_auth(token);
    }
    send_request(request.body(message.body.clone())).await
}

async fn send_gotify(
    client: &reqwest::Client,
    config: &WebhookConfig,
    message: &NotificationMessage,
) -> Result<(), String> {
    let base = require_non_empty(config.server_url.as_deref(), "Server URL")?.trim_end_matches('/');
    let token = require_non_empty(config.token.as_deref(), "App token")?;
    let url = format!("{base}/message?token={}", urlencoding::encode(token));
    send_json(
        client.post(url),
        &json!({
            "title": message.title,
            "message": message.body,
            "priority": config.priority.unwrap_or(DEFAULT_GOTIFY_PRIORITY),
        }),
    )
    .await
}

async fn send_json(request: reqwest::RequestBuilder, payload: &Value) -> Result<(), String> {
    send_request(request.json(payload)).await
}

async fn send_request(request: reqwest::RequestBuilder) -> Result<(), String> {
    let response = request
        .send()
        .await
        .map_err(|e| format!("Webhook request failed: {}", e.without_url()))?;
    ensure_success(response.status()).await
}

async fn ensure_success(status: StatusCode) -> Result<(), String> {
    if status.is_success() {
        Ok(())
    } else {
        Err(format!("Webhook returned HTTP {status}"))
    }
}

pub fn encrypt_config(
    crypto: &CryptoService,
    endpoint_id: &str,
    config: &WebhookConfig,
) -> Result<Vec<u8>, String> {
    let bytes = serde_json::to_vec(config)
        .map_err(|e| format!("Failed to serialize webhook config: {e}"))?;
    crypto
        .encrypt_for(CONFIG_PURPOSE, endpoint_id, &bytes)
        .map_err(|e| format!("Failed to encrypt webhook config: {e}"))
}

pub fn decrypt_config(
    crypto: &CryptoService,
    endpoint_id: &str,
    encrypted: &[u8],
) -> Result<WebhookConfig, String> {
    let bytes = crypto
        .decrypt_for(CONFIG_PURPOSE, endpoint_id, encrypted)
        .map_err(|e| format!("Failed to decrypt webhook config: {e}"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("Invalid webhook config: {e}"))
}

fn delivery_permits() -> &'static Semaphore {
    static PERMITS: OnceLock<Semaphore> = OnceLock::new();
    PERMITS.get_or_init(|| Semaphore::new(MAX_CONCURRENT_DELIVERIES))
}

/// Endpoints for one new message: enabled new-mail subscribers plus enabled
/// endpoints named by matching rules, each delivered at most once.
pub fn endpoints_for_new_mail(
    store: &Store,
    rule_endpoint_ids: &[String],
) -> Vec<StoredWebhookEndpoint> {
    let mut endpoints = store
        .list_enabled_webhook_endpoints_for_new_mail()
        .unwrap_or_else(|e| {
            warn!("Failed to list webhook endpoints for new mail: {e}");
            Vec::new()
        });
    for id in rule_endpoint_ids {
        if endpoints.iter().any(|stored| &stored.endpoint.id == id) {
            continue;
        }
        match store.get_webhook_endpoint(id) {
            Ok(Some(stored)) if stored.endpoint.is_enabled => endpoints.push(stored),
            Ok(Some(_)) => {}
            Ok(None) => warn!("Rule references missing webhook endpoint {id}"),
            Err(e) => warn!("Failed to load webhook endpoint {id}: {e}"),
        }
    }
    endpoints
}

pub async fn deliver_new_mail(
    store: Arc<Store>,
    crypto: Arc<CryptoService>,
    message: Message,
    rule_endpoint_ids: Vec<String>,
) {
    let endpoints = {
        let store = store.clone();
        match tokio::task::spawn_blocking(move || {
            endpoints_for_new_mail(&store, &rule_endpoint_ids)
        })
        .await
        {
            Ok(endpoints) => endpoints,
            Err(e) => {
                warn!("Webhook endpoint lookup failed: {e}");
                return;
            }
        }
    };
    if endpoints.is_empty() {
        return;
    }
    let notification = NotificationMessage::from_message(&message);
    for stored in endpoints {
        let config = match decrypt_config(&crypto, &stored.endpoint.id, &stored.encrypted_config) {
            Ok(config) => config,
            Err(e) => {
                warn!("Webhook endpoint {}: {e}", stored.endpoint.id);
                continue;
            }
        };
        let Ok(_permit) = delivery_permits().acquire().await else {
            return;
        };
        if let Err(e) = send_notification(&stored.endpoint.provider, &config, &notification).await {
            warn!(
                "Webhook endpoint {} failed for message {}: {e}",
                stored.endpoint.id, message.id
            );
        }
    }
}

fn display_sender(message: &NotificationMessage) -> String {
    if message.from_name.trim().is_empty() {
        message.from_address.clone()
    } else {
        format!("{} <{}>", message.from_name, message.from_address)
    }
}

fn hmac_sha256_base64(key: &[u8], data: &[u8]) -> Result<String, String> {
    let mut mac = HmacSha256::new_from_slice(key)
        .map_err(|e| format!("Failed to create webhook signature: {e}"))?;
    mac.update(data);
    let digest = mac.finalize().into_bytes();
    Ok(general_purpose::STANDARD.encode(digest))
}

pub fn dingtalk_sign(timestamp: &str, secret: &str) -> Result<String, String> {
    let string_to_sign = format!("{timestamp}\n{secret}");
    hmac_sha256_base64(secret.as_bytes(), string_to_sign.as_bytes())
}

pub fn feishu_sign(timestamp: &str, secret: &str) -> Result<String, String> {
    let key = format!("{timestamp}\n{secret}");
    hmac_sha256_base64(key.as_bytes(), b"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_provider_requirements() {
        assert!(validate_config(
            &WebhookProvider::Slack,
            &WebhookConfig {
                url: Some("https://hooks.slack.test/x".into()),
                ..Default::default()
            }
        )
        .is_ok());
        assert!(validate_config(
            &WebhookProvider::Slack,
            &WebhookConfig {
                url: Some("ftp://example.com".into()),
                ..Default::default()
            }
        )
        .is_err());
        assert!(validate_config(
            &WebhookProvider::Telegram,
            &WebhookConfig {
                bot_token: Some("token".into()),
                chat_id: Some("123".into()),
                ..Default::default()
            }
        )
        .is_ok());
    }

    #[test]
    fn signatures_match_provider_reference_vectors() {
        assert_eq!(
            dingtalk_sign("1700000000000", "secret").unwrap(),
            "OuzzJR5+xZ4/EYwqtNt6sMYZQMTa/HEGvc9miJe7XzY="
        );
        assert_eq!(
            feishu_sign("1700000000", "secret").unwrap(),
            "fiWS2+gh28DOydAv7hzONH/mDn9+b1Y4Y5ivXWXy8vA="
        );
    }

    #[test]
    fn rejects_invalid_generic_headers() {
        let mut headers = HashMap::new();
        headers.insert("Bad Header".to_string(), "x".to_string());
        assert!(validate_config(
            &WebhookProvider::Generic,
            &WebhookConfig {
                url: Some("https://example.com/hook".into()),
                headers,
                ..Default::default()
            }
        )
        .is_err());
    }

    #[test]
    fn config_encryption_is_bound_to_endpoint_id() {
        let crypto = CryptoService::from_key([9; 32]);
        let config = WebhookConfig {
            url: Some("https://example.com/hook".into()),
            ..Default::default()
        };
        let encrypted = encrypt_config(&crypto, "endpoint-a", &config).unwrap();
        assert!(!String::from_utf8_lossy(&encrypted).contains("example.com"));
        let decrypted = decrypt_config(&crypto, "endpoint-a", &encrypted).unwrap();
        assert_eq!(decrypted.url, config.url);
        assert!(decrypt_config(&crypto, "endpoint-b", &encrypted).is_err());
    }

    fn stored(id: &str, enabled: bool, new_mail: bool) -> StoredWebhookEndpoint {
        StoredWebhookEndpoint {
            endpoint: pebble_core::WebhookEndpoint {
                id: id.into(),
                name: id.into(),
                provider: WebhookProvider::Generic,
                is_enabled: enabled,
                notify_on_new_mail: new_mail,
                created_at: 1,
                updated_at: 1,
            },
            encrypted_config: vec![1],
        }
    }

    #[test]
    fn new_mail_endpoints_merge_rule_targets_once_and_skip_disabled() {
        let store = Store::open_in_memory().unwrap();
        store
            .insert_webhook_endpoint(&stored("global", true, true))
            .unwrap();
        store
            .insert_webhook_endpoint(&stored("rule", true, false))
            .unwrap();
        store
            .insert_webhook_endpoint(&stored("off", false, true))
            .unwrap();
        let ids: Vec<String> = endpoints_for_new_mail(
            &store,
            &[
                "global".into(),
                "rule".into(),
                "off".into(),
                "missing".into(),
            ],
        )
        .into_iter()
        .map(|stored| stored.endpoint.id)
        .collect();
        assert_eq!(ids, vec!["global".to_string(), "rule".to_string()]);
    }

    async fn capture_server(
        status: axum::http::StatusCode,
    ) -> (
        String,
        tokio::sync::mpsc::UnboundedReceiver<(axum::http::HeaderMap, String)>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let app = axum::Router::new().route(
            "/{*path}",
            axum::routing::any(move |headers: axum::http::HeaderMap, body: String| {
                let tx = tx.clone();
                async move {
                    let _ = tx.send((headers, body));
                    status
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{addr}"), rx)
    }

    fn sample_message() -> Message {
        Message {
            id: "m1".into(),
            account_id: "a1".into(),
            remote_id: "1".into(),
            message_id_header: None,
            in_reply_to: None,
            references_header: None,
            thread_id: None,
            subject: "Quarterly report".into(),
            snippet: "Numbers inside".into(),
            from_address: "boss@example.com".into(),
            from_name: "Boss".into(),
            to_list: Vec::new(),
            cc_list: Vec::new(),
            bcc_list: Vec::new(),
            body_text: String::new(),
            body_html_raw: String::new(),
            has_attachments: false,
            is_read: false,
            is_starred: false,
            is_draft: false,
            date: 1_700_000_000,
            remote_version: None,
            is_deleted: false,
            deleted_at: None,
            created_at: 1,
            updated_at: 1,
        }
    }

    #[tokio::test]
    async fn generic_webhook_posts_message_json_with_headers() {
        let (base, mut rx) = capture_server(axum::http::StatusCode::OK).await;
        let mut headers = HashMap::new();
        headers.insert("X-Pebble".to_string(), "yes".to_string());
        let config = WebhookConfig {
            url: Some(format!("{base}/hook")),
            headers,
            ..Default::default()
        };
        let notification = NotificationMessage::from_message(&sample_message());
        send_notification(&WebhookProvider::Generic, &config, &notification)
            .await
            .unwrap();
        let (received_headers, body) = rx.recv().await.unwrap();
        assert_eq!(received_headers["x-pebble"], "yes");
        let body: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(body["subject"], "Quarterly report");
        assert_eq!(body["fromAddress"], "boss@example.com");
        assert_eq!(body["messageId"], "m1");
    }

    #[tokio::test]
    async fn failed_delivery_reports_status_without_leaking_tokens() {
        let (base, _rx) = capture_server(axum::http::StatusCode::FORBIDDEN).await;
        let config = WebhookConfig {
            server_url: Some(base),
            token: Some("super-secret-token".into()),
            ..Default::default()
        };
        let error = send_notification(
            &WebhookProvider::Gotify,
            &config,
            &NotificationMessage::test(),
        )
        .await
        .unwrap_err();
        assert!(error.contains("403"), "{error}");
        assert!(!error.contains("super-secret-token"), "{error}");

        let unreachable = WebhookConfig {
            server_url: Some("http://127.0.0.1:9".into()),
            token: Some("super-secret-token".into()),
            ..Default::default()
        };
        let error = send_notification(
            &WebhookProvider::Gotify,
            &unreachable,
            &NotificationMessage::test(),
        )
        .await
        .unwrap_err();
        assert!(!error.contains("super-secret-token"), "{error}");
    }

    #[tokio::test]
    async fn deliver_new_mail_sends_to_subscribed_and_rule_endpoints() {
        let (base, mut rx) = capture_server(axum::http::StatusCode::OK).await;
        let store = Arc::new(Store::open_in_memory().unwrap());
        let crypto = Arc::new(CryptoService::from_key([3; 32]));
        for (id, new_mail) in [("global", true), ("rule", false), ("unused", false)] {
            let config = WebhookConfig {
                url: Some(format!("{base}/{id}")),
                headers: HashMap::from([("X-Endpoint".to_string(), id.to_string())]),
                ..Default::default()
            };
            let mut endpoint = stored(id, true, new_mail);
            endpoint.encrypted_config = encrypt_config(&crypto, id, &config).unwrap();
            store.insert_webhook_endpoint(&endpoint).unwrap();
        }
        deliver_new_mail(store, crypto, sample_message(), vec!["rule".into()]).await;
        let mut hit = Vec::new();
        while let Ok((headers, _)) = rx.try_recv() {
            hit.push(headers["x-endpoint"].to_str().unwrap().to_string());
        }
        hit.sort();
        assert_eq!(hit, vec!["global".to_string(), "rule".to_string()]);
    }
}
