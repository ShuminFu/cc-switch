use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use crate::error::AppError;

/// 获取用户主目录，带回退和日志
///
/// ## Windows 注意事项
///
/// - `dirs::home_dir()` 在 Windows 上使用 `SHGetKnownFolderPath(FOLDERID_Profile)`，
///   返回的是真实用户目录（类似 `C:\\Users\\Alice`），与 v3.10.2 行为一致。
/// - 不要直接使用 `HOME` 环境变量：它可能由 Git/Cygwin/MSYS 等第三方工具注入，
///   且不一定等于用户目录，可能导致 `.cc-switch/cc-switch.db` 路径变化，从而“看起来像数据丢失”。
///
/// ## 测试隔离
///
/// 为了让 Windows CI/本地测试能稳定隔离真实用户数据，可通过 `CC_SWITCH_TEST_HOME`
/// 显式覆盖 home dir（仅用于测试/调试场景）。该覆盖只在 `cfg(test)` 或
/// `test-hooks` feature 下生效（TEST-37）：发布构建忽略此环境变量。
pub fn get_home_dir() -> PathBuf {
    #[cfg(test)]
    if let Some(home) = thread_test_home() {
        return home;
    }
    if let Some(home) = test_home_override() {
        return home;
    }

    #[allow(clippy::disallowed_methods)] // the one sanctioned call site (TEST-38)
    dirs::home_dir().unwrap_or_else(|| {
        log::warn!("无法获取用户主目录，回退到当前目录");
        PathBuf::from(".")
    })
}

/// Where a tool's config directory was resolved from (CFG-12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "var")]
pub enum ConfigDirSource {
    /// The directory configured in cc-switch settings.
    Override,
    /// The tool's own environment variable (e.g. `CODEX_HOME`).
    Env(&'static str),
    /// The platform default (e.g. `~/.codex`).
    Default,
}

/// The `CC_SWITCH_TEST_HOME` override, if active.
///
/// TEST-37: the override is a test seam, so it is honoured only in unit tests
/// and in builds with the `test-hooks` feature (enabled for integration tests
/// through the self-referencing dev-dependency in Cargo.toml). A release build
/// always returns `None`, so a stray env var cannot redirect the shipped app.
#[cfg(any(test, feature = "test-hooks"))]
pub fn test_home_override() -> Option<PathBuf> {
    let home = std::env::var("CC_SWITCH_TEST_HOME").ok()?;
    let trimmed = home.trim();
    if trimmed.is_empty() {
        return None;
    }
    static WARNED: std::sync::Once = std::sync::Once::new();
    WARNED.call_once(|| log::warn!("CC_SWITCH_TEST_HOME override active: home dir = {trimmed}"));
    Some(PathBuf::from(trimmed))
}

// TEST-09: a per-thread test home. Unit tests that call
// `crate::test_support::isolated_home()` get their own home directory without
// touching process-wide state (env vars, the settings cache), so they need no
// `#[serial]`. Only visible to code running on the test's own thread (not to
// tasks a multi-threaded tokio runtime moves elsewhere).
#[cfg(test)]
thread_local! {
    static THREAD_TEST_HOME: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn thread_test_home() -> Option<PathBuf> {
    THREAD_TEST_HOME.with(|home| home.borrow().clone())
}

#[cfg(test)]
pub(crate) fn set_thread_test_home(home: Option<PathBuf>) -> Option<PathBuf> {
    THREAD_TEST_HOME.with(|slot| std::mem::replace(&mut *slot.borrow_mut(), home))
}

/// True while the current test thread runs with an isolated home: path
/// resolution then ignores process-wide overrides (settings dirs, tool env
/// vars) that concurrently running tests may change. Always false outside
/// unit tests.
#[inline]
pub(crate) fn thread_isolated() -> bool {
    #[cfg(test)]
    {
        thread_test_home().is_some()
    }
    #[cfg(not(test))]
    {
        false
    }
}

/// Release builds ignore `CC_SWITCH_TEST_HOME` (TEST-37).
#[cfg(not(any(test, feature = "test-hooks")))]
#[inline]
pub fn test_home_override() -> Option<PathBuf> {
    None
}

/// Expand a leading `~`, `~/` or `~\\` against [`get_home_dir`], so override
/// directories configured as `~/...` resolve under the test home in tests
/// (TEST-38) instead of the real profile.
pub fn expand_tilde(raw: &str) -> PathBuf {
    if raw == "~" {
        return get_home_dir();
    }
    if let Some(stripped) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        return get_home_dir().join(stripped);
    }
    PathBuf::from(raw)
}

/// Read a tool-native directory environment variable (`CODEX_HOME`,
/// `CLAUDE_CONFIG_DIR`, …). Empty values are ignored and a leading `~` is
/// expanded. Ignored while a test home isolates the process or thread, so a
/// developer's own environment can never redirect test writes.
pub fn tool_env_dir(var: &str) -> Option<PathBuf> {
    if test_home_override().is_some() || thread_isolated() {
        return None;
    }
    let raw = std::env::var_os(var)?;
    let value = raw.to_string_lossy();
    expand_tool_env_value(value.trim())
}

fn expand_tool_env_value(value: &str) -> Option<PathBuf> {
    if value.is_empty() {
        return None;
    }
    if value == "~" {
        return Some(get_home_dir());
    }
    if let Some(rest) = value
        .strip_prefix("~/")
        .or_else(|| value.strip_prefix("~\\"))
    {
        return Some(get_home_dir().join(rest));
    }
    Some(PathBuf::from(value))
}

/// Shared resolution order for every adapter: cc-switch override → the
/// tool's own environment variable → platform default.
pub fn resolve_tool_dir(
    override_dir: Option<PathBuf>,
    env: Option<(&'static str, PathBuf)>,
    default: impl FnOnce() -> PathBuf,
) -> (PathBuf, ConfigDirSource) {
    if let Some(dir) = override_dir {
        return (dir, ConfigDirSource::Override);
    }
    if let Some((var, dir)) = env {
        return (dir, ConfigDirSource::Env(var));
    }
    (default(), ConfigDirSource::Default)
}

/// Claude Code config directory and where it came from.
pub fn resolve_claude_config_dir() -> (PathBuf, ConfigDirSource) {
    resolve_tool_dir(
        crate::settings::get_claude_override_dir(),
        tool_env_dir("CLAUDE_CONFIG_DIR").map(|dir| ("CLAUDE_CONFIG_DIR", dir)),
        || get_home_dir().join(".claude"),
    )
}

/// 获取 Claude Code 配置目录路径
///
/// cc-switch 设置覆盖 → `CLAUDE_CONFIG_DIR` → `~/.claude`。
pub fn get_claude_config_dir() -> PathBuf {
    resolve_claude_config_dir().0
}

/// 默认 Claude MCP 配置文件路径 (~/.claude.json)
pub fn get_default_claude_mcp_path() -> PathBuf {
    get_home_dir().join(".claude.json")
}

fn normalize_path_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component.as_os_str());
                }
            }
            Component::Normal(part) => normalized.push(part),
            Component::RootDir | Component::Prefix(_) => normalized.push(component.as_os_str()),
        }
    }

    normalized
}

fn comparable_path_key(path: &Path) -> String {
    let mut key = normalize_path_lexically(path).to_string_lossy().to_string();

    #[cfg(windows)]
    {
        key = key.replace('\\', "/");
    }

    while key.len() > 1 && key.ends_with('/') {
        key.pop();
    }

    #[cfg(windows)]
    {
        key.make_ascii_lowercase();
    }

    key
}

fn path_eq_lexical(left: &Path, right: &Path) -> bool {
    comparable_path_key(left) == comparable_path_key(right)
}

#[cfg(windows)]
fn derive_wsl_default_mcp_path(dir: &Path) -> Option<PathBuf> {
    use std::path::Prefix;

    let normalized = normalize_path_lexically(dir);
    let mut components = normalized.components();
    let prefix = match components.next()? {
        Component::Prefix(prefix) => prefix,
        _ => return None,
    };

    let server = match prefix.kind() {
        Prefix::UNC(server, _) | Prefix::VerbatimUNC(server, _) => server.to_string_lossy(),
        _ => return None,
    };

    if !server.eq_ignore_ascii_case("wsl$") && !server.eq_ignore_ascii_case("wsl.localhost") {
        return None;
    }

    let mut parts = Vec::new();
    for component in components {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(part) => parts.push(part.to_string_lossy().to_string()),
            Component::ParentDir | Component::Prefix(_) => return None,
        }
    }

    let is_wsl_home_default =
        parts.len() == 3 && parts[0] == "home" && !parts[1].is_empty() && parts[2] == ".claude";
    let is_wsl_root_default = parts.len() == 2 && parts[0] == "root" && parts[1] == ".claude";

    if is_wsl_home_default || is_wsl_root_default {
        return normalized
            .parent()
            .map(|parent| parent.join(".claude.json"));
    }

    None
}

fn default_mcp_path_for_config_dir(dir: &Path) -> Option<PathBuf> {
    let default_config_dir = get_home_dir().join(".claude");
    if path_eq_lexical(dir, &default_config_dir) {
        return Some(get_default_claude_mcp_path());
    }

    #[cfg(windows)]
    {
        if let Some(path) = derive_wsl_default_mcp_path(dir) {
            return Some(path);
        }
    }

    None
}

fn derive_mcp_path_from_override(dir: &Path) -> PathBuf {
    dir.join(".claude.json")
}

/// 获取 Claude MCP 配置文件路径
///
/// Claude Code keeps `.claude.json` inside `CLAUDE_CONFIG_DIR` when that
/// variable is set, so an env-resolved directory is treated like an override.
pub fn get_claude_mcp_path() -> PathBuf {
    let (custom_dir, source) = resolve_claude_config_dir();
    if source == ConfigDirSource::Default {
        return get_default_claude_mcp_path();
    }
    if let Some(path) = default_mcp_path_for_config_dir(&custom_dir) {
        return path;
    }
    derive_mcp_path_from_override(&custom_dir)
}

/// 获取 Claude Code 主配置文件路径
pub fn get_claude_settings_path() -> PathBuf {
    let dir = get_claude_config_dir();
    let settings = dir.join("settings.json");
    if settings.exists() {
        return settings;
    }
    // 兼容旧版命名：若存在旧文件则继续使用
    let legacy = dir.join("claude.json");
    if legacy.exists() {
        return legacy;
    }
    // 默认新建：回落到标准文件名 settings.json（不再生成 claude.json）
    settings
}

/// 获取应用配置目录路径 (~/.cc-switch)
pub fn get_app_config_dir() -> PathBuf {
    if let Some(custom) = crate::app_store::get_app_config_dir_override() {
        return custom;
    }

    let default_dir = get_home_dir().join(".cc-switch");

    // 兼容 v3.10.3：当用户环境存在 `HOME` 且与真实用户目录不同，
    // v3.10.3 可能在 `HOME/.cc-switch/` 下创建/使用了数据库。
    // 这里仅在“默认位置没有数据库”时回退到旧位置，避免再次出现“供应商消失”问题，
    // 同时也避免新安装因为 `HOME` 被设置而写入非预期路径。
    #[cfg(windows)]
    {
        let default_db = default_dir.join("cc-switch.db");
        if !default_db.exists() {
            if let Ok(home_env) = std::env::var("HOME") {
                let trimmed = home_env.trim();
                if !trimmed.is_empty() {
                    let legacy_dir = PathBuf::from(trimmed).join(".cc-switch");
                    if legacy_dir.join("cc-switch.db").exists() {
                        log::info!(
                            "Detected v3.10.3 legacy database at {}, using it instead of {}",
                            legacy_dir.display(),
                            default_dir.display()
                        );
                        return legacy_dir;
                    }
                }
            }
        }
    }

    default_dir
}

/// 获取应用配置文件路径
pub fn get_app_config_path() -> PathBuf {
    get_app_config_dir().join("config.json")
}

/// 清理供应商名称，确保文件名安全
#[allow(dead_code)]
pub fn sanitize_provider_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '-',
            _ => c,
        })
        .collect::<String>()
        .to_lowercase()
}

/// 获取供应商配置文件路径
#[allow(dead_code)]
pub fn get_provider_config_path(provider_id: &str, provider_name: Option<&str>) -> PathBuf {
    let base_name = provider_name
        .map(sanitize_provider_name)
        .unwrap_or_else(|| sanitize_provider_name(provider_id));

    get_claude_config_dir().join(format!("settings-{base_name}.json"))
}

/// 读取 JSON 配置文件
///
/// 容忍 UTF-8/UTF-16 BOM（Windows 记事本/PowerShell 常见），见 CFG-14。
pub fn read_json_file<T: for<'a> Deserialize<'a>>(path: &Path) -> Result<T, AppError> {
    let Some(live) = crate::adapters::live_file::read_text_lenient(path)? else {
        return Err(AppError::Config(format!("文件不存在: {}", path.display())));
    };

    serde_json::from_str(&live.text).map_err(|e| AppError::json(path, e))
}

/// 递归排序 JSON 对象的键（按字母顺序），确保新建文件的输出是确定性的
fn sort_json_keys(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut sorted_map = Map::new();
            let mut keys: Vec<_> = map.keys().collect();
            keys.sort();
            for key in keys {
                sorted_map.insert(key.clone(), sort_json_keys(&map[key]));
            }
            Value::Object(sorted_map)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(sort_json_keys).collect()),
        other => other.clone(),
    }
}

/// 写入 JSON 配置文件
///
/// 已有文件：保留键的插入顺序（读入文件的顺序 / 结构体字段顺序），沿用已有文件的缩进、
/// 结尾换行、BOM 与换行符风格；新文件按键排序、两空格缩进并以换行结尾（CFG-28）。
pub fn write_json_file<T: Serialize>(path: &Path, data: &T) -> Result<(), AppError> {
    write_json_file_with_mode(path, data, NewFileMode::Default)
}

/// 写入携带密钥的 JSON 配置文件：新建文件权限为 0600（已有文件保留原权限）
pub fn write_json_file_secret<T: Serialize>(path: &Path, data: &T) -> Result<(), AppError> {
    write_json_file_with_mode(path, data, NewFileMode::Secret)
}

fn write_json_file_with_mode<T: Serialize>(
    path: &Path,
    data: &T,
    mode: NewFileMode,
) -> Result<(), AppError> {
    // 确保目录存在
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    let json = if path.exists() {
        // 已有文件：保留调用方给出的键顺序（通常就是从该文件读入的顺序）与文件风格
        let style = crate::adapters::live_file::JsonStyle::of_file(path);
        crate::adapters::live_file::to_json_string(data, &style)?
    } else {
        // 新文件：键按字母排序，保证同一逻辑配置的输出确定
        let value =
            serde_json::to_value(data).map_err(|e| AppError::JsonSerialize { source: e })?;
        crate::adapters::live_file::to_json_string(
            &sort_json_keys(&value),
            &crate::adapters::live_file::JsonStyle::default(),
        )?
    };

    if mode == NewFileMode::Secret {
        crate::adapters::live_file::ensure_no_vault_placeholder(path, &json)?;
    }
    atomic_write_with_mode(path, json.as_bytes(), mode)
}

/// 原子写入文本文件（用于 TOML/纯文本）
pub fn write_text_file(path: &Path, data: &str) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    atomic_write(path, data.as_bytes())
}

/// 原子写入携带密钥的文本文件：新建文件权限为 0600（已有文件保留原权限）
pub fn write_text_file_secret(path: &Path, data: &str) -> Result<(), AppError> {
    crate::adapters::live_file::ensure_no_vault_placeholder(path, data)?;
    atomic_write_secret(path, data.as_bytes())
}

// ─── Owner-only permissions for app data (SEC-05) ────────────
//
// `~/.cc-switch` holds the SQLite database (provider API keys), backups and
// SQL exports. On Unix the directories are made 0700 and these files 0600 so
// other local accounts cannot read them even with a permissive umask.
//
// Windows: nothing is changed. Files under `%USERPROFILE%\.cc-switch` inherit
// the profile directory's ACL, which by default grants access only to the
// user, SYSTEM and Administrators. Exports written elsewhere inherit the ACL
// of the chosen folder. See docs/secrets-vault.md.

/// Create `dir` (with parents) and restrict it to the owner (0700 on Unix).
///
/// Existing directories are tightened too, except the home directory itself
/// (guarding against a misconfigured app-config-dir override).
pub fn ensure_private_dir(dir: &Path) -> Result<(), AppError> {
    fs::create_dir_all(dir).map_err(|e| AppError::io(dir, e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        #[allow(clippy::disallowed_methods)] // compares against the real profile dir
        let is_home = dirs::home_dir().is_some_and(|home| home == dir);
        if !is_home {
            if let Ok(meta) = fs::metadata(dir) {
                if meta.permissions().mode() & 0o077 != 0 {
                    if let Err(e) = fs::set_permissions(dir, fs::Permissions::from_mode(0o700)) {
                        log::warn!("无法将目录权限收紧为 0700: {} ({e})", dir.display());
                    }
                }
            }
        }
    }
    Ok(())
}

/// Best-effort: restrict an existing file to the owner (0600 on Unix).
pub fn restrict_file_to_owner(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::symlink_metadata(path) {
            if meta.is_file() && meta.permissions().mode() & 0o077 != 0 {
                if let Err(e) = fs::set_permissions(path, fs::Permissions::from_mode(0o600)) {
                    log::warn!("无法将文件权限收紧为 0600: {} ({e})", path.display());
                }
            }
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Create `path` as an empty owner-only file if it does not exist yet, so a
/// file that is then opened by another library (e.g. SQLite) never exists
/// with umask-derived permissions. Existing files are tightened to 0600.
pub fn prepare_private_file(path: &Path) -> Result<(), AppError> {
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    match opts.open(path) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            restrict_file_to_owner(path);
            Ok(())
        }
        Err(e) => Err(AppError::io(path, e)),
    }
}

/// 目标文件不存在时新建文件的权限策略
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NewFileMode {
    /// 遵循进程 umask（与 `File::create` 相同）
    Default,
    /// 仅所有者可读写（0600），用于携带 API Key / Token 的文件
    Secret,
}

/// 原子写入：写入临时文件后 rename 替换，避免半写状态
///
/// - 临时文件在 rename 前 `sync_all()`；Unix 上 rename 后还会 fsync 父目录，
///   避免断电后出现零长度的配置文件；
/// - 任何失败路径都会删除临时文件，原文件保持不变；
/// - Windows 上直接 `rename`（std 使用 `MoveFileExW(MOVEFILE_REPLACE_EXISTING)`
///   原子替换已有文件），不再预先删除目标；遇到共享冲突时短暂重试；
/// - 目标为符号链接时写入其指向的真实文件，保留链接本身；
/// - 已有文件保留原权限；新建文件遵循 umask。携带密钥的文件请用 [`atomic_write_secret`]。
pub fn atomic_write(path: &Path, data: &[u8]) -> Result<(), AppError> {
    atomic_write_with_mode(path, data, NewFileMode::Default)
}

/// 与 [`atomic_write`] 相同，但新建文件的权限为 0600（Unix）。
pub fn atomic_write_secret(path: &Path, data: &[u8]) -> Result<(), AppError> {
    atomic_write_with_mode(path, data, NewFileMode::Secret)
}

/// 临时文件守卫：除非 rename 成功后 `disarm()`，否则在 drop 时删除临时文件
struct TempFileGuard {
    path: PathBuf,
    armed: bool,
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// 若 `path` 是符号链接，返回其最终指向的文件路径，以便替换真实文件而不是链接本身
fn resolve_write_target(path: &Path) -> PathBuf {
    let is_symlink = fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false);
    if !is_symlink {
        return path.to_path_buf();
    }
    if let Ok(real) = fs::canonicalize(path) {
        return real;
    }
    // 悬空链接：跟随一层链接内容
    match fs::read_link(path) {
        Ok(link) if link.is_absolute() => link,
        Ok(link) => path.parent().map(|p| p.join(&link)).unwrap_or(link),
        Err(_) => path.to_path_buf(),
    }
}

fn atomic_write_with_mode(path: &Path, data: &[u8], mode: NewFileMode) -> Result<(), AppError> {
    let target = resolve_write_target(path);
    let path = target.as_path();

    let parent = path
        .parent()
        .ok_or_else(|| AppError::Config("无效的路径".to_string()))?;
    if !parent.as_os_str().is_empty() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    let file_name = path
        .file_name()
        .ok_or_else(|| AppError::Config("无效的文件名".to_string()))?
        .to_string_lossy()
        .to_string();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let tmp = parent.join(format!("{file_name}.tmp.{ts}"));

    let existing_perms = fs::metadata(path).ok().map(|m| m.permissions());

    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // 已有文件（权限稍后复制）或密钥文件：先以 0600 创建，避免写入期间被其他用户读取
        if existing_perms.is_some() || mode == NewFileMode::Secret {
            opts.mode(0o600);
        }
    }
    #[cfg(not(unix))]
    let _ = mode;

    let mut f = opts.open(&tmp).map_err(|e| AppError::io(&tmp, e))?;
    let mut guard = TempFileGuard {
        path: tmp.clone(),
        armed: true,
    };
    f.write_all(data).map_err(|e| AppError::io(&tmp, e))?;
    f.flush().map_err(|e| AppError::io(&tmp, e))?;
    f.sync_all().map_err(|e| AppError::io(&tmp, e))?;
    drop(f);

    if let Some(perms) = existing_perms {
        #[cfg(unix)]
        fs::set_permissions(&tmp, perms).map_err(|e| AppError::io(&tmp, e))?;
        #[cfg(not(unix))]
        let _ = fs::set_permissions(&tmp, perms);
    }

    rename_replace(&tmp, path).map_err(|e| AppError::IoContext {
        context: format!("原子替换失败: {} -> {}", tmp.display(), path.display()),
        source: e,
    })?;
    guard.armed = false;

    #[cfg(unix)]
    {
        // fsync 父目录以持久化 rename（部分文件系统不支持，忽略错误）
        let dir = if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        };
        if let Ok(dir) = fs::File::open(dir) {
            let _ = dir.sync_all();
        }
    }

    Ok(())
}

/// 用 `tmp` 原子替换 `path`。
///
/// Windows 上 `std::fs::rename` 使用 `MoveFileExW(MOVEFILE_REPLACE_EXISTING)`，
/// 可以直接覆盖已有文件；目标被其他进程短暂占用（杀毒软件、编辑器）时会返回
/// 共享冲突/拒绝访问，这里有限次重试，最终失败时原文件保持不变。
fn rename_replace(tmp: &Path, path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        const ERROR_ACCESS_DENIED: i32 = 5;
        const ERROR_SHARING_VIOLATION: i32 = 32;
        const ERROR_LOCK_VIOLATION: i32 = 33;
        let mut attempt: u64 = 0;
        loop {
            match fs::rename(tmp, path) {
                Ok(()) => return Ok(()),
                Err(e)
                    if attempt < 5
                        && matches!(
                            e.raw_os_error(),
                            Some(ERROR_ACCESS_DENIED)
                                | Some(ERROR_SHARING_VIOLATION)
                                | Some(ERROR_LOCK_VIOLATION)
                        ) =>
                {
                    attempt += 1;
                    std::thread::sleep(std::time::Duration::from_millis(20 * attempt));
                }
                Err(e) => return Err(e),
            }
        }
    }

    #[cfg(not(windows))]
    {
        fs::rename(tmp, path)
    }
}

// ============================================================================
// Live 配置文件备份
// ============================================================================

/// 覆盖 live 配置文件前创建备份：`~/.cc-switch/backups/<app>/<stem>_<时间戳>.<ext>`
///
/// - 文件不存在时不备份；
/// - 内容与最近一次备份相同时不重复备份；
/// - 保留数量与 Hermes/OpenClaw 备份一致（`effective_backup_retain_count`）；
/// - 备份失败只记录警告，不阻断写入。
pub fn backup_live_file(app: &str, path: &Path) -> Option<PathBuf> {
    let dir = get_app_config_dir().join("backups").join(app);
    let retain = crate::settings::effective_backup_retain_count();
    match backup_file_into_dir(path, &dir, retain) {
        Ok(result) => {
            if let Some(backup) = &result {
                log::debug!("已备份 {} -> {}", path.display(), backup.display());
            }
            result
        }
        Err(e) => {
            log::warn!("备份 live 配置文件失败 {}: {e}", path.display());
            None
        }
    }
}

/// 备份文件名的前缀与后缀：`settings.json` -> ("settings", ".json")，`.env` -> ("env", ".bak")
fn backup_name_parts(path: &Path) -> Option<(String, String)> {
    let name = path.file_name()?.to_string_lossy();
    let trimmed = name.trim_start_matches('.');
    if trimmed.is_empty() {
        return None;
    }
    Some(match trimmed.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => {
            (sanitize_provider_name(stem), format!(".{ext}"))
        }
        _ => (sanitize_provider_name(trimmed), ".bak".to_string()),
    })
}

/// 备份排序键：(`YYYYmmdd_HHMMSS` 时间戳, 同一秒内的序号)
type BackupKey = (String, u64);

/// 解析备份文件名 `<stem>_<YYYYmmdd_HHMMSS>[_<n>]<suffix>`
fn parse_backup_key(name: &str, stem: &str, suffix: &str) -> Option<BackupKey> {
    let middle = name
        .strip_prefix(stem)?
        .strip_prefix('_')?
        .strip_suffix(suffix)?;
    let parts: Vec<&str> = middle.split('_').collect();
    if parts.is_empty()
        || parts
            .iter()
            .any(|p| p.is_empty() || !p.chars().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    match parts.as_slice() {
        [date, time] => Some((format!("{date}_{time}"), 0)),
        [date, time, n] => Some((format!("{date}_{time}"), n.parse().ok()?)),
        _ => None,
    }
}

/// 列出 `dir` 中属于 `stem`/`suffix` 的备份，按时间戳与序号升序
fn list_backups_with_keys(dir: &Path, stem: &str, suffix: &str) -> Vec<(BackupKey, PathBuf)> {
    let mut entries: Vec<(BackupKey, PathBuf)> = match fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let path = e.path();
                let key = parse_backup_key(path.file_name()?.to_str()?, stem, suffix)?;
                Some((key, path))
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    entries.sort();
    entries
}

fn list_backups(dir: &Path, stem: &str, suffix: &str) -> Vec<PathBuf> {
    list_backups_with_keys(dir, stem, suffix)
        .into_iter()
        .map(|(_, p)| p)
        .collect()
}

pub(crate) fn backup_file_into_dir(
    src: &Path,
    dir: &Path,
    retain: usize,
) -> Result<Option<PathBuf>, AppError> {
    if !src.is_file() {
        return Ok(None);
    }
    let Some((stem, suffix)) = backup_name_parts(src) else {
        return Ok(None);
    };
    let content = fs::read(src).map_err(|e| AppError::io(src, e))?;

    let existing = list_backups_with_keys(dir, &stem, &suffix);
    if let Some((_, latest)) = existing.last() {
        if fs::read(latest).ok().as_deref() == Some(content.as_slice()) {
            return Ok(Some(latest.clone()));
        }
    }

    fs::create_dir_all(dir).map_err(|e| AppError::io(dir, e))?;
    let ts = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
    // 同一秒内的多次备份使用递增序号，保证新备份总是排在最后（即使旧序号已被清理）
    let next_counter = existing
        .iter()
        .filter(|((t, _), _)| *t == ts)
        .map(|((_, n), _)| n + 1)
        .max();
    let mut counter = next_counter.unwrap_or(0);
    let name_for = |n: u64| {
        if n == 0 {
            format!("{stem}_{ts}{suffix}")
        } else {
            format!("{stem}_{ts}_{n}{suffix}")
        }
    };
    let mut backup_path = dir.join(name_for(counter));
    while backup_path.exists() {
        counter += 1;
        backup_path = dir.join(name_for(counter));
    }
    // 备份内容可能包含密钥
    atomic_write_secret(&backup_path, &content)?;

    let all = list_backups(dir, &stem, &suffix);
    let retain = retain.max(1);
    if all.len() > retain {
        for old in &all[..all.len() - retain] {
            if let Err(e) = fs::remove_file(old) {
                log::warn!("删除旧备份失败 {}: {e}", old.display());
            }
        }
    }

    Ok(Some(backup_path))
}

#[cfg(test)]
mod tests {
    #[test]
    fn resolve_tool_dir_prefers_override_then_env_then_default() {
        let default = || PathBuf::from("/home/u/.codex");
        assert_eq!(
            resolve_tool_dir(
                Some(PathBuf::from("/o")),
                Some(("CODEX_HOME", PathBuf::from("/e"))),
                default
            ),
            (PathBuf::from("/o"), ConfigDirSource::Override)
        );
        assert_eq!(
            resolve_tool_dir(None, Some(("CODEX_HOME", PathBuf::from("/e"))), default),
            (PathBuf::from("/e"), ConfigDirSource::Env("CODEX_HOME"))
        );
        assert_eq!(
            resolve_tool_dir(None, None, default),
            (PathBuf::from("/home/u/.codex"), ConfigDirSource::Default)
        );
    }

    #[test]
    fn tool_env_values_ignore_blank_and_expand_tilde() {
        assert_eq!(expand_tool_env_value(""), None);
        assert_eq!(
            expand_tool_env_value("/work/.codex"),
            Some(PathBuf::from("/work/.codex"))
        );
        assert_eq!(
            expand_tool_env_value("~/work/.codex"),
            Some(get_home_dir().join("work/.codex"))
        );
    }

    use super::*;

    // TEST-38: `~` expansion of override dirs resolves through the home seam,
    // so a test home (CC_SWITCH_TEST_HOME) also covers `~/...` overrides.
    #[test]
    fn expand_tilde_resolves_against_home_seam() {
        let home = get_home_dir();
        assert_eq!(expand_tilde("~"), home);
        assert_eq!(expand_tilde("~/a/b"), home.join("a/b"));
        assert_eq!(expand_tilde("~\\c"), home.join("c"));
        assert_eq!(expand_tilde("/abs/~/x"), PathBuf::from("/abs/~/x"));
        assert_eq!(expand_tilde("~user"), PathBuf::from("~user"));
    }

    #[test]
    fn derive_mcp_path_from_override_uses_config_dir_for_custom_path() {
        let override_dir = PathBuf::from("/tmp/profile/.claude");
        let derived = derive_mcp_path_from_override(&override_dir);
        assert_eq!(derived, PathBuf::from("/tmp/profile/.claude/.claude.json"));
    }

    #[test]
    fn derive_mcp_path_from_override_uses_config_dir_for_non_hidden_folder() {
        let override_dir = PathBuf::from("/data/claude-config");
        let derived = derive_mcp_path_from_override(&override_dir);
        assert_eq!(derived, PathBuf::from("/data/claude-config/.claude.json"));
    }

    #[test]
    fn derive_mcp_path_from_override_supports_relative_rootless_dir() {
        let override_dir = PathBuf::from("claude");
        let derived = derive_mcp_path_from_override(&override_dir);
        assert_eq!(derived, PathBuf::from("claude/.claude.json"));
    }

    #[test]
    fn derive_mcp_path_from_root_like_dir_uses_root_file() {
        let override_dir = PathBuf::from("/");
        let derived = derive_mcp_path_from_override(&override_dir);
        assert_eq!(derived, PathBuf::from("/.claude.json"));
    }

    #[test]
    fn derive_mcp_path_from_override_preserves_leading_parent_dirs() {
        let override_dir = PathBuf::from("../../profiles/work/.claude");
        let derived = derive_mcp_path_from_override(&override_dir);
        assert_eq!(derived, override_dir.join(".claude.json"));
    }

    #[cfg(windows)]
    #[test]
    fn wsl_unc_home_default_uses_split_mcp_path() {
        let override_dir = PathBuf::from(r"\\wsl$\Ubuntu\home\travis\.claude");
        let derived = default_mcp_path_for_config_dir(&override_dir)
            .expect("WSL home default should use split MCP path");
        assert_eq!(
            derived,
            PathBuf::from(r"\\wsl$\Ubuntu\home\travis\.claude.json")
        );
    }

    #[cfg(windows)]
    #[test]
    fn wsl_unc_root_default_uses_split_mcp_path() {
        let override_dir = PathBuf::from(r"\\wsl.localhost\Ubuntu\root\.claude");
        let derived = default_mcp_path_for_config_dir(&override_dir)
            .expect("WSL root default should use split MCP path");
        assert_eq!(
            derived,
            PathBuf::from(r"\\wsl.localhost\Ubuntu\root\.claude.json")
        );
    }

    #[cfg(windows)]
    #[test]
    fn wsl_unc_custom_dir_uses_nested_mcp_path() {
        let override_dir = PathBuf::from(r"\\wsl$\Ubuntu\opt\claude\.claude");
        assert!(default_mcp_path_for_config_dir(&override_dir).is_none());
        assert_eq!(
            derive_mcp_path_from_override(&override_dir),
            PathBuf::from(r"\\wsl$\Ubuntu\opt\claude\.claude\.claude.json")
        );
    }

    #[test]
    fn sort_json_keys_sorts_top_level_object() {
        let input = serde_json::json!({
            "z": 1,
            "a": 2,
            "m": 3,
        });
        let sorted = sort_json_keys(&input);
        let serialized = serde_json::to_string(&sorted).unwrap();
        assert_eq!(serialized, r#"{"a":2,"m":3,"z":1}"#);
    }

    #[test]
    fn sort_json_keys_recurses_into_nested_objects() {
        let input = serde_json::json!({
            "outer_b": {"z": 1, "a": 2},
            "outer_a": {"y": 3, "b": 4},
        });
        let sorted = sort_json_keys(&input);
        let serialized = serde_json::to_string(&sorted).unwrap();
        assert_eq!(
            serialized,
            r#"{"outer_a":{"b":4,"y":3},"outer_b":{"a":2,"z":1}}"#
        );
    }

    #[test]
    fn sort_json_keys_preserves_array_order() {
        let input = serde_json::json!([3, 1, 2]);
        let sorted = sort_json_keys(&input);
        let serialized = serde_json::to_string(&sorted).unwrap();
        assert_eq!(serialized, "[3,1,2]");
    }

    #[test]
    fn sort_json_keys_sorts_objects_inside_arrays_but_keeps_array_order() {
        let input = serde_json::json!([
            {"z": 1, "a": 2},
            {"y": 3, "b": 4},
        ]);
        let sorted = sort_json_keys(&input);
        let serialized = serde_json::to_string(&sorted).unwrap();
        assert_eq!(serialized, r#"[{"a":2,"z":1},{"b":4,"y":3}]"#);
    }

    #[test]
    fn sort_json_keys_passes_through_primitives() {
        let cases = vec![
            serde_json::json!("hello"),
            serde_json::json!(42),
            serde_json::json!(3.5),
            serde_json::json!(true),
            serde_json::json!(null),
        ];
        for value in cases {
            let sorted = sort_json_keys(&value);
            assert_eq!(sorted, value);
        }
    }

    #[test]
    fn sort_json_keys_handles_empty_collections() {
        let empty_obj = serde_json::json!({});
        assert_eq!(
            serde_json::to_string(&sort_json_keys(&empty_obj)).unwrap(),
            "{}"
        );

        let empty_arr = serde_json::json!([]);
        assert_eq!(
            serde_json::to_string(&sort_json_keys(&empty_arr)).unwrap(),
            "[]"
        );
    }

    #[test]
    fn sort_json_keys_produces_identical_output_for_different_insertion_orders() {
        // 核心保证：同一逻辑配置无论键的插入顺序如何，写出的字节序列必须一致。
        let mut a = Map::new();
        a.insert("env".to_string(), serde_json::json!({"PATH": "/usr/bin"}));
        a.insert("model".to_string(), serde_json::json!("claude-sonnet-4-5"));
        a.insert("permissions".to_string(), serde_json::json!({"allow": []}));

        let mut b = Map::new();
        b.insert("permissions".to_string(), serde_json::json!({"allow": []}));
        b.insert("model".to_string(), serde_json::json!("claude-sonnet-4-5"));
        b.insert("env".to_string(), serde_json::json!({"PATH": "/usr/bin"}));

        let sorted_a = sort_json_keys(&Value::Object(a));
        let sorted_b = sort_json_keys(&Value::Object(b));

        assert_eq!(
            serde_json::to_string(&sorted_a).unwrap(),
            serde_json::to_string(&sorted_b).unwrap(),
        );
    }

    fn leftover_tmp_files(dir: &Path) -> Vec<String> {
        fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains(".tmp."))
            .collect()
    }

    #[test]
    fn atomic_write_replaces_existing_file_and_leaves_no_tmp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, b"old").unwrap();

        atomic_write(&path, b"new").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert!(leftover_tmp_files(dir.path()).is_empty());
    }

    #[test]
    fn atomic_write_failed_rename_keeps_original_and_cleans_tmp() {
        let dir = tempfile::tempdir().unwrap();
        // 目标是非空目录：rename 一定失败
        let path = dir.path().join("config.toml");
        fs::create_dir(&path).unwrap();
        fs::write(path.join("keep.txt"), b"original").unwrap();

        let err = atomic_write(&path, b"new content");
        assert!(err.is_err(), "rename over a non-empty directory must fail");

        assert!(path.is_dir());
        assert_eq!(fs::read(path.join("keep.txt")).unwrap(), b"original");
        assert!(
            leftover_tmp_files(dir.path()).is_empty(),
            "temp file must be removed on failure"
        );
    }

    #[test]
    fn atomic_write_failed_create_leaves_no_tmp() {
        let dir = tempfile::tempdir().unwrap();
        // 父路径是普通文件：创建目录/临时文件失败
        let blocker = dir.path().join("blocker");
        fs::write(&blocker, b"x").unwrap();
        let path = blocker.join("settings.json");

        assert!(atomic_write(&path, b"data").is_err());
        assert_eq!(fs::read(&blocker).unwrap(), b"x");
        assert!(leftover_tmp_files(dir.path()).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_through_symlink_updates_target_and_keeps_link() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("dotfiles").join("settings.json");
        fs::create_dir_all(real.parent().unwrap()).unwrap();
        fs::write(&real, b"old").unwrap();
        let link = dir.path().join("settings.json");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        atomic_write(&link, b"new").unwrap();

        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read(&real).unwrap(), b"new");
        assert!(leftover_tmp_files(dir.path()).is_empty());
        assert!(leftover_tmp_files(real.parent().unwrap()).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_secret_creates_new_file_with_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.json");

        write_json_file_secret(&path, &serde_json::json!({"OPENAI_API_KEY": "sk"})).unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn private_dir_and_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let root = tempfile::tempdir().unwrap();

        // Fresh directory (with parents) is 0700; an existing 0755 one is tightened.
        let dir = root.path().join(".cc-switch").join("backups");
        ensure_private_dir(&dir).unwrap();
        assert_eq!(mode(&dir), 0o700);
        let loose = root.path().join("loose");
        fs::create_dir(&loose).unwrap();
        fs::set_permissions(&loose, fs::Permissions::from_mode(0o755)).unwrap();
        ensure_private_dir(&loose).unwrap();
        assert_eq!(mode(&loose), 0o700);

        // New file is created 0600 and empty; an existing 0644 file is tightened
        // without touching its content.
        let db = dir.join("cc-switch.db");
        prepare_private_file(&db).unwrap();
        assert_eq!(mode(&db), 0o600);
        assert_eq!(fs::metadata(&db).unwrap().len(), 0);
        let old = dir.join("old.db");
        fs::write(&old, b"data").unwrap();
        fs::set_permissions(&old, fs::Permissions::from_mode(0o644)).unwrap();
        prepare_private_file(&old).unwrap();
        assert_eq!(mode(&old), 0o600);
        assert_eq!(fs::read(&old).unwrap(), b"data");

        // SQLite opened on the prepared file keeps 0600 (WAL/SHM inherit it).
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE t(x);")
            .unwrap();
        assert_eq!(mode(&db), 0o600);
        let wal = dir.join("cc-switch.db-wal");
        if wal.exists() {
            assert_eq!(mode(&wal), 0o600);
        }
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_preserves_existing_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, b"a = 1").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();

        write_text_file_secret(&path, "a = 2").unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o640
        );

        atomic_write(&path, b"a = 3").unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }

    #[test]
    fn backup_file_into_dir_copies_dedupes_and_prunes() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("settings.json");
        let backups = dir.path().join("backups").join("claude");

        // 文件不存在：不备份
        assert!(backup_file_into_dir(&src, &backups, 2).unwrap().is_none());

        fs::write(&src, b"v1").unwrap();
        let b1 = backup_file_into_dir(&src, &backups, 2).unwrap().unwrap();
        assert_eq!(fs::read(&b1).unwrap(), b"v1");
        let name = b1.file_name().unwrap().to_string_lossy().to_string();
        assert!(name.starts_with("settings_") && name.ends_with(".json"));

        // 内容未变化：复用最近一次备份
        let again = backup_file_into_dir(&src, &backups, 2).unwrap().unwrap();
        assert_eq!(again, b1);

        for v in ["v2", "v3", "v4"] {
            fs::write(&src, v.as_bytes()).unwrap();
            backup_file_into_dir(&src, &backups, 2).unwrap().unwrap();
        }

        let remaining = list_backups(&backups, "settings", ".json");
        assert_eq!(remaining.len(), 2, "retention must prune old backups");
        let contents: Vec<Vec<u8>> = remaining.iter().map(|p| fs::read(p).unwrap()).collect();
        assert_eq!(contents, vec![b"v3".to_vec(), b"v4".to_vec()]);
    }

    #[test]
    fn backup_name_parts_handles_dotfiles() {
        assert_eq!(
            backup_name_parts(Path::new("/x/.env")),
            Some(("env".to_string(), ".bak".to_string()))
        );
        assert_eq!(
            backup_name_parts(Path::new("/x/config.toml")),
            Some(("config".to_string(), ".toml".to_string()))
        );
    }
}

/// 复制文件
pub fn copy_file(from: &Path, to: &Path) -> Result<(), AppError> {
    fs::copy(from, to).map_err(|e| AppError::IoContext {
        context: format!("复制文件失败 ({} -> {})", from.display(), to.display()),
        source: e,
    })?;
    Ok(())
}

/// 删除文件
pub fn delete_file(path: &Path) -> Result<(), AppError> {
    if path.exists() {
        fs::remove_file(path).map_err(|e| AppError::io(path, e))?;
    }
    Ok(())
}

/// 检查 Claude Code 配置状态
#[derive(Serialize, Deserialize)]
pub struct ConfigStatus {
    pub exists: bool,
    pub path: String,
}

/// 获取 Claude Code 配置状态
pub fn get_claude_config_status() -> ConfigStatus {
    let path = get_claude_settings_path();
    ConfigStatus {
        exists: path.exists(),
        path: path.to_string_lossy().to_string(),
    }
}
