//! OpenClaw MCP sync and import module
//!
//! Handles conversion between CC Switch unified MCP format and the `mcp.servers`
//! section of `~/.openclaw/openclaw.json` (JSON5).
//!
//! ## Format mapping
//!
//! | CC Switch unified (JSON)                                  | OpenClaw `mcp.servers.<id>`              |
//! |-----------------------------------------------------------|------------------------------------------|
//! | `{"type":"stdio","command":"npx","args":[...],"env":{}}`  | `{command, args, env, cwd}`              |
//! | `{"type":"sse","url":"...","headers":{}}`                 | `{url, headers, transport: "sse"}`       |
//! | `{"type":"http","url":"...","headers":{}}`                | `{url, headers, transport: "streamable-http"}` |
//!
//! Key differences from Claude format:
//! - OpenClaw has NO `type` field -- stdio is inferred from `command`, remote
//!   servers from `url` plus `transport`
//! - Any other per-server field (timeouts, tool filters, ...) belongs to OpenClaw:
//!   it is preserved on merge-on-write and stripped on import

use serde_json::{json, Map, Value};

use crate::error::AppError;
use crate::openclaw_config;

/// Connection fields CC Switch owns; every other key in an OpenClaw server
/// entry is OpenClaw-specific and survives a CC Switch write.
///
/// `workingDirectory` (alias of `cwd`) and `type` (the CLI-native spelling of
/// `transport`) are OpenClaw aliases: CC Switch writes `cwd` / `transport`, so
/// a stale alias must not survive next to them with a conflicting value.
const OPENCLAW_CORE_FIELDS: &[&str] = &[
    "command",
    "args",
    "env",
    "cwd",
    "workingDirectory",
    "url",
    "headers",
    "transport",
    "type",
];

const TRANSPORT_SSE: &str = "sse";
const TRANSPORT_STREAMABLE_HTTP: &str = "streamable-http";

// ============================================================================
// Helper Functions
// ============================================================================

/// Check if OpenClaw MCP sync should proceed
fn should_sync_openclaw_mcp() -> bool {
    openclaw_config::get_openclaw_dir().exists()
}

/// Copy `key` from `from` to `to` unless it is an empty array / object
fn copy_non_empty(from: &Map<String, Value>, to: &mut Map<String, Value>, key: &str) {
    let Some(value) = from.get(key) else {
        return;
    };
    let empty = match value {
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.is_empty(),
        Value::Null => true,
        _ => false,
    };
    if !empty {
        to.insert(key.into(), value.clone());
    }
}

// ============================================================================
// Format Conversion: CC Switch -> OpenClaw
// ============================================================================

/// Convert CC Switch unified format to OpenClaw format
///
/// Conversion rules:
/// - `stdio`: output `command`, `args`, `env`, `cwd` (strip `type` field)
/// - `sse`: output `url`, `headers`, `transport: "sse"`
/// - `http`: output `url`, `headers`, `transport: "streamable-http"`
fn convert_to_openclaw_format(spec: &Value) -> Result<Value, AppError> {
    let obj = spec
        .as_object()
        .ok_or_else(|| AppError::McpValidation("MCP spec must be a JSON object".into()))?;

    let typ = obj.get("type").and_then(|v| v.as_str()).unwrap_or("stdio");

    let mut result = Map::new();

    match typ {
        "stdio" => {
            for key in ["command", "args", "env", "cwd"] {
                copy_non_empty(obj, &mut result, key);
            }
        }
        "sse" | "http" => {
            copy_non_empty(obj, &mut result, "url");
            copy_non_empty(obj, &mut result, "headers");
            let transport = if typ == "http" {
                TRANSPORT_STREAMABLE_HTTP
            } else {
                TRANSPORT_SSE
            };
            result.insert("transport".into(), json!(transport));
        }
        _ => {
            return Err(AppError::McpValidation(format!("Unknown MCP type: {typ}")));
        }
    }

    Ok(Value::Object(result))
}

// ============================================================================
// Format Conversion: OpenClaw -> CC Switch
// ============================================================================

/// Convert OpenClaw format to CC Switch unified format
///
/// Conversion rules:
/// - If `command` exists: set `type: "stdio"`, extract `command`, `args`, `env`, `cwd`
///   (falling back to the `workingDirectory` alias)
/// - If `url` exists: `transport: "streamable-http"` (or `"http"`) becomes
///   `type: "http"`, anything else `type: "sse"`; extract `url`, `headers`
/// - OpenClaw-specific fields are stripped
fn convert_from_openclaw_format(id: &str, spec: &Value) -> Result<Value, AppError> {
    let obj = spec
        .as_object()
        .ok_or_else(|| AppError::McpValidation("OpenClaw MCP spec must be a JSON object".into()))?;

    let mut result = Map::new();

    if obj.contains_key("command") {
        result.insert("type".into(), json!("stdio"));
        for key in ["command", "args", "env", "cwd"] {
            copy_non_empty(obj, &mut result, key);
        }
        if !result.contains_key("cwd") {
            if let Some(dir) = obj.get("workingDirectory").filter(|v| v.is_string()) {
                result.insert("cwd".into(), dir.clone());
            }
        }
    } else if obj.contains_key("url") {
        let transport = obj
            .get("transport")
            .or_else(|| obj.get("type"))
            .and_then(Value::as_str)
            .unwrap_or(TRANSPORT_SSE);
        let typ = match transport {
            TRANSPORT_STREAMABLE_HTTP | "http" => "http",
            _ => "sse",
        };
        result.insert("type".into(), json!(typ));
        copy_non_empty(obj, &mut result, "url");
        copy_non_empty(obj, &mut result, "headers");
    } else {
        return Err(AppError::McpValidation(format!(
            "OpenClaw MCP server '{id}' has neither 'command' nor 'url' field"
        )));
    }

    Ok(Value::Object(result))
}

/// Merge new spec into existing OpenClaw spec.
///
/// Core fields ([`OPENCLAW_CORE_FIELDS`]) come only from `new_spec`, so a field
/// the new spec no longer has (e.g. `command` after switching to a remote
/// server) is dropped. Every other key is kept from `existing`.
///
/// The user's key order is kept: existing keys stay where they are (core ones
/// take the new value), and core fields the entry did not have are appended.
fn merge_openclaw_spec(existing: &Value, new_spec: &Value) -> Value {
    let empty = Map::new();
    let new_obj = new_spec.as_object().unwrap_or(&empty);
    let mut result = Map::new();

    if let Some(existing_obj) = existing.as_object() {
        for (key, val) in existing_obj {
            if !OPENCLAW_CORE_FIELDS.contains(&key.as_str()) {
                result.insert(key.clone(), val.clone());
            } else if let Some(new_val) = new_obj.get(key) {
                result.insert(key.clone(), new_val.clone());
            }
        }
    }

    for (key, val) in new_obj {
        if !result.contains_key(key) {
            result.insert(key.clone(), val.clone());
        }
    }

    Value::Object(result)
}

// ============================================================================
// Public API: Sync Functions
// ============================================================================

/// Sync a single MCP server to OpenClaw live config (merge-on-write)
///
/// Skipped when the OpenClaw config directory does not exist. The file is not
/// rewritten when the merged entry is unchanged.
pub fn sync_single_server_to_openclaw(id: &str, server_spec: &Value) -> Result<(), AppError> {
    if !should_sync_openclaw_mcp() {
        return Ok(());
    }

    let openclaw_spec = convert_to_openclaw_format(server_spec)?;

    openclaw_config::update_mcp_servers(|servers| {
        let merged = match servers.get(id) {
            Some(existing) => merge_openclaw_spec(existing, &openclaw_spec),
            None => openclaw_spec,
        };
        if servers.get(id) == Some(&merged) {
            return Ok(false);
        }
        servers.insert(id.to_string(), merged);
        Ok(true)
    })?;
    Ok(())
}

/// Remove a single MCP server from OpenClaw live config
pub fn remove_server_from_openclaw(id: &str) -> Result<(), AppError> {
    if !should_sync_openclaw_mcp() {
        return Ok(());
    }

    openclaw_config::update_mcp_servers(|servers| Ok(servers.shift_remove(id).is_some()))?;
    Ok(())
}

/// Read MCP servers from OpenClaw config, converted to the unified format
/// (not yet validated). Entries that fail conversion are logged and skipped.
pub(crate) fn read_servers_from_openclaw() -> Result<Vec<(String, Value)>, AppError> {
    let servers = openclaw_config::get_mcp_servers()?;
    let mut out = Vec::new();

    for (id, spec) in &servers {
        match convert_from_openclaw_format(id, spec) {
            Ok(unified) => out.push((id.clone(), unified)),
            Err(e) => log::warn!("Skip invalid OpenClaw MCP server '{id}': {e}"),
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stdio_to_openclaw_strips_type_and_empty_fields() {
        let spec = json!({
            "type": "stdio",
            "command": "npx",
            "args": ["-y", "@modelcontextprotocol/server-filesystem"],
            "env": {},
            "cwd": "/work"
        });

        let result = convert_to_openclaw_format(&spec).unwrap();
        assert_eq!(
            result,
            json!({
                "command": "npx",
                "args": ["-y", "@modelcontextprotocol/server-filesystem"],
                "cwd": "/work"
            })
        );
    }

    #[test]
    fn remote_types_map_to_openclaw_transports() {
        let sse =
            json!({ "type": "sse", "url": "https://example.com/sse", "headers": { "A": "1" } });
        assert_eq!(
            convert_to_openclaw_format(&sse).unwrap(),
            json!({ "url": "https://example.com/sse", "headers": { "A": "1" }, "transport": "sse" })
        );

        let http = json!({ "type": "http", "url": "https://example.com/mcp" });
        assert_eq!(
            convert_to_openclaw_format(&http).unwrap(),
            json!({ "url": "https://example.com/mcp", "transport": "streamable-http" })
        );
    }

    #[test]
    fn unknown_type_to_openclaw_fails() {
        let spec = json!({ "type": "grpc", "command": "foo" });
        assert!(convert_to_openclaw_format(&spec).is_err());
    }

    #[test]
    fn openclaw_entries_convert_back_to_unified() {
        let stdio = json!({
            "command": "uvx",
            "args": ["mcp-server-fetch"],
            "env": { "K": "V" },
            "connectionTimeoutMs": 5000
        });
        assert_eq!(
            convert_from_openclaw_format("fetch", &stdio).unwrap(),
            json!({ "type": "stdio", "command": "uvx", "args": ["mcp-server-fetch"], "env": { "K": "V" } })
        );

        let http = json!({ "url": "https://example.com/mcp", "transport": "streamable-http" });
        assert_eq!(
            convert_from_openclaw_format("remote", &http).unwrap()["type"],
            "http"
        );

        // A url without a transport is OpenClaw's default (SSE)
        let sse = json!({ "url": "https://example.com/sse" });
        assert_eq!(
            convert_from_openclaw_format("remote", &sse).unwrap(),
            json!({ "type": "sse", "url": "https://example.com/sse" })
        );

        assert!(convert_from_openclaw_format("bad", &json!({ "enabled": true })).is_err());
    }

    #[test]
    fn round_trip_keeps_the_unified_spec() {
        for spec in [
            json!({ "type": "stdio", "command": "node", "args": ["server.js"] }),
            json!({ "type": "sse", "url": "https://example.com/sse" }),
            json!({ "type": "http", "url": "https://example.com/mcp", "headers": { "A": "1" } }),
        ] {
            let openclaw = convert_to_openclaw_format(&spec).unwrap();
            assert_eq!(convert_from_openclaw_format("id", &openclaw).unwrap(), spec);
        }
    }

    #[test]
    fn merge_keeps_openclaw_fields_and_drops_stale_core_fields() {
        let existing = json!({
            "command": "old-cmd",
            "args": ["old-arg"],
            "cwd": "/old",
            "connectionTimeoutMs": 5000
        });
        let new_spec = json!({ "url": "https://example.com/mcp", "transport": "streamable-http" });

        assert_eq!(
            merge_openclaw_spec(&existing, &new_spec),
            json!({
                "connectionTimeoutMs": 5000,
                "url": "https://example.com/mcp",
                "transport": "streamable-http"
            })
        );
    }

    #[test]
    fn merge_keeps_the_user_key_order() {
        let existing = json!({
            "connectionTimeoutMs": 5000,
            "url": "https://old.example.com/sse",
            "transport": "sse",
            "requestTimeoutMs": 20000
        });
        let new_spec = json!({
            "url": "https://new.example.com/mcp",
            "headers": { "A": "1" },
            "transport": "streamable-http"
        });

        let merged = merge_openclaw_spec(&existing, &new_spec);
        let keys: Vec<&str> = merged
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "connectionTimeoutMs",
                "url",
                "transport",
                "requestTimeoutMs",
                "headers"
            ]
        );
        assert_eq!(merged["url"], "https://new.example.com/mcp");
        assert_eq!(merged["transport"], "streamable-http");
    }

    #[test]
    fn merge_drops_stale_aliases_of_core_fields() {
        let existing = json!({
            "command": "node",
            "workingDirectory": "/old",
            "connectionTimeoutMs": 5000
        });
        let new_spec = json!({ "command": "node", "cwd": "/new" });
        assert_eq!(
            merge_openclaw_spec(&existing, &new_spec),
            json!({ "command": "node", "connectionTimeoutMs": 5000, "cwd": "/new" })
        );

        let existing = json!({ "url": "https://example.com/mcp", "type": "http" });
        let new_spec = json!({ "url": "https://example.com/mcp", "transport": "sse" });
        assert_eq!(
            merge_openclaw_spec(&existing, &new_spec),
            json!({ "url": "https://example.com/mcp", "transport": "sse" })
        );
    }

    #[test]
    fn working_directory_alias_imports_as_cwd() {
        let spec = json!({ "command": "node", "workingDirectory": "/work" });
        assert_eq!(
            convert_from_openclaw_format("local", &spec).unwrap(),
            json!({ "type": "stdio", "command": "node", "cwd": "/work" })
        );

        let both = json!({ "command": "node", "cwd": "/a", "workingDirectory": "/b" });
        assert_eq!(
            convert_from_openclaw_format("local", &both).unwrap()["cwd"],
            "/a"
        );
    }
}
