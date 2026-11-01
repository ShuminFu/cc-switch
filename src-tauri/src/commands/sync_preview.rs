//! Sync download dry-run (SVC-27): what downloading the remote snapshot would
//! change locally, without applying it.

use serde_json::{json, Value};
use tauri::State;

use crate::error::AppError;
use crate::store::AppState;

/// Which configured sync backend to preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncBackend {
    WebDav,
    S3,
}

fn parse_backend(raw: &str) -> Result<SyncBackend, AppError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "webdav" => Ok(SyncBackend::WebDav),
        "s3" => Ok(SyncBackend::S3),
        other => Err(AppError::localized(
            "sync.preview_unknown_backend",
            format!("未知的同步后端: {other}"),
            format!("Unknown sync backend: {other}"),
        )),
    }
}

/// Diff the remote snapshot of `backend` (`"webdav"` | `"s3"`) against the
/// local data. Returns the per-table preview (row ids and key paths only,
/// never values) or `{ "empty": true }` when the remote has no snapshot.
#[tauri::command]
pub async fn preview_sync_download(
    state: State<'_, AppState>,
    backend: String,
) -> Result<Value, AppError> {
    let preview = match parse_backend(&backend)? {
        SyncBackend::WebDav => {
            let settings = super::webdav_sync::require_enabled_webdav_settings()?;
            crate::services::webdav_sync::preview(&state.db, &settings).await?
        }
        SyncBackend::S3 => {
            let settings = super::s3_sync::require_enabled_s3_settings()?;
            crate::services::s3_sync::preview(&state.db, &settings).await?
        }
    };
    match preview {
        Some(preview) => {
            serde_json::to_value(preview).map_err(|source| AppError::JsonSerialize { source })
        }
        None => Ok(json!({ "empty": true })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_names_are_parsed_case_insensitively() {
        assert_eq!(parse_backend("WebDAV").unwrap(), SyncBackend::WebDav);
        assert_eq!(parse_backend(" s3 ").unwrap(), SyncBackend::S3);
        let err = parse_backend("ftp").unwrap_err();
        assert_eq!(err.code(), "sync.preview_unknown_backend");
    }
}
