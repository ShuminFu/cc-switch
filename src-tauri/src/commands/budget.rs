//! Budget hard-limit enforcement switch (PRODUCT-05).

use tauri::State;

use crate::error::AppError;
use crate::services::budget;
use crate::store::AppState;

/// Whether the proxy should block requests to providers over their budget limit.
#[tauri::command]
pub fn get_budget_enforcement(state: State<'_, AppState>) -> Result<bool, AppError> {
    budget::is_enforcement_enabled(&state.db)
}

/// Turn budget hard-limit enforcement on or off.
#[tauri::command]
pub fn set_budget_enforcement(state: State<'_, AppState>, enabled: bool) -> Result<bool, AppError> {
    budget::set_enforcement_enabled(&state.db, enabled)?;
    Ok(enabled)
}
