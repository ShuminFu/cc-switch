use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::AppError;
use crate::gemini_config::get_gemini_settings_path;

/// 获取 Gemini MCP 配置文件路径（~/.gemini/settings.json）
fn user_config_path() -> PathBuf {
    get_gemini_settings_path()
}

fn read_json_value(path: &Path) -> Result<Value, AppError> {
    // 与供应商切换共用同一读取逻辑：容忍注释，无法解析时报错而不是覆盖
    crate::gemini_config::read_gemini_settings_value(path)
}

fn write_json_value(path: &Path, value: &Value) -> Result<(), AppError> {
    use crate::adapters::live_file::{write_json_preserving, WriteOptions};
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    // settings.json may be JSONC: keep comments, key order, indent and newline;
    // no-op when unchanged; backup first
    write_json_preserving(path, value, WriteOptions::backup("gemini"))?;
    Ok(())
}

/// 读取 Gemini settings.json 中的 mcpServers 映射
///
/// 执行反向格式转换以保持与统一 MCP 结构的兼容性：
/// - httpUrl → url + type: "http"
/// - 仅有 url 字段 → 补齐 type: "sse"（Gemini 以字段名推断传输类型）
/// - 仅有 command 字段 → 补齐 type: "stdio"
pub fn read_mcp_servers_map() -> Result<std::collections::HashMap<String, Value>, AppError> {
    let path = user_config_path();
    if !path.exists() {
        return Ok(std::collections::HashMap::new());
    }

    let root = read_json_value(&path)?;
    let mut servers: std::collections::HashMap<String, Value> = root
        .get("mcpServers")
        .and_then(|v| v.as_object())
        .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();

    // 反向格式转换：Gemini 特有格式 → 统一 MCP 格式
    for (_, spec) in servers.iter_mut() {
        if let Some(obj) = spec.as_object_mut() {
            // httpUrl → url + type: "http"
            if let Some(http_url) = obj.remove("httpUrl") {
                obj.insert("url".to_string(), http_url);
                obj.insert("type".to_string(), Value::String("http".to_string()));
            }

            // Gemini CLI 不使用 type 字段：这里补齐成统一结构，便于校验与导入
            if obj.get("type").is_none() {
                if obj.contains_key("command") {
                    obj.insert("type".to_string(), Value::String("stdio".to_string()));
                } else if obj.contains_key("url") {
                    obj.insert("type".to_string(), Value::String("sse".to_string()));
                }
            }
        }
    }

    Ok(servers)
}

/// Convert a unified MCP spec into a Gemini CLI `mcpServers` entry.
///
/// Gemini infers the transport from the field name (`httpUrl` = streamable
/// HTTP, `url` = SSE) and has a single `timeout` in milliseconds. A timeout is
/// only emitted when the source carried one (CFG-30): injecting a default
/// into every server polluted other apps when the spec was re-imported.
fn to_gemini_native(id: &str, spec: &Value) -> Result<Value, AppError> {
    let mut obj = if let Some(map) = spec.as_object() {
        map.clone()
    } else {
        return Err(AppError::McpValidation(format!(
            "MCP 服务器 '{id}' 不是对象"
        )));
    };

    // 提取 server 字段（如果存在）
    if let Some(server_val) = obj.remove("server") {
        let server_obj = server_val.as_object().cloned().ok_or_else(|| {
            AppError::McpValidation(format!("MCP 服务器 '{id}' server 字段不是对象"))
        })?;
        obj = server_obj;
    }

    let transport_type = obj.get("type").and_then(|v| v.as_str());
    if transport_type == Some("http") {
        // HTTP streaming: 将 "url" 重命名为 "httpUrl"
        if let Some(url_value) = obj.remove("url") {
            obj.insert("httpUrl".to_string(), url_value);
        }
    }
    // SSE 保持 "url" 字段不变

    // 移除 UI 辅助字段和 type 字段（Gemini 不需要）
    for key in [
        "type",
        "enabled",
        "source",
        "id",
        "name",
        "description",
        "tags",
        "homepage",
        "docs",
    ] {
        obj.remove(key);
    }

    // Timeout 转换：Claude/Codex 使用 startup_timeout_sec/tool_timeout_sec，
    // Gemini CLI 只支持 timeout（单位 ms），取两者最大值
    let extract_timeout =
        |obj: &mut Map<String, Value>, key: &str, multiplier: u64| -> Option<u64> {
            obj.remove(key).and_then(|val| {
                val.as_u64()
                    .map(|n| n * multiplier)
                    .or_else(|| val.as_f64().map(|f| (f * multiplier as f64) as u64))
            })
        };
    let startup_ms = extract_timeout(&mut obj, "startup_timeout_sec", 1000)
        .or_else(|| extract_timeout(&mut obj, "startup_timeout_ms", 1));
    let tool_ms = extract_timeout(&mut obj, "tool_timeout_sec", 1000)
        .or_else(|| extract_timeout(&mut obj, "tool_timeout_ms", 1));
    if let Some(timeout) = startup_ms.into_iter().chain(tool_ms).max() {
        obj.insert("timeout".to_string(), Value::Number(timeout.into()));
    }

    Ok(Value::Object(obj))
}

/// Apply a batch of MCP changes to `~/.gemini/settings.json` with one
/// read-modify-write. Only the listed servers are touched: entries the user
/// added directly in Gemini keep their exact content, and nothing is written
/// when the result is unchanged (CFG-41).
pub fn project_mcp_servers(enabled: &[(&str, &Value)], disabled: &[&str]) -> Result<(), AppError> {
    let path = user_config_path();
    let natives = enabled
        .iter()
        .map(|(id, spec)| Ok((id.to_string(), to_gemini_native(id, spec)?)))
        .collect::<Result<Vec<_>, AppError>>()?;

    let mut root = if path.exists() {
        read_json_value(&path)?
    } else {
        if natives.is_empty() {
            return Ok(());
        }
        serde_json::json!({})
    };
    let before = root.clone();
    {
        let obj = root
            .as_object_mut()
            .ok_or_else(|| AppError::Config("~/.gemini/settings.json 根必须是对象".into()))?;
        if natives.is_empty() && !obj.contains_key("mcpServers") {
            return Ok(());
        }
        let servers = obj
            .entry("mcpServers")
            .or_insert_with(|| Value::Object(Map::new()));
        let map = servers.as_object_mut().ok_or_else(|| {
            AppError::Config("~/.gemini/settings.json 的 mcpServers 必须是对象".into())
        })?;
        for (id, native) in natives {
            // keep Gemini-native keys (trust, includeTools, …) of an existing entry (CFG-42)
            let merged = crate::mcp::merge::merge_live_json_entry(map.get(&id), native);
            map.insert(id, merged);
        }
        for id in disabled {
            map.shift_remove(*id);
        }
    }
    if root == before {
        return Ok(());
    }
    write_json_value(&path, &root)
}

/// 将给定的启用 MCP 服务器映射写入到 Gemini settings.json 的 mcpServers 字段
/// 仅覆盖 mcpServers，其他字段保持不变
pub fn set_mcp_servers_map(
    servers: &std::collections::HashMap<String, Value>,
) -> Result<(), AppError> {
    let path = user_config_path();
    let mut root = if path.exists() {
        read_json_value(&path)?
    } else {
        serde_json::json!({})
    };

    // 构建 mcpServers 对象：移除 UI 辅助字段（enabled/source），仅保留实际 MCP 规范
    let mut out: Map<String, Value> = Map::new();
    for (id, spec) in servers.iter() {
        out.insert(id.clone(), to_gemini_native(id, spec)?);
    }

    {
        let obj = root
            .as_object_mut()
            .ok_or_else(|| AppError::Config("~/.gemini/settings.json 根必须是对象".into()))?;
        obj.insert("mcpServers".into(), Value::Object(out));
    }

    write_json_value(&path, &root)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn gemini_native_only_emits_timeout_when_source_had_one() {
        let plain = json!({"type": "stdio", "command": "npx", "args": ["-y"]});
        assert!(to_gemini_native("a", &plain)
            .unwrap()
            .get("timeout")
            .is_none());

        let timed = json!({"type": "stdio", "command": "npx", "startup_timeout_sec": 5, "tool_timeout_sec": 30});
        assert_eq!(
            to_gemini_native("a", &timed).unwrap()["timeout"],
            json!(30_000)
        );

        // a timeout read back from Gemini is kept as-is
        let native = json!({"command": "npx", "timeout": 1234});
        assert_eq!(
            to_gemini_native("a", &native).unwrap()["timeout"],
            json!(1234)
        );

        let http = json!({"type": "http", "url": "https://x/mcp"});
        assert_eq!(
            to_gemini_native("a", &http).unwrap(),
            json!({"httpUrl": "https://x/mcp"})
        );
    }
}
