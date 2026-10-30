use indexmap::IndexMap;
use std::str::FromStr;

use tauri::State;

use crate::app_config::AppType;
use crate::error::IpcError;
use crate::prompt::Prompt;
use crate::services::PromptService;
use crate::store::AppState;

#[tauri::command]
pub async fn get_prompts(
    app: String,
    state: State<'_, AppState>,
) -> Result<IndexMap<String, Prompt>, IpcError> {
    let app_type = AppType::from_str(&app).map_err(IpcError::from)?;
    PromptService::get_prompts(&state, app_type).map_err(IpcError::from)
}

#[tauri::command]
pub async fn upsert_prompt(
    app: String,
    id: String,
    prompt: Prompt,
    state: State<'_, AppState>,
) -> Result<(), IpcError> {
    let app_type = AppType::from_str(&app).map_err(IpcError::from)?;
    PromptService::upsert_prompt(&state, app_type, &id, prompt).map_err(IpcError::from)
}

#[tauri::command]
pub async fn delete_prompt(
    app: String,
    id: String,
    state: State<'_, AppState>,
) -> Result<(), IpcError> {
    let app_type = AppType::from_str(&app).map_err(IpcError::from)?;
    PromptService::delete_prompt(&state, app_type, &id).map_err(IpcError::from)
}

#[tauri::command]
pub async fn enable_prompt(
    app: String,
    id: String,
    state: State<'_, AppState>,
) -> Result<(), IpcError> {
    let app_type = AppType::from_str(&app).map_err(IpcError::from)?;
    PromptService::enable_prompt(&state, app_type, &id).map_err(IpcError::from)
}

#[tauri::command]
pub async fn import_prompt_from_file(
    app: String,
    state: State<'_, AppState>,
) -> Result<String, IpcError> {
    let app_type = AppType::from_str(&app).map_err(IpcError::from)?;
    PromptService::import_from_file(&state, app_type).map_err(IpcError::from)
}

#[tauri::command]
pub async fn get_current_prompt_file_content(app: String) -> Result<Option<String>, IpcError> {
    let app_type = AppType::from_str(&app).map_err(IpcError::from)?;
    PromptService::get_current_file_content(app_type).map_err(IpcError::from)
}
