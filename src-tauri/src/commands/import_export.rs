use serde_json::{json, Value};
use std::path::PathBuf;
use tauri::State;
use tauri_plugin_dialog::DialogExt;

use crate::commands::sync_support::{
    post_sync_warning_from_result, run_post_import_sync, success_payload_with_warning,
};
use crate::database::backup::BackupEntry;
use crate::database::{Database, DbStorageStats, Maintenance, MaintenanceReport};
use crate::error::AppError;
use crate::picked_paths::{self, PickPurpose};
use crate::services::provider::ProviderService;
use crate::store::AppState;

// ─── File import/export ──────────────────────────────────────

/// 导出数据库为 SQL 备份
#[tauri::command]
pub async fn export_config_to_file(
    file_path: String,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || export_sql_to_picked_path(&db, &file_path))
        .await
        .map_err(|e| format!("导出配置失败: {e}"))?
        .map_err(|e: AppError| e.to_string())
}

/// 导出到 `save_file_dialog` 选出的路径；其他路径一律拒绝
fn export_sql_to_picked_path(db: &Database, file_path: &str) -> Result<Value, AppError> {
    picked_paths::require_picked(PickPurpose::SqlExport, file_path)
        .map_err(AppError::InvalidInput)?;
    let target_path = PathBuf::from(file_path);
    db.export_sql(&target_path)?;
    Ok(json!({
        "success": true,
        "message": "SQL exported successfully",
        "filePath": file_path
    }))
}

/// 从 SQL 备份导入数据库
#[tauri::command]
pub async fn import_config_from_file(
    file_path: String,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let db = state.db.clone();
    let db_for_sync = db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let backup_id = import_sql_from_picked_path(&db, &file_path)?;
        let warning = post_sync_warning_from_result(Ok(run_post_import_sync(db_for_sync)));
        if let Some(msg) = warning.as_ref() {
            log::warn!("[Import] post-import sync warning: {msg}");
        }
        Ok::<_, AppError>(success_payload_with_warning(backup_id, warning))
    })
    .await
    .map_err(|e| format!("导入配置失败: {e}"))?
    .map_err(|e: AppError| e.to_string())
}

/// 从 `open_file_dialog` 选出的路径导入；其他路径一律拒绝
fn import_sql_from_picked_path(db: &Database, file_path: &str) -> Result<String, AppError> {
    picked_paths::require_picked(PickPurpose::SqlImport, file_path)
        .map_err(AppError::InvalidInput)?;
    db.import_sql(&PathBuf::from(file_path))
}

#[tauri::command]
pub async fn sync_current_providers_live(state: State<'_, AppState>) -> Result<Value, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let app_state = AppState::new(db);
        ProviderService::sync_current_to_live(&app_state)?;
        Ok::<_, AppError>(json!({
            "success": true,
            "message": "Live configuration synchronized"
        }))
    })
    .await
    .map_err(|e| format!("同步当前供应商失败: {e}"))?
    .map_err(|e: AppError| e.to_string())
}

// ─── File dialogs ────────────────────────────────────────────

/// 保存文件对话框
#[tauri::command]
pub async fn save_file_dialog<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    default_name: String,
) -> Result<Option<String>, String> {
    let dialog = app.dialog();
    let result = dialog
        .file()
        .add_filter("SQL", &["sql"])
        .set_file_name(&default_name)
        .blocking_save_file();

    Ok(result.map(|p| register_pick(PickPurpose::SqlExport, p.to_string())))
}

/// 打开文件对话框
#[tauri::command]
pub async fn open_file_dialog<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Option<String>, String> {
    let dialog = app.dialog();
    let result = dialog
        .file()
        .add_filter("SQL", &["sql"])
        .blocking_pick_file();

    Ok(result.map(|p| register_pick(PickPurpose::SqlImport, p.to_string())))
}

/// 打开 ZIP 文件选择对话框
#[tauri::command]
pub async fn open_zip_file_dialog<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Option<String>, String> {
    let dialog = app.dialog();
    let result = dialog
        .file()
        .add_filter("ZIP / Skill", &["zip", "skill"])
        .blocking_pick_file();

    Ok(result.map(|p| register_pick(PickPurpose::SkillZip, p.to_string())))
}

/// 保存 CSV 文件对话框
#[tauri::command]
pub async fn save_csv_file_dialog<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    default_name: String,
) -> Result<Option<String>, String> {
    let dialog = app.dialog();
    let result = dialog
        .file()
        .add_filter("CSV", &["csv"])
        .set_file_name(&default_name)
        .blocking_save_file();

    Ok(result.map(|p| register_pick(PickPurpose::CsvExport, p.to_string())))
}

/// 把前端生成的 CSV 写到 `save_csv_file_dialog` 选出的路径
#[tauri::command]
pub async fn write_csv_export(file_path: String, content: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || write_csv_to_picked_path(&file_path, &content))
        .await
        .map_err(|e| format!("导出 CSV 失败: {e}"))?
        .map_err(|e: AppError| e.to_string())
}

/// 只写入 CSV 对话框选出的路径；其他路径一律拒绝
fn write_csv_to_picked_path(file_path: &str, content: &str) -> Result<(), AppError> {
    picked_paths::require_picked(PickPurpose::CsvExport, file_path)
        .map_err(AppError::InvalidInput)?;
    crate::config::atomic_write(&PathBuf::from(file_path), content.as_bytes())
}

/// 登记对话框返回的路径，供后续命令校验
fn register_pick(purpose: PickPurpose, path: String) -> String {
    picked_paths::register(purpose, &path);
    path
}

// ─── Database backup management ─────────────────────────────

/// Manually create a database backup
#[tauri::command]
pub async fn create_db_backup(state: State<'_, AppState>) -> Result<String, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || match db.backup_database_file()? {
        Some(path) => Ok(path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default()),
        None => Err(AppError::Config(
            "Database file not found, backup skipped".to_string(),
        )),
    })
    .await
    .map_err(|e| format!("Backup failed: {e}"))?
    .map_err(|e: AppError| e.to_string())
}

/// List all database backup files
#[tauri::command]
pub fn list_db_backups() -> Result<Vec<BackupEntry>, String> {
    Database::list_backups().map_err(|e| e.to_string())
}

/// Restore database from a backup file
#[tauri::command]
pub async fn restore_db_backup(
    state: State<'_, AppState>,
    filename: String,
) -> Result<String, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || db.restore_from_backup(&filename))
        .await
        .map_err(|e| format!("Restore failed: {e}"))?
        .map_err(|e: AppError| e.to_string())
}

/// Downgrade restore from the db_version_too_new recovery screen: replace the
/// too-new database with a restorable (pre-migration) backup. The app must be
/// restarted afterwards. Returns the filename the newer database was kept as.
#[tauri::command]
pub async fn restore_backup_over_newer_db(filename: String) -> Result<String, String> {
    let in_recovery = crate::init_status::get_init_error()
        .map(|p| p.kind.as_deref() == Some(crate::init_status::DB_VERSION_TOO_NEW))
        .unwrap_or(false);
    if !in_recovery {
        return Err(
            "This restore is only available while the database is too new for this app".to_string(),
        );
    }
    tauri::async_runtime::spawn_blocking(move || Database::restore_backup_over_newer_db(&filename))
        .await
        .map_err(|e| format!("Restore failed: {e}"))?
        .map_err(|e: AppError| e.to_string())
}

/// Rename a database backup file
#[tauri::command]
pub fn rename_db_backup(old_filename: String, new_name: String) -> Result<String, String> {
    Database::rename_backup(&old_filename, &new_name).map_err(|e| e.to_string())
}

/// Delete a database backup file
#[tauri::command]
pub fn delete_db_backup(filename: String) -> Result<(), String> {
    Database::delete_backup(&filename).map_err(|e| e.to_string())
}

// ─── Database retention / maintenance ───────────────────────

/// Database size and per-table row counts for the retention panel
#[tauri::command]
pub async fn get_db_storage_stats(state: State<'_, AppState>) -> Result<DbStorageStats, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || Maintenance::storage_stats(&db))
        .await
        .map_err(|e| format!("Failed to read database stats: {e}"))?
        .map_err(|e: AppError| e.to_string())
}

/// Run database maintenance now with the retention policy from the current settings
#[tauri::command]
pub async fn run_db_maintenance(state: State<'_, AppState>) -> Result<MaintenanceReport, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || run_maintenance_with_current_policy(&db))
        .await
        .map_err(|e| format!("Maintenance failed: {e}"))
}

/// 读取当前设置中的保留策略（每次调用重新解析，保存设置后立即生效）并执行一次维护
fn run_maintenance_with_current_policy(db: &Database) -> MaintenanceReport {
    Maintenance::run(db, &crate::settings::effective_retention_policy())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::{self, Paths};
    use tempfile::TempDir;

    #[test]
    fn export_rejects_path_not_chosen_by_save_dialog() {
        let temp = TempDir::new().expect("tempdir");
        let _paths = paths::scoped(Paths::with_home(temp.path()));
        let db = Database::memory().expect("memory db");
        let target = temp.path().join("not-picked.sql");
        let target_str = target.to_string_lossy().to_string();

        let err = export_sql_to_picked_path(&db, &target_str).unwrap_err();
        assert!(err.to_string().contains("file dialog"), "{err}");
        assert!(!target.exists());

        // 由打开对话框选出的路径也不能用于导出
        picked_paths::register(PickPurpose::SqlImport, &target_str);
        assert!(export_sql_to_picked_path(&db, &target_str).is_err());
        assert!(!target.exists());
    }

    #[test]
    fn export_and_import_accept_dialog_picked_paths() {
        let temp = TempDir::new().expect("tempdir");
        let _paths = paths::scoped(Paths::with_home(temp.path()));
        let db = Database::memory().expect("memory db");
        let provider = crate::provider::Provider::with_id(
            "ccs-075".to_string(),
            "CCS 075".to_string(),
            json!({"env": {}}),
            None,
        );
        db.save_provider("claude", &provider)
            .expect("save provider");
        let target = temp.path().join("picked-export.sql");
        let target_str = target.to_string_lossy().to_string();

        picked_paths::register(PickPurpose::SqlExport, &target_str);
        let result = export_sql_to_picked_path(&db, &target_str).expect("export");
        assert_eq!(result["success"], json!(true));
        assert!(target.exists());

        picked_paths::register(PickPurpose::SqlImport, &target_str);
        let other = Database::memory().expect("memory db");
        import_sql_from_picked_path(&other, &target_str).expect("import");
    }

    #[test]
    fn csv_export_only_writes_csv_dialog_picked_paths() {
        let temp = TempDir::new().expect("tempdir");
        let target = temp.path().join("unpriced.csv");
        let target_str = target.to_string_lossy().to_string();

        let err = write_csv_to_picked_path(&target_str, "model\n").unwrap_err();
        assert!(err.to_string().contains("file dialog"), "{err}");
        assert!(!target.exists());

        // SQL 导出对话框选出的路径不能拿来写 CSV
        picked_paths::register(PickPurpose::SqlExport, &target_str);
        assert!(write_csv_to_picked_path(&target_str, "model\n").is_err());
        assert!(!target.exists());

        picked_paths::register(PickPurpose::CsvExport, &target_str);
        write_csv_to_picked_path(&target_str, "model\nfoo\n").expect("write csv");
        assert_eq!(
            std::fs::read_to_string(&target).expect("read csv"),
            "model\nfoo\n"
        );
    }

    #[test]
    fn import_rejects_path_not_chosen_by_open_dialog() {
        let temp = TempDir::new().expect("tempdir");
        let _paths = paths::scoped(Paths::with_home(temp.path()));
        let db = Database::memory().expect("memory db");
        let source = temp.path().join("attacker.sql");
        std::fs::write(&source, "-- not picked").expect("write");
        let source_str = source.to_string_lossy().to_string();

        let err = import_sql_from_picked_path(&db, &source_str).unwrap_err();
        assert!(err.to_string().contains("file dialog"), "{err}");

        picked_paths::register(PickPurpose::SqlExport, &source_str);
        let err = import_sql_from_picked_path(&db, &source_str).unwrap_err();
        assert!(err.to_string().contains("file dialog"), "{err}");
    }

    fn insert_detail_row(db: &Database, id: &str, days_ago: i64) {
        let created_at = chrono::Utc::now().timestamp() - days_ago * 86400;
        let conn = db.conn.lock().expect("lock conn");
        conn.execute(
            "INSERT INTO proxy_request_logs (
                request_id, provider_id, app_type, model,
                input_tokens, output_tokens, total_cost_usd,
                latency_ms, status_code, created_at
            ) VALUES (?1, 'p1', 'claude', 'claude-3', 10, 5, '0.01', 100, 200, ?2)",
            rusqlite::params![id, created_at],
        )
        .expect("insert detail row");
    }

    fn detail_rows(db: &Database) -> i64 {
        let conn = db.conn.lock().expect("lock conn");
        conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| {
            row.get(0)
        })
        .expect("count detail rows")
    }

    #[test]
    fn run_maintenance_uses_the_retention_policy_saved_in_settings() {
        let temp = TempDir::new().expect("tempdir");
        let _paths = paths::scoped(Paths::with_home(temp.path()));
        let _settings = crate::settings::scoped_settings_store();
        let db = Database::memory().expect("memory db");
        insert_detail_row(&db, "detail-40d", 40);

        // 默认只保留 30 天明细；保存 60 天后，40 天前的明细必须保留
        crate::settings::update_settings(crate::settings::AppSettings {
            usage_detail_retention_days: Some(60),
            ..crate::settings::get_settings()
        })
        .expect("save settings");
        let report = run_maintenance_with_current_policy(&db);
        assert_eq!(report.detail_rows_rolled_up, 0);
        assert_eq!(detail_rows(&db), 1);

        // 改为 20 天后再次运行，同一条明细被汇总并删除
        crate::settings::update_settings(crate::settings::AppSettings {
            usage_detail_retention_days: Some(20),
            ..crate::settings::get_settings()
        })
        .expect("save settings");
        let report = run_maintenance_with_current_policy(&db);
        assert_eq!(report.detail_rows_rolled_up, 1);
        assert_eq!(detail_rows(&db), 0);
    }
}
