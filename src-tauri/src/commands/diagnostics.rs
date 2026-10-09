//! 诊断报告命令

use crate::store::AppState;
use std::path::PathBuf;
use tauri_plugin_dialog::DialogExt;

/// 生成诊断报告（Markdown，不含密钥）
#[tauri::command]
pub async fn get_diagnostics_report(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    crate::services::diagnostics::build_report(&app, state.inner()).await
}

/// 保存诊断报告前的保存对话框
#[tauri::command]
pub async fn save_diagnostics_dialog<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    default_name: String,
) -> Result<Option<String>, String> {
    let result = app
        .dialog()
        .file()
        .add_filter("Markdown", &["md"])
        .set_file_name(&default_name)
        .blocking_save_file();
    Ok(result.map(|p| p.to_string()))
}

/// 生成并写入诊断报告，返回实际路径
#[tauri::command]
pub async fn export_diagnostics_report(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    target_path: String,
) -> Result<String, String> {
    let target = target_path.trim();
    if target.is_empty() {
        return Err("Export path is empty".to_string());
    }
    let report = crate::services::diagnostics::build_report(&app, state.inner()).await?;
    let path = PathBuf::from(target);
    crate::config::write_text_file(&path, &report).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}
