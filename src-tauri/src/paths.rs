//! 用户目录与各应用默认配置目录的统一解析。
//!
//! 生产环境下 [`current`] 返回 [`Paths::from_env`]，解析结果与直接调用
//! `dirs::home_dir()` 完全一致（仅 `CC_SWITCH_TEST_HOME` 可显式覆盖）。
//!
//! 测试可通过 [`scoped`] 在当前线程注入独立的 [`Paths`]，从而无需修改进程级
//! `HOME`/`CC_SWITCH_TEST_HOME`，多个测试即可并行运行、互不干扰。

use std::cell::RefCell;
use std::ffi::{OsStr, OsString};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use crate::app_config::AppType;

/// 用户主目录及由其派生的默认路径
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    home: Option<PathBuf>,
    /// 原样保存的 `%LOCALAPPDATA%`（仅 Windows 使用）。注入的 [`Paths::with_home`]
    /// 为 `None`，从而派生为 `<home>\AppData\Local`，测试不会写到真实用户目录。
    local_app_data: Option<OsString>,
}

impl Paths {
    /// 以指定目录作为用户主目录
    ///
    /// `%LOCALAPPDATA%` 一并派生为 `<home>\AppData\Local`（不读取进程环境变量），
    /// 使 Windows 上 Claude Desktop / Hermes 的默认目录也落在该主目录内。
    pub fn with_home(home: impl Into<PathBuf>) -> Self {
        Self {
            home: Some(home.into()),
            local_app_data: None,
        }
    }

    /// 系统默认：等价于 `dirs::home_dir()` 与环境变量 `LOCALAPPDATA`
    pub fn system() -> Self {
        Self {
            home: dirs::home_dir(),
            local_app_data: std::env::var_os("LOCALAPPDATA"),
        }
    }

    /// 生产默认：`CC_SWITCH_TEST_HOME`（trim 后非空）优先，否则 `dirs::home_dir()`
    ///
    /// ## Windows 注意事项
    ///
    /// - `dirs::home_dir()` 在 Windows 上使用 `SHGetKnownFolderPath(FOLDERID_Profile)`，
    ///   返回的是真实用户目录（类似 `C:\\Users\\Alice`），与 v3.10.2 行为一致。
    /// - 不要直接使用 `HOME` 环境变量：它可能由 Git/Cygwin/MSYS 等第三方工具注入，
    ///   且不一定等于用户目录，可能导致 `.cc-switch/cc-switch.db` 路径变化，从而“看起来像数据丢失”。
    pub fn from_env() -> Self {
        if let Ok(home) = std::env::var("CC_SWITCH_TEST_HOME") {
            let trimmed = home.trim();
            if !trimmed.is_empty() {
                return Self::with_home(trimmed);
            }
        }
        Self::system()
    }

    /// 用户主目录；无法获取时回退到当前目录
    pub fn home(&self) -> PathBuf {
        self.home.clone().unwrap_or_else(|| {
            log::warn!("无法获取用户主目录，回退到当前目录");
            PathBuf::from(".")
        })
    }

    /// 用户主目录（无法获取时为 `None`，不回退）
    pub fn user_home(&self) -> Option<&Path> {
        self.home.as_deref()
    }

    /// 原样的 `%LOCALAPPDATA%`（未设置或注入的主目录时为 `None`）
    pub fn local_app_data(&self) -> Option<&OsStr> {
        self.local_app_data.as_deref()
    }

    /// Windows 的 `%LOCALAPPDATA%`；未设置时回退 `<home>\AppData\Local`
    pub fn local_app_data_dir(&self) -> PathBuf {
        self.local_app_data
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| self.home().join("AppData").join("Local"))
    }

    /// 默认应用数据目录 (`~/.cc-switch`)，不考虑 Store 中的覆盖
    pub fn app_config_dir(&self) -> PathBuf {
        self.home().join(".cc-switch")
    }

    /// 旧版设置文件路径 (`~/.cc-switch/settings.json`)
    pub fn settings_path(&self) -> PathBuf {
        self.app_config_dir().join("settings.json")
    }

    /// 默认 Claude MCP 配置文件路径 (`~/.claude.json`)
    pub fn claude_mcp_path(&self) -> PathBuf {
        self.home().join(".claude.json")
    }

    /// 各应用在未配置覆盖目录时的默认配置目录
    ///
    /// Claude Desktop 返回其 3P 配置目录（仅 macOS/Windows 支持，其他平台为 `None`）；
    /// Windows 上的 Claude Desktop 实际目录还会在 `%LOCALAPPDATA%` 下按前缀匹配，
    /// 这里只给出精确名称对应的默认值。
    pub fn default_app_dir(&self, app: &AppType) -> Option<PathBuf> {
        (app.descriptor().default_dir)(self)
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn claude_desktop_default_dir(paths: &Paths) -> Option<PathBuf> {
    Some(
        paths
            .home()
            .join("Library")
            .join("Application Support")
            .join("Claude-3p"),
    )
}

#[cfg(windows)]
pub(crate) fn claude_desktop_default_dir(paths: &Paths) -> Option<PathBuf> {
    Some(paths.local_app_data_dir().join("Claude-3p"))
}

#[cfg(not(any(target_os = "macos", windows)))]
pub(crate) fn claude_desktop_default_dir(_paths: &Paths) -> Option<PathBuf> {
    None
}

#[cfg(target_os = "windows")]
pub(crate) fn hermes_default_dir(paths: &Paths) -> PathBuf {
    crate::hermes_config::windows_local_hermes_dir(paths.local_app_data(), &paths.home())
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn hermes_default_dir(paths: &Paths) -> PathBuf {
    paths.home().join(".hermes")
}

thread_local! {
    static SCOPED_PATHS: RefCell<Option<Paths>> = const { RefCell::new(None) };
}

/// 当前生效的 [`Paths`]：当前线程注入的值优先，否则为 [`Paths::from_env`]
pub fn current() -> Paths {
    SCOPED_PATHS
        .with(|scoped| scoped.borrow().clone())
        .unwrap_or_else(Paths::from_env)
}

/// 在当前线程注入 [`Paths`]，guard 释放时恢复之前的值
///
/// 注入只对当前线程生效（guard 不可跨线程传递），仅用于测试隔离。
pub fn scoped(paths: Paths) -> ScopedPaths {
    let previous = SCOPED_PATHS.with(|scoped| scoped.borrow_mut().replace(paths));
    ScopedPaths {
        previous,
        _not_send: PhantomData,
    }
}

/// [`scoped`] 返回的 guard
#[must_use = "dropping the guard immediately restores the previous paths"]
pub struct ScopedPaths {
    previous: Option<Paths>,
    _not_send: PhantomData<*const ()>,
}

impl Drop for ScopedPaths {
    fn drop(&mut self) {
        let previous = self.previous.take();
        SCOPED_PATHS.with(|scoped| *scoped.borrow_mut() = previous);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    const ALL_APPS: [AppType; 8] = [
        AppType::Claude,
        AppType::ClaudeDesktop,
        AppType::Codex,
        AppType::Gemini,
        AppType::GrokBuild,
        AppType::OpenCode,
        AppType::OpenClaw,
        AppType::Hermes,
    ];

    /// 引入 `Paths` 之前各应用默认目录的解析方式（直接基于 `dirs::home_dir()`）
    fn legacy_default_app_dir(app: &AppType) -> Option<PathBuf> {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        match app {
            AppType::Claude => Some(home.join(".claude")),
            AppType::ClaudeDesktop => {
                #[cfg(target_os = "macos")]
                {
                    Some(home.join("Library/Application Support/Claude-3p"))
                }
                #[cfg(windows)]
                {
                    Some(
                        std::env::var_os("LOCALAPPDATA")
                            .map(PathBuf::from)
                            .unwrap_or_else(|| home.join("AppData").join("Local"))
                            .join("Claude-3p"),
                    )
                }
                #[cfg(not(any(target_os = "macos", windows)))]
                {
                    None
                }
            }
            AppType::Codex => Some(home.join(".codex")),
            AppType::Gemini => Some(home.join(".gemini")),
            AppType::GrokBuild => Some(home.join(".grok")),
            AppType::OpenCode => Some(home.join(".config/opencode")),
            AppType::OpenClaw => Some(home.join(".openclaw")),
            AppType::Hermes => {
                #[cfg(windows)]
                {
                    let local = std::env::var("LOCALAPPDATA")
                        .ok()
                        .map(|v| v.trim().to_string())
                        .filter(|v| !v.is_empty())
                        .map(PathBuf::from)
                        .unwrap_or_else(|| home.join("AppData").join("Local"));
                    Some(local.join("hermes"))
                }
                #[cfg(not(windows))]
                {
                    Some(home.join(".hermes"))
                }
            }
        }
    }

    #[test]
    #[serial]
    fn production_default_paths_match_dirs_home_dir_for_every_app() {
        let saved = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::remove_var("CC_SWITCH_TEST_HOME");

        let from_env = Paths::from_env();
        let current_paths = current();

        if let Some(value) = saved {
            std::env::set_var("CC_SWITCH_TEST_HOME", value);
        }

        assert_eq!(from_env, Paths::system());
        assert_eq!(current_paths, Paths::system());
        assert_eq!(from_env.user_home(), dirs::home_dir().as_deref());

        let legacy_home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        assert_eq!(from_env.home(), legacy_home);
        assert_eq!(from_env.app_config_dir(), legacy_home.join(".cc-switch"));
        assert_eq!(
            from_env.settings_path(),
            legacy_home.join(".cc-switch").join("settings.json")
        );
        assert_eq!(from_env.claude_mcp_path(), legacy_home.join(".claude.json"));

        for app in ALL_APPS.iter() {
            assert_eq!(
                from_env.default_app_dir(app),
                legacy_default_app_dir(app),
                "default dir mismatch for {app:?}"
            );
        }
    }

    #[test]
    #[serial]
    fn test_home_env_override_is_honored() {
        let saved = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", "  /tmp/cc-switch-paths-override  ");
        let paths = Paths::from_env();
        match saved {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }

        assert_eq!(paths.home(), PathBuf::from("/tmp/cc-switch-paths-override"));
    }

    #[test]
    fn scoped_paths_are_thread_local_and_restored_on_drop() {
        let outer = Paths::with_home("/tmp/cc-switch-outer");
        let inner = Paths::with_home("/tmp/cc-switch-inner");

        let outer_guard = scoped(outer.clone());
        assert_eq!(current(), outer);
        {
            let _inner_guard = scoped(inner.clone());
            assert_eq!(current(), inner);
            assert_eq!(
                crate::config::get_home_dir(),
                PathBuf::from("/tmp/cc-switch-inner")
            );

            // 其他线程不受当前线程注入影响
            let other = std::thread::spawn(current).join().expect("join thread");
            assert_ne!(other, inner);
        }
        assert_eq!(current(), outer);
        drop(outer_guard);
        assert_ne!(current(), outer);
    }

    #[test]
    fn every_app_dir_is_rooted_in_injected_home() {
        let paths = Paths::with_home("/tmp/cc-switch-rooted");
        for app in ALL_APPS.iter() {
            if let Some(dir) = paths.default_app_dir(app) {
                // 包括 Windows 上基于 %LOCALAPPDATA% 的 Claude Desktop/Hermes：注入的主目录
                // 不读取进程环境变量，否则测试会写到真实用户的 AppData（CCS-001）。
                assert!(
                    dir.starts_with("/tmp/cc-switch-rooted"),
                    "{app:?} dir {dir:?} should be under injected home"
                );
            }
        }
    }

    #[test]
    #[serial]
    fn injected_home_ignores_process_localappdata() {
        let saved = std::env::var_os("LOCALAPPDATA");
        std::env::set_var("LOCALAPPDATA", "/tmp/cc-switch-real-localappdata");
        let paths = Paths::with_home("/tmp/cc-switch-injected");
        let system = Paths::system();
        match saved {
            Some(value) => std::env::set_var("LOCALAPPDATA", value),
            None => std::env::remove_var("LOCALAPPDATA"),
        }

        assert_eq!(paths.local_app_data(), None);
        assert_eq!(
            paths.local_app_data_dir(),
            Path::new("/tmp/cc-switch-injected")
                .join("AppData")
                .join("Local")
        );
        assert_eq!(
            system.local_app_data_dir(),
            PathBuf::from("/tmp/cc-switch-real-localappdata")
        );
    }
}
