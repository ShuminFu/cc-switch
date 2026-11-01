#![allow(non_snake_case)]

use super::{timed_blocking, timed_blocking_with_state};
use serde_json::{json, Value};
use std::path::PathBuf;
use tauri::State;
use tauri_plugin_dialog::DialogExt;

use crate::commands::sync_support::{
    post_sync_warning_from_result, run_post_import_sync, success_payload_with_warning,
};
use crate::database::backup::{BackupEntry, BackupKind};
use crate::database::Database;
use crate::error::AppError;
use crate::services::provider::ProviderService;
use crate::store::AppState;

// ─── File import/export ──────────────────────────────────────

/// 导出数据库为 SQL 备份
#[tauri::command]
#[specta::specta]
pub async fn export_config_to_file(
    #[allow(non_snake_case)] filePath: String,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let target_path = PathBuf::from(&filePath);
        db.export_sql(&target_path)?;
        Ok::<_, AppError>(json!({
            "success": true,
            "message": "SQL exported successfully",
            "filePath": filePath
        }))
    })
    .await
    .map_err(|e| format!("导出配置失败: {e}"))?
    .map_err(|e: AppError| e.to_string())
}

/// 从 SQL 备份导入数据库
#[tauri::command]
#[specta::specta]
pub async fn import_config_from_file(
    #[allow(non_snake_case)] filePath: String,
    app: tauri::AppHandle,
) -> Result<Value, String> {
    timed_blocking_with_state("import_config_from_file", app, move |state: &AppState| {
        let path_buf = PathBuf::from(&filePath);
        let backup_id = state.db.import_sql(&path_buf)?;
        let warning = post_sync_warning_from_result(Ok(run_post_import_sync(state)));
        if let Some(msg) = warning.as_ref() {
            log::warn!("[Import] post-import sync warning: {msg}");
        }
        Ok::<_, AppError>(success_payload_with_warning(backup_id, warning))
    })
    .await
    .map_err(|e: AppError| format!("导入配置失败: {e}"))
}

#[tauri::command]
#[specta::specta]
pub async fn sync_current_providers_live(app: tauri::AppHandle) -> Result<Value, String> {
    // Uses the managed AppState: the live sync must see the real ProxyService's
    // takeover state, which a throwaway `AppState::new` would not (CMD-19).
    timed_blocking_with_state("sync_current_providers_live", app, |state: &AppState| {
        ProviderService::sync_current_to_live(state)?;
        Ok::<_, AppError>(json!({
            "success": true,
            "message": "Live configuration synchronized"
        }))
    })
    .await
    .map_err(|e: AppError| format!("同步当前供应商失败: {e}"))
}

// ─── File dialogs ────────────────────────────────────────────

/// 保存文件对话框
#[tauri::command]
pub async fn save_file_dialog<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    #[allow(non_snake_case)] defaultName: String,
) -> Result<Option<String>, String> {
    // The dialog is modal and can stay open for minutes: run it on the blocking
    // pool so it does not park a tokio worker shared with the proxy (CMD-22).
    let result = timed_blocking("save_file_dialog", move || {
        Ok::<_, String>(
            app.dialog()
                .file()
                .add_filter("SQL", &["sql"])
                .set_file_name(&defaultName)
                .blocking_save_file(),
        )
    })
    .await?;

    Ok(result.map(|p| p.to_string()))
}

/// 打开文件对话框
#[tauri::command]
pub async fn open_file_dialog<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Option<String>, String> {
    let result = timed_blocking("open_file_dialog", move || {
        Ok::<_, String>(
            app.dialog()
                .file()
                .add_filter("SQL", &["sql"])
                .blocking_pick_file(),
        )
    })
    .await?;

    Ok(result.map(|p| p.to_string()))
}

/// 打开 ZIP 文件选择对话框
#[tauri::command]
pub async fn open_zip_file_dialog<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Option<String>, String> {
    let result = timed_blocking("open_zip_file_dialog", move || {
        Ok::<_, String>(
            app.dialog()
                .file()
                .add_filter("ZIP / Skill", &["zip", "skill"])
                .blocking_pick_file(),
        )
    })
    .await?;

    Ok(result.map(|p| p.to_string()))
}

// ─── Database backup management ─────────────────────────────

/// Manually create a database backup
#[tauri::command]
#[specta::specta]
pub async fn create_db_backup(state: State<'_, AppState>) -> Result<String, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        match db.backup_database_file_of_kind(BackupKind::Manual, None)? {
            Some(path) => Ok(path
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default()),
            None => Err(AppError::Config(
                "Database file not found, backup skipped".to_string(),
            )),
        }
    })
    .await
    .map_err(|e| format!("Backup failed: {e}"))?
    .map_err(|e: AppError| e.to_string())
}

/// List all database backup files
#[tauri::command]
pub async fn list_db_backups() -> Result<Vec<BackupEntry>, String> {
    timed_blocking("list_db_backups", move || {
        Database::list_backups().map_err(|e| e.to_string())
    })
    .await
}

/// Restore database from a backup file
#[tauri::command]
#[specta::specta]
pub async fn restore_db_backup(app: tauri::AppHandle, filename: String) -> Result<String, String> {
    timed_blocking_with_state("restore_db_backup", app, move |state: &AppState| {
        let safety_id = state.db.restore_from_backup(&filename)?;
        // Same post-replace pipeline as SQL import and WebDAV/S3 download:
        // rewrite live files from the restored current providers and reload
        // settings. Takeover state is kept from this device by the restore, so
        // the takeover-aware live sync stays consistent with the files on disk.
        if let Some(msg) = post_sync_warning_from_result(Ok(run_post_import_sync(state))) {
            log::warn!("[Restore] post-restore sync warning: {msg}");
        }
        Ok::<_, AppError>(safety_id)
    })
    .await
    .map_err(|e: AppError| format!("Restore failed: {e}"))
}

/// Restore a database backup; `keep_usage_history = false` restores the
/// backup's own usage logs and statistics instead of keeping this device's.
#[tauri::command]
pub async fn restore_db_backup_with_options(
    app: tauri::AppHandle,
    filename: String,
    #[allow(non_snake_case)] keepUsageHistory: bool,
) -> Result<String, String> {
    timed_blocking_with_state(
        "restore_db_backup_with_options",
        app,
        move |state: &AppState| {
            let safety_id = state
                .db
                .restore_from_backup_with_options(&filename, keepUsageHistory)?;
            if let Some(msg) = post_sync_warning_from_result(Ok(run_post_import_sync(state))) {
                log::warn!("[Restore] post-restore sync warning: {msg}");
            }
            Ok::<_, AppError>(safety_id)
        },
    )
    .await
    .map_err(|e: AppError| format!("Restore failed: {e}"))
}

/// Rename a database backup file
#[tauri::command]
#[specta::specta]
pub async fn rename_db_backup(
    #[allow(non_snake_case)] oldFilename: String,
    #[allow(non_snake_case)] newName: String,
) -> Result<String, String> {
    timed_blocking("rename_db_backup", move || {
        Database::rename_backup(&oldFilename, &newName).map_err(|e| e.to_string())
    })
    .await
}

/// Delete a database backup file
#[tauri::command]
#[specta::specta]
pub async fn delete_db_backup(filename: String) -> Result<(), String> {
    timed_blocking("delete_db_backup", move || {
        Database::delete_backup(&filename).map_err(|e| e.to_string())
    })
    .await
}
