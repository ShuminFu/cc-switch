//! MCP server import from deep link
//!
//! Handles batch import of MCP server configurations via ccswitch:// URLs.

use super::utils::decode_base64_param;
use super::DeepLinkImportRequest;
use crate::app_config::{McpApps, McpServer};
use crate::error::AppError;
use crate::services::McpService;
use crate::store::AppState;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// MCP import result
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpImportResult {
    /// Number of successfully imported MCP servers
    pub imported_count: usize,
    /// IDs of successfully imported MCP servers
    pub imported_ids: Vec<String>,
    /// Failed imports with error messages
    pub failed: Vec<McpImportError>,
}

/// MCP import error
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpImportError {
    /// MCP server ID
    pub id: String,
    /// Error message
    pub error: String,
}

/// Import MCP servers from deep link request
///
/// This function handles batch import of MCP servers from standard MCP JSON format.
/// If a server already exists, only the apps flags are merged (existing config preserved).
///
/// Trust model: an MCP stdio server is arbitrary command execution inside the
/// user's coding tools, so live tool configs are only touched when the request
/// carries `enabled == Some(true)` (set by the confirmation dialog after an
/// explicit opt-in). Otherwise new servers are stored in the CC Switch database
/// with every app flag off, and existing servers keep their current app flags.
pub fn import_mcp_from_deeplink(
    state: &AppState,
    request: DeepLinkImportRequest,
) -> Result<McpImportResult, AppError> {
    // Verify this is an MCP request
    if request.resource != "mcp" {
        return Err(AppError::InvalidInput(format!(
            "Expected mcp resource, got '{}'",
            request.resource
        )));
    }

    // Extract and validate apps parameter
    let apps_str = request
        .apps
        .as_ref()
        .ok_or_else(|| AppError::InvalidInput("Missing 'apps' parameter for MCP".to_string()))?;

    // Parse apps into McpApps struct
    let target_apps = parse_mcp_apps(apps_str)?;

    // Extract config
    let config_b64 = request
        .config
        .as_ref()
        .ok_or_else(|| AppError::InvalidInput("Missing 'config' parameter for MCP".to_string()))?;

    // Decode Base64 config
    let decoded = decode_base64_param("config", config_b64)?;

    let config_str = String::from_utf8(decoded)
        .map_err(|e| AppError::InvalidInput(format!("Invalid UTF-8 in config: {e}")))?;

    // Parse JSON
    let config_json: Value = serde_json::from_str(&config_str)
        .map_err(|e| AppError::InvalidInput(format!("Invalid JSON in MCP config: {e}")))?;

    // Extract mcpServers object
    let mcp_servers = config_json
        .get("mcpServers")
        .and_then(|v| v.as_object())
        .ok_or_else(|| {
            AppError::InvalidInput("MCP config must contain 'mcpServers' object".to_string())
        })?;

    if mcp_servers.is_empty() {
        return Err(AppError::InvalidInput(
            "No MCP servers found in config".to_string(),
        ));
    }
    if mcp_servers.len() > MAX_DEEPLINK_SERVERS {
        return Err(AppError::InvalidInput(format!(
            "Too many MCP servers in one link (max {MAX_DEEPLINK_SERVERS})"
        )));
    }

    // Only write into live tool configs after an explicit opt-in.
    let enable_now = request.enabled == Some(true);

    // Get existing servers to check for duplicates
    let existing_servers = state.db.get_all_mcp_servers()?;

    // Import each MCP server
    let mut imported_ids = Vec::new();
    let mut failed = Vec::new();

    for (id, server_spec) in mcp_servers.iter() {
        if let Err(e) = validate_deeplink_server(id, server_spec) {
            log::warn!("Rejected MCP server '{id}' from deep link: {e}");
            failed.push(McpImportError {
                id: id.clone(),
                error: format!("{e}"),
            });
            continue;
        }

        // Check if server already exists
        let server = if let Some(existing) = existing_servers.get(id) {
            // Server exists - merge apps only, keep other fields unchanged
            log::info!("MCP server '{id}' already exists, merging apps only");

            let merged_apps = if enable_now {
                merge_mcp_apps(&existing.apps, &target_apps)
            } else {
                existing.apps.clone()
            };

            McpServer {
                id: existing.id.clone(),
                name: existing.name.clone(),
                server: existing.server.clone(), // Keep existing server config
                apps: merged_apps,               // Merged apps
                description: existing.description.clone(),
                homepage: existing.homepage.clone(),
                docs: existing.docs.clone(),
                tags: existing.tags.clone(),
            }
        } else {
            // New server - create with provided config
            log::info!("Creating new MCP server: {id}");
            McpServer {
                id: id.clone(),
                name: id.clone(),
                server: server_spec.clone(),
                apps: if enable_now {
                    target_apps.clone()
                } else {
                    McpApps::default()
                },
                description: None,
                homepage: None,
                docs: None,
                tags: vec!["imported".to_string()],
            }
        };

        match McpService::upsert_server(state, server) {
            Ok(_) => {
                imported_ids.push(id.clone());
                log::info!("Successfully imported/updated MCP server: {id}");
            }
            Err(e) => {
                failed.push(McpImportError {
                    id: id.clone(),
                    error: format!("{e}"),
                });
                log::warn!("Failed to import MCP server '{id}': {e}");
            }
        }
    }

    Ok(McpImportResult {
        imported_count: imported_ids.len(),
        imported_ids,
        failed,
    })
}

/// Maximum number of MCP servers one deep link may import (SESS-33).
pub(crate) const MAX_DEEPLINK_SERVERS: usize = 50;

/// Maximum accepted length of an MCP server id coming from a deep link.
const MAX_SERVER_ID_LEN: usize = 128;

/// Validate one server entry from a deep link payload before anything is stored.
pub(crate) fn validate_deeplink_server(id: &str, spec: &Value) -> Result<(), AppError> {
    if id.trim().is_empty()
        || id.len() > MAX_SERVER_ID_LEN
        || id.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err(AppError::InvalidInput(format!(
            "Invalid MCP server id '{id}': must be 1-{MAX_SERVER_ID_LEN} characters without whitespace"
        )));
    }

    crate::mcp::validate_server_spec(spec)?;

    // On Windows cc-switch rewrites npx/node/... to `cmd /c ...`, where unquoted
    // `& | < > ^` in an argument would be interpreted by cmd.exe. Refuse such
    // specs from untrusted deep links instead of silently changing their meaning.
    if cfg!(windows) {
        if let Some(arg) = crate::mcp::find_cmd_wrap_unsafe_arg(spec) {
            return Err(AppError::InvalidInput(format!(
                "MCP server '{id}' argument '{arg}' contains cmd.exe metacharacters (& | < > ^) and cannot be wrapped safely with 'cmd /c' on Windows"
            )));
        }
    }

    Ok(())
}

/// Parse apps string into McpApps struct
pub(crate) fn parse_mcp_apps(apps_str: &str) -> Result<McpApps, AppError> {
    let mut apps = McpApps {
        claude: false,
        codex: false,
        gemini: false,
        grokbuild: false,
        opencode: false,
        hermes: false,
    };

    for app in apps_str.split(',') {
        match app.trim() {
            "claude" => apps.claude = true,
            "codex" => apps.codex = true,
            "gemini" => apps.gemini = true,
            "grokbuild" | "grok" => apps.grokbuild = true,
            "opencode" => apps.opencode = true,
            "openclaw" => {
                // OpenClaw doesn't support MCP, ignore silently
                log::debug!("OpenClaw doesn't support MCP, ignoring in apps parameter");
            }
            "hermes" => apps.hermes = true,
            other => {
                return Err(AppError::InvalidInput(format!(
                    "Invalid app in 'apps': {other}"
                )))
            }
        }
    }

    if apps.is_empty() {
        return Err(AppError::InvalidInput(
            "At least one app must be specified in 'apps'".to_string(),
        ));
    }

    Ok(apps)
}

fn merge_mcp_apps(existing: &McpApps, target: &McpApps) -> McpApps {
    let mut merged = existing.clone();
    for app in target.enabled_apps() {
        merged.set_enabled_for(&app, true);
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enabled_apps_merge_covers_every_supported_mcp_client() {
        let existing = McpApps {
            claude: true,
            ..McpApps::default()
        };
        let target = McpApps {
            codex: true,
            gemini: true,
            grokbuild: true,
            opencode: true,
            hermes: true,
            ..McpApps::default()
        };
        let merged = merge_mcp_apps(&existing, &target);

        assert!(merged.claude);
        assert!(merged.codex);
        assert!(merged.gemini);
        assert!(merged.grokbuild);
        assert!(merged.opencode);
        assert!(merged.hermes);
    }
}
