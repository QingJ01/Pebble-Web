use crate::state::AppStateRef;
use tokio::sync::watch;
use tokio::time::{Duration, MissedTickBehavior};

pub async fn run(state: AppStateRef, mut shutdown: watch::Receiver<bool>) {
    let mut interval = tokio::time::interval(Duration::from_secs(30));
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = interval.tick() => {}
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() { return; }
                continue;
            }
        }
        let store = state.store.clone();
        let due = tokio::task::spawn_blocking(move || {
            store.get_due_snoozed(pebble_core::now_timestamp())
        })
        .await;
        let due = match due {
            Ok(Ok(due)) => due,
            Ok(Err(error)) => {
                tracing::error!("Failed to load due snoozes: {error}");
                continue;
            }
            Err(error) => {
                tracing::error!("Due-snooze task failed: {error}");
                continue;
            }
        };
        for snoozed in due {
            let store = state.store.clone();
            let snoozed_for_task = snoozed.clone();
            let result = tokio::task::spawn_blocking(move || {
                store.move_message_to_folder(
                    &snoozed_for_task.message_id,
                    &snoozed_for_task.return_to,
                )?;
                store.unsnooze_message(&snoozed_for_task.message_id)
            })
            .await;
            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    tracing::error!("Failed to unsnooze {}: {error}", snoozed.message_id);
                    continue;
                }
                Err(error) => {
                    tracing::error!("Unsnooze task failed for {}: {error}", snoozed.message_id);
                    continue;
                }
            }
            let _ = state.ws_broadcast.send(
                serde_json::json!({
                    "type": "mail:unsnoozed",
                    "payload": { "message_id": snoozed.message_id }
                })
                .to_string(),
            );
        }
        if let Err(error) = state.store.purge_old_tombstones(30 * 24 * 60 * 60) {
            tracing::warn!("Failed to purge old deleted messages: {error}");
        }
    }
}
