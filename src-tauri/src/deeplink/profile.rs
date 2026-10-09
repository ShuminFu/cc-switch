//! `ccswitch://v1/apply?profile=<id|name>&scope=<app>` — confirm-gated project profile apply

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::services::profile::ProfileScope;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepLinkProfileRequest {
    /// Profile id or name as written in the link
    pub profile: String,
    /// Normalized scope (`claude`, `claude-desktop`, `codex`, `gemini`, `grokbuild`)
    pub scope: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedProfileRequest {
    pub profile_id: String,
    pub profile_name: String,
    pub scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_profile_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_profile_name: Option<String>,
    pub already_current: bool,
}

fn non_empty(params: &HashMap<String, String>, key: &str) -> Option<String> {
    params
        .get(key)
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

pub(super) fn parse_profile_params(
    params: &HashMap<String, String>,
) -> Result<DeepLinkProfileRequest, AppError> {
    let profile = non_empty(params, "profile")
        .ok_or_else(|| AppError::InvalidInput("Missing 'profile' parameter".to_string()))?;
    let scope_raw = non_empty(params, "scope")
        .or_else(|| non_empty(params, "app"))
        .ok_or_else(|| AppError::InvalidInput("Missing 'scope' parameter".to_string()))?;
    let app_type = AppType::from_str(&scope_raw)
        .map_err(|_| AppError::InvalidInput(format!("Unsupported scope: {scope_raw}")))?;
    let scope = ProfileScope::for_app(&app_type).ok_or_else(|| {
        AppError::InvalidInput(format!(
            "{} does not support project profiles",
            app_type.as_str()
        ))
    })?;
    Ok(DeepLinkProfileRequest {
        profile,
        scope: scope.as_str().to_string(),
    })
}

/// Exact id first, then case-insensitive name; reports the scope's current profile.
pub fn resolve_profile_request(
    db: &Database,
    request: &DeepLinkProfileRequest,
) -> Result<ResolvedProfileRequest, AppError> {
    let scope = ProfileScope::parse(&request.scope)?;
    let profiles = db.get_all_profiles()?;
    let wanted = request.profile.trim();
    let wanted_lower = wanted.to_lowercase();
    let matched = profiles
        .iter()
        .find(|p| p.id == wanted)
        .or_else(|| {
            profiles
                .iter()
                .find(|p| p.name.trim().to_lowercase() == wanted_lower)
        })
        .ok_or_else(|| {
            AppError::InvalidInput(format!(
                "No project profile matches '{wanted}' (by id or name)"
            ))
        })?;
    let current_profile_id = db.get_current_profile_id(scope.as_str())?;
    let current_profile_name = current_profile_id
        .as_ref()
        .and_then(|id| profiles.iter().find(|p| &p.id == id))
        .map(|p| p.name.clone());
    Ok(ResolvedProfileRequest {
        profile_id: matched.id.clone(),
        profile_name: matched.name.clone(),
        scope: scope.as_str().to_string(),
        already_current: current_profile_id.as_deref() == Some(matched.id.as_str()),
        current_profile_id,
        current_profile_name,
    })
}
