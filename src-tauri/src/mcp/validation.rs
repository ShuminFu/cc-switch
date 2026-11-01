//! MCP 服务器配置验证模块

use serde_json::Value;

use crate::error::AppError;

/// 校验 MCP 服务器连接定义（schema 级别）。
///
/// - `type` 可省略（视为 stdio）；若提供必须是字符串 `stdio` / `http` / `sse`
/// - stdio：`command` 必须是非空字符串；`args`（可选）必须是字符串数组；
///   `env`（可选）必须是值为字符串的对象；`cwd`（可选）必须是字符串
/// - http/sse：`url` 必须是 scheme 为 http/https 的合法 URL；
///   `headers`（可选）必须是值为字符串的对象
pub fn validate_server_spec(spec: &Value) -> Result<(), AppError> {
    let obj = spec.as_object().ok_or_else(|| {
        AppError::localized(
            "mcp.error.jsonInvalid",
            "MCP 服务器连接定义必须为 JSON 对象",
            "The MCP server definition must be a JSON object",
        )
    })?;

    let t_opt = match obj.get("type") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.as_str()),
        Some(_) => {
            return Err(AppError::localized(
                "mcp.error.jsonInvalid",
                "MCP 服务器 type 字段必须为字符串",
                "The MCP server type field must be a string",
            ))
        }
    };
    // 支持三种：stdio/http/sse；若缺省 type 则按 stdio 处理（与社区常见 .mcp.json 一致）
    let is_stdio = t_opt.map(|t| t == "stdio").unwrap_or(true);
    let is_http = t_opt.map(|t| t == "http").unwrap_or(false);
    let is_sse = t_opt.map(|t| t == "sse").unwrap_or(false);

    if !(is_stdio || is_http || is_sse) {
        return Err(AppError::localized(
            "mcp.error.jsonInvalid",
            "MCP 服务器 type 必须是 'stdio'、'http' 或 'sse'（或省略表示 stdio）",
            "The MCP server type must be 'stdio', 'http' or 'sse' (omit it for stdio)",
        ));
    }

    if is_stdio {
        let cmd = obj.get("command").and_then(|x| x.as_str()).unwrap_or("");
        if cmd.trim().is_empty() {
            return Err(AppError::localized(
                "mcp.error.commandRequired",
                "stdio 类型的 MCP 服务器缺少 command 字段",
                "A stdio MCP server needs a command",
            ));
        }
        if let Some(args) = obj.get("args") {
            let ok = args
                .as_array()
                .map(|arr| arr.iter().all(is_scalar))
                .unwrap_or(false);
            if !ok {
                return Err(AppError::McpValidation(
                    "MCP 服务器 args 字段必须为字符串（或数字/布尔）数组".into(),
                ));
            }
        }
        validate_string_map(obj.get("env"), "env")?;
        if let Some(cwd) = obj.get("cwd") {
            if !cwd.is_string() {
                return Err(AppError::McpValidation(
                    "MCP 服务器 cwd 字段必须为字符串".into(),
                ));
            }
        }
    }

    if is_http || is_sse {
        let kind = if is_http { "http" } else { "sse" };
        let url = obj.get("url").and_then(|x| x.as_str()).unwrap_or("");
        if url.trim().is_empty() {
            return Err(AppError::localized(
                "mcp.wizard.urlRequired",
                format!("{kind} 类型的 MCP 服务器缺少 url 字段"),
                format!("A {kind} MCP server needs a URL"),
            ));
        }
        let parsed = url::Url::parse(url.trim()).map_err(|e| {
            AppError::McpValidation(format!("{kind} 类型的 MCP 服务器 url 无效: {e}"))
        })?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(AppError::McpValidation(format!(
                "{kind} 类型的 MCP 服务器 url 必须使用 http 或 https（当前为 '{}'）",
                parsed.scheme()
            )));
        }
        validate_string_map(obj.get("headers"), "headers")?;
    }

    Ok(())
}

/// Live configs (TOML in particular) legitimately carry numbers and booleans in
/// `args`/`env`/`headers` (e.g. `PORT = 3000`); every tool stringifies them.
/// Only nested arrays/objects and null are rejected, so existing servers are
/// not silently dropped on import.
fn is_scalar(v: &Value) -> bool {
    v.is_string() || v.is_number() || v.is_boolean()
}

fn validate_string_map(value: Option<&Value>, field: &str) -> Result<(), AppError> {
    let Some(value) = value else {
        return Ok(());
    };
    let ok = value
        .as_object()
        .map(|m| m.values().all(is_scalar))
        .unwrap_or(false);
    if ok {
        Ok(())
    } else {
        Err(AppError::McpValidation(format!(
            "MCP 服务器 {field} 字段必须为值均为字符串（或数字/布尔）的对象"
        )))
    }
}

/// Commands that cc-switch rewrites to `cmd /c <command> <args...>` on Windows
/// (see `claude_mcp::wrap_command_for_windows`).
const WINDOWS_CMD_WRAPPED_COMMANDS: &[&str] =
    &["npx", "npm", "yarn", "pnpm", "node", "bun", "deno"];

/// Characters that `cmd.exe` interprets as command separators / redirections
/// when an argument is passed through `cmd /c` unquoted. `%` and `"` are
/// intentionally not included: legitimate configs rely on `%APPDATA%`-style
/// expansion and quoted arguments.
const CMD_METACHARACTERS: &[char] = &['&', '|', '<', '>', '^'];

/// Return the first argument that would gain shell semantics once the server is
/// wrapped in `cmd /c` on Windows, if any.
///
/// This is a pure function so it can be tested on every platform; callers
/// decide whether to enforce it (deep-link imports enforce it on Windows).
pub fn find_cmd_wrap_unsafe_arg(spec: &Value) -> Option<String> {
    let obj = spec.as_object()?;
    let server_type = obj.get("type").and_then(Value::as_str).unwrap_or("stdio");
    if server_type != "stdio" {
        return None;
    }
    let cmd = obj.get("command").and_then(Value::as_str)?;
    // Split on both separators so the check behaves the same on every host
    // platform (Windows paths use `\`).
    let file_name = cmd.rsplit(['/', '\\']).next().unwrap_or(cmd);
    let cmd_name = file_name
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .filter(|stem| !stem.is_empty())
        .unwrap_or(file_name);
    let wrapped = WINDOWS_CMD_WRAPPED_COMMANDS
        .iter()
        .any(|c| cmd_name.eq_ignore_ascii_case(c));
    if !wrapped {
        return None;
    }
    obj.get("args")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(Value::as_str)
        .find(|arg| arg.contains(CMD_METACHARACTERS))
        .map(str::to_string)
}

/// 从 MCP 条目中提取服务器规范
pub fn extract_server_spec(entry: &Value) -> Result<Value, AppError> {
    let obj = entry.as_object().ok_or_else(|| {
        AppError::localized(
            "mcp.error.jsonInvalid",
            "MCP 服务器条目必须为 JSON 对象",
            "The MCP server entry must be a JSON object",
        )
    })?;
    let server = obj.get("server").ok_or_else(|| {
        AppError::localized(
            "mcp.error.jsonInvalid",
            "MCP 服务器条目缺少 server 字段",
            "The MCP server entry has no server field",
        )
    })?;

    if !server.is_object() {
        return Err(AppError::localized(
            "mcp.error.jsonInvalid",
            "MCP 服务器 server 字段必须为 JSON 对象",
            "The MCP server field must be a JSON object",
        ));
    }

    Ok(server.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn accepts_common_valid_shapes() {
        for spec in [
            json!({"command": "npx", "args": ["-y", "pkg"], "env": {"A": "b"}}),
            json!({"type": "stdio", "command": "echo", "cwd": "/tmp"}),
            json!({"type": "http", "url": "https://example.com/mcp", "headers": {"Authorization": "Bearer x"}}),
            json!({"type": "sse", "url": "http://localhost:3000/sse"}),
        ] {
            validate_server_spec(&spec).unwrap_or_else(|e| panic!("{spec} rejected: {e}"));
        }
    }

    #[test]
    fn rejects_invalid_shapes() {
        for spec in [
            json!("not an object"),
            json!({"type": 1, "command": "echo"}),
            json!({"type": "websocket", "url": "https://x"}),
            json!({"command": ""}),
            json!({"command": 42}),
            json!({"command": "echo", "args": "-c evil"}),
            json!({"command": "echo", "env": ["A=b"]}),
            json!({"command": "echo", "args": ["ok", {"x": 1}]}),
            json!({"type": "http", "url": "https://x", "headers": {"A": [1]}}),
            json!({"command": "echo", "cwd": 1}),
            json!({"type": "http"}),
            json!({"type": "http", "url": "not a url"}),
            json!({"type": "http", "url": "file:///etc/passwd"}),
            json!({"type": "sse", "url": "javascript:alert(1)"}),
        ] {
            assert!(
                validate_server_spec(&spec).is_err(),
                "{spec} should be rejected"
            );
        }
    }

    #[test]
    fn detects_cmd_metacharacters_only_for_wrapped_commands() {
        let spec = json!({"command": "npx", "args": ["a", "&", "calc"]});
        assert_eq!(find_cmd_wrap_unsafe_arg(&spec).as_deref(), Some("&"));

        let spec = json!({"command": "C:\\tools\\npx.cmd", "args": ["x|y"]});
        assert_eq!(find_cmd_wrap_unsafe_arg(&spec).as_deref(), Some("x|y"));

        // Not wrapped by cc-switch: left alone.
        let spec = json!({"command": "python", "args": ["a & b"]});
        assert!(find_cmd_wrap_unsafe_arg(&spec).is_none());

        // `%VAR%` expansion and quotes are legitimate.
        let spec = json!({"command": "npx", "args": ["%APPDATA%\\x", "\"quoted\""]});
        assert!(find_cmd_wrap_unsafe_arg(&spec).is_none());

        let spec = json!({"type": "http", "url": "https://x", "args": ["&"]});
        assert!(find_cmd_wrap_unsafe_arg(&spec).is_none());
    }
}

#[cfg(test)]
mod scalar_value_tests {
    use super::validate_server_spec;
    use serde_json::json;

    #[test]
    fn numeric_and_boolean_env_and_args_are_accepted() {
        let spec = json!({
            "command": "npx",
            "args": ["server", "--port", 3000, true],
            "env": {"PORT": 3000, "DEBUG": false, "NAME": "x"}
        });
        assert!(validate_server_spec(&spec).is_ok());
    }

    #[test]
    fn nested_values_are_rejected() {
        let bad_args = json!({"command": "npx", "args": [["nested"]]});
        assert!(validate_server_spec(&bad_args).is_err());
        let bad_env = json!({"command": "npx", "env": {"K": {"x": 1}}});
        assert!(validate_server_spec(&bad_env).is_err());
        let null_env = json!({"command": "npx", "env": {"K": null}});
        assert!(validate_server_spec(&null_env).is_err());
    }
}
