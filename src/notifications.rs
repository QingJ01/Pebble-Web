use std::collections::HashMap;
use std::time::Duration;

use base64::{engine::general_purpose, Engine as _};
use hmac::{Hmac, Mac};
use pebble_core::{Message, WebhookProvider};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Sha256;

const DEFAULT_TIMEOUT_SECS: u64 = 10;
const DEFAULT_GOTIFY_PRIORITY: i32 = 5;

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
            snippet: "If you can see this message, your webhook endpoint is configured correctly.".to_string(),
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
        | WebhookProvider::Wecom => require_url(config.url.as_deref()),
        WebhookProvider::Telegram => {
            require_non_empty(config.bot_token.as_deref(), "Bot token")?;
            require_non_empty(config.chat_id.as_deref(), "Chat ID")?;
            Ok(())
        }
        WebhookProvider::Ntfy => {
            if config.url.as_deref().is_some_and(|url| !url.trim().is_empty()) {
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
    let mut request = match config.method.as_deref().unwrap_or("POST").to_ascii_uppercase().as_str()
    {
        "PUT" => client.put(url),
        "PATCH" => client.patch(url),
        _ => client.post(url),
    };
    for (key, value) in &config.headers {
        request = request.header(key.as_str(), value.as_str());
    }
    send_json(request, &json!({
        "title": message.title,
        "body": message.body,
        "subject": message.subject,
        "fromName": message.from_name,
        "fromAddress": message.from_address,
        "accountId": message.account_id,
        "messageId": message.message_id,
        "snippet": message.snippet,
        "date": message.date,
    }))
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
    send_json(client.post(require_non_empty(config.url.as_deref(), "Webhook URL")?), &json!({
        "content": message.title,
        "embeds": [{
            "title": message.subject,
            "description": message.snippet,
            "fields": [
                { "name": "From", "value": display_sender(message), "inline": false },
                { "name": "Message ID", "value": message.message_id, "inline": false }
            ]
        }]
    }))
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
    if let Some(parse_mode) = config.parse_mode.as_deref().filter(|v| !v.trim().is_empty()) {
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
    send_json(client.post(require_non_empty(config.url.as_deref(), "Webhook URL")?), &payload).await
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
    send_json(client.post(url), &json!({
        "msgtype": "markdown",
        "markdown": { "title": message.title, "text": message.body }
    }))
    .await
}

async fn send_wecom(
    client: &reqwest::Client,
    config: &WebhookConfig,
    message: &NotificationMessage,
) -> Result<(), String> {
    send_json(client.post(require_non_empty(config.url.as_deref(), "Webhook URL")?), &json!({
        "msgtype": "markdown",
        "markdown": { "content": message.body }
    }))
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
        let base = require_non_empty(config.server_url.as_deref(), "Server URL")?.trim_end_matches('/');
        let topic = require_non_empty(config.topic.as_deref(), "Topic")?.trim_start_matches('/');
        format!("{base}/{topic}")
    };
    let mut request = client
        .post(url)
        .header("Title", message.title.as_str())
        .header("Priority", config.priority.unwrap_or(DEFAULT_GOTIFY_PRIORITY).to_string());
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
    send_json(client.post(url), &json!({
        "title": message.title,
        "message": message.body,
        "priority": config.priority.unwrap_or(DEFAULT_GOTIFY_PRIORITY),
    }))
    .await
}

async fn send_json(request: reqwest::RequestBuilder, payload: &Value) -> Result<(), String> {
    send_request(request.json(payload)).await
}

async fn send_request(request: reqwest::RequestBuilder) -> Result<(), String> {
    let response = request
        .send()
        .await
        .map_err(|e| format!("Webhook request failed: {e}"))?;
    ensure_success(response.status()).await
}

async fn ensure_success(status: StatusCode) -> Result<(), String> {
    if status.is_success() {
        Ok(())
    } else {
        Err(format!("Webhook returned HTTP {status}"))
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
            &WebhookConfig { url: Some("https://hooks.slack.test/x".into()), ..Default::default() }
        )
        .is_ok());
        assert!(validate_config(
            &WebhookProvider::Slack,
            &WebhookConfig { url: Some("ftp://example.com".into()), ..Default::default() }
        )
        .is_err());
        assert!(validate_config(
            &WebhookProvider::Telegram,
            &WebhookConfig { bot_token: Some("token".into()), chat_id: Some("123".into()), ..Default::default() }
        )
        .is_ok());
    }

    #[test]
    fn signature_helpers_are_stable() {
        let dingtalk = dingtalk_sign("123456789", "secret").unwrap();
        let feishu = feishu_sign("123456789", "secret").unwrap();
        assert_eq!(dingtalk, dingtalk_sign("123456789", "secret").unwrap());
        assert_eq!(feishu, feishu_sign("123456789", "secret").unwrap());
        assert!(!dingtalk.is_empty());
        assert!(!feishu.is_empty());
        assert_ne!(dingtalk, feishu);
    }
}
