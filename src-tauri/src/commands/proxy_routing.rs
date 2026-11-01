//! Per-app proxy routing settings (PROXY-19): strategy, sticky sessions and
//! per-provider concurrency limits. Stored in the `settings` table; a running
//! proxy picks a change up within `proxy::routing::CONFIG_CACHE_TTL`.

use crate::app_config::AppType;
use crate::proxy::routing::{self, RoutingConfig};
use crate::store::AppState;
use std::str::FromStr;

fn app_key(app_type: &str) -> Result<String, String> {
    AppType::from_str(app_type)
        .map(|app| app.as_str().to_string())
        .map_err(|e| e.to_string())
}

/// Read one app's routing config (the default reproduces the priority list).
#[tauri::command]
pub async fn get_proxy_routing_config(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<RoutingConfig, String> {
    let app = app_key(&app_type)?;
    routing::load_routing_config(&state.db, &app).map_err(|e| e.to_string())
}

/// Store one app's routing config.
#[tauri::command]
pub async fn set_proxy_routing_config(
    state: tauri::State<'_, AppState>,
    app_type: String,
    config: RoutingConfig,
) -> Result<(), String> {
    let app = app_key(&app_type)?;
    routing::save_routing_config(&state.db, &app, &config).map_err(|e| e.to_string())
}
