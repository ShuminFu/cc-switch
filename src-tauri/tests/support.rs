use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use cc_switch_lib::paths::{self, Paths, ScopedPaths};
use cc_switch_lib::{
    scoped_settings_store, update_settings, AppSettings, AppState, Database, MultiAppConfig,
    ScopedSettingsStore,
};
use tempfile::TempDir;

/// 单个测试的隔离环境：独立的临时 HOME、注入到当前线程的 [`Paths`] 与设置缓存。
///
/// 不再修改进程级 `HOME`，也不需要全局互斥锁，测试可按默认并行度运行。
/// 注入仅对创建它的线程生效；`#[tokio::test]` 默认的 current_thread 运行时满足这一点。
pub struct TestEnv {
    // 字段按声明顺序 drop：先撤销设置缓存与路径注入，最后删除临时目录。
    _settings: ScopedSettingsStore,
    _paths: ScopedPaths,
    #[allow(dead_code)]
    home: TempDir,
}

impl TestEnv {
    pub fn new() -> Self {
        install_fallback_home();
        let home = tempfile::Builder::new()
            .prefix("cc-switch-test-home-")
            .tempdir()
            .expect("create test home");
        let paths = paths::scoped(Paths::with_home(home.path()));
        let settings = scoped_settings_store();
        Self {
            _settings: settings,
            _paths: paths,
            home,
        }
    }

    /// 本测试的 HOME 目录
    #[allow(dead_code)]
    pub fn home(&self) -> &Path {
        self.home.path()
    }
}

impl Default for TestEnv {
    fn default() -> Self {
        Self::new()
    }
}

/// 兜底：未继承线程注入的代码（例如库内部另起线程）解析到一个共享的临时目录，
/// 而不是真实用户目录。正常测试路径不会落到这里。
fn install_fallback_home() {
    static FALLBACK: OnceLock<PathBuf> = OnceLock::new();
    FALLBACK.get_or_init(|| {
        let base = std::env::temp_dir().join(format!(
            "cc-switch-test-fallback-home-{}",
            std::process::id()
        ));
        // Windows 上 `dirs::home_dir()` 不受 HOME/USERPROFILE 影响（走 Known Folder API），
        // 用 CC_SWITCH_TEST_HOME 显式覆盖，以确保测试不会污染真实用户目录。
        std::env::set_var("CC_SWITCH_TEST_HOME", &base);
        std::env::set_var("HOME", &base);
        #[cfg(windows)]
        std::env::set_var("USERPROFILE", &base);
        base
    });
}

#[allow(dead_code)]
pub fn enable_codex_official_auth_preservation() {
    update_settings(AppSettings {
        preserve_codex_official_auth_on_switch: true,
        ..Default::default()
    })
    .expect("enable Codex official auth preservation");
}

/// 创建测试用的 AppState，包含一个空的数据库
#[allow(dead_code)]
pub fn create_test_state() -> Result<AppState, Box<dyn std::error::Error>> {
    let db = Arc::new(Database::init()?);
    Ok(AppState::new(db))
}

/// 创建测试用的 AppState，并从 MultiAppConfig 迁移数据
#[allow(dead_code)]
pub fn create_test_state_with_config(
    config: &MultiAppConfig,
) -> Result<AppState, Box<dyn std::error::Error>> {
    let db = Arc::new(Database::init()?);
    db.migrate_from_json(config)?;
    Ok(AppState::new(db))
}
