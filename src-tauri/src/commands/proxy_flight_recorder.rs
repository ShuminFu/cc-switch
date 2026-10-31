//! Recent proxy requests from the in-memory flight recorder (PROXY-21).
//! No bodies, headers or credentials are recorded.

use crate::proxy::flight_recorder::{self, RequestRecord};

/// The last requests handled by the proxy, newest first.
#[tauri::command]
pub async fn list_recent_proxy_requests() -> Result<Vec<RequestRecord>, String> {
    Ok(flight_recorder::global().recent())
}

/// The same records as JSON Lines (oldest first), for bug reports.
#[tauri::command]
pub async fn export_recent_proxy_requests() -> Result<String, String> {
    Ok(flight_recorder::global().export_jsonl())
}
