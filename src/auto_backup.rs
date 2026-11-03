use crate::state::AppStateRef;
use pebble_store::cloud_sync::{WebDavClient, SETTINGS_BACKUP_FILENAME};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::sync::watch;

const CONFIG_KEY: &str = "auto_backup_config";
const STATUS_KEY: &str = "auto_backup_status";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoBackupConfig {
    pub url: String,
    pub username: String,
    pub password: String,
    pub secret_passphrase: Option<String>,
    pub interval_minutes: u64,
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AutoBackupStatus {
    pub last_success_at: Option<i64>,
    pub last_error: Option<String>,
}

fn validate(config: &AutoBackupConfig) -> Result<(), String> {
    if config.interval_minutes == 0 || config.interval_minutes > 43_200 {
        return Err("Backup interval must be between 1 and 43200 minutes".into());
    }
    if config.enabled && (config.url.is_empty() || config.username.is_empty()) {
        return Err("Enabled automatic backup requires a URL and username".into());
    }
    Ok(())
}

fn read_encrypted<T: for<'de> Deserialize<'de>>(
    state: &AppStateRef,
    key: &str,
) -> Result<Option<T>, String> {
    let Some(bytes) = state
        .store
        .get_secure_user_data(key)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let plaintext = state
        .crypto
        .decrypt_for("secure_user_data.value", key, &bytes)
        .map_err(|error| format!("Failed to decrypt {key}: {error}"))?;
    serde_json::from_slice(&plaintext)
        .map(Some)
        .map_err(|error| format!("Invalid {key}: {error}"))
}

fn write_encrypted<T: Serialize>(state: &AppStateRef, key: &str, value: &T) -> Result<(), String> {
    let plaintext = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    let bytes = state
        .crypto
        .encrypt_for("secure_user_data.value", key, &plaintext)
        .map_err(|error| error.to_string())?;
    state
        .store
        .set_secure_user_data(key, &bytes)
        .map_err(|error| error.to_string())
}

pub fn save_config(state: &AppStateRef, config: AutoBackupConfig) -> Result<(), String> {
    validate(&config)?;
    write_encrypted(state, CONFIG_KEY, &config)?;
    state.auto_backup_wake.notify_one();
    Ok(())
}

pub fn load_config(state: &AppStateRef) -> Result<Option<AutoBackupConfig>, String> {
    read_encrypted(state, CONFIG_KEY)
}

pub fn delete_config(state: &AppStateRef) -> Result<(), String> {
    state
        .store
        .delete_secure_user_data(CONFIG_KEY)
        .map_err(|error| error.to_string())?;
    state.auto_backup_wake.notify_one();
    Ok(())
}

pub fn load_status(state: &AppStateRef) -> Result<AutoBackupStatus, String> {
    Ok(read_encrypted(state, STATUS_KEY)?.unwrap_or_default())
}

async fn run_backup(state: &AppStateRef, config: &AutoBackupConfig) -> Result<(), String> {
    validate(config)?;
    if config.secret_passphrase.is_some() {
        return Err(
            "Passphrase-encrypted backup secrets are not supported by the Web backend".into(),
        );
    }
    let data = state
        .store
        .export_settings()
        .map_err(|error| format!("Failed to export settings: {error}"))?;
    let client = WebDavClient::new(
        config.url.clone(),
        config.username.clone(),
        config.password.clone(),
    )
    .map_err(|error| error.to_string())?;
    client
        .upload(SETTINGS_BACKUP_FILENAME, &data)
        .await
        .map_err(|error| error.to_string())
}

pub async fn run_scheduler(state: AppStateRef, mut shutdown: watch::Receiver<bool>) {
    loop {
        if *shutdown.borrow() {
            return;
        }
        let config = match load_config(&state) {
            Ok(config) => config.filter(|config| config.enabled),
            Err(error) => {
                tracing::error!("Automatic backup configuration could not be read: {error}");
                None
            }
        };
        let Some(config) = config else {
            tokio::select! {
                _ = state.auto_backup_wake.notified() => {},
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() { return; }
                }
            }
            continue;
        };
        let interval = Duration::from_secs(config.interval_minutes.saturating_mul(60));
        let status = match load_status(&state) {
            Ok(status) => status,
            Err(error) => {
                tracing::error!("Automatic backup status could not be read: {error}");
                AutoBackupStatus::default()
            }
        };
        let due_at = status
            .last_success_at
            .unwrap_or(0)
            .saturating_add(interval.as_secs() as i64);
        let delay = due_at.saturating_sub(pebble_core::now_timestamp()).max(0) as u64;
        if delay > 0 {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(delay)) => {},
                _ = state.auto_backup_wake.notified() => continue,
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() { return; }
                }
            }
        }
        let outcome = tokio::select! {
            result = run_backup(&state, &config) => result,
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() { return; }
                continue;
            }
        };
        let mut status = load_status(&state).unwrap_or_default();
        match outcome {
            Ok(()) => {
                status.last_success_at = Some(pebble_core::now_timestamp());
                status.last_error = None;
            }
            Err(error) => {
                status.last_error = Some(error.clone());
                tracing::error!("Automatic backup failed: {error}");
            }
        }
        if let Err(error) = write_encrypted(&state, STATUS_KEY, &status) {
            tracing::error!("Failed to persist automatic backup status: {error}");
        }
        if status.last_error.is_some() {
            tokio::select! {
                _ = tokio::time::sleep(interval) => {},
                _ = state.auto_backup_wake.notified() => {},
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() { return; }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_backup_interval_is_bounded_and_positive() {
        let mut config = AutoBackupConfig {
            url: "https://dav.example".into(),
            username: "user".into(),
            password: "secret".into(),
            secret_passphrase: None,
            interval_minutes: 1,
            enabled: true,
        };
        assert!(validate(&config).is_ok());
        config.interval_minutes = 0;
        assert!(validate(&config).is_err());
        config.interval_minutes = 43_201;
        assert!(validate(&config).is_err());
    }
}
