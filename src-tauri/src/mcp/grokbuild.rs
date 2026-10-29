//! Grok Build MCP synchronization and import.
//!
//! Grok Build uses the same top-level `[mcp_servers]` TOML layout as Codex,
//! stored alongside its model configuration in `~/.grok/config.toml`.

use serde_json::{json, Value};

use crate::app_config::{AppType, MultiAppConfig};
use crate::error::AppError;

use super::codex::json_server_to_toml_table;
use super::common::import_into_config;

fn should_sync_grokbuild_mcp() -> bool {
    crate::grok_config::get_grok_config_dir().exists()
}

fn read_config_text() -> Result<String, AppError> {
    let path = crate::grok_config::get_grok_config_path();
    Ok(crate::config::read_text_file_if_exists(&path)?.unwrap_or_default())
}

fn json_server_to_grokbuild_toml_table(server_spec: &Value) -> Result<toml_edit::Table, AppError> {
    let mut table = json_server_to_toml_table(server_spec)?;
    // Grok infers transport from `command` or `url` and uses `headers`, while
    // Codex writes an explicit `type` plus `http_headers`.
    table.remove("type");
    if let Some(headers) = table.remove("http_headers") {
        table.insert("headers", headers);
    }
    Ok(table)
}

fn toml_server_to_json(entry: &toml::value::Table) -> Value {
    fn convert(value: &toml::Value) -> Option<Value> {
        match value {
            toml::Value::String(value) => Some(json!(value)),
            toml::Value::Integer(value) => Some(json!(value)),
            toml::Value::Float(value) => Some(json!(value)),
            toml::Value::Boolean(value) => Some(json!(value)),
            toml::Value::Datetime(value) => Some(json!(value.to_string())),
            toml::Value::Array(values) => Some(Value::Array(
                values.iter().filter_map(convert).collect::<Vec<_>>(),
            )),
            toml::Value::Table(values) => Some(Value::Object(
                values
                    .iter()
                    .filter_map(|(key, value)| convert(value).map(|value| (key.clone(), value)))
                    .collect(),
            )),
        }
    }

    let mut spec = serde_json::Map::new();
    for (key, value) in entry {
        let output_key = if key == "http_headers" {
            "headers"
        } else {
            key
        };
        if let Some(value) = convert(value) {
            spec.insert(output_key.to_string(), value);
        }
    }
    let default_type = if spec.contains_key("url") {
        "http"
    } else {
        "stdio"
    };
    spec.entry("type".to_string())
        .or_insert_with(|| json!(default_type));
    Value::Object(spec)
}

/// 读取 ~/.grok/config.toml 的 `[mcp_servers]`（统一结构，尚未校验）
pub(crate) fn read_servers_from_grokbuild() -> Result<Vec<(String, Value)>, AppError> {
    let text = read_config_text()?;
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let root: toml::Table = toml::from_str(&text)
        .map_err(|e| AppError::McpValidation(format!("解析 ~/.grok/config.toml 失败: {e}")))?;
    let Some(entries) = root.get("mcp_servers").and_then(toml::Value::as_table) else {
        return Ok(Vec::new());
    };

    Ok(entries
        .iter()
        .filter_map(|(id, entry)| {
            entry
                .as_table()
                .map(|entry| (id.clone(), toml_server_to_json(entry)))
        })
        .collect())
}

pub fn import_from_grokbuild(config: &mut MultiAppConfig) -> Result<usize, AppError> {
    let entries = read_servers_from_grokbuild()?;
    Ok(import_into_config(config, &AppType::GrokBuild, entries))
}

pub fn sync_single_server_to_grokbuild(
    _config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    if !should_sync_grokbuild_mcp() {
        return Ok(());
    }
    use toml_edit::Item;

    let path = crate::grok_config::get_grok_config_path();
    let text = read_config_text()?;
    let mut doc = if text.trim().is_empty() {
        toml_edit::DocumentMut::new()
    } else {
        text.parse::<toml_edit::DocumentMut>().map_err(|e| {
            AppError::McpValidation(format!("解析 Grok Build config.toml 失败: {e}"))
        })?
    };
    if !doc.contains_key("mcp_servers") {
        doc["mcp_servers"] = toml_edit::table();
    }
    doc["mcp_servers"][id] = Item::Table(json_server_to_grokbuild_toml_table(server_spec)?);
    crate::config::write_text_file(&path, &doc.to_string())
}

pub fn remove_server_from_grokbuild(id: &str) -> Result<(), AppError> {
    if !should_sync_grokbuild_mcp() {
        return Ok(());
    }
    let path = crate::grok_config::get_grok_config_path();
    let Some(text) = crate::config::read_text_file_if_exists(&path)? else {
        return Ok(());
    };
    let mut doc = match text.parse::<toml_edit::DocumentMut>() {
        Ok(doc) => doc,
        Err(error) => {
            log::warn!("解析 Grok Build config.toml 失败: {error}，跳过删除操作");
            return Ok(());
        }
    };
    if let Some(servers) = doc
        .get_mut("mcp_servers")
        .and_then(toml_edit::Item::as_table_mut)
    {
        servers.remove(id);
    }
    crate::config::write_text_file(&path, &doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_http_headers_to_unified_headers() {
        let entry: toml::value::Table = toml::from_str(
            r#"type = "http"
url = "https://example.com/mcp"
http_headers = { Authorization = "Bearer token" }
"#,
        )
        .expect("parse table");
        let spec = toml_server_to_json(&entry);
        assert_eq!(spec["type"], "http");
        assert_eq!(spec["headers"]["Authorization"], "Bearer token");
    }

    #[test]
    fn writes_grokbuild_remote_server_without_codex_only_fields() {
        let table = json_server_to_grokbuild_toml_table(&json!({
            "type": "http",
            "url": "https://example.com/mcp",
            "headers": { "Authorization": "Bearer token" }
        }))
        .expect("convert server");

        assert!(!table.contains_key("type"));
        assert!(!table.contains_key("http_headers"));
        assert_eq!(
            table
                .get("headers")
                .and_then(toml_edit::Item::as_table)
                .and_then(|headers| headers.get("Authorization"))
                .and_then(toml_edit::Item::as_str),
            Some("Bearer token")
        );
    }
}
