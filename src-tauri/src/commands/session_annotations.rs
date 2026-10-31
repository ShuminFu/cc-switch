//! Session pin / archive / tags (SESS-18). Stored outside the main database in
//! `~/.cc-switch/session-annotations.json`; see `session_manager::annotations`.

use crate::session_manager::annotations::{self, SessionAnnotation};
use std::collections::BTreeMap;

/// Every session annotation, keyed by session key.
#[tauri::command]
pub async fn get_session_annotations() -> Result<BTreeMap<String, SessionAnnotation>, String> {
    tauri::async_runtime::spawn_blocking(annotations::load)
        .await
        .map_err(|e| format!("Failed to read session annotations: {e}"))
}

/// Replace one session's annotation; returns the stored (normalized) value.
#[tauri::command]
pub async fn set_session_annotation(
    session_key: String,
    annotation: SessionAnnotation,
) -> Result<SessionAnnotation, String> {
    tauri::async_runtime::spawn_blocking(move || annotations::set(&session_key, annotation))
        .await
        .map_err(|e| format!("Failed to save session annotation: {e}"))?
}
