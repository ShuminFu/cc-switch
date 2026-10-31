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

/// Render a provider as shell environment variables (SVC-35): `shell` is
/// `bash`, `fish`, `powershell` or `dotenv`.
#[tauri::command]
pub async fn render_provider_env_profile(
    state: tauri::State<'_, crate::store::AppState>,
    app: String,
    provider_id: String,
    shell: crate::services::env_profile::EnvShell,
) -> Result<String, String> {
    use std::str::FromStr;
    let app_type = crate::app_config::AppType::from_str(&app).map_err(|e| e.to_string())?;
    let provider = state
        .db
        .get_provider_by_id(&provider_id, app_type.as_str())
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Provider not found: {provider_id}"))?;
    crate::services::env_profile::render(&app_type, &provider, shell).map_err(|e| e.to_string())
}
