use crate::error::ApiError;
use std::path::{Path, PathBuf};

const STAGED_PREFIX: &str = "staged:";

pub fn safe_filename(value: &str) -> Result<String, ApiError> {
    let basename = Path::new(value)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .trim();
    let sanitized: String = basename
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | ' ') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let sanitized = sanitized.trim_matches([' ', '.']).to_string();
    if sanitized.is_empty() || sanitized == "." || sanitized == ".." {
        return Err(ApiError::BadRequest("Invalid attachment filename".into()));
    }
    Ok(sanitized.chars().take(180).collect())
}

pub fn stored_filename(value: &str) -> Result<String, ApiError> {
    let sanitized = crate::mail_service::attachments::sanitize_stored_filename(value);
    let mut limited = String::new();
    for ch in sanitized.chars() {
        if limited.len() + ch.len_utf8() > 200 {
            break;
        }
        limited.push(ch);
    }
    let limited = limited.trim_matches([' ', '.']).to_string();
    if limited.is_empty() {
        return Err(ApiError::BadRequest("Invalid attachment filename".into()));
    }
    Ok(limited)
}

pub async fn stage_bytes(root: &Path, filename: &str, bytes: &[u8]) -> Result<String, ApiError> {
    if bytes.is_empty() || bytes.len() > 25 * 1024 * 1024 {
        return Err(ApiError::BadRequest(
            "Attachment must be between 1 byte and 25 MiB".into(),
        ));
    }
    let filename = stored_filename(filename)?;
    let id = pebble_core::new_id();
    let canonical_root = root
        .canonicalize()
        .map_err(|_| ApiError::BadRequest("Attachment storage is unavailable".into()))?;
    let staging_root = canonical_root.join("staging");
    tokio::fs::create_dir_all(&staging_root)
        .await
        .map_err(|error| {
            ApiError::Internal(format!("Failed to create staging storage: {error}"))
        })?;
    let canonical_staging_root = staging_root.canonicalize().map_err(|error| {
        ApiError::Internal(format!("Failed to resolve staging storage: {error}"))
    })?;
    if !canonical_staging_root.starts_with(&canonical_root) {
        return Err(ApiError::BadRequest(
            "Staging storage outside attachment storage".into(),
        ));
    }
    let directory = canonical_staging_root.join(&id);
    tokio::fs::create_dir(&directory).await.map_err(|error| {
        ApiError::Internal(format!("Failed to create staging directory: {error}"))
    })?;
    let path = directory.join(&filename);
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .await
        .map_err(|error| {
            ApiError::Internal(format!("Failed to create staged attachment: {error}"))
        })?;
    use tokio::io::AsyncWriteExt;
    if let Err(error) = file.write_all(bytes).await {
        drop(file);
        let _ = tokio::fs::remove_dir_all(&directory).await;
        return Err(ApiError::Internal(format!(
            "Failed to write staged attachment: {error}"
        )));
    }
    Ok(format!("{STAGED_PREFIX}{id}/{filename}"))
}

pub fn staged_token_path(root: &Path, token: &str) -> Result<PathBuf, ApiError> {
    let relative = token
        .strip_prefix(STAGED_PREFIX)
        .ok_or_else(|| ApiError::BadRequest("Invalid staged attachment token".into()))?;
    let (id, name) = relative
        .split_once('/')
        .ok_or_else(|| ApiError::BadRequest("Invalid staged attachment token".into()))?;
    if id.len() != 36
        || !id.chars().all(|ch| ch.is_ascii_hexdigit() || ch == '-')
        || name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name == "."
        || name == ".."
    {
        return Err(ApiError::BadRequest(
            "Invalid staged attachment token".into(),
        ));
    }
    Ok(root.join("staging").join(id).join(name))
}

pub fn resolve_attachment_path(root: &Path, supplied: &str) -> Result<PathBuf, ApiError> {
    let root = root
        .canonicalize()
        .map_err(|_| ApiError::BadRequest("Attachment storage is unavailable".into()))?;
    let path = if supplied.starts_with(STAGED_PREFIX) {
        staged_token_path(&root, supplied)?
    } else {
        PathBuf::from(supplied)
    };
    let canonical = path
        .canonicalize()
        .map_err(|_| ApiError::BadRequest("Attachment not found".into()))?;
    if !canonical.starts_with(&root) || !canonical.is_file() {
        return Err(ApiError::BadRequest(
            "Attachment path outside allowed directory".into(),
        ));
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staged_tokens_reject_traversal_and_untrusted_filenames() {
        let root = Path::new("/srv/attachments");
        assert!(staged_token_path(root, "staged:../etc/passwd").is_err());
        assert!(
            staged_token_path(root, "staged:01234567-89ab-cdef-0123-456789abcdef/../x").is_err()
        );
        assert_eq!(safe_filename("../../report.pdf").unwrap(), "report.pdf");
        assert!(safe_filename("..").is_err());
        assert_eq!(
            stored_filename("../报告 2024.pdf").unwrap(),
            "报告 2024.pdf"
        );
        assert_eq!(stored_filename("..").unwrap(), "attachment");
    }
}
