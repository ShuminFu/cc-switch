#![allow(non_snake_case)]

use crate::session_manager;
use crate::session_manager::export::{render_transcript, TranscriptFormat};
use std::path::PathBuf;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
pub async fn list_sessions() -> Result<Vec<session_manager::SessionMeta>, String> {
    let sessions = tauri::async_runtime::spawn_blocking(session_manager::scan_sessions)
        .await
        .map_err(|e| format!("Failed to scan sessions: {e}"))?;
    Ok(sessions)
}

#[tauri::command]
pub async fn get_session_messages(
    providerId: String,
    sourcePath: String,
) -> Result<Vec<session_manager::SessionMessage>, String> {
    let provider_id = providerId.clone();
    let source_path = sourcePath.clone();
    tauri::async_runtime::spawn_blocking(move || {
        session_manager::load_messages(&provider_id, &source_path)
    })
    .await
    .map_err(|e| format!("Failed to load session messages: {e}"))?
}

#[tauri::command]
pub async fn launch_session_terminal(
    command: String,
    cwd: Option<String>,
    custom_config: Option<String>,
) -> Result<bool, String> {
    let command = command.clone();
    let cwd = cwd.clone();
    let custom_config = custom_config.clone();

    // Read preferred terminal from global settings
    let preferred = crate::settings::get_preferred_terminal();
    // Map global setting terminal names to session terminal names
    // Global uses "iterm2", session terminal uses "iterm"
    let target = match preferred.as_deref() {
        Some("iterm2") => "iterm".to_string(),
        Some(t) => t.to_string(),
        None => "terminal".to_string(), // Default to Terminal.app on macOS
    };

    tauri::async_runtime::spawn_blocking(move || {
        session_manager::terminal::launch_terminal(
            &target,
            &command,
            cwd.as_deref(),
            custom_config.as_deref(),
        )
    })
    .await
    .map_err(|e| format!("Failed to launch terminal: {e}"))??;

    Ok(true)
}

#[tauri::command]
pub async fn delete_session(
    providerId: String,
    sessionId: String,
    sourcePath: String,
) -> Result<bool, String> {
    let provider_id = providerId.clone();
    let session_id = sessionId.clone();
    let source_path = sourcePath.clone();

    tauri::async_runtime::spawn_blocking(move || {
        session_manager::delete_session(&provider_id, &session_id, &source_path)
    })
    .await
    .map_err(|e| format!("Failed to delete session: {e}"))?
}

/// 导出会话记录前的保存对话框（按格式设置扩展名过滤）
#[tauri::command]
pub async fn save_session_transcript_dialog<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    defaultName: String,
    format: String,
) -> Result<Option<String>, String> {
    let format: TranscriptFormat = format.parse()?;
    let result = app
        .dialog()
        .file()
        .add_filter(format.dialog_label(), &[format.extension()])
        .set_file_name(&defaultName)
        .blocking_save_file();
    Ok(result.map(|p| p.to_string()))
}

/// 将会话记录渲染为 Markdown / JSON 并原子写入目标路径，返回实际写入的路径
#[tauri::command]
pub async fn export_session_transcript(
    session: session_manager::SessionMeta,
    targetPath: String,
    format: String,
) -> Result<String, String> {
    let format: TranscriptFormat = format.parse()?;
    let source_path = session
        .source_path
        .clone()
        .filter(|p| !p.is_empty())
        .ok_or_else(|| "Session has no source file to export".to_string())?;
    let target = PathBuf::from(targetPath.trim());
    if target.as_os_str().is_empty() {
        return Err("Export path is empty".to_string());
    }

    tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
        let messages = session_manager::load_messages(&session.provider_id, &source_path)?;
        let rendered = render_transcript(&session, &messages, format)?;
        crate::config::write_text_file(&target, &rendered).map_err(|e| e.to_string())?;
        Ok(target.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| format!("Failed to export session: {e}"))?
}

/// 在系统文件管理器中显示会话的项目目录或源文件
#[tauri::command]
pub async fn reveal_session_path<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    path: String,
) -> Result<(), String> {
    let target = PathBuf::from(path.trim());
    if !target.exists() {
        return Err(format!("Path does not exist: {}", target.display()));
    }
    if target.is_dir() {
        app.opener()
            .open_path(target.to_string_lossy().to_string(), None::<String>)
            .map_err(|e| e.to_string())
    } else {
        app.opener()
            .reveal_item_in_dir(&target)
            .map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub async fn delete_sessions(
    items: Vec<session_manager::DeleteSessionRequest>,
) -> Result<Vec<session_manager::DeleteSessionOutcome>, String> {
    tauri::async_runtime::spawn_blocking(move || session_manager::delete_sessions(&items))
        .await
        .map_err(|e| format!("Failed to delete sessions: {e}"))
}
