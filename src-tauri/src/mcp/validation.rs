//! MCP 服务器配置验证模块

use serde_json::Value;

use crate::error::AppError;

/// 基础校验：允许 stdio/http/sse；或省略 type（视为 stdio）。对应必填字段存在
pub fn validate_server_spec(spec: &Value) -> Result<(), AppError> {
    if !spec.is_object() {
        return Err(AppError::McpValidation(
            "MCP 服务器连接定义必须为 JSON 对象".into(),
        ));
    }
    // A present-but-non-string type (number, bool, object, null) is malformed;
    // only an absent field means "default to stdio".
    let t_opt = match spec.get("type") {
        None => None,
        Some(Value::String(t)) => Some(t.as_str()),
        Some(_) => {
            return Err(AppError::McpValidation(
                "MCP 服务器 type 必须是 'stdio'、'http' 或 'sse'（或省略表示 stdio）".into(),
            ))
        }
    };
    // 支持三种：stdio/http/sse；若缺省 type 则按 stdio 处理（与社区常见 .mcp.json 一致）
    let is_stdio = t_opt.map(|t| t == "stdio").unwrap_or(true);
    let is_http = t_opt.map(|t| t == "http").unwrap_or(false);
    let is_sse = t_opt.map(|t| t == "sse").unwrap_or(false);

    if !(is_stdio || is_http || is_sse) {
        return Err(AppError::McpValidation(
            "MCP 服务器 type 必须是 'stdio'、'http' 或 'sse'（或省略表示 stdio）".into(),
        ));
    }

    if is_stdio {
        let cmd = spec.get("command").and_then(|x| x.as_str()).unwrap_or("");
        if cmd.trim().is_empty() {
            return Err(AppError::McpValidation(
                "stdio 类型的 MCP 服务器缺少 command 字段".into(),
            ));
        }
    }
    if is_http {
        let url = spec.get("url").and_then(|x| x.as_str()).unwrap_or("");
        if url.trim().is_empty() {
            return Err(AppError::McpValidation(
                "http 类型的 MCP 服务器缺少 url 字段".into(),
            ));
        }
    }
    if is_sse {
        let url = spec.get("url").and_then(|x| x.as_str()).unwrap_or("");
        if url.trim().is_empty() {
            return Err(AppError::McpValidation(
                "sse 类型的 MCP 服务器缺少 url 字段".into(),
            ));
        }
    }
    Ok(())
}

/// 从 MCP 条目中提取服务器规范
pub fn extract_server_spec(entry: &Value) -> Result<Value, AppError> {
    let obj = entry
        .as_object()
        .ok_or_else(|| AppError::McpValidation("MCP 服务器条目必须为 JSON 对象".into()))?;
    let server = obj
        .get("server")
        .ok_or_else(|| AppError::McpValidation("MCP 服务器条目缺少 server 字段".into()))?;

    if !server.is_object() {
        return Err(AppError::McpValidation(
            "MCP 服务器 server 字段必须为 JSON 对象".into(),
        ));
    }

    Ok(server.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn missing_type_is_treated_as_stdio() {
        assert!(validate_server_spec(&json!({"command": "npx"})).is_ok());
        assert!(validate_server_spec(&json!({"url": "https://x"})).is_err());
    }

    #[test]
    fn stdio_requires_non_blank_command() {
        for spec in [
            json!({"type": "stdio"}),
            json!({"type": "stdio", "command": "   "}),
            json!({"type": "stdio", "command": 42}),
        ] {
            assert!(validate_server_spec(&spec).is_err(), "{spec}");
        }
        assert!(validate_server_spec(&json!({"type": "stdio", "command": "uvx"})).is_ok());
    }

    #[test]
    fn http_and_sse_require_non_blank_url() {
        for t in ["http", "sse"] {
            assert!(validate_server_spec(&json!({"type": t, "url": "https://x/mcp"})).is_ok());
            let err = validate_server_spec(&json!({"type": t, "url": " "}))
                .unwrap_err()
                .to_string();
            assert!(err.contains(t), "{err}");
            // A command alone does not satisfy a remote transport
            assert!(validate_server_spec(&json!({"type": t, "command": "npx"})).is_err());
        }
    }

    #[test]
    fn unknown_type_is_rejected() {
        assert!(validate_server_spec(&json!({"type": "websocket", "url": "ws://x"})).is_err());
        // Type matching is exact; upper-case variants are not silently accepted
        assert!(validate_server_spec(&json!({"type": "HTTP", "url": "https://x"})).is_err());
    }

    #[test]
    fn non_string_type_is_rejected_instead_of_defaulting_to_stdio() {
        for t in [
            json!(42),
            json!(true),
            json!(null),
            json!({"kind": "http"}),
            json!(["stdio"]),
        ] {
            let spec = json!({"type": t, "command": "npx", "url": "https://x"});
            let err = validate_server_spec(&spec).unwrap_err().to_string();
            assert!(err.contains("type"), "{spec}: {err}");
        }
    }

    #[test]
    fn non_object_spec_is_rejected() {
        for spec in [json!(null), json!("npx"), json!(["npx"])] {
            assert!(validate_server_spec(&spec).is_err(), "{spec}");
        }
    }

    #[test]
    fn extract_server_spec_requires_object_server_field() {
        let entry = json!({"id": "a", "server": {"command": "npx"}});
        assert_eq!(
            extract_server_spec(&entry).unwrap(),
            json!({"command": "npx"})
        );
        assert!(extract_server_spec(&json!({"id": "a"})).is_err());
        assert!(extract_server_spec(&json!({"server": "npx"})).is_err());
        assert!(extract_server_spec(&json!([])).is_err());
    }
}
