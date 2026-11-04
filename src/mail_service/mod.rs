//! Server-side mail mutation, compose, draft, and pending-operation services
//! ported from the desktop command layer. Commands and REST routes share
//! these services so local state, provider writeback, and retries stay
//! consistent.

pub mod attachments;
pub mod batch;
pub mod compose;
pub mod drafts;
pub mod flags;
pub mod gmail_labels;
pub mod lifecycle;
pub mod pending_ops;
pub mod provider_dispatch;
pub mod rules;

use crate::credentials::{decrypt_credentials, AccountCredentials};
use crate::state::AppState;
use pebble_core::{FolderRole, HttpProxyConfig, Message, OAuthTokens, PebbleError};
use pebble_crypto::CryptoService;
use pebble_mail::{GmailProvider, ImapConfig, ImapProvider, OutlookProvider, Pop3Config};
use pebble_search::TantivySearch;
use pebble_store::Store;
use serde_json::json;

pub(crate) const MAIL_PENDING_OPS_CHANGED: &str = "mail:pending-ops-changed";

pub(crate) fn emit_pending_ops_changed(state: &AppState) {
    emit_pending_ops_changed_via(&state.ws_broadcast);
}

pub(crate) fn emit_pending_ops_changed_via(ws: &tokio::sync::broadcast::Sender<String>) {
    let _ = ws.send(json!({ "type": MAIL_PENDING_OPS_CHANGED, "payload": null }).to_string());
}

/// Moves an IMAP message and records the UID assigned by the destination
/// mailbox, so later mutations from that mailbox target the right message.
pub(crate) async fn imap_move_tracked(
    state: &AppState,
    imap: &ImapProvider,
    local_message_id: &str,
    source_mailbox: &str,
    uid: u32,
    dest_mailbox: &str,
) -> Result<(), PebbleError> {
    let message = state.store.get_message(local_message_id)?;
    let header = message
        .as_ref()
        .and_then(|message| message.message_id_header.clone())
        .filter(|header| !header.trim().is_empty());
    let Some(header) = header else {
        tracing::warn!("IMAP message {local_message_id} has no Message-ID; UID not refreshed");
        return imap
            .move_message_with_dedup(source_mailbox, uid, dest_mailbox, None)
            .await;
    };

    // A UID is only valid in the mailbox that assigned it; resolve the
    // current one so a stale UID cannot silently move nothing.
    let source_uid = match imap.find_uid_by_message_id(source_mailbox, &header).await? {
        Some(current) => current,
        None => {
            return Err(PebbleError::Network(format!(
                "Message {local_message_id} was not found in remote folder {source_mailbox}"
            )))
        }
    };
    imap.move_message_with_dedup(source_mailbox, source_uid, dest_mailbox, Some(&header))
        .await?;
    if source_uid != uid {
        tracing::info!("IMAP UID for {local_message_id} resolved from {uid} to {source_uid}");
    }

    let new_uid = match imap.find_uid_by_message_id(dest_mailbox, &header).await {
        Ok(Some(new_uid)) => new_uid,
        Ok(None) => {
            tracing::warn!("Moved IMAP message {local_message_id} was not found in {dest_mailbox}");
            return Ok(());
        }
        Err(error) => {
            tracing::warn!("Failed to look up moved IMAP UID for {local_message_id}: {error}");
            return Ok(());
        }
    };
    let account_id = message
        .map(|message| message.account_id)
        .unwrap_or_default();
    let dest_folder = state
        .store
        .list_folders(&account_id)?
        .into_iter()
        .find(|folder| folder.remote_id == dest_mailbox);
    let stored = match dest_folder {
        Some(folder) => state.store.move_message_to_folder_with_remote_id(
            local_message_id,
            &folder.id,
            &new_uid.to_string(),
        ),
        None => state
            .store
            .update_remote_id(local_message_id, &new_uid.to_string()),
    };
    if let Err(error) = stored {
        tracing::warn!("Failed to store moved IMAP UID for {local_message_id}: {error}");
    }
    Ok(())
}

pub(crate) struct ResolvedOAuthAuth {
    pub tokens: OAuthTokens,
    pub proxy: Option<HttpProxyConfig>,
}

fn account_sync_state(store: &Store, account_id: &str) -> Result<serde_json::Value, PebbleError> {
    let raw = store
        .get_account_sync_state(account_id)?
        .ok_or_else(|| PebbleError::Internal(format!("No config for account {account_id}")))?;
    serde_json::from_str(&raw)
        .map_err(|e| PebbleError::Internal(format!("Invalid account sync state: {e}")))
}

fn account_credentials(
    store: &Store,
    crypto: &CryptoService,
    account_id: &str,
) -> Result<(AccountCredentials, serde_json::Value, String), PebbleError> {
    let sync_state = account_sync_state(store, account_id)?;
    let encrypted = sync_state["credentials"]
        .as_str()
        .ok_or_else(|| PebbleError::Internal("No credentials in sync state".into()))?;
    let creds = decrypt_credentials(crypto, encrypted).map_err(PebbleError::Internal)?;
    let email = store
        .get_account(account_id)?
        .map(|account| account.email)
        .ok_or_else(|| PebbleError::Internal(format!("Account not found: {account_id}")))?;
    Ok((creds, sync_state, email))
}

pub(crate) fn load_imap_config(
    store: &Store,
    crypto: &CryptoService,
    account_id: &str,
) -> Result<ImapConfig, PebbleError> {
    let (creds, sync_state, email) = account_credentials(store, crypto, account_id)?;
    let AccountCredentials::Imap { imap, .. } = creds else {
        return Err(PebbleError::UnsupportedProvider(
            "Account does not use IMAP".into(),
        ));
    };
    let global = crate::sync::load_global_proxy(store, crypto).map_err(PebbleError::Internal)?;
    crate::sync::build_imap_config(&imap, &sync_state, &email, global.as_ref())
        .map_err(PebbleError::Internal)
}

#[allow(dead_code)]
pub(crate) fn load_pop3_config(
    store: &Store,
    crypto: &CryptoService,
    account_id: &str,
) -> Result<Pop3Config, PebbleError> {
    let (creds, sync_state, email) = account_credentials(store, crypto, account_id)?;
    let AccountCredentials::Pop3 { pop3, .. } = creds else {
        return Err(PebbleError::UnsupportedProvider(
            "Account does not use POP3".into(),
        ));
    };
    let global = crate::sync::load_global_proxy(store, crypto).map_err(PebbleError::Internal)?;
    crate::sync::build_pop3_config(&pop3, &sync_state, &email, global.as_ref())
        .map_err(PebbleError::Internal)
}

pub(crate) fn load_smtp_config(
    store: &Store,
    crypto: &CryptoService,
    account_id: &str,
) -> Result<pebble_mail::SmtpConfig, PebbleError> {
    let (creds, sync_state, email) = account_credentials(store, crypto, account_id)?;
    let smtp = match creds {
        AccountCredentials::Imap { smtp, .. } | AccountCredentials::Pop3 { smtp, .. } => smtp,
    };
    let global = crate::sync::load_global_proxy(store, crypto).map_err(PebbleError::Internal)?;
    crate::sync::build_smtp_config(&smtp, &sync_state, &email, global.as_ref())
        .map_err(PebbleError::Internal)
}

pub(crate) fn parse_mailbox_identity(
    provider: &str,
    profile: &serde_json::Value,
) -> Result<pebble_core::OAuthMailboxIdentity, PebbleError> {
    let non_empty = |value: &serde_json::Value| {
        value
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    if provider == "gmail" {
        let verified = profile["verified_email"]
            .as_bool()
            .or_else(|| profile["email_verified"].as_bool())
            .unwrap_or(false);
        if !verified {
            return Err(PebbleError::Auth(
                "Provider mailbox address is not verified".into(),
            ));
        }
    }
    let email = if provider == "gmail" {
        non_empty(&profile["email"])
    } else {
        non_empty(&profile["mail"]).or_else(|| non_empty(&profile["userPrincipalName"]))
    }
    .ok_or_else(|| {
        PebbleError::Auth("Provider could not confirm the actual mailbox address".into())
    })?;
    let subject = non_empty(&profile["id"])
        .or_else(|| non_empty(&profile["sub"]))
        .ok_or_else(|| {
            PebbleError::Auth("Provider did not return a stable mailbox identity".into())
        })?;
    let display_name = non_empty(&profile["name"]).or_else(|| non_empty(&profile["displayName"]));
    let identity = pebble_core::OAuthMailboxIdentity {
        subject: format!("{provider}:{subject}"),
        email,
        display_name,
    };
    pebble_mail::sender::sender_mailbox(&pebble_core::EmailAddress {
        name: identity.display_name.clone(),
        address: identity.email.clone(),
    })?;
    Ok(identity)
}

pub(crate) async fn fetch_mailbox_identity(
    provider: &str,
    auth: &ResolvedOAuthAuth,
) -> Result<pebble_core::OAuthMailboxIdentity, PebbleError> {
    let url = match provider {
        "gmail" => "https://www.googleapis.com/oauth2/v2/userinfo",
        "outlook" => {
            "https://graph.microsoft.com/v1.0/me?$select=id,mail,userPrincipalName,displayName"
        }
        _ => return Err(PebbleError::UnsupportedProvider(provider.to_string())),
    };
    let network = pebble_oauth::OAuthNetworkConfig {
        proxy: auth.proxy.clone(),
    };
    let client = pebble_oauth::build_http_client(&network)
        .map_err(|e| PebbleError::Network(format!("Mailbox verification failed: {e}")))?;
    let profile: serde_json::Value = client
        .get(url)
        .bearer_auth(&auth.tokens.access_token)
        .send()
        .await
        .map_err(|e| PebbleError::Network(format!("Mailbox verification failed: {e}")))?
        .error_for_status()
        .map_err(|e| PebbleError::Auth(format!("Mailbox verification failed: {e}")))?
        .json()
        .await
        .map_err(|e| PebbleError::Network(format!("Mailbox profile was invalid: {e}")))?;
    parse_mailbox_identity(provider, &profile)
}

pub(crate) async fn ensure_account_oauth_auth(
    state: &AppState,
    account_id: &str,
    provider: &str,
) -> Result<ResolvedOAuthAuth, PebbleError> {
    let tokens = crate::routes::oauth::refresh_account_tokens(
        state.store.clone(),
        state.crypto.clone(),
        state.oauth_refresh_locks.clone(),
        account_id.to_string(),
        provider.to_string(),
    )
    .await
    .map_err(PebbleError::Auth)?;
    let sync_state = account_sync_state(&state.store, account_id)?;
    let global = crate::sync::load_global_proxy(&state.store, &state.crypto)
        .map_err(PebbleError::Internal)?;
    let proxy =
        crate::sync::oauth_proxy(&sync_state, global.as_ref()).map_err(PebbleError::Internal)?;
    Ok(ResolvedOAuthAuth { tokens, proxy })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemoteMutationOutcome {
    Applied,
    Queued,
    QueuedLocalCommit,
    LocalOnly,
    #[allow(dead_code)]
    Failed,
}

pub(crate) fn remote_mutation_allows_local_commit(outcome: RemoteMutationOutcome) -> bool {
    matches!(
        outcome,
        RemoteMutationOutcome::Applied
            | RemoteMutationOutcome::QueuedLocalCommit
            | RemoteMutationOutcome::LocalOnly
    )
}

pub(crate) fn remote_delete_is_already_absent(error: &PebbleError) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    message.contains("status 404")
        || message.contains("404 not found")
        || message.contains("message not found")
        || message.contains("no such message")
}

pub(crate) fn classify_remote_delete_result(
    result: std::result::Result<(), PebbleError>,
    permanent: bool,
) -> std::result::Result<(), PebbleError> {
    match result {
        Err(error) if permanent && remote_delete_is_already_absent(&error) => Ok(()),
        result => result,
    }
}

pub(crate) fn queue_pending_remote_op(
    state: &AppState,
    message: &Message,
    op_type: &str,
    payload: serde_json::Value,
    error: &str,
) -> std::result::Result<RemoteMutationOutcome, PebbleError> {
    let payload = json!({
        "provider_account_id": message.account_id,
        "remote_id": message.remote_id,
        "op": op_type,
        "payload": payload,
    });
    let op_id = state.store.insert_pending_mail_op(
        &message.account_id,
        &message.id,
        op_type,
        &payload.to_string(),
    )?;
    state.store.mark_pending_mail_op_failed(&op_id, error)?;
    Ok(RemoteMutationOutcome::Queued)
}

pub(crate) fn queue_pending_remote_op_for_local_commit(
    state: &AppState,
    message: &Message,
    op_type: &str,
    payload: serde_json::Value,
    error: &str,
) -> std::result::Result<RemoteMutationOutcome, PebbleError> {
    let outcome = queue_pending_remote_op(state, message, op_type, payload, error)?;
    debug_assert_eq!(outcome, RemoteMutationOutcome::Queued);
    Ok(RemoteMutationOutcome::QueuedLocalCommit)
}

pub(crate) fn queued_remote_error(op_type: &str, error: &str) -> PebbleError {
    PebbleError::Network(format!(
        "Remote {op_type} failed and was queued for retry: {error}"
    ))
}

pub(crate) async fn connect_gmail(
    state: &AppState,
    account_id: &str,
) -> std::result::Result<GmailProvider, PebbleError> {
    let auth = ensure_account_oauth_auth(state, account_id, "gmail").await?;
    GmailProvider::new_with_proxy(auth.tokens.access_token, auth.proxy)
}

pub(crate) async fn connect_outlook(
    state: &AppState,
    account_id: &str,
) -> std::result::Result<OutlookProvider, PebbleError> {
    let auth = ensure_account_oauth_auth(state, account_id, "outlook").await?;
    OutlookProvider::new_with_proxy(auth.tokens.access_token, account_id.to_string(), auth.proxy)
}

pub(crate) fn refresh_search_document(
    state: &AppState,
    message_id: &str,
) -> std::result::Result<(), PebbleError> {
    refresh_search_document_with_store(&state.store, &state.search, message_id)
}

pub(crate) fn refresh_search_document_with_store(
    store: &Store,
    search: &TantivySearch,
    message_id: &str,
) -> std::result::Result<(), PebbleError> {
    let ids = vec![message_id.to_string()];
    store.add_search_pending(&ids, "index")?;

    match store.get_message(message_id)? {
        Some(message) if !message.is_deleted => {
            let folder_ids = store.get_message_folder_ids(message_id)?;
            if folder_ids.is_empty() {
                search.remove_message(message_id)?;
            } else {
                search.index_message(&message, &folder_ids)?;
            }
        }
        Some(_) | None => {
            search.remove_message(message_id)?;
        }
    }

    search.commit()?;
    store.clear_search_pending(&ids)?;
    Ok(())
}

pub(crate) fn remove_search_documents(
    state: &AppState,
    message_ids: &[String],
) -> std::result::Result<(), PebbleError> {
    if message_ids.is_empty() {
        return Ok(());
    }
    state.store.add_search_pending(message_ids, "remove")?;
    for message_id in message_ids {
        state.search.remove_message(message_id)?;
    }
    state.search.commit()?;
    state.store.clear_search_pending(message_ids)?;
    Ok(())
}

/// Refresh multiple search documents with a single index commit at the end.
/// Use this instead of calling `refresh_search_document` in a loop: one commit
/// for N messages is dramatically cheaper than N commits (segment flush +
/// reader reload per doc).
pub(crate) fn refresh_search_documents(
    state: &AppState,
    message_ids: &[String],
) -> std::result::Result<(), PebbleError> {
    if message_ids.is_empty() {
        return Ok(());
    }
    state.store.add_search_pending(message_ids, "index")?;
    for message_id in message_ids {
        match state.store.get_message(message_id)? {
            Some(message) if !message.is_deleted => {
                let folder_ids = state.store.get_message_folder_ids(message_id)?;
                if folder_ids.is_empty() {
                    state.search.remove_message(message_id)?;
                } else {
                    state.search.index_message(&message, &folder_ids)?;
                }
            }
            Some(_) | None => {
                state.search.remove_message(message_id)?;
            }
        }
    }
    state.search.commit()?;
    state.store.clear_search_pending(message_ids)?;
    Ok(())
}

/// Resolve an IMAP connection from the account's auth data.
pub(crate) async fn connect_imap(
    state: &AppState,
    account_id: &str,
) -> std::result::Result<ImapProvider, PebbleError> {
    let imap_config = load_imap_config(&state.store, &state.crypto, account_id)?;
    let provider = ImapProvider::new(imap_config);
    provider.connect().await?;
    Ok(provider)
}

/// Find the folder with a given role for an account.
pub(crate) fn find_folder_by_role(
    state: &AppState,
    account_id: &str,
    role: FolderRole,
) -> std::result::Result<pebble_core::Folder, PebbleError> {
    let folders = state.store.list_folders(account_id)?;
    folders
        .into_iter()
        .find(|f| f.role == Some(role.clone()))
        .ok_or_else(|| PebbleError::Internal(format!("No {:?} folder found", role)))
}

/// Find the folder containing a given message (via the message_folders junction table).
pub(crate) fn find_message_folder(
    state: &AppState,
    message_id: &str,
    account_id: &str,
) -> std::result::Result<pebble_core::Folder, PebbleError> {
    let folder_ids = state.store.get_message_folder_ids(message_id)?;
    if folder_ids.is_empty() {
        return Err(PebbleError::Internal(
            "Message not found in any folder".to_string(),
        ));
    }
    let folders = state.store.list_folders(account_id)?;
    // Return the first matching folder (prefer inbox-like folders)
    for fid in &folder_ids {
        if let Some(folder) = folders.iter().find(|f| &f.id == fid) {
            return Ok(folder.clone());
        }
    }
    Err(PebbleError::Internal(
        "Message folder not found".to_string(),
    ))
}
