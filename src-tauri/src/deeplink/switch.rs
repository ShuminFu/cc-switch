//! `ccswitch://v1/switch?app=<app>&provider=<id|name>` — confirm-gated provider switch
//!
//! The link only names the target; the backend resolves it against the
//! provider list and the frontend asks the user before anything is written.

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepLinkSwitchRequest {
    /// Normalized app id (`claude`, `codex`, `gemini`, `grokbuild`, `claude-desktop`)
    pub app: String,
    /// Provider id or display name as written in the link
    pub provider: String,
}

/// Resolved against the database: what the confirmation dialog shows and
/// what the frontend switches to once confirmed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedSwitchRequest {
    pub app: String,
    pub provider_id: String,
    pub provider_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_provider_name: Option<String>,
    pub already_current: bool,
}

fn non_empty(params: &HashMap<String, String>, key: &str) -> Option<String> {
    params
        .get(key)
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

pub(super) fn parse_switch_params(
    params: &HashMap<String, String>,
) -> Result<DeepLinkSwitchRequest, AppError> {
    let app = non_empty(params, "app")
        .ok_or_else(|| AppError::InvalidInput("Missing 'app' parameter".to_string()))?;
    let app_type = AppType::from_str(&app)
        .map_err(|_| AppError::InvalidInput(format!("Unsupported app: {app}")))?;
    if app_type.is_additive_mode() {
        return Err(AppError::InvalidInput(format!(
            "{} has no current provider to switch",
            app_type.as_str()
        )));
    }
    let provider = non_empty(params, "provider")
        .ok_or_else(|| AppError::InvalidInput("Missing 'provider' parameter".to_string()))?;
    Ok(DeepLinkSwitchRequest {
        app: app_type.as_str().to_string(),
        provider,
    })
}

/// Match the link's `provider` against the app's providers: exact id first,
/// then case-insensitive display name. Also reports the current provider so
/// the dialog can show what the switch replaces.
pub fn resolve_switch_request(
    db: &Database,
    request: &DeepLinkSwitchRequest,
) -> Result<ResolvedSwitchRequest, AppError> {
    let app_type = AppType::from_str(&request.app)
        .map_err(|_| AppError::InvalidInput(format!("Unsupported app: {}", request.app)))?;
    let providers = db.get_all_providers(app_type.as_str())?;
    let wanted = request.provider.trim();
    let wanted_lower = wanted.to_lowercase();
    let matched = providers
        .get(wanted)
        .or_else(|| {
            providers
                .values()
                .find(|p| p.name.trim().to_lowercase() == wanted_lower)
        })
        .ok_or_else(|| {
            AppError::InvalidInput(format!(
                "No {} provider matches '{wanted}' (by id or name)",
                app_type.as_str()
            ))
        })?;
    let current_provider_id = crate::settings::get_effective_current_provider(db, &app_type)?;
    let current_provider_name = current_provider_id
        .as_ref()
        .and_then(|id| providers.get(id))
        .map(|p| p.name.clone());
    Ok(ResolvedSwitchRequest {
        app: app_type.as_str().to_string(),
        provider_id: matched.id.clone(),
        provider_name: matched.name.clone(),
        already_current: current_provider_id.as_deref() == Some(matched.id.as_str()),
        current_provider_id,
        current_provider_name,
    })
}
