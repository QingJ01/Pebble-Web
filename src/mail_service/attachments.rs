use pebble_core::{Attachment, PebbleError};
use std::path::{Path, PathBuf};

fn is_windows_reserved_name(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_uppercase().as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

fn copy_attachment_file_safely<F>(
    source: &Path,
    save_path: &Path,
    mut on_progress: F,
) -> Result<PathBuf, PebbleError>
where
    F: FnMut(u64, u64),
{
    use std::io::{Read, Write};

    let mut src_file = std::fs::File::open(source)
        .map_err(|e| PebbleError::Internal(format!("Failed to open source: {e}")))?;
    let total_bytes = src_file
        .metadata()
        .map_err(|e| PebbleError::Internal(format!("Failed to read file metadata: {e}")))?
        .len();

    let (actual_save_path, mut dst_file) = create_unique_target(save_path)?;

    let mut buf = [0u8; 8192];
    let mut bytes_copied: u64 = 0;
    let copy_result: std::result::Result<(), PebbleError> = (|| {
        loop {
            let n = src_file
                .read(&mut buf)
                .map_err(|e| PebbleError::Internal(format!("Read error: {e}")))?;
            if n == 0 {
                break;
            }
            dst_file
                .write_all(&buf[..n])
                .map_err(|e| PebbleError::Internal(format!("Write error: {e}")))?;
            bytes_copied += n as u64;
            on_progress(bytes_copied, total_bytes);
        }
        dst_file
            .sync_all()
            .map_err(|e| PebbleError::Internal(format!("Failed to flush file: {e}")))?;
        Ok(())
    })();

    if let Err(e) = copy_result {
        drop(dst_file);
        let _ = std::fs::remove_file(&actual_save_path);
        return Err(e);
    }

    Ok(actual_save_path)
}

fn create_unique_target(save_path: &Path) -> Result<(PathBuf, std::fs::File), PebbleError> {
    const MAX_UNIQUE_ATTEMPTS: u32 = 1000;

    for attempt in 0..MAX_UNIQUE_ATTEMPTS {
        let candidate = unique_save_path(save_path, attempt);
        // create_new refuses to follow or replace an existing target, including
        // a symlink planted after path validation and before the file is opened.
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => return Ok((candidate, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(PebbleError::Internal(format!(
                    "Failed to create target file: {e}"
                )))
            }
        }
    }

    Err(PebbleError::Validation(
        "Could not choose an unused filename for attachment download".to_string(),
    ))
}

fn unique_save_path(save_path: &Path, attempt: u32) -> PathBuf {
    if attempt == 0 {
        return save_path.to_path_buf();
    }

    let parent = save_path.parent().unwrap_or_else(|| Path::new(""));
    let stem = save_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("download");
    let extension = save_path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{value}"))
        .unwrap_or_default();

    parent.join(format!("{stem} ({attempt}){extension}"))
}

pub(crate) fn sanitize_stored_filename(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    if base == "." || base == ".." {
        return "attachment".to_string();
    }

    let mut cleaned = base.to_string();
    while cleaned.contains("..") {
        cleaned = cleaned.replace("..", ".");
    }

    let sanitized: String = cleaned
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '|' | '?' | '*' => '_',
            _ => c,
        })
        .filter(|c| !c.is_control())
        .collect();
    let trimmed = sanitized
        .trim()
        .trim_matches(|c: char| c == '.' || c == ' ');

    if trimmed.is_empty() {
        return "attachment".to_string();
    }

    let stem = Path::new(trimmed)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if is_windows_reserved_name(stem) {
        return "attachment".to_string();
    }

    trimmed.to_string()
}

pub(crate) fn stage_local_attachment_records(
    attachments_root: &Path,
    message_id: &str,
    source_paths: &[String],
) -> Result<Vec<Attachment>, PebbleError> {
    if source_paths.is_empty() {
        return Ok(Vec::new());
    }

    let message_dir = attachments_root.join(message_id);
    std::fs::create_dir_all(&message_dir).map_err(|e| {
        PebbleError::Internal(format!(
            "Failed to create local attachment directory {}: {e}",
            message_dir.display()
        ))
    })?;

    let mut records = Vec::with_capacity(source_paths.len());
    for source in source_paths {
        let source_path = Path::new(source);
        let source_metadata = match std::fs::metadata(source_path) {
            Ok(metadata) => metadata,
            Err(error) => {
                cleanup_staged_attachment_records(&records);
                return Err(PebbleError::Internal(format!(
                    "Attachment source file not available: {source} ({error})"
                )));
            }
        };
        if !source_metadata.is_file() {
            cleanup_staged_attachment_records(&records);
            return Err(PebbleError::Validation(format!(
                "Attachment source is not a file: {source}"
            )));
        }

        let filename = source_path
            .file_name()
            .and_then(|value| value.to_str())
            .map(sanitize_stored_filename)
            .unwrap_or_else(|| "attachment".to_string());
        let attachment_dir = message_dir.join(pebble_core::new_id());
        if let Err(error) = std::fs::create_dir_all(&attachment_dir) {
            cleanup_staged_attachment_records(&records);
            return Err(PebbleError::Internal(format!(
                "Failed to create local attachment directory {}: {error}",
                attachment_dir.display()
            )));
        }
        let target = attachment_dir.join(&filename);
        let staged_path =
            match copy_attachment_file_safely(source_path, &target, |_copied, _total| {}) {
                Ok(path) => path,
                Err(error) => {
                    let _ = std::fs::remove_dir_all(&attachment_dir);
                    cleanup_staged_attachment_records(&records);
                    return Err(error);
                }
            };
        let size = std::fs::metadata(&staged_path)
            .map(|metadata| metadata.len().min(i64::MAX as u64) as i64)
            .unwrap_or(0);

        records.push(Attachment {
            id: pebble_core::new_id(),
            message_id: message_id.to_string(),
            filename,
            mime_type: "application/octet-stream".to_string(),
            size,
            local_path: Some(staged_path.to_string_lossy().to_string()),
            content_id: None,
            is_inline: false,
        });
    }

    Ok(records)
}

pub(crate) fn cleanup_staged_attachment_records(records: &[Attachment]) {
    for record in records {
        let Some(path) = record.local_path.as_deref().map(Path::new) else {
            continue;
        };
        if let Err(error) = std::fs::remove_file(path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(
                    "Failed to remove staged attachment {}: {error}",
                    path.display()
                );
            }
        }
        if let Some(parent) = path.parent() {
            let _ = std::fs::remove_dir(parent);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_attachment_file_safely_uses_unique_target_when_requested_name_exists() {
        let unique = pebble_core::new_id();
        let base = std::env::temp_dir().join(format!("pebble-attachment-copy-{unique}"));
        std::fs::create_dir_all(&base).expect("test dir");
        let source = base.join("source.txt");
        let target = base.join("target.txt");
        std::fs::write(&source, b"new content").expect("source write");
        std::fs::write(&target, b"existing content").expect("target write");

        let actual = copy_attachment_file_safely(&source, &target, |_copied, _total| {})
            .expect("existing targets should get a unique filename");

        assert_eq!(
            std::fs::read(&target).expect("target read"),
            b"existing content"
        );
        assert_ne!(actual, target);
        assert_eq!(
            actual.file_name().and_then(|name| name.to_str()),
            Some("target (1).txt")
        );
        assert_eq!(
            std::fs::read(actual).expect("unique target read"),
            b"new content"
        );

        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn stage_local_attachment_records_copies_sources_into_message_attachment_dir() {
        let unique = pebble_core::new_id();
        let base = std::env::temp_dir().join(format!("pebble-attachment-stage-{unique}"));
        let source_dir = base.join("source");
        let attachments_root = base.join("attachments");
        std::fs::create_dir_all(&source_dir).expect("source dir");
        let source = source_dir.join("report.txt");
        std::fs::write(&source, b"payload").expect("source write");

        let records = stage_local_attachment_records(
            &attachments_root,
            "message-1",
            &[source.to_string_lossy().to_string()],
        )
        .expect("local attachment should be staged");

        assert_eq!(records.len(), 1);
        let staged_path = records[0]
            .local_path
            .as_ref()
            .map(PathBuf::from)
            .expect("record should point at staged file");
        assert_ne!(staged_path, source);
        assert!(staged_path.starts_with(attachments_root.join("message-1")));
        assert_eq!(
            std::fs::read(&staged_path).expect("staged read"),
            b"payload"
        );

        std::fs::remove_file(&source).expect("remove original");
        let target = base.join("downloaded.txt");
        copy_attachment_file_safely(&staged_path, &target, |_copied, _total| {})
            .expect("download should use staged copy, not the original file");
        assert_eq!(std::fs::read(target).expect("downloaded read"), b"payload");

        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn stage_local_attachment_records_keeps_duplicate_filenames_distinct() {
        let unique = pebble_core::new_id();
        let base = std::env::temp_dir().join(format!("pebble-attachment-duplicates-{unique}"));
        let first_dir = base.join("first");
        let second_dir = base.join("second");
        let attachments_root = base.join("attachments");
        std::fs::create_dir_all(&first_dir).unwrap();
        std::fs::create_dir_all(&second_dir).unwrap();
        let first = first_dir.join("report.txt");
        let second = second_dir.join("report.txt");
        std::fs::write(&first, b"first").unwrap();
        std::fs::write(&second, b"second").unwrap();

        let records = stage_local_attachment_records(
            &attachments_root,
            "message-1",
            &[
                first.to_string_lossy().to_string(),
                second.to_string_lossy().to_string(),
            ],
        )
        .unwrap();

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].filename, "report.txt");
        assert_eq!(records[1].filename, "report.txt");
        assert_ne!(records[0].local_path, records[1].local_path);
        assert_eq!(
            std::fs::read(records[0].local_path.as_ref().unwrap()).unwrap(),
            b"first"
        );
        assert_eq!(
            std::fs::read(records[1].local_path.as_ref().unwrap()).unwrap(),
            b"second"
        );

        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn stage_local_attachment_records_cleans_partial_copy_on_error() {
        let unique = pebble_core::new_id();
        let base = std::env::temp_dir().join(format!("pebble-attachment-rollback-{unique}"));
        let source_dir = base.join("source");
        let attachments_root = base.join("attachments");
        std::fs::create_dir_all(&source_dir).unwrap();
        let valid = source_dir.join("valid.txt");
        std::fs::write(&valid, b"payload").unwrap();
        let missing = source_dir.join("missing.txt");

        let result = stage_local_attachment_records(
            &attachments_root,
            "message-1",
            &[
                valid.to_string_lossy().to_string(),
                missing.to_string_lossy().to_string(),
            ],
        );

        assert!(result.is_err());
        let message_dir = attachments_root.join("message-1");
        let remaining_files = std::fs::read_dir(&message_dir)
            .unwrap()
            .flat_map(|entry| std::fs::read_dir(entry.unwrap().path()).unwrap())
            .count();
        assert_eq!(remaining_files, 0);

        let _ = std::fs::remove_dir_all(base);
    }
}
