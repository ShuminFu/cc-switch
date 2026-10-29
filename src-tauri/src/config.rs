use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use crate::error::AppError;

/// 获取用户主目录，带回退和日志
///
/// 实际解析由 [`crate::paths::current`] 完成：生产环境下等价于 `dirs::home_dir()`
/// （Windows 注意事项见 [`crate::paths::Paths::from_env`]）。
///
/// ## 测试隔离
///
/// 为了让 Windows CI/本地测试能稳定隔离真实用户数据，可通过 `CC_SWITCH_TEST_HOME`
/// 显式覆盖 home dir（仅用于测试/调试场景），或用 [`crate::paths::scoped`]
/// 在当前线程注入独立的 [`crate::paths::Paths`]。
pub fn get_home_dir() -> PathBuf {
    crate::paths::current().home()
}

/// 获取 Claude Code 配置目录路径
pub fn get_claude_config_dir() -> PathBuf {
    if let Some(custom) = crate::settings::get_claude_override_dir() {
        return custom;
    }

    crate::paths::current()
        .default_app_dir(&crate::app_config::AppType::Claude)
        .unwrap_or_else(|| get_home_dir().join(".claude"))
}

/// 默认 Claude MCP 配置文件路径 (~/.claude.json)
pub fn get_default_claude_mcp_path() -> PathBuf {
    crate::paths::current().claude_mcp_path()
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
pub fn get_claude_mcp_path() -> PathBuf {
    if let Some(custom_dir) = crate::settings::get_claude_override_dir() {
        if let Some(path) = default_mcp_path_for_config_dir(&custom_dir) {
            return path;
        }
        return derive_mcp_path_from_override(&custom_dir);
    }
    get_default_claude_mcp_path()
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

    let default_dir = crate::paths::current().app_config_dir();

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
pub fn read_json_file<T: for<'a> Deserialize<'a>>(path: &Path) -> Result<T, AppError> {
    if !path.exists() {
        return Err(AppError::Config(format!("文件不存在: {}", path.display())));
    }

    let content = fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;

    serde_json::from_str(&content).map_err(|e| AppError::json(path, e))
}

/// 递归排序 JSON 对象的键（按字母顺序），确保序列化输出是确定性的
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

/// 写入 JSON 配置文件（键按字母排序，确保确定性输出）
pub fn write_json_file<T: Serialize>(path: &Path, data: &T) -> Result<(), AppError> {
    // 确保目录存在
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    let value = serde_json::to_value(data).map_err(|e| AppError::JsonSerialize { source: e })?;
    let sorted_value = sort_json_keys(&value);
    let json = serde_json::to_string_pretty(&sorted_value)
        .map_err(|e| AppError::JsonSerialize { source: e })?;

    atomic_write(path, json.as_bytes())
}

/// 原子写入文本文件（用于 TOML/纯文本）
pub fn write_text_file(path: &Path, data: &str) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    atomic_write(path, data.as_bytes())
}

/// 原子写入：写入临时文件后 rename 替换，避免半写状态
///
/// - 临时文件在 rename 前 `sync_all`，unix 上 rename 后再 fsync 父目录，
///   保证断电后目标要么是旧内容、要么是完整的新内容；
/// - 直接依赖 `fs::rename` 的替换语义（Windows 上为 `MoveFileExW` +
///   `MOVEFILE_REPLACE_EXISTING`），不再预先删除目标文件；
/// - 任一步骤失败都会清理临时文件（其中可能含有密钥）；
/// - 目标是符号链接（如 stow/chezmoi 管理的配置）时写入链接指向的真实文件，
///   链接本身保持不变，见 [`resolve_write_target`]。
pub fn atomic_write(path: &Path, data: &[u8]) -> Result<(), AppError> {
    atomic_write_impl(path, data, false)
}

/// 原子写入敏感文件（OAuth token 等长期凭据）
///
/// 与 [`atomic_write`] 相同（fsync、失败清理、跟随符号链接），但不沿用目标的
/// 现有权限，每次写入都强制仅属主可访问：
/// - unix：文件权限固定为 0600；
/// - Windows：rename 前为临时文件设置受保护的 DACL，仅授予当前用户完全控制，
///   不继承父目录的 ACE（同卷 rename 保留安全描述符）。
pub fn atomic_write_secret(path: &Path, data: &[u8]) -> Result<(), AppError> {
    atomic_write_impl(path, data, true)
}

fn atomic_write_impl(path: &Path, data: &[u8], owner_only: bool) -> Result<(), AppError> {
    let resolved = resolve_write_target(path)?;
    let path = resolved.as_path();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    let parent = path
        .parent()
        .ok_or_else(|| AppError::Config("无效的路径".to_string()))?;
    let mut tmp = parent.to_path_buf();
    let file_name = path
        .file_name()
        .ok_or_else(|| AppError::Config("无效的文件名".to_string()))?
        .to_string_lossy()
        .to_string();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    tmp.push(format!("{file_name}.tmp.{ts}"));

    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    // 新建文件默认仅属主可读写（可能含 API Key）；目标已存在时在 write_and_sync 中沿用其权限
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let f = opts.open(&tmp).map_err(|e| AppError::io(&tmp, e))?;
    // 从这里开始临时文件已存在：任何提前返回都由 guard 删除它
    let mut guard = TempFileGuard::new(&tmp);
    // 按值传入，返回时句柄即关闭（Windows 上 rename/删除前必须先关闭）
    write_and_sync(f, &tmp, path, data, owner_only)?;
    #[cfg(windows)]
    if owner_only {
        owner_only_acl::apply(&tmp).map_err(|e| AppError::IoContext {
            context: format!("设置仅属主 ACL 失败: {}", tmp.display()),
            source: e,
        })?;
    }

    rename_replace(&tmp, path).map_err(|e| AppError::IoContext {
        context: format!("原子替换失败: {} -> {}", tmp.display(), path.display()),
        source: e,
    })?;
    guard.disarm();

    // rename 已完成，因此目录 fsync 失败只记录日志：
    // 部分文件系统（如某些网络文件系统）不支持对目录 fsync。
    #[cfg(unix)]
    if let Err(e) = sync_dir(parent) {
        log::warn!("fsync 目录失败: {}: {e}", parent.display());
    }

    Ok(())
}

/// 符号链接最多跟随的跳数（与 Linux 的 `MAXSYMLINKS` 一致）
const MAX_SYMLINK_HOPS: usize = 40;

/// 解析 `atomic_write` 的真实写入目标
///
/// rename 会替换链接本身而不是其指向的文件：链接被换成普通文件，真实目标
/// 静默过期（Windows 上链接被删除）。因此：
/// - 非链接（或路径不存在）原样返回；
/// - 链接可解析时使用 `canonicalize` 的结果；
/// - 悬空链接逐跳 `read_link`（相对目标按链接所在目录解析），直到遇到非链接路径，
///   超过 [`MAX_SYMLINK_HOPS`] 视为循环并报错。
fn resolve_write_target(path: &Path) -> Result<PathBuf, AppError> {
    let is_link = |p: &Path| {
        fs::symlink_metadata(p)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
    };

    if !is_link(path) {
        return Ok(path.to_path_buf());
    }
    if let Ok(canonical) = fs::canonicalize(path) {
        return Ok(canonical);
    }

    let mut current = path.to_path_buf();
    for _ in 0..MAX_SYMLINK_HOPS {
        let target = fs::read_link(&current).map_err(|e| AppError::io(&current, e))?;
        current = match current.parent() {
            Some(parent) if target.is_relative() => parent.join(target),
            _ => target,
        };
        if !is_link(&current) {
            return Ok(current);
        }
    }

    Err(AppError::Config(format!(
        "符号链接层级过多: {}",
        path.display()
    )))
}

/// 写入临时文件并在关闭前 `sync_all`。unix 上 `owner_only` 时强制 0600，
/// 否则目标已存在时沿用其权限。
fn write_and_sync(
    mut f: fs::File,
    tmp: &Path,
    target: &Path,
    data: &[u8],
    owner_only: bool,
) -> Result<(), AppError> {
    f.write_all(data).map_err(|e| AppError::io(tmp, e))?;
    f.flush().map_err(|e| AppError::io(tmp, e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if owner_only {
            // 创建时的 0600 可能被 umask 进一步收紧，这里显式固定
            f.set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|e| AppError::io(tmp, e))?;
        } else if let Ok(meta) = fs::metadata(target) {
            let perm = meta.permissions().mode();
            let _ = f.set_permissions(fs::Permissions::from_mode(perm));
        }
    }
    #[cfg(not(unix))]
    let _ = (target, owner_only);

    f.sync_all().map_err(|e| AppError::io(tmp, e))
}

/// 在离开作用域时删除临时文件，除非已调用 [`TempFileGuard::disarm`]
struct TempFileGuard<'a> {
    path: &'a Path,
    armed: bool,
}

impl<'a> TempFileGuard<'a> {
    fn new(path: &'a Path) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TempFileGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(self.path);
        }
    }
}

/// 用 `from` 替换 `to`。Windows 上目标被其它进程（杀毒软件、索引器、编辑器）
/// 短暂占用时会返回共享冲突，此时短暂重试。
fn rename_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        // ERROR_ACCESS_DENIED / ERROR_SHARING_VIOLATION / ERROR_LOCK_VIOLATION
        const RETRYABLE: [i32; 3] = [5, 32, 33];
        const ATTEMPTS: u32 = 5;
        let mut attempt = 1;
        loop {
            match fs::rename(from, to) {
                Ok(()) => return Ok(()),
                Err(e)
                    if attempt < ATTEMPTS
                        && e.raw_os_error().is_some_and(|c| RETRYABLE.contains(&c)) =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(20 * u64::from(attempt)));
                    attempt += 1;
                }
                Err(e) => return Err(e),
            }
        }
    }

    #[cfg(not(windows))]
    {
        fs::rename(from, to)
    }
}

/// Windows 上为文件设置仅当前用户可访问的 ACL
#[cfg(windows)]
mod owner_only_acl {
    use std::ffi::c_void;
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::ptr;

    use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, ERROR_SUCCESS, HANDLE};
    use windows_sys::Win32::Security::Authorization::{
        SetEntriesInAclW, SetNamedSecurityInfoW, EXPLICIT_ACCESS_W, NO_MULTIPLE_TRUSTEE,
        SET_ACCESS, SE_FILE_OBJECT, TRUSTEE_IS_SID, TRUSTEE_IS_USER, TRUSTEE_W,
    };
    use windows_sys::Win32::Security::{
        GetTokenInformation, TokenUser, ACL, DACL_SECURITY_INFORMATION, NO_INHERITANCE,
        PROTECTED_DACL_SECURITY_INFORMATION, TOKEN_QUERY, TOKEN_USER,
    };
    use windows_sys::Win32::Storage::FileSystem::FILE_ALL_ACCESS;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    /// 当前进程用户的 `TOKEN_USER`（缓冲区内含 SID）
    pub(super) struct CurrentUser {
        // u64 对齐，满足 TOKEN_USER 内指针的对齐要求
        buf: Vec<u64>,
    }

    impl CurrentUser {
        pub(super) fn query() -> io::Result<Self> {
            let mut token: HANDLE = ptr::null_mut();
            // SAFETY: GetCurrentProcess 返回伪句柄；token 为有效输出指针
            if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let result = Self::from_token(token);
            // SAFETY: token 由 OpenProcessToken 成功返回
            unsafe { CloseHandle(token) };
            result
        }

        fn from_token(token: HANDLE) -> io::Result<Self> {
            let mut len = 0u32;
            // SAFETY: 首次调用仅查询所需长度
            unsafe { GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut len) };
            if len == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut buf = vec![0u64; (len as usize).div_ceil(8)];
            // SAFETY: buf 至少 len 字节
            if unsafe {
                GetTokenInformation(
                    token,
                    TokenUser,
                    buf.as_mut_ptr().cast::<c_void>(),
                    len,
                    &mut len,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(Self { buf })
        }

        pub(super) fn sid(&self) -> *mut c_void {
            // SAFETY: buf 由 GetTokenInformation(TokenUser) 填充
            unsafe { (*self.buf.as_ptr().cast::<TOKEN_USER>()).User.Sid }
        }
    }

    pub(super) fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    /// 用仅含当前用户（完全控制）的受保护 DACL 替换文件的 DACL
    pub(super) fn apply(path: &Path) -> io::Result<()> {
        let user = CurrentUser::query()?;
        let access = EXPLICIT_ACCESS_W {
            grfAccessPermissions: FILE_ALL_ACCESS,
            grfAccessMode: SET_ACCESS,
            grfInheritance: NO_INHERITANCE,
            Trustee: TRUSTEE_W {
                pMultipleTrustee: ptr::null_mut(),
                MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
                TrusteeForm: TRUSTEE_IS_SID,
                TrusteeType: TRUSTEE_IS_USER,
                ptstrName: user.sid().cast::<u16>(),
            },
        };

        let mut acl: *mut ACL = ptr::null_mut();
        // SAFETY: access 在调用期间有效；acl 由系统分配，下面 LocalFree 释放
        let rc = unsafe { SetEntriesInAclW(1, &access, ptr::null(), &mut acl) };
        if rc != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(rc as i32));
        }

        let name = wide(path);
        // SAFETY: name 以 NUL 结尾；acl 为上面构造的有效 ACL
        let rc = unsafe {
            SetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                acl,
                ptr::null(),
            )
        };
        // SAFETY: acl 由 SetEntriesInAclW 通过 LocalAlloc 分配
        unsafe { LocalFree(acl.cast::<c_void>()) };
        if rc != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(rc as i32));
        }
        Ok(())
    }
}

/// fsync 父目录以持久化 rename 产生的目录项
#[cfg(unix)]
fn sync_dir(dir: &Path) -> std::io::Result<()> {
    // 相对路径 `foo.json` 的 parent 是空路径，需按当前目录处理
    let dir = if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    };
    fs::File::open(dir)?.sync_all()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn leftover_tmp_files(dir: &Path) -> Vec<String> {
        fs::read_dir(dir)
            .expect("read dir")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|name| name.contains(".tmp."))
            .collect()
    }

    #[test]
    fn atomic_write_overwrites_and_creates_without_leftover_tmp() {
        let dir = tempfile::tempdir().expect("tempdir");
        let existing = dir.path().join("settings.json");
        fs::write(&existing, b"old content that is longer").expect("seed");

        atomic_write(&existing, b"new").expect("overwrite");
        assert_eq!(fs::read(&existing).expect("read"), b"new");

        let fresh = dir.path().join("nested").join("fresh.toml");
        atomic_write(&fresh, b"fresh = true\n").expect("create");
        assert_eq!(fs::read(&fresh).expect("read"), b"fresh = true\n");

        assert!(leftover_tmp_files(dir.path()).is_empty());
        assert!(leftover_tmp_files(&dir.path().join("nested")).is_empty());
    }

    #[test]
    fn atomic_write_removes_tmp_when_rename_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        // 目标是一个非空目录：rename 一定失败
        let target = dir.path().join("config.json");
        fs::create_dir(&target).expect("mkdir");
        fs::write(target.join("keep"), b"x").expect("seed");

        let err = atomic_write(&target, b"secret").expect_err("rename must fail");
        assert!(matches!(err, AppError::IoContext { .. }), "{err:?}");

        assert!(leftover_tmp_files(dir.path()).is_empty());
        assert!(target.is_dir());
        assert_eq!(fs::read(target.join("keep")).expect("read"), b"x");
    }

    #[cfg(unix)]
    #[test]
    fn sync_dir_treats_empty_parent_as_current_dir() {
        // `Path::new("foo.json").parent()` 为空路径；不应因此跳过目录 fsync
        let parent = Path::new("foo.json").parent().expect("parent");
        assert!(parent.as_os_str().is_empty());
        assert!(fs::File::open(parent).is_err());
        sync_dir(parent).expect("empty parent must fsync the current dir");
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_preserves_existing_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("auth.json");
        fs::write(&path, b"{}").expect("seed");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("chmod");

        atomic_write(&path, b"{\"k\":1}").expect("write");
        let mode = fs::metadata(&path).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_creates_new_files_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let fresh = dir.path().join("new").join("auth.json");
        atomic_write(&fresh, b"{\"OPENAI_API_KEY\":\"sk\"}").expect("create");
        let mode = fs::metadata(&fresh).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);

        // 已存在的目标保留原权限（即使比 0600 宽松）
        let shared = dir.path().join("settings.json");
        fs::write(&shared, b"{}").expect("seed");
        fs::set_permissions(&shared, fs::Permissions::from_mode(0o640)).expect("chmod");
        atomic_write(&shared, b"{\"k\":1}").expect("overwrite");
        let mode = fs::metadata(&shared).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, 0o640);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_secret_forces_owner_only_on_create_and_rewrite() {
        use std::os::unix::fs::PermissionsExt;
        let mode_of = |p: &Path| fs::metadata(p).expect("meta").permissions().mode() & 0o777;

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("store").join("token.json");
        atomic_write_secret(&path, b"{\"v\":1}").expect("create");
        assert_eq!(mode_of(&path), 0o600);

        // 与 atomic_write 不同：已存在的宽松权限会被收紧，而不是沿用
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("chmod");
        atomic_write_secret(&path, b"{\"v\":2}").expect("rewrite");
        assert_eq!(mode_of(&path), 0o600);
        assert_eq!(fs::read(&path).expect("read"), b"{\"v\":2}");
        assert!(leftover_tmp_files(path.parent().unwrap()).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_secret_keeps_symlink_and_restricts_target() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("dotfiles").join("token.json");
        fs::create_dir_all(target.parent().unwrap()).expect("mkdir");
        fs::write(&target, b"{}").expect("seed");
        fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).expect("chmod");
        let link = dir.path().join("token.json");
        symlink(&target, &link).expect("symlink");

        atomic_write_secret(&link, b"{\"v\":1}").expect("write via link");

        assert_link_written_through(&link, &target, b"{\"v\":1}");
        let mode = fs::metadata(&target).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    /// 读取文件 DACL，断言只有一条授予当前用户的 ACCESS_ALLOWED ACE，且不继承
    #[cfg(windows)]
    fn assert_owner_only_acl(path: &Path) {
        use std::ffi::c_void;
        use std::ptr;
        use windows_sys::Win32::Foundation::{LocalFree, ERROR_SUCCESS};
        use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
        use windows_sys::Win32::Security::{
            AclSizeInformation, EqualSid, GetAce, GetAclInformation, GetSecurityDescriptorControl,
            ACCESS_ALLOWED_ACE, ACL, ACL_SIZE_INFORMATION, DACL_SECURITY_INFORMATION,
            PSECURITY_DESCRIPTOR, SE_DACL_PROTECTED,
        };
        // winnt.h ACCESS_ALLOWED_ACE_TYPE
        const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;

        let user = owner_only_acl::CurrentUser::query().expect("current user");
        let name = owner_only_acl::wide(path);
        let mut dacl: *mut ACL = ptr::null_mut();
        let mut sd: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: name 以 NUL 结尾；输出指针均有效，sd 在末尾 LocalFree
        let rc = unsafe {
            GetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut dacl,
                ptr::null_mut(),
                &mut sd,
            )
        };
        assert_eq!(rc, ERROR_SUCCESS, "GetNamedSecurityInfoW failed");
        assert!(!dacl.is_null(), "file has a NULL DACL (everyone allowed)");

        let mut control = 0u16;
        let mut revision = 0u32;
        // SAFETY: sd 由 GetNamedSecurityInfoW 返回
        assert_ne!(
            unsafe { GetSecurityDescriptorControl(sd, &mut control, &mut revision) },
            0
        );
        assert_ne!(control & SE_DACL_PROTECTED, 0, "DACL must not inherit");

        let mut info = ACL_SIZE_INFORMATION::default();
        // SAFETY: info 大小与 AclSizeInformation 匹配
        assert_ne!(
            unsafe {
                GetAclInformation(
                    dacl,
                    (&mut info as *mut ACL_SIZE_INFORMATION).cast::<c_void>(),
                    std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
                    AclSizeInformation,
                )
            },
            0
        );
        assert_eq!(info.AceCount, 1, "DACL must contain only the current user");

        let mut ace: *mut c_void = ptr::null_mut();
        // SAFETY: 索引 0 < AceCount
        assert_ne!(unsafe { GetAce(dacl, 0, &mut ace) }, 0);
        let ace = ace.cast::<ACCESS_ALLOWED_ACE>();
        // SAFETY: ace 指向 DACL 内的有效 ACE；SidStart 为 SID 起始位置
        unsafe {
            assert_eq!((*ace).Header.AceType, ACCESS_ALLOWED_ACE_TYPE);
            let sid = ptr::addr_of_mut!((*ace).SidStart).cast::<c_void>();
            assert_ne!(
                EqualSid(sid, user.sid()),
                0,
                "ACE is not for the current user"
            );
            LocalFree(sd);
        }
    }

    #[cfg(windows)]
    #[test]
    fn atomic_write_secret_sets_owner_only_acl() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("store").join("token.json");
        atomic_write_secret(&path, b"{\"v\":1}").expect("create");
        assert_owner_only_acl(&path);

        atomic_write_secret(&path, b"{\"v\":2}").expect("rewrite");
        assert_owner_only_acl(&path);
        assert_eq!(fs::read(&path).expect("read"), b"{\"v\":2}");
        assert!(leftover_tmp_files(path.parent().unwrap()).is_empty());
    }

    #[cfg(unix)]
    fn assert_link_written_through(link: &Path, target: &Path, content: &[u8]) {
        let meta = fs::symlink_metadata(link).expect("link meta");
        assert!(meta.file_type().is_symlink(), "link was replaced by a file");
        assert_eq!(fs::read(target).expect("read target"), content);
        assert_eq!(fs::read(link).expect("read via link"), content);
        assert!(leftover_tmp_files(link.parent().unwrap()).is_empty());
        assert!(leftover_tmp_files(target.parent().unwrap()).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_follows_absolute_symlink() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let dir = tempfile::tempdir().expect("tempdir");
        let dotfiles = dir.path().join("dotfiles");
        let home = dir.path().join("home");
        fs::create_dir_all(&dotfiles).expect("mkdir");
        fs::create_dir_all(&home).expect("mkdir");
        let target = dotfiles.join("settings.json");
        fs::write(&target, b"{}").expect("seed");
        fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).expect("chmod");
        let link = home.join("settings.json");
        symlink(&target, &link).expect("symlink");

        atomic_write(&link, b"{\"k\":1}").expect("write");
        assert_link_written_through(&link, &target, b"{\"k\":1}");
        let mode = fs::metadata(&target).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, 0o640);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_follows_relative_symlink_chain() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().expect("tempdir");
        let dotfiles = dir.path().join("dotfiles");
        let home = dir.path().join("home");
        fs::create_dir_all(&dotfiles).expect("mkdir");
        fs::create_dir_all(&home).expect("mkdir");
        let target = dotfiles.join("config.toml");
        fs::write(&target, b"old").expect("seed");
        // home/config.toml -> home/.hop -> ../dotfiles/config.toml
        symlink("../dotfiles/config.toml", home.join(".hop")).expect("symlink");
        let link = home.join("config.toml");
        symlink(".hop", &link).expect("symlink");

        atomic_write(&link, b"new = true\n").expect("write");
        assert_link_written_through(&link, &target, b"new = true\n");
        assert!(fs::symlink_metadata(home.join(".hop"))
            .expect("hop meta")
            .file_type()
            .is_symlink());
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_creates_target_of_dangling_symlink() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().expect("tempdir");
        let dotfiles = dir.path().join("dotfiles");
        let home = dir.path().join("home");
        fs::create_dir_all(&dotfiles).expect("mkdir");
        fs::create_dir_all(&home).expect("mkdir");
        let target = dotfiles.join("auth.json");
        let link = home.join("auth.json");
        symlink("../dotfiles/auth.json", &link).expect("symlink");
        assert!(!target.exists());

        atomic_write(&link, b"{\"token\":\"x\"}").expect("write");
        assert_link_written_through(&link, &target, b"{\"token\":\"x\"}");
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_rejects_symlink_loop() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().expect("tempdir");
        let a = dir.path().join("a.json");
        let b = dir.path().join("b.json");
        symlink(&b, &a).expect("symlink");
        symlink(&a, &b).expect("symlink");

        let err = atomic_write(&a, b"{}").expect_err("loop must fail");
        assert!(matches!(err, AppError::Config(_)), "{err:?}");
        assert!(fs::symlink_metadata(&a)
            .expect("meta")
            .file_type()
            .is_symlink());
        assert!(leftover_tmp_files(dir.path()).is_empty());
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
}
