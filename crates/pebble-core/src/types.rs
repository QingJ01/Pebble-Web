use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub color: Option<String>,
    pub provider: ProviderType,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderType {
    Imap,
    Gmail,
    Outlook,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HttpProxyConfig {
    pub host: String,
    pub port: u16,
}

impl HttpProxyConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.host.trim().is_empty() {
            return Err("Proxy host is required".to_string());
        }
        if self.port == 0 {
            return Err("Proxy port must be between 1 and 65535".to_string());
        }
        Ok(())
    }

    pub fn socks5h_uri(&self) -> Result<String, String> {
        self.validate()?;
        Ok(format!("socks5h://{}:{}", self.host.trim(), self.port))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Folder {
    pub id: String,
    pub account_id: String,
    pub remote_id: String,
    pub name: String,
    pub folder_type: FolderType,
    pub role: Option<FolderRole>,
    pub parent_id: Option<String>,
    pub color: Option<String>,
    pub is_system: bool,
    pub sort_order: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum FolderType {
    Folder,
    Label,
    Category,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum FolderRole {
    Inbox,
    Sent,
    Drafts,
    Trash,
    Archive,
    Spam,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub account_id: String,
    pub remote_id: String,
    pub message_id_header: Option<String>,
    pub in_reply_to: Option<String>,
    pub references_header: Option<String>,
    pub thread_id: Option<String>,
    pub subject: String,
    pub snippet: String,
    pub from_address: String,
    pub from_name: String,
    pub to_list: Vec<EmailAddress>,
    pub cc_list: Vec<EmailAddress>,
    pub bcc_list: Vec<EmailAddress>,
    pub body_text: String,
    pub body_html_raw: String,
    pub has_attachments: bool,
    pub is_read: bool,
    pub is_starred: bool,
    pub is_draft: bool,
    pub date: i64,
    pub remote_version: Option<String>,
    pub is_deleted: bool,
    pub deleted_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Lightweight message data for list views (excludes body_text and body_html_raw).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSummary {
    pub id: String,
    pub account_id: String,
    pub remote_id: String,
    pub message_id_header: Option<String>,
    pub in_reply_to: Option<String>,
    pub references_header: Option<String>,
    pub thread_id: Option<String>,
    pub subject: String,
    pub snippet: String,
    pub from_address: String,
    pub from_name: String,
    pub to_list: Vec<EmailAddress>,
    pub cc_list: Vec<EmailAddress>,
    pub bcc_list: Vec<EmailAddress>,
    pub has_attachments: bool,
    pub is_read: bool,
    pub is_starred: bool,
    pub is_draft: bool,
    pub date: i64,
    pub remote_version: Option<String>,
    pub is_deleted: bool,
    pub deleted_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailAddress {
    pub name: Option<String>,
    pub address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub message_id: String,
    pub filename: String,
    pub mime_type: String,
    pub size: i64,
    pub local_path: Option<String>,
    pub content_id: Option<String>,
    pub is_inline: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserLabel {
    pub id: String,
    pub name: String,
    pub color: String,
    pub is_system: bool,
    pub rule_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum KanbanColumn {
    Todo,
    Waiting,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KanbanCard {
    pub message_id: String,
    pub column: KanbanColumn,
    pub position: i32,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnoozedMessage {
    pub message_id: String,
    pub snoozed_at: i64,
    pub unsnoozed_at: i64,
    pub return_to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustedSender {
    pub account_id: String,
    pub email: String,
    pub trust_type: TrustType,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TrustType {
    Images,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub name: String,
    pub priority: i32,
    pub conditions: String,
    pub actions: String,
    pub is_enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WebhookProvider {
    Generic,
    Slack,
    Discord,
    Telegram,
    Feishu,
    Dingtalk,
    Wecom,
    Ntfy,
    Gotify,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookEndpoint {
    pub id: String,
    pub name: String,
    pub provider: WebhookProvider,
    pub is_enabled: bool,
    pub notify_on_new_mail: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct StoredWebhookEndpoint {
    pub endpoint: WebhookEndpoint,
    pub encrypted_config: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PrivacyMode {
    Strict,
    TrustSender(String),
    LoadOnce,
    Off,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderedHtml {
    pub html: String,
    pub trackers_blocked: Vec<TrackerInfo>,
    pub images_blocked: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackerInfo {
    pub domain: String,
    pub tracker_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    pub has_labels: bool,
    pub has_folders: bool,
    pub has_categories: bool,
    pub has_push: bool,
    pub has_threads: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslateConfig {
    pub id: String,
    pub provider_type: String,
    pub config: String,
    pub is_enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadSummary {
    pub thread_id: String,
    pub subject: String,
    pub snippet: String,
    pub last_date: i64,
    pub message_count: u32,
    pub unread_count: u32,
    pub is_starred: bool,
    pub participants: Vec<String>,
    pub has_attachments: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnownContact {
    pub name: Option<String>,
    pub address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftMessage {
    pub id: Option<String>,
    pub to: Vec<EmailAddress>,
    pub cc: Vec<EmailAddress>,
    pub bcc: Vec<EmailAddress>,
    pub subject: String,
    pub body_text: String,
    pub body_html: Option<String>,
    pub in_reply_to: Option<String>,
    #[serde(default)]
    pub attachment_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<i64>,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthConfig {
    pub client_id: String,
    pub auth_url: String,
    pub token_url: String,
    pub scopes: Vec<String>,
    pub redirect_port: u16,
}

pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

pub fn now_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
