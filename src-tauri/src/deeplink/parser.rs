//! Deep link URL parser
//!
//! Parses ccswitch:// URLs into DeepLinkImportRequest structures.

use super::utils::validate_url;
use super::DeepLinkImportRequest;
use crate::error::AppError;
use std::collections::HashMap;
use url::Url;

/// Parse a ccswitch:// URL into a DeepLinkImportRequest
///
/// Expected format:
/// ccswitch://v1/import?resource={type}&...
pub fn parse_deeplink_url(url_str: &str) -> Result<DeepLinkImportRequest, AppError> {
    // A managed policy can switch deep-link imports off entirely (PRODUCT-24).
    crate::features::policy::enforce_deeplink_allowed()?;

    // Reject oversized links before any parsing or decoding (SESS-33).
    if url_str.len() > MAX_URL_BYTES {
        return Err(AppError::InvalidInput(format!(
            "Deep link is too large ({} bytes, max {MAX_URL_BYTES})",
            url_str.len()
        )));
    }

    // Parse URL
    let url = Url::parse(url_str)
        .map_err(|e| AppError::InvalidInput(format!("Invalid deep link URL: {e}")))?;

    // Validate scheme
    let scheme = url.scheme();
    if scheme != "ccswitch" {
        return Err(AppError::InvalidInput(format!(
            "Invalid scheme: expected 'ccswitch', got '{scheme}'"
        )));
    }

    // Extract version from host
    let version = url
        .host_str()
        .ok_or_else(|| AppError::InvalidInput("Missing version in URL host".to_string()))?
        .to_string();

    // Validate version: v1 (flat query) and v2 (envelope). A higher
    // version means the link was made for a newer app (SESS-30).
    if version != "v1" && version != "v2" {
        let newer = version
            .strip_prefix('v')
            .and_then(|n| n.parse::<u32>().ok())
            .is_some_and(|n| n > 2);
        return Err(AppError::InvalidInput(if newer {
            format!(
                "This link uses deep-link protocol {version}, which needs a newer CC Switch. Please update CC Switch."
            )
        } else {
            format!("Unsupported protocol version: {version}")
        }));
    }

    // Extract path (should be "/import")
    let path = url.path();
    if path != "/import" {
        return Err(AppError::InvalidInput(format!(
            "Invalid path: expected '/import', got '{path}'"
        )));
    }

    // Parse query parameters
    let params: HashMap<String, String> = url.query_pairs().into_owned().collect();

    if version == "v2" {
        return super::v2::parse_v2(&params);
    }
    parse_params(&params, version)
}

/// Maximum accepted deep-link URL length.
pub const MAX_URL_BYTES: usize = 64 * 1024;

/// Apps a deep link may target, with accepted aliases (SESS-32).
const APP_IDS: &[&str] = &[
    "claude",
    "codex",
    "gemini",
    "grokbuild",
    "opencode",
    "openclaw",
    "hermes",
];

/// Normalize an app id: trims, lowercases and maps aliases (`grok` →
/// `grokbuild`). Shared by every resource type.
pub(crate) fn parse_app_id(raw: &str, field: &str) -> Result<String, AppError> {
    let normalized = raw.trim().to_ascii_lowercase();
    let canonical = match normalized.as_str() {
        "grok" => "grokbuild",
        other => other,
    };
    if APP_IDS.contains(&canonical) {
        Ok(canonical.to_string())
    } else {
        Err(AppError::InvalidInput(format!(
            "Invalid app in '{field}': must be one of {}, got '{raw}'",
            APP_IDS.join(", ")
        )))
    }
}

/// Parse one resource from v1-style parameters (also used for v2 items).
pub(crate) fn parse_params(
    params: &HashMap<String, String>,
    version: String,
) -> Result<DeepLinkImportRequest, AppError> {
    // Extract and validate resource type
    let resource = params
        .get("resource")
        .ok_or_else(|| AppError::InvalidInput("Missing 'resource' parameter".to_string()))?
        .clone();

    // Dispatch to appropriate parser based on resource type
    match resource.as_str() {
        "provider" => parse_provider_deeplink(params, version, resource),
        "prompt" => parse_prompt_deeplink(params, version, resource),
        "mcp" => parse_mcp_deeplink(params, version, resource),
        "skill" => parse_skill_deeplink(params, version, resource),
        _ => Err(AppError::InvalidInput(format!(
            "Unsupported resource type: {resource}"
        ))),
    }
}

/// Parse provider deep link parameters
fn parse_provider_deeplink(
    params: &HashMap<String, String>,
    version: String,
    resource: String,
) -> Result<DeepLinkImportRequest, AppError> {
    let app = params
        .get("app")
        .ok_or_else(|| AppError::InvalidInput("Missing 'app' parameter".to_string()))?;
    let app = parse_app_id(app, "app")?;

    let name = params
        .get("name")
        .ok_or_else(|| AppError::InvalidInput("Missing 'name' parameter".to_string()))?
        .clone();

    // Make these optional for config file auto-fill (v3.8+)
    let homepage = params.get("homepage").cloned();
    let endpoint = params.get("endpoint").cloned();
    let api_key = params.get("apiKey").cloned();

    // Validate URLs only if provided
    if let Some(ref hp) = homepage {
        if !hp.is_empty() {
            validate_url(hp, "homepage")?;
        }
    }
    // Validate each endpoint (supports comma-separated multiple URLs)
    if let Some(ref ep) = endpoint {
        for (i, url) in ep.split(',').enumerate() {
            let trimmed = url.trim();
            if !trimmed.is_empty() {
                validate_url(trimmed, &format!("endpoint[{i}]"))?;
            }
        }
    }

    // Extract optional fields
    let model = params.get("model").cloned();
    let notes = params.get("notes").cloned();
    let haiku_model = params.get("haikuModel").cloned();
    let sonnet_model = params.get("sonnetModel").cloned();
    let opus_model = params.get("opusModel").cloned();
    let icon = params
        .get("icon")
        .map(|v| v.trim().to_lowercase())
        .filter(|v| !v.is_empty());
    let config = params.get("config").cloned();
    let config_format = params.get("configFormat").cloned();
    // Remote config URLs are not supported (SESS-44): fail clearly instead of
    // previewing a link whose import can only fail.
    if params.contains_key("configUrl") && config.is_none() {
        return Err(AppError::InvalidInput(
            "configUrl links are not supported. Ask the link author for a link with an inline 'config' parameter.".to_string(),
        ));
    }
    let config_url = None;
    let enabled = params.get("enabled").and_then(|v| v.parse::<bool>().ok());

    // Extract usage script fields (v3.9+)
    let usage_enabled = params
        .get("usageEnabled")
        .and_then(|v| v.parse::<bool>().ok());
    let usage_script = params.get("usageScript").cloned();
    let usage_api_key = params.get("usageApiKey").cloned();
    let usage_base_url = params.get("usageBaseUrl").cloned();
    let usage_access_token = params.get("usageAccessToken").cloned();
    let usage_user_id = params.get("usageUserId").cloned();
    let usage_auto_interval = params
        .get("usageAutoInterval")
        .and_then(|v| v.parse::<u64>().ok());

    Ok(DeepLinkImportRequest {
        version,
        resource,
        app: Some(app),
        name: Some(name),
        enabled,
        homepage,
        endpoint,
        api_key,
        icon,
        model,
        notes,
        haiku_model,
        sonnet_model,
        opus_model,
        content: None,
        description: None,
        apps: None,
        repo: None,
        directory: None,
        branch: None,
        config,
        config_format,
        config_url,
        usage_enabled,
        usage_script,
        usage_api_key,
        usage_base_url,
        usage_access_token,
        usage_user_id,
        usage_auto_interval,
        nonce: None,
        bundle: None,
    })
}

/// Parse prompt deep link parameters
fn parse_prompt_deeplink(
    params: &HashMap<String, String>,
    version: String,
    resource: String,
) -> Result<DeepLinkImportRequest, AppError> {
    let app = params
        .get("app")
        .ok_or_else(|| AppError::InvalidInput("Missing 'app' parameter for prompt".to_string()))?;
    let app = parse_app_id(app, "app")?;

    let name = params
        .get("name")
        .ok_or_else(|| AppError::InvalidInput("Missing 'name' parameter for prompt".to_string()))?
        .clone();

    let content = params
        .get("content")
        .ok_or_else(|| {
            AppError::InvalidInput("Missing 'content' parameter for prompt".to_string())
        })?
        .clone();

    let description = params.get("description").cloned();
    let enabled = params.get("enabled").and_then(|v| v.parse::<bool>().ok());

    Ok(DeepLinkImportRequest {
        version,
        resource,
        app: Some(app),
        name: Some(name),
        enabled,
        content: Some(content),
        description,
        icon: None,
        homepage: None,
        endpoint: None,
        api_key: None,
        model: None,
        notes: None,
        haiku_model: None,
        sonnet_model: None,
        opus_model: None,
        apps: None,
        repo: None,
        directory: None,
        branch: None,
        config: None,
        config_format: None,
        config_url: None,
        usage_enabled: None,
        usage_script: None,
        usage_api_key: None,
        usage_base_url: None,
        usage_access_token: None,
        usage_user_id: None,
        usage_auto_interval: None,
        nonce: None,
        bundle: None,
    })
}

/// Parse MCP deep link parameters
fn parse_mcp_deeplink(
    params: &HashMap<String, String>,
    version: String,
    resource: String,
) -> Result<DeepLinkImportRequest, AppError> {
    let apps = params
        .get("apps")
        .ok_or_else(|| AppError::InvalidInput("Missing 'apps' parameter for MCP".to_string()))?;

    // Validate and normalize every app (aliases map to canonical ids).
    let apps = apps
        .split(',')
        .map(|app| parse_app_id(app, "apps"))
        .collect::<Result<Vec<_>, _>>()?
        .join(",");

    let config = params
        .get("config")
        .ok_or_else(|| AppError::InvalidInput("Missing 'config' parameter for MCP".to_string()))?
        .clone();

    let enabled = params.get("enabled").and_then(|v| v.parse::<bool>().ok());

    Ok(DeepLinkImportRequest {
        version,
        resource,
        apps: Some(apps),
        enabled,
        config: Some(config),
        config_format: Some("json".to_string()), // MCP config is always JSON
        app: None,
        name: None,
        icon: None,
        homepage: None,
        endpoint: None,
        api_key: None,
        model: None,
        notes: None,
        haiku_model: None,
        sonnet_model: None,
        opus_model: None,
        content: None,
        description: None,
        repo: None,
        directory: None,
        branch: None,
        config_url: None,
        usage_enabled: None,
        usage_script: None,
        usage_api_key: None,
        usage_base_url: None,
        usage_access_token: None,
        usage_user_id: None,
        usage_auto_interval: None,
        nonce: None,
        bundle: None,
    })
}

/// Parse skill deep link parameters
fn parse_skill_deeplink(
    params: &HashMap<String, String>,
    version: String,
    resource: String,
) -> Result<DeepLinkImportRequest, AppError> {
    let repo = params
        .get("repo")
        .ok_or_else(|| AppError::InvalidInput("Missing 'repo' parameter for skill".to_string()))?
        .clone();

    // Validate repo format (should be "owner/name") and each part (SESS-22)
    let parts: Vec<&str> = repo.split('/').collect();
    if parts.len() != 2 || !parts.iter().all(|part| is_valid_repo_part(part)) {
        return Err(AppError::InvalidInput(format!(
            "Invalid repo format: expected 'owner/name' (letters, digits, '.', '_' or '-'), got '{repo}'"
        )));
    }

    let branch = params.get("branch").cloned();
    if let Some(branch) = branch.as_deref() {
        if !is_valid_branch(branch) {
            return Err(AppError::InvalidInput(format!(
                "Invalid branch name: '{branch}'"
            )));
        }
    }
    // `directory` was never honoured by the importer (a skill link adds the
    // repository as a skill source); it is ignored instead of being shown.
    let directory = None;

    Ok(DeepLinkImportRequest {
        version,
        resource,
        repo: Some(repo),
        directory,
        branch,
        icon: None,
        app: Some("claude".to_string()), // Skills are Claude-only
        name: None,
        enabled: None,
        homepage: None,
        endpoint: None,
        api_key: None,
        model: None,
        notes: None,
        haiku_model: None,
        sonnet_model: None,
        opus_model: None,
        content: None,
        description: None,
        apps: None,
        config: None,
        config_format: None,
        config_url: None,
        usage_enabled: None,
        usage_script: None,
        usage_api_key: None,
        usage_base_url: None,
        usage_access_token: None,
        usage_user_id: None,
        usage_auto_interval: None,
        nonce: None,
        bundle: None,
    })
}

/// GitHub owner / repository name: 1-100 of `[A-Za-z0-9_.-]`, not `.`/`..`.
fn is_valid_repo_part(part: &str) -> bool {
    !part.is_empty()
        && part.len() <= 100
        && part != "."
        && part != ".."
        && part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

/// Git branch: 1-255 of `[A-Za-z0-9._/-]`, no `..`, no leading `/` or `-`.
fn is_valid_branch(branch: &str) -> bool {
    !branch.is_empty()
        && branch.len() <= 255
        && !branch.contains("..")
        && !branch.starts_with(['/', '-'])
        && !branch.ends_with('/')
        && branch
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | '/'))
}
