use super::run_blocking;
use crate::services::env_checker::{check_env_conflicts as check_conflicts, EnvConflict};
use crate::services::env_manager::{
    delete_env_vars as delete_vars, restore_from_backup, BackupInfo,
};

/// Check environment variable conflicts for a specific app
#[tauri::command]
pub async fn check_env_conflicts(app: String) -> Result<Vec<EnvConflict>, String> {
    run_blocking(move || check_conflicts(&app)).await
}

/// Delete environment variables with backup
#[tauri::command]
pub async fn delete_env_vars(conflicts: Vec<EnvConflict>) -> Result<BackupInfo, String> {
    run_blocking(move || delete_vars(conflicts)).await
}

/// Restore environment variables from backup file
#[tauri::command]
#[specta::specta]
pub async fn restore_env_backup(backup_path: String) -> Result<(), String> {
    run_blocking(move || restore_from_backup(backup_path)).await
}
