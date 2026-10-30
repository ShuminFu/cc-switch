use crate::session_manager;

#[tauri::command]
pub async fn list_sessions() -> Result<Vec<session_manager::SessionMeta>, String> {
    let sessions = tauri::async_runtime::spawn_blocking(session_manager::scan_sessions)
        .await
        .map_err(|e| format!("Failed to scan sessions: {e}"))?;
    Ok(sessions)
}

#[tauri::command]
pub async fn get_session_messages(
    provider_id: String,
    source_path: String,
) -> Result<Vec<session_manager::SessionMessage>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        session_manager::load_messages(&provider_id, &source_path)
    })
    .await
    .map_err(|e| format!("Failed to load session messages: {e}"))?
}

/// 在终端中恢复会话。webview 只能指定 `{providerId, sessionId}`：
/// 恢复命令与工作目录都由后端根据扫描到的会话元数据和固定模板生成，
/// 任何 IPC 参数都不会被当作 shell 字符串执行。
#[tauri::command]
pub async fn launch_session_terminal(
    provider_id: String,
    session_id: String,
) -> Result<bool, String> {
    // Read preferred terminal from global settings
    let preferred = crate::settings::get_preferred_terminal();
    // Map global setting terminal names to session terminal names
    // Global uses "iterm2", session terminal uses "iterm"
    let target = match preferred.as_deref() {
        Some("iterm2") => "iterm".to_string(),
        Some(t) => t.to_string(),
        // Default to Terminal.app on macOS; Linux/Windows resolve their own preferred terminal
        None => "terminal".to_string(),
    };

    tauri::async_runtime::spawn_blocking(move || {
        let resume = session_manager::resolve_resume_target(&provider_id, &session_id)?;
        crate::platform::terminal::launch_resume_terminal(&target, &resume, None)
    })
    .await
    .map_err(|e| format!("Failed to launch terminal: {e}"))??;

    Ok(true)
}

#[tauri::command]
pub async fn delete_session(
    provider_id: String,
    session_id: String,
    source_path: String,
) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        session_manager::delete_session(&provider_id, &session_id, &source_path)
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
