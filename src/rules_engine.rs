use crate::notifications::{send_notification, NotificationMessage, WebhookConfig};
use crate::state::AppStateRef;
use pebble_core::{KanbanCard, KanbanColumn, Message, Rule};
use serde::Deserialize;
use serde_json::Value;
use tracing::warn;

#[derive(Debug, Clone, Deserialize)]
struct RuleCondition {
    field: String,
    op: String,
    value: String,
}

#[derive(Debug, Clone)]
struct RuleAction {
    action_type: String,
    value: Option<String>,
}

pub async fn handle_new_message(state: AppStateRef, message: Message, notify_new_mail: bool) {
    let notification = NotificationMessage::from_message(&message);
    if notify_new_mail {
        send_new_mail_notifications(&state, &notification).await;
    }
    apply_rules(&state, &message, &notification).await;
}

async fn send_new_mail_notifications(state: &AppStateRef, notification: &NotificationMessage) {
    let endpoints = match state.store.list_enabled_webhook_endpoints_for_new_mail() {
        Ok(endpoints) => endpoints,
        Err(e) => {
            warn!("Failed to list webhook endpoints for new mail notification: {e}");
            return;
        }
    };

    for endpoint in endpoints {
        let config = match decrypt_webhook_config(state, &endpoint.encrypted_config) {
            Ok(config) => config,
            Err(e) => {
                warn!("Failed to decrypt webhook config for {}: {e}", endpoint.endpoint.id);
                continue;
            }
        };
        if let Err(e) = send_notification(&endpoint.endpoint.provider, &config, notification).await {
            warn!(
                "Webhook endpoint {} failed while sending new mail notification: {e}",
                endpoint.endpoint.id
            );
        }
    }
}

async fn apply_rules(state: &AppStateRef, message: &Message, notification: &NotificationMessage) {
    let rules = match state.store.list_rules() {
        Ok(rules) => rules,
        Err(e) => {
            warn!("Failed to load rules for new message: {e}");
            return;
        }
    };

    for rule in rules.into_iter().filter(|rule| rule.is_enabled) {
        if !rule_matches(&rule, message) {
            continue;
        }
        for action in parse_actions(&rule.actions) {
            if let Err(e) = execute_action(state, message, notification, &action).await {
                warn!(
                    "Rule {} action {} failed for message {}: {e}",
                    rule.id, action.action_type, message.id
                );
            }
        }
    }
}

fn rule_matches(rule: &Rule, message: &Message) -> bool {
    let conditions = parse_conditions(&rule.conditions);
    !conditions.is_empty() && conditions.iter().all(|condition| condition_matches(condition, message))
}

fn parse_conditions(json: &str) -> Vec<RuleCondition> {
    let Ok(value) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    let raw_conditions = if let Some(array) = value.as_array() {
        array.clone()
    } else if let Some(array) = value.get("conditions").and_then(Value::as_array) {
        array.clone()
    } else if let Some(object) = value.as_object() {
        return object
            .iter()
            .filter_map(|(field, value)| {
                if field == "operator" || field == "conditions" {
                    return None;
                }
                Some(RuleCondition {
                    field: field.clone(),
                    op: if field == "has_attachment" { "equals" } else { "contains" }.to_string(),
                    value: value
                        .as_str()
                        .map(ToString::to_string)
                        .unwrap_or_else(|| value.to_string()),
                })
            })
            .collect();
    } else {
        Vec::new()
    };

    raw_conditions
        .into_iter()
        .filter_map(|value| serde_json::from_value(value).ok())
        .collect()
}

fn parse_actions(json: &str) -> Vec<RuleAction> {
    let Ok(Value::Array(values)) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };

    values.into_iter().filter_map(parse_action_value).collect()
}

fn parse_action_value(value: Value) -> Option<RuleAction> {
    if let Some(action_type) = value.as_str() {
        return Some(RuleAction {
            action_type: normalize_action_type(action_type).to_string(),
            value: None,
        });
    }

    let object = value.as_object()?;
    if let Some(action_type) = object.get("type").and_then(Value::as_str) {
        return Some(RuleAction {
            action_type: normalize_action_type(action_type).to_string(),
            value: object
                .get("value")
                .and_then(Value::as_str)
                .map(ToString::to_string),
        });
    }

    let (action_type, value) = object.iter().next()?;
    Some(RuleAction {
        action_type: normalize_action_type(action_type).to_string(),
        value: value.as_str().map(ToString::to_string).or_else(|| Some(value.to_string())),
    })
}

fn normalize_action_type(action_type: &str) -> &str {
    match action_type {
        "archive" => "Archive",
        "mark_read" | "markread" => "MarkRead",
        "add_label" | "label" => "AddLabel",
        "move_to_folder" | "move" => "MoveToFolder",
        "set_kanban_column" => "SetKanbanColumn",
        "send_webhook" | "webhook" => "SendWebhook",
        "star" => "Star",
        _ => action_type,
    }
}

fn condition_matches(condition: &RuleCondition, message: &Message) -> bool {
    let target = match condition.field.as_str() {
        "from" => format!("{} {}", message.from_name, message.from_address),
        "to" => message
            .to_list
            .iter()
            .map(|addr| format!("{} {}", addr.name.clone().unwrap_or_default(), addr.address))
            .collect::<Vec<_>>()
            .join(" "),
        "subject" => message.subject.clone(),
        "body" => format!("{} {}", message.body_text, message.snippet),
        "has_attachment" => message.has_attachments.to_string(),
        "domain" => message
            .from_address
            .split('@')
            .nth(1)
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    };

    compare_text(&target, &condition.op, &condition.value)
}

fn compare_text(target: &str, op: &str, value: &str) -> bool {
    let target = target.to_ascii_lowercase();
    let value = value.to_ascii_lowercase();
    match op {
        "contains" => target.contains(&value),
        "not_contains" => !target.contains(&value),
        "equals" => target == value,
        "starts_with" => target.starts_with(&value),
        "ends_with" => target.ends_with(&value),
        _ => false,
    }
}

async fn execute_action(
    state: &AppStateRef,
    message: &Message,
    notification: &NotificationMessage,
    action: &RuleAction,
) -> Result<(), String> {
    match action.action_type.as_str() {
        "AddLabel" => state
            .store
            .add_label_for_account(
                &message.account_id,
                &message.id,
                required_value(action, "Label name")?,
            )
            .map_err(|e| e.to_string()),
        "MoveToFolder" => {
            let value = required_value(action, "Folder")?;
            let folders = state.store.list_folders(&message.account_id).map_err(|e| e.to_string())?;
            let folder = folders
                .iter()
                .find(|folder| folder.id == value || folder.name.eq_ignore_ascii_case(value));
            let Some(folder) = folder else {
                return Err(format!("Folder not found: {value}"));
            };
            state
                .store
                .move_message_to_folder(&message.id, &folder.id)
                .map_err(|e| e.to_string())
        }
        "MarkRead" => state
            .store
            .update_message_flags(&message.id, Some(true), None)
            .map_err(|e| e.to_string()),
        "Star" => state
            .store
            .update_message_flags(&message.id, None, Some(true))
            .map_err(|e| e.to_string()),
        "Archive" => state.store.archive_message(&message.id).map_err(|e| e.to_string()),
        "SetKanbanColumn" => {
            let now = pebble_core::now_timestamp();
            state
                .store
                .upsert_kanban_card(&KanbanCard {
                    message_id: message.id.clone(),
                    column: parse_kanban_column(action.value.as_deref().unwrap_or("todo")),
                    position: 0,
                    created_at: now,
                    updated_at: now,
                })
                .map_err(|e| e.to_string())
        }
        "SendWebhook" => {
            let id = required_value(action, "Webhook endpoint")?;
            let endpoint = state
                .store
                .get_webhook_endpoint(id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("Webhook endpoint not found: {id}"))?;
            if !endpoint.endpoint.is_enabled {
                return Ok(());
            }
            let config = decrypt_webhook_config(state, &endpoint.encrypted_config)?;
            send_notification(&endpoint.endpoint.provider, &config, notification).await
        }
        _ => Ok(()),
    }
}

fn required_value<'a>(action: &'a RuleAction, field: &str) -> Result<&'a str, String> {
    action
        .value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{field} is required"))
}

fn parse_kanban_column(value: &str) -> KanbanColumn {
    match value {
        "waiting" => KanbanColumn::Waiting,
        "done" => KanbanColumn::Done,
        _ => KanbanColumn::Todo,
    }
}

fn decrypt_webhook_config(state: &AppStateRef, encrypted: &[u8]) -> Result<WebhookConfig, String> {
    let bytes = state
        .crypto
        .decrypt(encrypted)
        .map_err(|e| format!("Failed to decrypt webhook config: {e}"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("Invalid webhook config: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pebble_core::EmailAddress;

    fn message() -> Message {
        Message {
            id: "message-1".into(),
            account_id: "account-1".into(),
            remote_id: "remote-1".into(),
            message_id_header: None,
            in_reply_to: None,
            references_header: None,
            thread_id: None,
            subject: "Urgent deploy".into(),
            snippet: "Please review".into(),
            from_address: "alerts@example.com".into(),
            from_name: "Alerts".into(),
            to_list: vec![EmailAddress { name: None, address: "me@example.com".into() }],
            cc_list: vec![],
            bcc_list: vec![],
            body_text: "Deploy failed".into(),
            body_html_raw: String::new(),
            has_attachments: false,
            is_read: false,
            is_starred: false,
            is_draft: false,
            date: 1,
            remote_version: None,
            is_deleted: false,
            deleted_at: None,
            created_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn parses_rule_conditions_from_object() {
        let conditions = parse_conditions(
            r#"{"operator":"and","conditions":[{"field":"subject","op":"contains","value":"urgent"}]}"#,
        );
        assert_eq!(conditions.len(), 1);
        assert!(condition_matches(&conditions[0], &message()));
    }

    #[test]
    fn parses_modern_and_legacy_actions() {
        let actions = parse_actions(
            r#"[{"type":"SendWebhook","value":"webhook-1"},{"AddLabel":"alerts"},"Archive"]"#,
        );
        assert_eq!(actions.len(), 3);
        assert_eq!(actions[0].action_type, "SendWebhook");
        assert_eq!(actions[0].value.as_deref(), Some("webhook-1"));
        assert_eq!(actions[1].action_type, "AddLabel");
        assert_eq!(actions[1].value.as_deref(), Some("alerts"));
        assert_eq!(actions[2].action_type, "Archive");
    }
}
