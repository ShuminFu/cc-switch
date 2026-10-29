use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{atomic_write, get_claude_mcp_path};
use crate::error::AppError;

/// 需要在 Windows 上用 cmd /c 包装的命令
/// 这些命令在 Windows 上实际是 .cmd 批处理文件，需要通过 cmd /c 来执行
#[cfg(windows)]
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

    let mut new_args = vec![Value::String("/c".into()), Value::String(cmd.into())];
    new_args.extend(original_args);

    obj.insert("command".into(), Value::String("cmd".into()));
    obj.insert("args".into(), Value::Array(new_args));
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    atomic_write(path, json.as_bytes())
}

/// 读-改-写冲突时最多尝试的次数（含首次）
const MAX_UPDATE_ATTEMPTS: usize = 3;

/// 读取时记录的文件指纹：长度、修改时间与内容哈希；文件不存在时为 None
#[derive(Debug, PartialEq, Eq)]
struct FileFingerprint {
    len: u64,
    modified: Option<std::time::SystemTime>,
    hash: [u8; 32],
}

fn fingerprint_of(path: &Path, bytes: &[u8]) -> Result<FileFingerprint, AppError> {
    use sha2::{Digest, Sha256};
    let meta = fs::metadata(path).map_err(|e| AppError::io(path, e))?;
    Ok(FileFingerprint {
        len: meta.len(),
        modified: meta.modified().ok(),
        hash: Sha256::digest(bytes).into(),
    })
}

/// 读取文件内容及其指纹；文件不存在时返回空对象与 None
fn read_with_fingerprint(path: &Path) -> Result<(Value, Option<FileFingerprint>), AppError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok((serde_json::json!({}), None));
        }
        Err(e) => return Err(AppError::io(path, e)),
    };
    let fingerprint = fingerprint_of(path, &bytes)?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|e| AppError::json(path, e))?;
    Ok((value, Some(fingerprint)))
}

/// 仅重新计算指纹（不解析 JSON），用于写入前确认文件未被他人修改
fn current_fingerprint(path: &Path) -> Result<Option<FileFingerprint>, AppError> {
    match fs::read(path) {
        Ok(bytes) => fingerprint_of(path, &bytes).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(AppError::io(path, e)),
    }
}

#[cfg(test)]
type BeforeCommitHook = Box<dyn FnMut(&Path)>;

#[cfg(test)]
thread_local! {
    /// 测试钩子：在修改完成、写入前的指纹校验之前调用，用于模拟 Claude Code 并发保存
    static BEFORE_COMMIT_HOOK: std::cell::RefCell<Option<BeforeCommitHook>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn run_before_commit_hook(path: &Path) {
    BEFORE_COMMIT_HOOK.with(|hook| {
        if let Some(f) = hook.borrow_mut().as_mut() {
            f(path);
        }
    });
}

#[cfg(not(test))]
fn run_before_commit_hook(_path: &Path) {}

/// 以乐观并发方式对 ~/.claude.json 做读-改-写
///
/// 读取时记录文件的长度、修改时间与哈希；`mutate` 在内存中对根对象做键级修改，
/// 返回 false 表示无需写入。写入前再次校验指纹，若文件已被他人（如 Claude Code）
/// 修改，则重新读取并重新应用 `mutate`，最多尝试 [`MAX_UPDATE_ATTEMPTS`] 次；
/// 次数用尽后按最后一次读取的结果写入，与旧行为一致。
///
/// 返回是否实际写入了文件。
fn update_json_file<F>(path: &Path, mut mutate: F) -> Result<bool, AppError>
where
    F: FnMut(&mut Value, bool) -> Result<bool, AppError>,
{
    for attempt in 1..=MAX_UPDATE_ATTEMPTS {
        let (mut root, fingerprint) = read_with_fingerprint(path)?;
        if !mutate(&mut root, fingerprint.is_some())? {
            return Ok(false);
        }

        run_before_commit_hook(path);

        if current_fingerprint(path)? != fingerprint {
            if attempt < MAX_UPDATE_ATTEMPTS {
                log::info!(
                    "{} 在读取后被修改，重新读取并应用变更（第 {attempt} 次）",
                    path.display()
                );
                continue;
            }
            log::warn!(
                "{} 持续被并发修改，已重试 {MAX_UPDATE_ATTEMPTS} 次，按最后一次读取结果写入",
                path.display()
            );
        }

        write_json_value(path, &root)?;
        return Ok(true);
    }
    unreachable!("MAX_UPDATE_ATTEMPTS 至少为 1")
}

fn root_object_mut(root: &mut Value) -> Result<&mut Map<String, Value>, AppError> {
    root.as_object_mut()
        .ok_or_else(|| AppError::Config("~/.claude.json 根必须是对象".into()))
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
    update_json_file(&user_config_path(), |root, _exists| {
        let obj = root_object_mut(root)?;
        let already = obj
            .get("hasCompletedOnboarding")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if already {
            return Ok(false);
        }
        obj.insert("hasCompletedOnboarding".into(), Value::Bool(true));
        Ok(true)
    })
}

/// 删除 ~/.claude.json 根对象的 hasCompletedOnboarding 字段（恢复 Claude Code 初次安装确认）
/// 仅增量删除该字段，其他字段保持不变
pub fn clear_has_completed_onboarding() -> Result<bool, AppError> {
    update_json_file(&user_config_path(), |root, exists| {
        if !exists {
            return Ok(false);
        }
        Ok(root_object_mut(root)?
            .remove("hasCompletedOnboarding")
            .is_some())
    })
}

pub fn upsert_mcp_server(id: &str, spec: Value) -> Result<bool, AppError> {
    if id.trim().is_empty() {
        return Err(AppError::InvalidInput("MCP 服务器 ID 不能为空".into()));
    }
    // 基础字段校验（尽量宽松）
    if !spec.is_object() {
        return Err(AppError::McpValidation(
            "MCP 服务器定义必须为 JSON 对象".into(),
        ));
    }
    let t_opt = spec.get("type").and_then(|x| x.as_str());
    let is_stdio = t_opt.map(|t| t == "stdio").unwrap_or(true); // 兼容缺省（按 stdio 处理）
    let is_http = t_opt.map(|t| t == "http").unwrap_or(false);
    let is_sse = t_opt.map(|t| t == "sse").unwrap_or(false);
    if !(is_stdio || is_http || is_sse) {
        return Err(AppError::McpValidation(
            "MCP 服务器 type 必须是 'stdio'、'http' 或 'sse'（或省略表示 stdio）".into(),
        ));
    }

    // stdio 类型必须有 command
    if is_stdio {
        let cmd = spec.get("command").and_then(|x| x.as_str()).unwrap_or("");
        if cmd.is_empty() {
            return Err(AppError::McpValidation(
                "stdio 类型的 MCP 服务器缺少 command 字段".into(),
            ));
        }
    }

    // http/sse 类型必须有 url
    if is_http || is_sse {
        let url = spec.get("url").and_then(|x| x.as_str()).unwrap_or("");
        if url.is_empty() {
            return Err(AppError::McpValidation(if is_http {
                "http 类型的 MCP 服务器缺少 url 字段".into()
            } else {
                "sse 类型的 MCP 服务器缺少 url 字段".into()
            }));
        }
    }

    update_json_file(&user_config_path(), |root, exists| {
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
            servers.insert(id.to_string(), spec.clone());
        }

        Ok(!(before == *root && exists))
    })
}

pub fn delete_mcp_server(id: &str) -> Result<bool, AppError> {
    if id.trim().is_empty() {
        return Err(AppError::InvalidInput("MCP 服务器 ID 不能为空".into()));
    }
    update_json_file(&user_config_path(), |root, exists| {
        if !exists {
            return Ok(false);
        }
        let Some(servers) = root.get_mut("mcpServers").and_then(|v| v.as_object_mut()) else {
            return Ok(false);
        };
        Ok(servers.remove(id).is_some())
    })
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

/// 读取 ~/.claude.json 中的 mcpServers 映射
pub fn read_mcp_servers_map() -> Result<std::collections::HashMap<String, Value>, AppError> {
    let path = user_config_path();
    if !path.exists() {
        return Ok(std::collections::HashMap::new());
    }

    let root = read_json_value(&path)?;
    let servers = root
        .get("mcpServers")
        .and_then(|v| v.as_object())
        .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();

    Ok(servers)
}

/// 将给定的启用 MCP 服务器映射写入到用户级 ~/.claude.json 的 mcpServers 字段
/// 仅覆盖 mcpServers，其他字段保持不变
pub fn set_mcp_servers_map(
    servers: &std::collections::HashMap<String, Value>,
) -> Result<(), AppError> {
    set_mcp_servers_map_at(&user_config_path(), servers)
}

fn set_mcp_servers_map_at(
    path: &Path,
    servers: &std::collections::HashMap<String, Value>,
) -> Result<(), AppError> {
    // 构建 mcpServers 对象：移除 UI 辅助字段（enabled/source），仅保留实际 MCP 规范
    // 检测目标路径是否为 WSL，若是则跳过 cmd /c 包装
    let is_wsl_target = is_wsl_path(path);
    if is_wsl_target {
        log::info!("检测到 WSL 路径，跳过 cmd /c 包装: {}", path.display());
    }
    let mut out: Map<String, Value> = Map::new();
    for (id, spec) in servers.iter() {
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

        out.insert(id.clone(), Value::Object(obj));
    }

    let out = Value::Object(out);
    update_json_file(path, |root, _exists| {
        root_object_mut(root)?.insert("mcpServers".into(), out.clone());
        Ok(true)
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 设置写入前钩子，返回时自动清除
    struct HookGuard;

    impl HookGuard {
        fn install(f: impl FnMut(&Path) + 'static) -> Self {
            BEFORE_COMMIT_HOOK.with(|hook| *hook.borrow_mut() = Some(Box::new(f)));
            HookGuard
        }
    }

    impl Drop for HookGuard {
        fn drop(&mut self) {
            BEFORE_COMMIT_HOOK.with(|hook| *hook.borrow_mut() = None);
        }
    }

    /// 模拟 Claude Code 在读取与写入之间保存了文件（递增 numStartups）
    fn bump_num_startups(path: &Path) {
        let mut root: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        let n = root["numStartups"].as_u64().unwrap_or(0);
        root["numStartups"] = json!(n + 1);
        fs::write(path, serde_json::to_vec_pretty(&root).unwrap()).unwrap();
    }

    fn read_root(path: &Path) -> Value {
        serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
    }

    #[test]
    fn set_mcp_servers_map_keeps_concurrent_num_startups_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".claude.json");
        fs::write(
            &path,
            serde_json::to_vec_pretty(&json!({"numStartups": 5, "mcpServers": {}})).unwrap(),
        )
        .unwrap();

        // 仅在第一次写入前模拟一次并发保存
        let mut fired = false;
        let _guard = HookGuard::install(move |p| {
            if !fired {
                fired = true;
                bump_num_startups(p);
            }
        });

        let mut servers = std::collections::HashMap::new();
        servers.insert(
            "echo".to_string(),
            json!({"command": "echo", "args": ["hi"]}),
        );
        set_mcp_servers_map_at(&path, &servers).unwrap();

        let root = read_root(&path);
        assert_eq!(root["numStartups"], json!(6));
        assert_eq!(root["mcpServers"]["echo"]["command"], json!("echo"));
    }

    #[test]
    fn update_json_file_gives_up_after_max_attempts_and_still_applies_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".claude.json");
        fs::write(&path, br#"{"numStartups": 0}"#).unwrap();

        // 每次写入前都被并发修改：重试用尽后仍写入本次变更，且保留最后一次读取到的内容
        let calls = std::rc::Rc::new(std::cell::Cell::new(0usize));
        let calls_in_hook = calls.clone();
        let _guard = HookGuard::install(move |p| {
            calls_in_hook.set(calls_in_hook.get() + 1);
            bump_num_startups(p);
        });

        let mut applied = 0;
        let wrote = update_json_file(&path, |root, _| {
            applied += 1;
            root_object_mut(root)?.insert("hasCompletedOnboarding".into(), Value::Bool(true));
            Ok(true)
        })
        .unwrap();

        assert!(wrote);
        assert_eq!(applied, MAX_UPDATE_ATTEMPTS);
        assert_eq!(calls.get(), MAX_UPDATE_ATTEMPTS);
        let root = read_root(&path);
        assert_eq!(root["hasCompletedOnboarding"], json!(true));
        assert_eq!(root["numStartups"], json!(MAX_UPDATE_ATTEMPTS as u64 - 1));
    }

    #[test]
    fn update_json_file_skips_write_when_mutation_reports_no_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".claude.json");

        let wrote = update_json_file(&path, |_, exists| {
            assert!(!exists);
            Ok(false)
        })
        .unwrap();

        assert!(!wrote);
        assert!(!path.exists());
    }

    #[test]
    fn update_json_file_creates_missing_file_and_detects_concurrent_creation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".claude.json");

        // 读取时文件不存在，写入前被 Claude Code 创建：应重新读取并保留其内容
        let mut fired = false;
        let _guard = HookGuard::install(move |p| {
            if !fired {
                fired = true;
                fs::write(p, br#"{"numStartups": 1}"#).unwrap();
            }
        });

        let wrote = update_json_file(&path, |root, _| {
            root_object_mut(root)?.insert("mcpServers".into(), json!({}));
            Ok(true)
        })
        .unwrap();

        assert!(wrote);
        let root = read_root(&path);
        assert_eq!(root["numStartups"], json!(1));
        assert_eq!(root["mcpServers"], json!({}));
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
