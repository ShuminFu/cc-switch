//! "Show in folder" for the database backup manager (FE-UI-37).

use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use crate::config;

fn backups_dir() -> std::path::PathBuf {
    config::get_app_config_dir().join("backups")
}

/// Open the directory that holds the database backups in the file manager.
#[tauri::command]
#[specta::specta]
pub async fn open_backups_folder(handle: AppHandle) -> Result<bool, String> {
    let dir = backups_dir();
    if !dir.exists() {
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create {}: {e}", dir.display()))?;
    }
    handle
        .opener()
        .open_path(dir.to_string_lossy().to_string(), None::<String>)
        .map_err(|e| format!("Failed to open {}: {e}", dir.display()))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    #[test]
    fn backups_dir_is_under_the_app_config_dir() {
        let dir = super::backups_dir();
        assert_eq!(dir.file_name().and_then(|n| n.to_str()), Some("backups"));
        assert_eq!(
            dir.parent(),
            Some(crate::config::get_app_config_dir().as_path())
        );
    }
}
