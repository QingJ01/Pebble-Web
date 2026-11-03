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
    }
}
