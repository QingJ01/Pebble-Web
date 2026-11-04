//! Applies user rules to newly synced mail on the server, independent of any
//! open browser tab. Remote-capable actions are queued as pending mail ops.

use super::pending_ops::queue_pending_mail_op;
use pebble_store::Store;
use serde_json::json;
use tracing::{info, warn};

pub(crate) fn apply_rule_action(
    store: &Store,
    account_id: &str,
    message_id: &str,
    action: &pebble_rules::types::RuleAction,
) -> pebble_core::Result<()> {
    use pebble_rules::types::RuleAction;
    match action {
        RuleAction::MarkRead => {
            if queue_remote_rule_action(store, account_id, message_id, action)? {
                info!("Rule: queued remote mark-read for message {}", message_id);
                return Ok(());
            }
            store.update_message_flags(message_id, Some(true), None)?;
            info!("Rule: marked message {} as read", message_id);
        }
        RuleAction::Archive => {
            if queue_remote_rule_action(store, account_id, message_id, action)? {
                info!("Rule: queued remote archive for message {}", message_id);
                return Ok(());
            }
            if let Some(archive_folder) =
                store.find_folder_by_role(account_id, pebble_core::FolderRole::Archive)?
            {
                store.move_message_to_folder(message_id, &archive_folder.id)?;
                info!(
                    "Rule: archived message {} to folder {}",
                    message_id, archive_folder.name
                );
            } else {
                store.soft_delete_message(message_id)?;
                info!(
                    "Rule: archived (soft-deleted) message {} (no archive folder)",
                    message_id
                );
            }
        }
        RuleAction::AddLabel(label) => {
            store.add_label(message_id, label)?;
            info!("Rule: added label '{}' to message {}", label, message_id);
        }
        RuleAction::MoveToFolder(folder_name) => {
            if queue_remote_rule_action(store, account_id, message_id, action)? {
                info!(
                    "Rule: queued remote move for message {} to folder '{}'",
                    message_id, folder_name
                );
                return Ok(());
            }
            if let Some(target_folder) = store.find_folder_by_name(account_id, folder_name)? {
                store.move_message_to_folder(message_id, &target_folder.id)?;
                info!(
                    "Rule: moved message {} to folder '{}'",
                    message_id, target_folder.name
                );
            } else {
                warn!(
                    "Rule: target folder '{}' not found for account {}",
                    folder_name, account_id
                );
            }
        }
        RuleAction::SetKanbanColumn(column) => {
            let now = pebble_core::now_timestamp();
            let card = pebble_core::KanbanCard {
                message_id: message_id.to_string(),
                column: column.clone(),
                position: 0,
                created_at: now,
                updated_at: now,
            };
            store.upsert_kanban_card(&card)?;
            info!(
                "Rule: added message {} to kanban column {:?}",
                message_id, column
            );
        }
        RuleAction::SendWebhook(_) => {}
    }
    Ok(())
}

fn queue_remote_rule_action(
    store: &Store,
    account_id: &str,
    message_id: &str,
    action: &pebble_rules::types::RuleAction,
) -> pebble_core::Result<bool> {
    use super::gmail_labels::gmail_move_label_delta;
    use pebble_core::{FolderRole, ProviderType};
    use pebble_rules::types::RuleAction;

    let Some(account) = store.get_account(account_id)? else {
        return Ok(false);
    };
    let Some(message) = store.get_message(message_id)? else {
        return Ok(false);
    };
    if account.provider == ProviderType::Pop3 {
        return Ok(false);
    }
    let source_folder = store
        .get_message_folder_ids(message_id)?
        .into_iter()
        .next()
        .and_then(|folder_id| {
            store
                .list_folders(account_id)
                .ok()?
                .into_iter()
                .find(|folder| folder.id == folder_id)
        });

    match action {
        RuleAction::MarkRead => {
            if account.provider == ProviderType::Imap
                && source_folder
                    .as_ref()
                    .is_some_and(|folder| folder.remote_id.starts_with("__local_"))
            {
                return Ok(false);
            }

            let mut payload = json!({
                "is_read": true,
                "is_starred": null,
            });
            if account.provider == ProviderType::Gmail {
                payload["add_labels"] = json!([]);
                payload["remove_labels"] = json!(["UNREAD"]);
            }
            if let Some(folder) = source_folder.as_ref() {
                payload["folder_remote_id"] = json!(folder.remote_id);
            }
            queue_pending_mail_op(store, &message, "update_flags", payload)?;
            Ok(true)
        }
        RuleAction::Archive => {
            let archive_folder = store.find_folder_by_role(account_id, FolderRole::Archive)?;
            if let Some(archive) = archive_folder.as_ref() {
                if archive.remote_id.starts_with("__local_") {
                    return Ok(false);
                }
            } else if account.provider != ProviderType::Gmail {
                return Ok(false);
            }

            let mut payload = json!({
                "source_folder_id": source_folder.as_ref().map(|folder| folder.id.as_str()),
                "source_folder_remote_id": source_folder.as_ref().map(|folder| folder.remote_id.as_str()),
                "target_folder_id": archive_folder.as_ref().map(|folder| folder.id.as_str()),
                "target_folder_remote_id": archive_folder.as_ref().map(|folder| folder.remote_id.as_str()),
            });
            if account.provider == ProviderType::Gmail {
                payload["add_labels"] = json!([]);
                payload["remove_labels"] = json!(["INBOX"]);
            }
            queue_pending_mail_op(store, &message, "archive", payload)?;
            Ok(true)
        }
        RuleAction::MoveToFolder(folder_name) => {
            let Some(target_folder) = store.find_folder_by_name(account_id, folder_name)? else {
                return Ok(false);
            };
            if target_folder.remote_id.starts_with("__local_") {
                return Ok(false);
            }

            let mut payload = json!({
                "source_folder_id": source_folder.as_ref().map(|folder| folder.id.as_str()),
                "source_folder_remote_id": source_folder.as_ref().map(|folder| folder.remote_id.as_str()),
                "target_folder_id": target_folder.id.as_str(),
                "target_folder_remote_id": target_folder.remote_id.as_str(),
            });
            if account.provider == ProviderType::Gmail {
                let delta = gmail_move_label_delta(
                    source_folder
                        .as_ref()
                        .map(|folder| folder.remote_id.as_str()),
                    &target_folder.remote_id,
                    target_folder.role,
                );
                payload["add_labels"] = json!(delta.add_labels);
                payload["remove_labels"] = json!(delta.remove_labels);
            }
            queue_pending_mail_op(store, &message, "move_to_folder", payload)?;
            Ok(true)
        }
        RuleAction::AddLabel(_) | RuleAction::SetKanbanColumn(_) | RuleAction::SendWebhook(_) => {
            Ok(false)
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct RuleOutcome {
    /// True when any action changed stored message state.
    pub modified: bool,
    /// Webhook endpoints requested by matching rules, deduplicated in order.
    pub webhook_endpoint_ids: Vec<String>,
}

/// Evaluates rules for one stored message.
pub(crate) fn apply_rules_to_message(
    store: &Store,
    rules: &[pebble_core::Rule],
    message: &pebble_core::Message,
) -> RuleOutcome {
    let mut outcome = RuleOutcome::default();
    if rules.is_empty() {
        return outcome;
    }
    let engine = pebble_rules::RuleEngine::new(rules);
    for action in engine.evaluate(message) {
        if let pebble_rules::types::RuleAction::SendWebhook(endpoint_id) = &action {
            if !outcome.webhook_endpoint_ids.contains(endpoint_id) {
                outcome.webhook_endpoint_ids.push(endpoint_id.clone());
            }
            continue;
        }
        match apply_rule_action(store, &message.account_id, &message.id, &action) {
            Ok(()) => outcome.modified = true,
            Err(error) => warn!("Rule action failed for message {}: {error}", message.id),
        }
    }
    outcome
}
