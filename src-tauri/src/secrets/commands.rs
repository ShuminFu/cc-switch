//! Tauri commands for the provider API-key vault (DB-02 / PRODUCT-01).

use tauri::State;

use super::provider_vault_admin::{self, VaultStatus};
use crate::error::AppError;
use crate::store::AppState;

/// Vault state on this device: setting, keychain availability and counts.
#[tauri::command]
pub async fn get_provider_secret_vault_status(
    state: State<'_, AppState>,
) -> Result<VaultStatus, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || provider_vault_admin::status(&db))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e: AppError| e.to_string())
}

/// Switch the vault on (refused without a working keychain) or off, and
/// migrate the stored provider rows accordingly.
#[tauri::command]
pub async fn set_provider_secret_vault(
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<VaultStatus, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || provider_vault_admin::set_enabled(&db, enabled))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e: AppError| e.to_string())
}
