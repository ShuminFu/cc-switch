//! Preset catalog commands (PRODUCT-07).

use crate::services::catalog::{self, CatalogResponse, CatalogStatus};

/// The preset overlay in effect (verified remote catalog or bundled snapshot).
#[tauri::command]
pub async fn get_preset_catalog() -> Result<CatalogResponse, String> {
    tauri::async_runtime::spawn_blocking(catalog::get_catalog)
        .await
        .map_err(|e| format!("Failed to load preset catalog: {e}"))
}

/// Check for a newer signed catalog (at most daily unless `force`).
#[tauri::command]
pub async fn refresh_preset_catalog(force: Option<bool>) -> Result<CatalogStatus, String> {
    Ok(catalog::refresh_catalog(force == Some(true)).await)
}

/// Turn the daily catalog check on or off.
#[tauri::command]
pub async fn set_preset_catalog_auto_update(enabled: bool) -> Result<CatalogStatus, String> {
    tauri::async_runtime::spawn_blocking(move || catalog::set_auto_update(enabled))
        .await
        .map_err(|e| format!("Failed to update catalog settings: {e}"))
}
