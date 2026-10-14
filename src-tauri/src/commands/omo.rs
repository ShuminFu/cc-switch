use tauri::{AppHandle, State};

use crate::commands::blocking::with_state_serialized;
use crate::services::omo::{OmoLocalFileData, OmoVariant, SLIM, STANDARD};
use crate::services::OmoService;
use crate::store::AppState;

#[tauri::command]
pub async fn read_omo_local_file() -> Result<OmoLocalFileData, String> {
    OmoService::read_local_file(&STANDARD).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_current_omo_provider_id(state: State<'_, AppState>) -> Result<String, String> {
    let provider = state
        .db
        .get_current_omo_provider("opencode", "omo")
        .map_err(|e| e.to_string())?;
    Ok(provider.map(|p| p.id).unwrap_or_default())
}

/// Clears the current OMO pointer and deletes its config file; serialized
/// with provider switches that rewrite the same file.
#[tauri::command]
pub async fn disable_current_omo(handle: AppHandle) -> Result<(), String> {
    with_state_serialized(handle, |state| {
        disable_current_omo_variant(state, "omo", &STANDARD)
    })
    .await
}

// ── OMO Slim commands ───────────────────────────────────────

#[tauri::command]
pub async fn read_omo_slim_local_file() -> Result<OmoLocalFileData, String> {
    OmoService::read_local_file(&SLIM).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_current_omo_slim_provider_id(
    state: State<'_, AppState>,
) -> Result<String, String> {
    let provider = state
        .db
        .get_current_omo_provider("opencode", "omo-slim")
        .map_err(|e| e.to_string())?;
    Ok(provider.map(|p| p.id).unwrap_or_default())
}

/// Clears the current OMO pointer and deletes its config file; serialized
/// with provider switches that rewrite the same file.
#[tauri::command]
pub async fn disable_current_omo_slim(handle: AppHandle) -> Result<(), String> {
    with_state_serialized(handle, |state| {
        disable_current_omo_variant(state, "omo-slim", &SLIM)
    })
    .await
}

fn disable_current_omo_variant(
    state: &AppState,
    category: &str,
    variant: &OmoVariant,
) -> Result<(), String> {
    let providers = state
        .db
        .get_all_providers("opencode")
        .map_err(|e| e.to_string())?;
    for (id, p) in &providers {
        if p.category.as_deref() == Some(category) {
            state
                .db
                .clear_omo_provider_current("opencode", id, category)
                .map_err(|e| e.to_string())?;
        }
    }
    OmoService::delete_config_file(variant).map_err(|e| e.to_string())?;
    Ok(())
}
