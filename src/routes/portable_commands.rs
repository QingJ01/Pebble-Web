use crate::error::ApiError;
use crate::state::AppStateRef;
use pebble_core::{
    new_id, now_timestamp, KanbanCard, KanbanColumn, Rule, SnoozedMessage, TrustType, TrustedSender,
};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::{json, Value};

fn parse<T: DeserializeOwned>(args: Value) -> Result<T, ApiError> {
    serde_json::from_value(args)
        .map_err(|error| ApiError::BadRequest(format!("Invalid command arguments: {error}")))
}

async fn store_call<T, F>(state: &AppStateRef, operation: F) -> Result<Value, ApiError>
where
    T: serde::Serialize + Send + 'static,
    F: FnOnce(&pebble_store::Store) -> pebble_core::Result<T> + Send + 'static,
{
    let store = state.store.clone();
    let value = tokio::task::spawn_blocking(move || operation(&store))
        .await
        .map_err(|error| ApiError::Internal(format!("Store task failed: {error}")))?
        .map_err(|error| ApiError::Internal(error.to_string()))?;
    serde_json::to_value(value)
        .map_err(|error| ApiError::Internal(format!("Failed to encode command result: {error}")))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContactListArgs {
    query: Option<String>,
    favorite_only: Option<bool>,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContactArgs {
    contact_id: String,
    suppress_addresses: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountQueryArgs {
    account_id: String,
    query: String,
    limit: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessageIdsArgs {
    message_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContactInputArgs {
    input: pebble_core::ContactInput,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddressArgs {
    address: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct KanbanArgs {
    message_id: String,
    column: Option<KanbanColumn>,
    position: Option<i32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnoozeArgs {
    message_id: String,
    until: i64,
    return_to: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuleArgs {
    rule: Rule,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuleCreateArgs {
    name: String,
    priority: i32,
    conditions: String,
    actions: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuleIdArgs {
    rule_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrustArgs {
    account_id: String,
    email: String,
    trust_type: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoveTrustArgs {
    account_id: String,
    email: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessageLabelArgs {
    message_id: String,
    label_name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PendingArgs {
    account_id: Option<String>,
    limit: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListMessagesArgs {
    folder_id: Option<String>,
    folder_ids: Option<Vec<String>>,
    limit: Option<u32>,
    offset: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessageIdArgs {
    message_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadArgs {
    thread_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StarredArgs {
    account_id: String,
    limit: Option<u32>,
    offset: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchQuery {
    text: Option<String>,
    from: Option<String>,
    to: Option<String>,
    subject: Option<String>,
    date_from: Option<i64>,
    date_to: Option<i64>,
    has_attachment: Option<bool>,
    folder_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdvancedSearchArgs {
    query: SearchQuery,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrivacyArgs {
    message_id: String,
    privacy_mode: Option<pebble_core::PrivacyMode>,
}

pub async fn invoke(
    state: &AppStateRef,
    command: &str,
    args: Value,
) -> Result<Option<Value>, ApiError> {
    let result = match command {
        "list_contacts" => {
            let args: ContactListArgs = parse(args)?;
            store_call(state, move |store| {
                store.list_contacts(
                    args.query.as_deref(),
                    args.favorite_only.unwrap_or(false),
                    args.limit.unwrap_or(50).clamp(1, 500),
                    args.offset.unwrap_or(0).max(0),
                )
            })
            .await?
        }
        "get_contact_by_email" => {
            let args: AddressArgs = parse(args)?;
            store_call(state, move |store| {
                store.get_contact_by_email(&args.address)
            })
            .await?
        }
        "save_contact" => {
            let args: ContactInputArgs = parse(args)?;
            store_call(state, move |store| store.save_contact(&args.input)).await?
        }
        "delete_contact" => {
            let args: ContactArgs = parse(args)?;
            store_call(state, move |store| {
                store.delete_contact(&args.contact_id, args.suppress_addresses.unwrap_or(false))
            })
            .await?;
            Value::Null
        }
        "set_contact_favorite" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct FavoriteArgs {
                contact_id: String,
                is_favorite: bool,
            }
            let args: FavoriteArgs = parse(args)?;
            store_call(state, move |store| {
                store.set_contact_favorite(&args.contact_id, args.is_favorite)
            })
            .await?;
            Value::Null
        }
        "search_contact_suggestions" => {
            let args: AccountQueryArgs = parse(args)?;
            store_call(state, move |store| {
                store.search_contact_suggestions(
                    &args.account_id,
                    &args.query,
                    args.limit.unwrap_or(20),
                )
            })
            .await?
        }
        "search_contacts" => {
            let args: AccountQueryArgs = parse(args)?;
            store_call(state, move |store| {
                store.list_known_contacts(&args.account_id, &args.query, args.limit.unwrap_or(20))
            })
            .await?
        }
        "suppress_contact_suggestion" => {
            let args: AddressArgs = parse(args)?;
            store_call(state, move |store| {
                store.suppress_contact_suggestion(&args.address)
            })
            .await?;
            Value::Null
        }
        "import_contacts_vcard" => {
            #[derive(Deserialize)]
            struct DataArgs {
                data: String,
            }
            let args: DataArgs = parse(args)?;
            store_call(state, move |store| store.import_contacts_vcard(&args.data)).await?
        }
        "export_contacts_vcard" => {
            store_call(state, pebble_store::Store::export_contacts_vcard).await?
        }
        "list_kanban_cards" => {
            #[derive(Deserialize)]
            struct ColumnArgs {
                column: Option<KanbanColumn>,
            }
            let args: ColumnArgs = parse(args)?;
            store_call(state, move |store| {
                store.list_kanban_cards(args.column.as_ref())
            })
            .await?
        }
        "move_to_kanban" => {
            let args: KanbanArgs = parse(args)?;
            let message_id = args.message_id.clone();
            let column = args
                .column
                .ok_or_else(|| ApiError::BadRequest("column is required".into()))?;
            let position = args.position.unwrap_or(0);
            store_call(state, move |store| {
                if store.get_message(&message_id)?.is_none() {
                    return Err(pebble_core::PebbleError::Validation(
                        "Message not found".into(),
                    ));
                }
                let card = KanbanCard {
                    message_id,
                    column,
                    position,
                    created_at: now_timestamp(),
                    updated_at: now_timestamp(),
                };
                store.upsert_kanban_card(&card)
            })
            .await?;
            Value::Null
        }
        "remove_from_kanban" => {
            let args: KanbanArgs = parse(args)?;
            store_call(state, move |store| {
                store.delete_kanban_card(&args.message_id)
            })
            .await?;
            Value::Null
        }
        "set_kanban_context_note" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                message_id: String,
                note: String,
            }
            let args: Args = parse(args)?;
            let result = crate::routes::kanban::set_context_note(
                axum::extract::State(state.clone()),
                axum::extract::Path(args.message_id),
                axum::Json(crate::routes::kanban::SetContextNoteRequest { note: args.note }),
            )
            .await?;
            serde_json::to_value(result.0).map_err(|error| ApiError::Internal(error.to_string()))?
        }
        "list_kanban_context_notes" => {
            let result =
                crate::routes::kanban::list_context_notes(axum::extract::State(state.clone()))
                    .await?;
            serde_json::to_value(result.0).map_err(|error| ApiError::Internal(error.to_string()))?
        }
        "merge_kanban_context_notes" => {
            #[derive(Deserialize)]
            struct Args {
                notes: std::collections::HashMap<String, String>,
            }
            let args: Args = parse(args)?;
            let result = crate::routes::kanban::merge_context_notes(
                axum::extract::State(state.clone()),
                axum::Json(crate::routes::kanban::MergeContextNotesRequest { notes: args.notes }),
            )
            .await?;
            serde_json::to_value(result.0).map_err(|error| ApiError::Internal(error.to_string()))?
        }
        "send_email" => {
            let request: crate::routes::compose::ComposeRequest = parse(args)?;
            crate::routes::compose::send_with_service(state, request).await?;
            Value::Null
        }
        "save_draft" => {
            let request: crate::routes::drafts::SaveDraftRequest = parse(args)?;
            json!(crate::routes::drafts::save_with_service(state, request).await?)
        }
        "delete_draft" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                account_id: String,
                draft_id: String,
            }
            let args: Args = parse(args)?;
            crate::mail_service::drafts::delete_draft(state, args.account_id, args.draft_id)
                .await?;
            Value::Null
        }
        "update_message_flags" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                message_id: String,
                is_read: Option<bool>,
                is_starred: Option<bool>,
            }
            let args: Args = parse(args)?;
            crate::mail_service::flags::update_message_flags(
                state,
                args.message_id,
                args.is_read,
                args.is_starred,
            )
            .await?;
            Value::Null
        }
        "archive_message" | "delete_message" | "restore_message" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                message_id: String,
            }
            let args: Args = parse(args)?;
            use crate::mail_service::lifecycle;
            match command {
                "archive_message" => {
                    json!(lifecycle::archive_message(state, args.message_id).await?)
                }
                "delete_message" => {
                    lifecycle::delete_message(state, args.message_id).await?;
                    Value::Null
                }
                _ => {
                    lifecycle::restore_message(state, args.message_id).await?;
                    Value::Null
                }
            }
        }
        "move_to_folder" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                message_id: String,
                target_folder_id: String,
            }
            let args: Args = parse(args)?;
            crate::mail_service::lifecycle::move_to_folder(
                state,
                args.message_id,
                args.target_folder_id,
            )
            .await?;
            Value::Null
        }
        "empty_trash" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                account_id: String,
            }
            let args: Args = parse(args)?;
            json!(crate::mail_service::lifecycle::empty_trash(state, args.account_id).await?)
        }
        "batch_archive" | "batch_delete" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                message_ids: Vec<String>,
            }
            let args: Args = parse(args)?;
            use crate::mail_service::batch;
            if command == "batch_archive" {
                json!(batch::batch_archive(state, args.message_ids).await?)
            } else {
                json!(batch::batch_delete(state, args.message_ids).await?)
            }
        }
        "batch_mark_read" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                message_ids: Vec<String>,
                is_read: bool,
            }
            let args: Args = parse(args)?;
            json!(
                crate::mail_service::batch::batch_mark_read(state, args.message_ids, args.is_read)
                    .await?
            )
        }
        "batch_star" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                message_ids: Vec<String>,
                starred: bool,
            }
            let args: Args = parse(args)?;
            json!(
                crate::mail_service::batch::batch_star(state, args.message_ids, args.starred)
                    .await?
            )
        }
        "snooze_message" => {
            let args: SnoozeArgs = parse(args)?;
            store_call(state, move |store| {
                store.snooze_message(&SnoozedMessage {
                    message_id: args.message_id,
                    snoozed_at: now_timestamp(),
                    unsnoozed_at: args.until,
                    return_to: args.return_to,
                })
            })
            .await?;
            Value::Null
        }
        "unsnooze_message" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct IdArgs {
                message_id: String,
            }
            let args: IdArgs = parse(args)?;
            let message_id = args.message_id;
            let store_message_id = message_id.clone();
            store_call(state, move |store| {
                store.unsnooze_message(&store_message_id)
            })
            .await?;
            let _ = state.ws_broadcast.send(
                json!({
                    "type": "mail:unsnoozed",
                    "payload": { "message_id": message_id }
                })
                .to_string(),
            );
            Value::Null
        }
        "list_snoozed" => store_call(state, pebble_store::Store::list_snoozed_messages).await?,
        "list_rules" => store_call(state, pebble_store::Store::list_rules).await?,
        "create_rule" => {
            let args: RuleCreateArgs = parse(args)?;
            let rule = Rule {
                id: new_id(),
                name: args.name,
                priority: args.priority,
                conditions: args.conditions,
                actions: args.actions,
                is_enabled: true,
                created_at: now_timestamp(),
                updated_at: now_timestamp(),
            };
            store_call(state, move |store| store.insert_rule(&rule).map(|_| rule)).await?
        }
        "update_rule" => {
            let args: RuleArgs = parse(args)?;
            store_call(state, move |store| store.update_rule(&args.rule)).await?;
            Value::Null
        }
        "delete_rule" => {
            let args: RuleIdArgs = parse(args)?;
            store_call(state, move |store| store.delete_rule(&args.rule_id)).await?;
            Value::Null
        }
        "list_trusted_senders" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Account {
                account_id: String,
            }
            let args: Account = parse(args)?;
            store_call(state, move |store| {
                store.list_trusted_senders(&args.account_id)
            })
            .await?
        }
        "is_trusted_sender" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                account_id: String,
                email: String,
            }
            let args: Args = parse(args)?;
            let store = state.store.clone();
            let trusted = tokio::task::spawn_blocking(move || {
                store.is_trusted_sender(&args.account_id, &args.email)
            })
            .await
            .map_err(|error| ApiError::Internal(error.to_string()))?
            .map_err(|error| ApiError::Internal(error.to_string()))?;
            json!(trusted.is_some())
        }
        "trust_sender" => {
            let args: TrustArgs = parse(args)?;
            let trust_type = match args.trust_type.as_str() {
                "images" => TrustType::Images,
                "all" => TrustType::All,
                _ => return Err(ApiError::BadRequest("Invalid trust type".into())),
            };
            let sender = TrustedSender {
                account_id: args.account_id,
                email: args.email,
                trust_type,
                created_at: now_timestamp(),
            };
            store_call(state, move |store| store.trust_sender(&sender)).await?;
            Value::Null
        }
        "remove_trusted_sender" => {
            let args: RemoveTrustArgs = parse(args)?;
            store_call(state, move |store| {
                store.remove_trusted_sender(&args.account_id, &args.email)
            })
            .await?;
            Value::Null
        }
        "get_message_labels" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct IdArgs {
                message_id: String,
            }
            let args: IdArgs = parse(args)?;
            store_call(state, move |store| {
                store.get_message_labels(&args.message_id)
            })
            .await?
        }
        "get_message_labels_batch" => {
            let args: MessageIdsArgs = parse(args)?;
            store_call(state, move |store| {
                store.get_message_labels_batch(&args.message_ids)
            })
            .await?
        }
        "add_message_label" => {
            let args: MessageLabelArgs = parse(args)?;
            store_call(state, move |store| {
                store.add_label(&args.message_id, &args.label_name)
            })
            .await?;
            Value::Null
        }
        "remove_message_label" => {
            let args: MessageLabelArgs = parse(args)?;
            store_call(state, move |store| {
                store.remove_label(&args.message_id, &args.label_name)
            })
            .await?;
            Value::Null
        }
        "list_labels" => store_call(state, pebble_store::Store::list_labels).await?,
        "get_pending_mail_ops_summary" => {
            let args: PendingArgs = parse(args)?;
            serde_json::to_value(
                crate::mail_service::pending_ops::get_pending_mail_ops_summary(
                    state,
                    args.account_id,
                )?,
            )
            .map_err(|error| ApiError::Internal(error.to_string()))?
        }
        "list_pending_mail_ops" => {
            let args: PendingArgs = parse(args)?;
            serde_json::to_value(crate::mail_service::pending_ops::list_pending_mail_ops(
                state,
                args.account_id,
                args.limit,
            )?)
            .map_err(|error| ApiError::Internal(error.to_string()))?
        }
        "dismiss_failed_pending_mail_ops" => {
            let args: PendingArgs = parse(args)?;
            json!(
                crate::mail_service::pending_ops::dismiss_failed_pending_mail_ops(
                    state,
                    args.account_id
                )?
            )
        }
        "search_messages" => {
            #[derive(Deserialize)]
            struct SearchArgs {
                query: String,
                limit: Option<usize>,
            }
            let args: SearchArgs = parse(args)?;
            let search = state.search.clone();
            let hits = tokio::task::spawn_blocking(move || {
                search.search(&args.query, args.limit.unwrap_or(50).clamp(1, 500))
            })
            .await
            .map_err(|error| ApiError::Internal(error.to_string()))?
            .map_err(|error| ApiError::BadRequest(error.to_string()))?;
            serde_json::to_value(hits).map_err(|error| ApiError::Internal(error.to_string()))?
        }
        "advanced_search" => {
            let args: AdvancedSearchArgs = parse(args)?;
            let search = state.search.clone();
            let query = args.query;
            let hits = tokio::task::spawn_blocking(move || {
                search.advanced_search(pebble_search::AdvancedSearchParams {
                    text: query.text.as_deref(),
                    from: query.from.as_deref(),
                    to: query.to.as_deref(),
                    subject: query.subject.as_deref(),
                    date_from: query.date_from,
                    date_to: query.date_to,
                    has_attachment: query.has_attachment,
                    folder_id: query.folder_id.as_deref(),
                    limit: args.limit.unwrap_or(50).clamp(1, 500),
                })
            })
            .await
            .map_err(|error| ApiError::Internal(error.to_string()))?
            .map_err(|error| ApiError::BadRequest(error.to_string()))?;
            serde_json::to_value(hits).map_err(|error| ApiError::Internal(error.to_string()))?
        }
        "list_messages" => {
            let args: ListMessagesArgs = parse(args)?;
            let folder_ids = args
                .folder_ids
                .unwrap_or_else(|| args.folder_id.into_iter().collect());
            store_call(state, move |store| {
                store.list_messages_by_folders(
                    &folder_ids,
                    args.limit.unwrap_or(50).clamp(1, 500),
                    args.offset.unwrap_or(0),
                )
            })
            .await?
        }
        "list_starred_messages" => {
            let args: StarredArgs = parse(args)?;
            store_call(state, move |store| {
                store.list_starred_messages(
                    &args.account_id,
                    args.limit.unwrap_or(50).clamp(1, 500),
                    args.offset.unwrap_or(0),
                )
            })
            .await?
        }
        "get_message" => {
            let args: MessageIdArgs = parse(args)?;
            store_call(state, move |store| store.get_message(&args.message_id)).await?
        }
        "get_messages_batch" => {
            let args: MessageIdsArgs = parse(args)?;
            if args.message_ids.len() > 500 {
                return Err(ApiError::BadRequest(
                    "At most 500 message IDs are allowed".into(),
                ));
            }
            store_call(state, move |store| {
                store.get_messages_batch(&args.message_ids)
            })
            .await?
        }
        "list_threads" => {
            let args: ListMessagesArgs = parse(args)?;
            let folder_ids = args
                .folder_ids
                .unwrap_or_else(|| args.folder_id.into_iter().collect());
            store_call(state, move |store| {
                store.list_threads_by_folders(
                    &folder_ids,
                    args.limit.unwrap_or(50).clamp(1, 500),
                    args.offset.unwrap_or(0),
                )
            })
            .await?
        }
        "list_thread_messages" => {
            let args: ThreadArgs = parse(args)?;
            store_call(state, move |store| {
                store.list_messages_by_thread(&args.thread_id)
            })
            .await?
        }
        "list_folders" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                account_id: String,
            }
            let args: Args = parse(args)?;
            store_call(state, move |store| store.list_folders(&args.account_id)).await?
        }
        "get_message_with_html" => {
            let args: PrivacyArgs = parse(args)?;
            let result = crate::routes::messages::get_message_with_html(
                axum::extract::State(state.clone()),
                axum::extract::Path(args.message_id),
                axum::Json(crate::routes::messages::PrivacyModeRequest {
                    privacy_mode: args.privacy_mode,
                }),
            )
            .await;
            match result {
                Ok(axum::Json(value)) => value,
                Err(ApiError::NotFound(_)) => Value::Null,
                Err(error) => return Err(error),
            }
        }
        "get_rendered_html" => {
            let args: PrivacyArgs = parse(args)?;
            let result = crate::routes::messages::render_html(
                axum::extract::State(state.clone()),
                axum::extract::Path(args.message_id),
                axum::Json(crate::routes::messages::PrivacyModeRequest {
                    privacy_mode: args.privacy_mode,
                }),
            )
            .await?;
            result.0
        }
        "list_attachments" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                message_id: String,
            }
            let args: Args = parse(args)?;
            store_call(state, move |store| {
                store.list_attachments_by_message(&args.message_id)
            })
            .await?
        }
        "get_attachment_path" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                attachment_id: String,
            }
            let args: Args = parse(args)?;
            let attachment_id = args.attachment_id.clone();
            let store = state.store.clone();
            let attachment =
                tokio::task::spawn_blocking(move || store.get_attachment(&attachment_id))
                    .await
                    .map_err(|error| ApiError::Internal(error.to_string()))?
                    .map_err(|error| ApiError::Internal(error.to_string()))?;
            if attachment.is_some() {
                json!(format!(
                    "/api/v1/attachments/{}/download",
                    args.attachment_id
                ))
            } else {
                Value::Null
            }
        }
        "read_app_log" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                max_bytes: Option<usize>,
            }
            let args: Args = parse(args)?;
            let data_root = state.config.data_dir.canonicalize().map_err(|error| {
                ApiError::Internal(format!("Data directory unavailable: {error}"))
            })?;
            let log_dir = state.config.data_dir.join("logs");
            let max_bytes = args.max_bytes.unwrap_or(64 * 1024).clamp(1, 1024 * 1024);
            let log_path = log_dir.join("pebble.log");
            let mut content = String::new();
            let mut truncated = false;
            if log_path.exists() {
                let canonical = log_path.canonicalize().map_err(|error| {
                    ApiError::Internal(format!("Log file unavailable: {error}"))
                })?;
                if !canonical.starts_with(&data_root) {
                    return Err(ApiError::BadRequest(
                        "Log file is outside application data".into(),
                    ));
                }
                let bytes = tokio::fs::read(&canonical)
                    .await
                    .map_err(|error| ApiError::Internal(format!("Failed to read log: {error}")))?;
                truncated = bytes.len() > max_bytes;
                let tail = if truncated {
                    &bytes[bytes.len() - max_bytes..]
                } else {
                    &bytes[..]
                };
                content = String::from_utf8_lossy(tail).into_owned();
                for (needle, replacement) in [
                    ("password=", "password=[REDACTED]"),
                    ("token=", "token=[REDACTED]"),
                    ("secret=", "secret=[REDACTED]"),
                    ("Authorization: Bearer ", "Authorization: Bearer [REDACTED]"),
                    ("body=", "body=[REDACTED]"),
                    ("access_token:", "access_token:[REDACTED]"),
                    ("refresh_token:", "refresh_token:[REDACTED]"),
                ] {
                    content = redact_log_value(&content, needle, replacement);
                }
            }
            json!({"path":"logs/pebble.log","content":content,"truncated":truncated})
        }
        "translate_text" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                text: String,
                from_lang: String,
                to_lang: String,
            }
            let args: Args = parse(args)?;
            let result = crate::routes::translate::translate(
                axum::extract::State(state.clone()),
                axum::Json(crate::routes::translate::TranslateRequest {
                    text: args.text,
                    source_lang: Some(args.from_lang),
                    target_lang: args.to_lang,
                }),
            )
            .await?;
            json!({"translated": result.0.translated})
        }
        "get_translate_config" => {
            let config = state.store.get_translate_config().map_err(|error| {
                ApiError::Internal(format!("Failed to load translation config: {error}"))
            })?;
            serde_json::to_value(config).map_err(|error| ApiError::Internal(error.to_string()))?
        }
        "save_translate_config" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                provider_type: String,
                config: String,
                is_enabled: bool,
            }
            let args: Args = parse(args)?;
            let _ = crate::routes::translate::save_translate_config(
                axum::extract::State(state.clone()),
                axum::Json(crate::routes::translate::SaveTranslateConfigRequest {
                    provider_type: args.provider_type,
                    config: args.config,
                    is_enabled: args.is_enabled,
                }),
            )
            .await?;
            Value::Null
        }
        "test_translate_connection" => {
            #[derive(Deserialize)]
            struct Args {
                config: String,
            }
            let args: Args = parse(args)?;
            let result = crate::routes::translate::test_translate_connection(axum::Json(
                crate::routes::translate::SaveTranslateConfigRequest {
                    provider_type: String::new(),
                    config: args.config,
                    is_enabled: true,
                },
            ))
            .await?;
            json!(result.0["result"].as_str().unwrap_or_default())
        }
        "test_webdav_connection"
        | "backup_to_webdav"
        | "preview_webdav_backup"
        | "restore_from_webdav" => {
            #[derive(Deserialize)]
            struct Args {
                url: String,
                username: String,
                password: String,
                #[serde(default, rename = "secretPassphrase")]
                secret_passphrase: Option<String>,
            }
            let args: Args = parse(args)?;
            if args.secret_passphrase.is_some() {
                return Err(ApiError::BadRequest(
                    "Passphrase-encrypted backup secrets are not yet supported by the Web backend"
                        .into(),
                ));
            }
            let body = crate::routes::cloud_sync::WebDavRequest {
                url: args.url,
                username: args.username,
                password: args.password,
            };
            match command {
                "test_webdav_connection" => {
                    let result =
                        crate::routes::cloud_sync::test_webdav_connection(axum::Json(body)).await?;
                    json!(result.0["result"].as_str().unwrap_or("ok"))
                }
                "backup_to_webdav" => {
                    let _ = crate::routes::cloud_sync::backup_to_webdav(
                        axum::extract::State(state.clone()),
                        axum::Json(body),
                    )
                    .await?;
                    json!("Backup uploaded")
                }
                "preview_webdav_backup" => {
                    let preview =
                        crate::routes::cloud_sync::preview_webdav_backup(axum::Json(body)).await?;
                    serde_json::to_value(preview.0)
                        .map_err(|error| ApiError::Internal(error.to_string()))?
                }
                _ => {
                    let _ = crate::routes::cloud_sync::restore_from_webdav(
                        axum::extract::State(state.clone()),
                        axum::Json(body),
                    )
                    .await?;
                    json!("Backup restored")
                }
            }
        }
        "export_backup_file" => {
            #[derive(Deserialize)]
            struct Args {
                #[serde(default, rename = "secretPassphrase")]
                secret_passphrase: Option<String>,
            }
            let args: Args = parse(args)?;
            if args.secret_passphrase.is_some() {
                return Err(ApiError::BadRequest(
                    "Passphrase-encrypted backup secrets are not yet supported by the Web backend"
                        .into(),
                ));
            }
            let data = state
                .store
                .export_settings()
                .map_err(|error| ApiError::Internal(error.to_string()))?;
            json!(String::from_utf8(data).map_err(|error| ApiError::Internal(error.to_string()))?)
        }
        "preview_backup_file" => {
            #[derive(Deserialize)]
            struct Args {
                data: String,
            }
            let args: Args = parse(args)?;
            let preview = pebble_store::cloud_sync::preview_backup(args.data.as_bytes())
                .map_err(|error| ApiError::BadRequest(error.to_string()))?;
            serde_json::to_value(preview).map_err(|error| ApiError::Internal(error.to_string()))?
        }
        "import_backup_file" => {
            #[derive(Deserialize)]
            struct Args {
                data: String,
                #[serde(default, rename = "secretPassphrase")]
                secret_passphrase: Option<String>,
            }
            let args: Args = parse(args)?;
            if args.secret_passphrase.is_some() {
                return Err(ApiError::BadRequest(
                    "Passphrase-encrypted backup secrets are not yet supported by the Web backend"
                        .into(),
                ));
            }
            state
                .store
                .import_settings(args.data.as_bytes())
                .map_err(|error| ApiError::BadRequest(error.to_string()))?;
            json!("Backup imported")
        }
        "get_folder_unread_counts" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Account {
                account_id: String,
            }
            let args: Account = parse(args)?;
            store_call(state, move |store| {
                store.get_folder_unread_counts(&args.account_id)
            })
            .await?
        }
        "stage_compose_attachment" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct StageArgs {
                filename: String,
                bytes: Vec<u8>,
            }
            let args: StageArgs = parse(args)?;
            json!(
                crate::routes::safe_files::stage_bytes(
                    &state.attachments_dir,
                    &args.filename,
                    &args.bytes
                )
                .await?
            )
        }
        "cleanup_staged_compose_attachment" => {
            #[derive(Deserialize)]
            struct PathArgs {
                path: String,
            }
            let args: PathArgs = parse(args)?;
            let path =
                crate::routes::safe_files::staged_token_path(&state.attachments_dir, &args.path)?;
            let canonical_root = state
                .attachments_dir
                .canonicalize()
                .map_err(|_| ApiError::BadRequest("Attachment storage is unavailable".into()))?;
            let canonical = path
                .canonicalize()
                .map_err(|_| ApiError::BadRequest("Staged attachment not found".into()))?;
            if !canonical.starts_with(canonical_root.join("staging")) || !canonical.is_file() {
                return Err(ApiError::BadRequest(
                    "Invalid staged attachment path".into(),
                ));
            }
            tokio::fs::remove_file(&canonical).await.map_err(|error| {
                ApiError::Internal(format!("Failed to remove staged attachment: {error}"))
            })?;
            if let Some(parent) = canonical.parent() {
                let _ = tokio::fs::remove_dir(parent).await;
            }
            Value::Null
        }
        _ => return Ok(None),
    };
    Ok(Some(result))
}

fn redact_log_value(content: &str, needle: &str, replacement: &str) -> String {
    let mut output = String::with_capacity(content.len());
    let mut remainder = content;
    let lowered_needle = needle.to_ascii_lowercase();
    while let Some(index) = remainder.to_ascii_lowercase().find(&lowered_needle) {
        let after_key = index + needle.len();
        output.push_str(&remainder[..after_key]);
        output.push_str(replacement);
        let rest = &remainder[after_key..];
        let end = rest
            .find(|ch: char| ch.is_whitespace() || matches!(ch, ',' | ';' | '"' | '\''))
            .unwrap_or(rest.len());
        remainder = &rest[end..];
    }
    output.push_str(remainder);
    output
}
