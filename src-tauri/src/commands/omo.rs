use tauri::State;

use crate::error::IpcError;
use crate::services::omo::{OmoLocalFileData, SLIM, STANDARD};
use crate::services::OmoService;
use crate::store::AppState;

#[tauri::command]
pub async fn read_omo_local_file() -> Result<OmoLocalFileData, IpcError> {
    OmoService::read_local_file(&STANDARD).map_err(IpcError::from)
}

#[tauri::command]
pub async fn get_current_omo_provider_id(state: State<'_, AppState>) -> Result<String, IpcError> {
    OmoService::current_provider_id(&state, &STANDARD).map_err(IpcError::from)
}

#[tauri::command]
pub async fn disable_current_omo(state: State<'_, AppState>) -> Result<(), IpcError> {
    OmoService::disable_current(&state, &STANDARD).map_err(IpcError::from)
}

// ── OMO Slim commands ───────────────────────────────────────

#[tauri::command]
pub async fn read_omo_slim_local_file() -> Result<OmoLocalFileData, IpcError> {
    OmoService::read_local_file(&SLIM).map_err(IpcError::from)
}

#[tauri::command]
pub async fn get_current_omo_slim_provider_id(
    state: State<'_, AppState>,
) -> Result<String, IpcError> {
    OmoService::current_provider_id(&state, &SLIM).map_err(IpcError::from)
}

#[tauri::command]
pub async fn disable_current_omo_slim(state: State<'_, AppState>) -> Result<(), IpcError> {
    OmoService::disable_current(&state, &SLIM).map_err(IpcError::from)
}
