use super::{run_blocking, run_blocking_with_state};
use std::collections::HashMap;

use crate::openclaw_config;

// ============================================================================
// OpenClaw Provider Commands (migrated from provider.rs)
// ============================================================================

/// Import providers from OpenClaw live config to database.
///
/// OpenClaw uses additive mode — users may already have providers
/// configured in openclaw.json.
#[tauri::command]
pub async fn import_openclaw_providers_from_live(
    handle: tauri::AppHandle,
) -> Result<usize, String> {
    run_blocking_with_state(handle, move |state| {
        crate::services::provider::import_openclaw_providers_from_live(state)
            .map_err(|e| e.to_string())
    })
    .await
}

/// Get provider IDs in the OpenClaw live config.
#[tauri::command]
pub async fn get_openclaw_live_provider_ids() -> Result<Vec<String>, String> {
    run_blocking(move || {
        openclaw_config::get_providers()
            .map(|providers| providers.keys().cloned().collect())
            .map_err(|e| e.to_string())
    })
    .await
}

/// Get a single OpenClaw provider fragment from live config.
#[tauri::command]
pub async fn get_openclaw_live_provider(
    #[allow(non_snake_case)] providerId: String,
) -> Result<Option<serde_json::Value>, String> {
    run_blocking(move || openclaw_config::get_provider(&providerId).map_err(|e| e.to_string()))
        .await
}

/// Scan openclaw.json for known configuration hazards.
#[tauri::command]
pub async fn scan_openclaw_config_health(
) -> Result<Vec<openclaw_config::OpenClawHealthWarning>, String> {
    run_blocking(move || openclaw_config::scan_openclaw_config_health().map_err(|e| e.to_string()))
        .await
}

// ============================================================================
// Agents Configuration Commands
// ============================================================================

/// Get OpenClaw default model config (agents.defaults.model)
#[tauri::command]
pub async fn get_openclaw_default_model(
) -> Result<Option<openclaw_config::OpenClawDefaultModel>, String> {
    run_blocking(move || openclaw_config::get_default_model().map_err(|e| e.to_string())).await
}

/// Set OpenClaw default model config (agents.defaults.model)
#[tauri::command]
pub async fn set_openclaw_default_model(
    model: openclaw_config::OpenClawDefaultModel,
) -> Result<openclaw_config::OpenClawWriteOutcome, String> {
    run_blocking(move || openclaw_config::set_default_model(&model).map_err(|e| e.to_string()))
        .await
}

/// Get OpenClaw model catalog/allowlist (agents.defaults.models)
#[tauri::command]
pub async fn get_openclaw_model_catalog(
) -> Result<Option<HashMap<String, openclaw_config::OpenClawModelCatalogEntry>>, String> {
    run_blocking(move || openclaw_config::get_model_catalog().map_err(|e| e.to_string())).await
}

/// Set OpenClaw model catalog/allowlist (agents.defaults.models)
#[tauri::command]
pub async fn set_openclaw_model_catalog(
    catalog: HashMap<String, openclaw_config::OpenClawModelCatalogEntry>,
) -> Result<openclaw_config::OpenClawWriteOutcome, String> {
    run_blocking(move || openclaw_config::set_model_catalog(&catalog).map_err(|e| e.to_string()))
        .await
}

/// Get full agents.defaults config (all fields)
#[tauri::command]
pub async fn get_openclaw_agents_defaults(
) -> Result<Option<openclaw_config::OpenClawAgentsDefaults>, String> {
    run_blocking(move || openclaw_config::get_agents_defaults().map_err(|e| e.to_string())).await
}

/// Set full agents.defaults config (all fields)
#[tauri::command]
pub async fn set_openclaw_agents_defaults(
    defaults: openclaw_config::OpenClawAgentsDefaults,
) -> Result<openclaw_config::OpenClawWriteOutcome, String> {
    run_blocking(move || openclaw_config::set_agents_defaults(&defaults).map_err(|e| e.to_string()))
        .await
}

// ============================================================================
// Env Configuration Commands
// ============================================================================

/// Get OpenClaw env config (env section of openclaw.json)
#[tauri::command]
pub async fn get_openclaw_env() -> Result<openclaw_config::OpenClawEnvConfig, String> {
    run_blocking(move || openclaw_config::get_env_config().map_err(|e| e.to_string())).await
}

/// Set OpenClaw env config (env section of openclaw.json)
#[tauri::command]
pub async fn set_openclaw_env(
    env: openclaw_config::OpenClawEnvConfig,
) -> Result<openclaw_config::OpenClawWriteOutcome, String> {
    run_blocking(move || openclaw_config::set_env_config(&env).map_err(|e| e.to_string())).await
}

// ============================================================================
// Tools Configuration Commands
// ============================================================================

/// Get OpenClaw tools config (tools section of openclaw.json)
#[tauri::command]
pub async fn get_openclaw_tools() -> Result<openclaw_config::OpenClawToolsConfig, String> {
    run_blocking(move || openclaw_config::get_tools_config().map_err(|e| e.to_string())).await
}

/// Set OpenClaw tools config (tools section of openclaw.json)
#[tauri::command]
pub async fn set_openclaw_tools(
    tools: openclaw_config::OpenClawToolsConfig,
) -> Result<openclaw_config::OpenClawWriteOutcome, String> {
    run_blocking(move || openclaw_config::set_tools_config(&tools).map_err(|e| e.to_string())).await
}
