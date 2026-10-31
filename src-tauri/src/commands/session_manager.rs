#![allow(non_snake_case)]

use crate::session_manager;

#[tauri::command]
pub async fn list_sessions() -> Result<Vec<session_manager::SessionMeta>, String> {
    let sessions = tauri::async_runtime::spawn_blocking(session_manager::scan_sessions)
        .await
        .map_err(|e| format!("Failed to scan sessions: {e}"))?;
    Ok(sessions)
}

/// Incremental rescan through the persistent session index. Returns the
/// sessions plus per-provider scan errors and a generation counter that
/// changes whenever the list changes (cheap to poll: unchanged files are only
/// `stat`ed).
#[tauri::command]
pub async fn scan_sessions_report(
    knownGeneration: Option<u64>,
) -> Result<session_manager::index::ScanReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        session_manager::scan_report().elide_if_known(knownGeneration)
    })
    .await
    .map_err(|e| format!("Failed to scan sessions: {e}"))
}

/// Sessions stored by the last scan (possibly from a previous run), returned
/// without touching provider directories so the list renders instantly.
#[tauri::command]
pub async fn get_session_index_snapshot() -> Result<Vec<session_manager::SessionMeta>, String> {
    tauri::async_runtime::spawn_blocking(|| session_manager::index::global().snapshot())
        .await
        .map_err(|e| format!("Failed to read session index: {e}"))
}

/// Full-text search across session titles, paths and transcripts.
/// Transcripts are indexed lazily (redacted) on the first searches; the
/// response's `complete` flag is false while indexing is still in progress.
#[tauri::command]
pub async fn search_sessions(
    query: String,
    limit: Option<usize>,
) -> Result<session_manager::index::SearchResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let index = session_manager::index::global();
        index.refresh(session_manager::registry::PROVIDERS);
        index.search(
            session_manager::registry::PROVIDERS,
            &query,
            limit.unwrap_or(100),
            session_manager::index::SEARCH_INDEX_BUDGET,
        )
    })
    .await
    .map_err(|e| format!("Failed to search sessions: {e}"))
}

#[tauri::command]
pub async fn get_session_messages(
    providerId: String,
    sourcePath: String,
) -> Result<Vec<session_manager::SessionMessage>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        session_manager::load_messages(&providerId, &sourcePath)
    })
    .await
    .map_err(|e| format!("Failed to load session messages: {e}"))?
}

/// One bounded page of a transcript (SESS-14). `offset` defaults to 0,
/// `limit` to 200 messages and `maxContentBytes` to 64 KiB per message; a page
/// never carries more than 8 MiB of content. Indices whose content was cut are
/// listed in `truncated` (ask again with a larger `maxContentBytes` and
/// `limit: 1` to load one message in full).
#[tauri::command]
pub async fn get_session_messages_page(
    providerId: String,
    sourcePath: String,
    offset: Option<usize>,
    limit: Option<usize>,
    maxContentBytes: Option<usize>,
) -> Result<session_manager::paging::SessionMessagePage, String> {
    use session_manager::paging::{DEFAULT_MAX_CONTENT_BYTES, DEFAULT_PAGE_LIMIT};
    tauri::async_runtime::spawn_blocking(move || {
        session_manager::load_messages_page(
            &providerId,
            &sourcePath,
            offset.unwrap_or(0),
            limit.unwrap_or(DEFAULT_PAGE_LIMIT),
            maxContentBytes.unwrap_or(DEFAULT_MAX_CONTENT_BYTES),
        )
    })
    .await
    .map_err(|e| format!("Failed to load session messages: {e}"))?
}

/// Token and cost totals for sessions (SESS-17), keyed by session id. Ids
/// without synced usage are absent. At most 2000 ids per call.
#[tauri::command]
pub async fn get_session_usage_summaries(
    handle: tauri::AppHandle,
    sessionIds: Vec<String>,
) -> Result<
    std::collections::HashMap<String, session_manager::usage::SessionUsageSummary>,
    crate::error::AppError,
> {
    super::timed_blocking_with_state("get_session_usage_summaries", handle, move |state| {
        session_manager::usage::summaries(&state.db, &sessionIds)
    })
    .await
}

/// Table of contents for a transcript: role, timestamp and a 160-char preview
/// per message (SESS-14).
#[tauri::command]
pub async fn list_session_turns(
    providerId: String,
    sourcePath: String,
) -> Result<Vec<session_manager::paging::SessionTurn>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        session_manager::load_session_turns(&providerId, &sourcePath)
    })
    .await
    .map_err(|e| format!("Failed to load session turns: {e}"))?
}

/// Open a terminal that resumes a session. The resume command is rebuilt in
/// Rust from the provider/session identifiers; the webview never supplies a
/// command string.
#[tauri::command]
#[specta::specta]
pub async fn launch_session_terminal(
    providerId: String,
    sessionId: String,
    cwd: Option<String>,
) -> Result<bool, String> {
    // Read preferred terminal from global settings
    let preferred = crate::settings::get_preferred_terminal();
    // Map global setting terminal names to session terminal names
    // Global uses "iterm2", session terminal uses "iterm"
    let target = match preferred.as_deref() {
        Some("iterm2") => "iterm".to_string(),
        Some(t) => t.to_string(),
        None => session_manager::terminal::default_terminal().to_string(),
    };

    tauri::async_runtime::spawn_blocking(move || {
        session_manager::terminal::launch_session_terminal(
            &target,
            &providerId,
            &sessionId,
            cwd.as_deref(),
        )
    })
    .await
    .map_err(|e| format!("Failed to launch terminal: {e}"))??;

    Ok(true)
}

#[tauri::command]
#[specta::specta]
pub async fn delete_session(
    providerId: String,
    sessionId: String,
    sourcePath: String,
) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        session_manager::delete_session(&providerId, &sessionId, &sourcePath)
    })
    .await
    .map_err(|e| format!("Failed to delete session: {e}"))?
}

#[tauri::command]
pub async fn delete_sessions(
    items: Vec<session_manager::DeleteSessionRequest>,
) -> Result<Vec<session_manager::DeleteSessionOutcome>, String> {
    tauri::async_runtime::spawn_blocking(move || session_manager::delete_sessions(&items))
        .await
        .map_err(|e| format!("Failed to delete sessions: {e}"))
}

/// Recently deleted (trashed) file-based sessions, newest first (SESS-41).
#[tauri::command]
pub async fn list_session_trash() -> Result<Vec<session_manager::trash::TrashEntry>, String> {
    tauri::async_runtime::spawn_blocking(session_manager::trash::list)
        .await
        .map_err(|e| format!("Failed to list session trash: {e}"))
}

/// Move a trashed session back to where it was deleted from (SESS-41).
#[tauri::command]
pub async fn restore_session_from_trash(
    id: String,
) -> Result<session_manager::trash::TrashEntry, String> {
    tauri::async_runtime::spawn_blocking(move || session_manager::trash::restore(&id))
        .await
        .map_err(|e| format!("Failed to restore session: {e}"))?
}
