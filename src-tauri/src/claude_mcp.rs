use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{atomic_write_secret, get_claude_mcp_path};
use crate::error::AppError;

/// 需要在 Windows 上用 cmd /c 包装的命令
/// 这些命令在 Windows 上实际是 .cmd 批处理文件，需要通过 cmd /c 来执行
#[cfg_attr(not(windows), allow(dead_code))]
const WINDOWS_WRAP_COMMANDS: &[&str] = &["npx", "npm", "yarn", "pnpm", "node", "bun", "deno"];

/// Windows 平台：将 `npx args...` 转换为 `cmd /c npx args...`
/// 解决 Claude Code /doctor 报告的 "Windows requires 'cmd /c' wrapper to execute npx" 警告
#[cfg(windows)]
fn wrap_command_for_windows(obj: &mut Map<String, Value>) {
    // 只处理 stdio 类型（默认或显式）
    let server_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("stdio");
    if server_type != "stdio" {
        return;
    }

    let Some(cmd) = obj.get("command").and_then(|v| v.as_str()) else {
        return;
    };

    // 已经是 cmd 的不重复包装
    if cmd.eq_ignore_ascii_case("cmd") || cmd.eq_ignore_ascii_case("cmd.exe") {
        return;
    }

    // 提取命令名（去掉 .cmd 后缀和路径）
    let cmd_name = Path::new(cmd)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(cmd);

    let needs_wrap = WINDOWS_WRAP_COMMANDS
        .iter()
        .any(|&c| cmd_name.eq_ignore_ascii_case(c));

    if !needs_wrap {
        return;
    }

    // 构建新的 args: ["/c", "原命令", ...原args]
    let original_args = obj
        .get("args")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    // cmd.exe 会重新解释 & | < > ^：逐个转义，无法安全转义时不包装（CFG-05）
    let Some(escaped) = cmd_c_escape_args(&original_args) else {
        log::warn!(
            "MCP command '{cmd}' has arguments cmd.exe cannot pass through safely; written without the cmd /c wrapper"
        );
        return;
    };

    let mut new_args = vec![Value::String("/c".into()), Value::String(cmd.into())];
    new_args.extend(escaped);

    obj.insert("command".into(), Value::String("cmd".into()));
    obj.insert("args".into(), Value::Array(new_args));
}

/// Characters `cmd.exe /c` interprets outside double quotes.
const CMD_METACHARS: &[char] = &['&', '|', '<', '>', '^'];

/// Escape arguments for `cmd /c <cmd> args…` (CFG-05).
///
/// Claude Code spawns `cmd` with an argv that Node/libuv joins into one
/// command line, quoting an argument only when it is empty or contains a
/// space, tab or `"`. cmd.exe then re-parses that line, so a bare `&`, `|`,
/// `<`, `>` or `^` would split or redirect the command. Unquoted arguments
/// get each metacharacter caret-escaped; quoted ones are already literal for
/// cmd unless they also contain `"` (libuv's `\"` toggles cmd's quote state),
/// which cannot be escaped reliably, so `None` is returned and the caller
/// must not wrap. `%VAR%` expansion is kept on purpose: Windows configs rely
/// on it.
#[cfg_attr(not(windows), allow(dead_code))]
fn cmd_c_escape_args(args: &[Value]) -> Option<Vec<Value>> {
    args.iter()
        .map(|arg| {
            let Some(s) = arg.as_str() else {
                return Some(arg.clone());
            };
            if !s.contains(CMD_METACHARS) {
                return Some(arg.clone());
            }
            let quoted = s.is_empty() || s.contains([' ', '\t', '"']);
            if quoted {
                return (!s.contains('"')).then(|| arg.clone());
            }
            let mut out = String::with_capacity(s.len() + 4);
            for c in s.chars() {
                if CMD_METACHARS.contains(&c) {
                    out.push('^');
                }
                out.push(c);
            }
            Some(Value::String(out))
        })
        .collect()
}

/// Inverse of [`cmd_c_escape_args`] for one unquoted argument.
fn cmd_c_unescape_arg(arg: &str) -> String {
    if arg.contains([' ', '\t', '"']) || !arg.contains('^') {
        return arg.to_string();
    }
    let mut out = String::with_capacity(arg.len());
    let mut chars = arg.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '^' {
            if let Some(&next) = chars.peek() {
                if CMD_METACHARS.contains(&next) {
                    out.push(next);
                    chars.next();
                    continue;
                }
            }
        }
        out.push(c);
    }
    out
}

/// 非 Windows 平台无需处理
#[cfg(not(windows))]
fn wrap_command_for_windows(_obj: &mut Map<String, Value>) {
    // 非 Windows 平台不做任何处理
}

/// 检测路径是否为 WSL 网络路径（如 \\wsl$\Ubuntu\... 或 \\wsl.localhost\Ubuntu\...）
/// WSL 环境运行的是 Linux，不需要 cmd /c 包装
/// 注意：仅检测直接 UNC 路径，映射磁盘符（如 Z: -> \\wsl$\...）无法检测
#[cfg(windows)]
fn is_wsl_path(path: &Path) -> bool {
    use std::path::{Component, Prefix};
    if let Some(Component::Prefix(prefix)) = path.components().next() {
        match prefix.kind() {
            Prefix::UNC(server, _) | Prefix::VerbatimUNC(server, _) => {
                let s = server.to_string_lossy();
                s.eq_ignore_ascii_case("wsl$") || s.eq_ignore_ascii_case("wsl.localhost")
            }
            _ => false,
        }
    } else {
        false
    }
}

#[cfg(not(windows))]
fn is_wsl_path(_path: &Path) -> bool {
    false
}

/// Undo the `cmd /c <npx|npm|…> args…` wrapping cc-switch applies when writing
/// `~/.claude.json` on Windows, so the DB keeps the portable spec (CFG-30).
/// Only the exact form produced by `wrap_command_for_windows` is unwrapped.
pub fn unwrap_windows_command(spec: &Value) -> Value {
    let mut out = spec.clone();
    let Some(obj) = out.as_object_mut() else {
        return out;
    };
    let is_cmd = obj
        .get("command")
        .and_then(Value::as_str)
        .is_some_and(|c| c.eq_ignore_ascii_case("cmd") || c.eq_ignore_ascii_case("cmd.exe"));
    if !is_cmd {
        return out;
    }
    let Some(args) = obj.get("args").and_then(Value::as_array).cloned() else {
        return out;
    };
    let (Some(flag), Some(inner)) = (
        args.first().and_then(Value::as_str),
        args.get(1).and_then(Value::as_str),
    ) else {
        return out;
    };
    let inner_name = Path::new(inner)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(inner);
    if !flag.eq_ignore_ascii_case("/c")
        || !WINDOWS_WRAP_COMMANDS
            .iter()
            .any(|c| inner_name.eq_ignore_ascii_case(c))
    {
        return out;
    }
    obj.insert("command".into(), Value::String(inner.to_string()));
    let rest: Vec<Value> = args[2..]
        .iter()
        .map(|arg| match arg.as_str() {
            Some(s) => Value::String(cmd_c_unescape_arg(s)),
            None => arg.clone(),
        })
        .collect();
    if rest.is_empty() {
        obj.remove("args");
    } else {
        obj.insert("args".into(), Value::Array(rest));
    }
    out
}

/// Convert a unified MCP spec into the entry written to `~/.claude.json`.
fn to_claude_native(id: &str, spec: &Value, is_wsl_target: bool) -> Result<Value, AppError> {
    let mut obj = if let Some(map) = spec.as_object() {
        map.clone()
    } else {
        return Err(AppError::McpValidation(format!(
            "MCP 服务器 '{id}' 不是对象"
        )));
    };

    if let Some(server_val) = obj.remove("server") {
        let server_obj = server_val.as_object().cloned().ok_or_else(|| {
            AppError::McpValidation(format!("MCP 服务器 '{id}' server 字段不是对象"))
        })?;
        obj = server_obj;
    }

    obj.remove("enabled");
    obj.remove("source");
    obj.remove("id");
    obj.remove("name");
    obj.remove("description");
    obj.remove("tags");
    obj.remove("homepage");
    obj.remove("docs");

    // Windows 平台自动包装 npx/npm 等命令为 cmd /c 格式（WSL 路径除外）
    if !is_wsl_target {
        wrap_command_for_windows(&mut obj);
    }

    Ok(Value::Object(obj))
}

/// Apply a batch of MCP changes to `~/.claude.json` with a single
/// read-modify-write (CFG-41).
///
/// `~/.claude.json` is owned by Claude Code, which rewrites it constantly:
/// the update re-reads the file right before writing and re-applies the
/// change when it moved meanwhile, so Claude Code's own edits are never
/// overwritten (CFG-39). Existing entries keep their position, other servers
/// are not touched, and nothing is written when the result is unchanged.
pub fn project_mcp_servers(enabled: &[(&str, &Value)], disabled: &[&str]) -> Result<(), AppError> {
    let path = user_config_path();
    let is_wsl_target = is_wsl_path(&path);
    let natives = enabled
        .iter()
        .map(|(id, spec)| Ok((id.to_string(), to_claude_native(id, spec, is_wsl_target)?)))
        .collect::<Result<Vec<_>, AppError>>()?;

    crate::adapters::live_file::update_json_file(
        &path,
        crate::adapters::live_file::WriteOptions::default().secret(),
        |root| {
            let obj = root
                .as_object_mut()
                .ok_or_else(|| AppError::Config("~/.claude.json 根必须是对象".into()))?;
            if natives.is_empty() && !obj.contains_key("mcpServers") {
                return Ok(());
            }
            let servers = obj
                .entry("mcpServers")
                .or_insert_with(|| Value::Object(Map::new()));
            let map = servers.as_object_mut().ok_or_else(|| {
                AppError::Config("~/.claude.json 的 mcpServers 必须是对象".into())
            })?;
            for (id, native) in &natives {
                map.insert(id.clone(), native.clone());
            }
            for id in disabled {
                map.shift_remove(*id);
            }
            Ok(())
        },
    )?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct McpStatus {
    pub user_config_path: String,
    pub user_config_exists: bool,
    pub server_count: usize,
}

fn user_config_path() -> PathBuf {
    get_claude_mcp_path()
}

fn read_json_value(path: &Path) -> Result<Value, AppError> {
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let content = fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
    let value: Value = serde_json::from_str(&content).map_err(|e| AppError::json(path, e))?;
    Ok(value)
}

fn write_json_value(path: &Path, value: &Value) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    let json =
        serde_json::to_string_pretty(value).map_err(|e| AppError::JsonSerialize { source: e })?;
    atomic_write_secret(path, json.as_bytes())
}

pub fn get_mcp_status() -> Result<McpStatus, AppError> {
    let path = user_config_path();
    let (exists, count) = if path.exists() {
        let v = read_json_value(&path)?;
        let servers = v.get("mcpServers").and_then(|x| x.as_object());
        (true, servers.map(|m| m.len()).unwrap_or(0))
    } else {
        (false, 0)
    };

    Ok(McpStatus {
        user_config_path: path.to_string_lossy().to_string(),
        user_config_exists: exists,
        server_count: count,
    })
}

pub fn read_mcp_json() -> Result<Option<String>, AppError> {
    let path = user_config_path();
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    Ok(Some(content))
}

/// 在 ~/.claude.json 根对象写入 hasCompletedOnboarding=true（用于跳过 Claude Code 初次安装确认）
/// 仅增量写入该字段，其他字段保持不变
pub fn set_has_completed_onboarding() -> Result<bool, AppError> {
    let path = user_config_path();
    let mut root = if path.exists() {
        read_json_value(&path)?
    } else {
        serde_json::json!({})
    };

    let obj = root
        .as_object_mut()
        .ok_or_else(|| AppError::Config("~/.claude.json 根必须是对象".into()))?;

    let already = obj
        .get("hasCompletedOnboarding")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if already {
        return Ok(false);
    }

    obj.insert("hasCompletedOnboarding".into(), Value::Bool(true));
    write_json_value(&path, &root)?;
    Ok(true)
}

/// 删除 ~/.claude.json 根对象的 hasCompletedOnboarding 字段（恢复 Claude Code 初次安装确认）
/// 仅增量删除该字段，其他字段保持不变
pub fn clear_has_completed_onboarding() -> Result<bool, AppError> {
    let path = user_config_path();
    if !path.exists() {
        return Ok(false);
    }

    let mut root = read_json_value(&path)?;
    let obj = root
        .as_object_mut()
        .ok_or_else(|| AppError::Config("~/.claude.json 根必须是对象".into()))?;

    let existed = obj.remove("hasCompletedOnboarding").is_some();
    if !existed {
        return Ok(false);
    }

    write_json_value(&path, &root)?;
    Ok(true)
}

pub fn upsert_mcp_server(id: &str, spec: Value) -> Result<bool, AppError> {
    if id.trim().is_empty() {
        return Err(AppError::localized(
            "mcp.error.idRequired",
            "MCP 服务器 ID 不能为空",
            "The MCP server ID is required",
        ));
    }
    // 基础字段校验（尽量宽松）
    if !spec.is_object() {
        return Err(AppError::localized(
            "mcp.error.jsonInvalid",
            "MCP 服务器定义必须为 JSON 对象",
            "The MCP server definition must be a JSON object",
        ));
    }
    let t_opt = spec.get("type").and_then(|x| x.as_str());
    let is_stdio = t_opt.map(|t| t == "stdio").unwrap_or(true); // 兼容缺省（按 stdio 处理）
    let is_http = t_opt.map(|t| t == "http").unwrap_or(false);
    let is_sse = t_opt.map(|t| t == "sse").unwrap_or(false);
    if !(is_stdio || is_http || is_sse) {
        return Err(AppError::localized(
            "mcp.error.jsonInvalid",
            "MCP 服务器 type 必须是 'stdio'、'http' 或 'sse'（或省略表示 stdio）",
            "The MCP server type must be 'stdio', 'http' or 'sse' (omit it for stdio)",
        ));
    }

    // stdio 类型必须有 command
    if is_stdio {
        let cmd = spec.get("command").and_then(|x| x.as_str()).unwrap_or("");
        if cmd.is_empty() {
            return Err(AppError::localized(
                "mcp.error.commandRequired",
                "stdio 类型的 MCP 服务器缺少 command 字段",
                "A stdio MCP server needs a command",
            ));
        }
    }

    // http/sse 类型必须有 url
    if is_http || is_sse {
        let url = spec.get("url").and_then(|x| x.as_str()).unwrap_or("");
        if url.is_empty() {
            return Err(AppError::localized(
                "mcp.wizard.urlRequired",
                if is_http {
                    "http 类型的 MCP 服务器缺少 url 字段"
                } else {
                    "sse 类型的 MCP 服务器缺少 url 字段"
                },
                if is_http {
                    "An http MCP server needs a URL"
                } else {
                    "An sse MCP server needs a URL"
                },
            ));
        }
    }

    let path = user_config_path();
    let mut root = if path.exists() {
        read_json_value(&path)?
    } else {
        serde_json::json!({})
    };

    // 确保 mcpServers 对象存在
    {
        let obj = root
            .as_object_mut()
            .ok_or_else(|| AppError::Config("mcp.json 根必须是对象".into()))?;
        if !obj.contains_key("mcpServers") {
            obj.insert("mcpServers".into(), serde_json::json!({}));
        }
    }

    let before = root.clone();
    if let Some(servers) = root.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
        servers.insert(id.to_string(), spec);
    }

    if before == root && path.exists() {
        return Ok(false);
    }

    write_json_value(&path, &root)?;
    Ok(true)
}

pub fn delete_mcp_server(id: &str) -> Result<bool, AppError> {
    if id.trim().is_empty() {
        return Err(AppError::localized(
            "mcp.error.idRequired",
            "MCP 服务器 ID 不能为空",
            "The MCP server ID is required",
        ));
    }
    let path = user_config_path();
    if !path.exists() {
        return Ok(false);
    }
    let mut root = read_json_value(&path)?;
    let Some(servers) = root.get_mut("mcpServers").and_then(|v| v.as_object_mut()) else {
        return Ok(false);
    };
    let existed = servers.remove(id).is_some();
    if !existed {
        return Ok(false);
    }
    write_json_value(&path, &root)?;
    Ok(true)
}

pub fn validate_command_in_path(cmd: &str) -> Result<bool, AppError> {
    if cmd.trim().is_empty() {
        return Ok(false);
    }
    // 如果包含路径分隔符，直接判断是否存在可执行文件
    if cmd.contains('/') || cmd.contains('\\') {
        return Ok(Path::new(cmd).exists());
    }

    let path_var = env::var_os("PATH").unwrap_or_default();
    let paths = env::split_paths(&path_var);

    #[cfg(windows)]
    let exts: Vec<String> = env::var("PATHEXT")
        .unwrap_or(".COM;.EXE;.BAT;.CMD".into())
        .split(';')
        .map(|s| s.trim().to_uppercase())
        .collect();

    for p in paths {
        let candidate = p.join(cmd);
        if candidate.is_file() {
            return Ok(true);
        }
        #[cfg(windows)]
        {
            for ext in &exts {
                let cand = p.join(format!("{}{}", cmd, ext));
                if cand.is_file() {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

/// 将给定的启用 MCP 服务器映射写入到用户级 ~/.claude.json 的 mcpServers 字段
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
    // 检测目标路径是否为 WSL，若是则跳过 cmd /c 包装
    let is_wsl_target = is_wsl_path(&path);
    if is_wsl_target {
        log::info!("检测到 WSL 路径，跳过 cmd /c 包装: {}", path.display());
    }
    let mut out: Map<String, Value> = Map::new();
    for (id, spec) in servers.iter() {
        out.insert(id.clone(), to_claude_native(id, spec, is_wsl_target)?);
    }

    {
        let obj = root
            .as_object_mut()
            .ok_or_else(|| AppError::Config("~/.claude.json 根必须是对象".into()))?;
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
    fn cmd_c_escape_neutralises_shell_metacharacters() {
        let args = vec![
            json!("-y"),
            json!("a&calc"),
            json!("x|y"),
            json!("%APPDATA%"),
        ];
        assert_eq!(
            cmd_c_escape_args(&args).unwrap(),
            vec![
                json!("-y"),
                json!("a^&calc"),
                json!("x^|y"),
                json!("%APPDATA%")
            ]
        );
        // quoted by libuv (contains a space): already literal for cmd
        assert_eq!(
            cmd_c_escape_args(&[json!("a & b")]).unwrap(),
            vec![json!("a & b")]
        );
        // an embedded quote plus a metacharacter cannot be passed safely
        assert!(cmd_c_escape_args(&[json!("a\"&calc")]).is_none());
    }

    #[test]
    fn unwrap_reverses_cmd_c_escaping() {
        let wrapped = json!({
            "command": "cmd",
            "args": ["/c", "npx", "-y", "a^&calc", "a & b"]
        });
        let spec = unwrap_windows_command(&wrapped);
        assert_eq!(spec["command"], json!("npx"));
        assert_eq!(spec["args"], json!(["-y", "a&calc", "a & b"]));
    }

    #[test]
    fn unwrap_windows_command_restores_portable_spec() {
        let wrapped =
            json!({"type": "stdio", "command": "cmd", "args": ["/c", "npx", "-y", "pkg"]});
        assert_eq!(
            unwrap_windows_command(&wrapped),
            json!({"type": "stdio", "command": "npx", "args": ["-y", "pkg"]})
        );
        let only_cmd = json!({"command": "CMD.EXE", "args": ["/C", "node"]});
        assert_eq!(
            unwrap_windows_command(&only_cmd),
            json!({"command": "node"})
        );
        // user-written cmd wrappers around other programs are left alone
        let bat = json!({"command": "cmd", "args": ["/c", "run.bat"]});
        assert_eq!(unwrap_windows_command(&bat), bat);
        let plain = json!({"command": "npx", "args": ["-y"]});
        assert_eq!(unwrap_windows_command(&plain), plain);
    }

    /// 测试 Windows 命令包装功能
    /// 由于使用条件编译，在非 Windows 平台上测试的是空函数
    #[test]
    fn test_wrap_command_for_windows_npx() {
        let mut obj = json!({"command": "npx", "args": ["-y", "@upstash/context7-mcp"]})
            .as_object()
            .unwrap()
            .clone();
        wrap_command_for_windows(&mut obj);

        #[cfg(windows)]
        {
            assert_eq!(obj["command"], "cmd");
            assert_eq!(
                obj["args"],
                json!(["/c", "npx", "-y", "@upstash/context7-mcp"])
            );
        }

        #[cfg(not(windows))]
        {
            // 非 Windows 平台不做任何处理
            assert_eq!(obj["command"], "npx");
        }
    }

    #[test]
    fn test_wrap_command_for_windows_npm() {
        let mut obj = json!({"command": "npm", "args": ["run", "start"]})
            .as_object()
            .unwrap()
            .clone();
        wrap_command_for_windows(&mut obj);

        #[cfg(windows)]
        {
            assert_eq!(obj["command"], "cmd");
            assert_eq!(obj["args"], json!(["/c", "npm", "run", "start"]));
        }
    }

    #[test]
    fn test_wrap_command_for_windows_already_cmd() {
        // 已经是 cmd 的不应该重复包装
        let mut obj = json!({"command": "cmd", "args": ["/c", "npx", "-y", "foo"]})
            .as_object()
            .unwrap()
            .clone();
        wrap_command_for_windows(&mut obj);

        assert_eq!(obj["command"], "cmd");
        // args 应该保持不变，不会变成 ["/c", "cmd", "/c", "npx", ...]
        assert_eq!(obj["args"], json!(["/c", "npx", "-y", "foo"]));
    }

    #[test]
    fn test_wrap_command_for_windows_http_type_skipped() {
        // http 类型不应该被处理
        let mut obj = json!({"type": "http", "url": "https://example.com/mcp"})
            .as_object()
            .unwrap()
            .clone();
        wrap_command_for_windows(&mut obj);

        assert!(!obj.contains_key("command"));
        assert_eq!(obj["url"], "https://example.com/mcp");
    }

    #[test]
    fn test_wrap_command_for_windows_other_command_skipped() {
        // 非目标命令（如 python）不应该被包装
        let mut obj = json!({"command": "python", "args": ["server.py"]})
            .as_object()
            .unwrap()
            .clone();
        wrap_command_for_windows(&mut obj);

        // python 不在 WINDOWS_WRAP_COMMANDS 列表中，不应该被包装
        assert_eq!(obj["command"], "python");
        assert_eq!(obj["args"], json!(["server.py"]));
    }

    #[test]
    fn test_wrap_command_for_windows_no_args() {
        // 没有 args 的情况
        let mut obj = json!({"command": "npx"}).as_object().unwrap().clone();
        wrap_command_for_windows(&mut obj);

        #[cfg(windows)]
        {
            assert_eq!(obj["command"], "cmd");
            assert_eq!(obj["args"], json!(["/c", "npx"]));
        }
    }

    #[test]
    fn test_wrap_command_for_windows_with_cmd_suffix() {
        // 处理 npx.cmd 格式
        let mut obj = json!({"command": "npx.cmd", "args": ["-y", "foo"]})
            .as_object()
            .unwrap()
            .clone();
        wrap_command_for_windows(&mut obj);

        #[cfg(windows)]
        {
            assert_eq!(obj["command"], "cmd");
            assert_eq!(obj["args"], json!(["/c", "npx.cmd", "-y", "foo"]));
        }
    }

    #[test]
    fn test_wrap_command_for_windows_case_insensitive() {
        // 大小写不敏感
        let mut obj = json!({"command": "NPX", "args": ["-y", "foo"]})
            .as_object()
            .unwrap()
            .clone();
        wrap_command_for_windows(&mut obj);

        #[cfg(windows)]
        {
            assert_eq!(obj["command"], "cmd");
            assert_eq!(obj["args"], json!(["/c", "NPX", "-y", "foo"]));
        }
    }

    /// 测试 WSL 路径检测功能
    #[test]
    fn test_is_wsl_path_wsl_dollar() {
        // wsl$ 格式 - 各种发行版
        #[cfg(windows)]
        {
            assert!(is_wsl_path(Path::new(r"\\wsl$\Ubuntu\home\user\.claude")));
            assert!(is_wsl_path(Path::new(r"\\wsl$\Debian\home\user\.claude")));
            assert!(is_wsl_path(Path::new(
                r"\\wsl$\openSUSE-Leap-15.2\home\user"
            )));
            assert!(is_wsl_path(Path::new(r"\\wsl$\kali-linux\home\user")));
            assert!(is_wsl_path(Path::new(r"\\wsl$\Arch\home\user")));
            assert!(is_wsl_path(Path::new(r"\\wsl$\Alpine\home\user")));
            assert!(is_wsl_path(Path::new(r"\\wsl$\Fedora\home\user")));
        }

        #[cfg(not(windows))]
        {
            // 非 Windows 平台始终返回 false
            assert!(!is_wsl_path(Path::new(r"\\wsl$\Ubuntu\home\user\.claude")));
        }
    }

    #[test]
    fn test_is_wsl_path_wsl_localhost() {
        // wsl.localhost 格式
        #[cfg(windows)]
        {
            assert!(is_wsl_path(Path::new(
                r"\\wsl.localhost\Ubuntu\home\user\.claude"
            )));
            assert!(is_wsl_path(Path::new(r"\\wsl.localhost\Debian\home\user")));
            assert!(is_wsl_path(Path::new(
                r"\\wsl.localhost\openSUSE-Leap-15.2\home\user"
            )));
        }
    }

    #[test]
    fn test_is_wsl_path_case_insensitive() {
        // 大小写不敏感
        #[cfg(windows)]
        {
            assert!(is_wsl_path(Path::new(r"\\WSL$\Ubuntu\home\user")));
            assert!(is_wsl_path(Path::new(r"\\Wsl$\Ubuntu\home\user")));
            assert!(is_wsl_path(Path::new(r"\\WSL.LOCALHOST\Ubuntu\home\user")));
            assert!(is_wsl_path(Path::new(r"\\Wsl.Localhost\Ubuntu\home\user")));
        }
    }

    #[test]
    fn test_is_wsl_path_non_wsl() {
        // 非 WSL 路径
        assert!(!is_wsl_path(Path::new(r"C:\Users\user\.claude")));
        assert!(!is_wsl_path(Path::new(r"D:\Workspace\project")));
        #[cfg(windows)]
        {
            assert!(!is_wsl_path(Path::new(r"\\server\share\path")));
            assert!(!is_wsl_path(Path::new(r"\\localhost\c$\Users")));
            assert!(!is_wsl_path(Path::new(r"\\192.168.1.1\share")));
        }
    }
}
