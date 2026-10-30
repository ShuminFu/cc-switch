//! Shared harness for the integration tests under `src-tauri/tests/`.
//!
//! Every test should start with `let env = TestEnv::new();`. The guard:
//!
//! * serialises tests that touch process-global state (the `HOME` /
//!   `CC_SWITCH_TEST_HOME` env vars and the in-memory settings cache) through a
//!   lock that **recovers from poisoning**, so one failing test no longer makes
//!   every later test in the same binary fail with `PoisonError`;
//! * points `HOME`, `USERPROFILE` and `CC_SWITCH_TEST_HOME` at a fresh,
//!   per-test temporary directory (deleted on drop), so no state leaks between
//!   tests even when a previous test panicked half-way through;
//! * resets the settings cache to `AppSettings::default()` on entry and exit.
//!
//! The older helpers (`test_mutex()`, `ensure_test_home()`, `reset_test_fs()`)
//! are kept as thin compatibility shims so that test files written against the
//! previous API keep compiling: `test_mutex().lock()` now yields a `TestEnv`,
//! and `ensure_test_home()` returns the home directory of the active `TestEnv`.
#![allow(dead_code)]

use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use cc_switch_lib::{update_settings, AppSettings, AppState, Database, MultiAppConfig};

/// Lock that serialises access to the process-wide environment. A panic while
/// holding it poisons the inner `Mutex`, which we deliberately ignore: every
/// `TestEnv` rebuilds the state it needs from scratch, so a poisoned lock
/// carries no stale data worth protecting.
fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn lock_env() -> MutexGuard<'static, ()> {
    env_lock().lock().unwrap_or_else(PoisonError::into_inner)
}

/// Home directory handed out by `ensure_test_home()`. It is replaced by every
/// `TestEnv::new()`; the paths are leaked (a few bytes per test) so that the
/// legacy `&'static Path` signature can be preserved.
fn current_home() -> &'static Mutex<Option<&'static Path>> {
    static CURRENT: OnceLock<Mutex<Option<&'static Path>>> = OnceLock::new();
    CURRENT.get_or_init(|| Mutex::new(None))
}

fn point_env_at(home: &Path) {
    // On Windows `dirs::home_dir()` ignores HOME/USERPROFILE (Known Folder
    // API), so CC_SWITCH_TEST_HOME is the override that actually matters.
    std::env::set_var("CC_SWITCH_TEST_HOME", home);
    std::env::set_var("HOME", home);
    #[cfg(windows)]
    std::env::set_var("USERPROFILE", home);
}

/// Per-test isolated environment. See the module docs.
pub struct TestEnv {
    // Field order matters: the temp dir is removed before the lock is released.
    dir: Option<tempfile::TempDir>,
    home: &'static Path,
    _guard: MutexGuard<'static, ()>,
}

impl TestEnv {
    pub fn new() -> Self {
        let guard = lock_env();
        let dir = tempfile::Builder::new()
            .prefix("cc-switch-test-home-")
            .tempdir()
            .expect("create per-test home directory");
        let home: &'static Path = Box::leak(dir.path().to_path_buf().into_boxed_path());
        point_env_at(home);
        *current_home()
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(home);
        let _ = update_settings(AppSettings::default());
        Self {
            dir: Some(dir),
            home,
            _guard: guard,
        }
    }

    /// The isolated home directory for this test.
    pub fn home(&self) -> &'static Path {
        self.home
    }
}

impl Default for TestEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        let _ = update_settings(AppSettings::default());
        *current_home()
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
        if let Some(dir) = self.dir.take() {
            // Best effort: on Windows an open SQLite handle can keep files
            // locked; a leftover temp dir is harmless.
            let _ = dir.close();
        }
    }
}

/// Compatibility shim for `test_mutex().lock().expect(..)`: the "lock" now
/// returns a full `TestEnv` and can never fail with `PoisonError`.
pub struct TestMutex;

impl TestMutex {
    pub fn lock(&self) -> Result<TestEnv, Infallible> {
        Ok(TestEnv::new())
    }
}

/// Legacy entry point; prefer `TestEnv::new()`.
pub fn test_mutex() -> &'static TestMutex {
    static MUTEX: TestMutex = TestMutex;
    &MUTEX
}

/// Returns the home directory of the active `TestEnv`. Outside a `TestEnv`
/// (legacy callers) it lazily creates a shared fallback directory.
pub fn ensure_test_home() -> &'static Path {
    let mut current = current_home()
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if let Some(home) = *current {
        return home;
    }
    let base = std::env::temp_dir().join(format!("cc-switch-test-home-{}", std::process::id()));
    std::fs::create_dir_all(&base).expect("create fallback test home");
    let home: &'static Path = Box::leak(base.into_boxed_path());
    point_env_at(home);
    *current = Some(home);
    home
}

/// Removes the app/CLI config directories inside the current test home and
/// resets the settings cache. With `TestEnv` the home is already fresh, so
/// this is only needed by tests that want to wipe live configs mid-test;
/// unrelated files (e.g. an export written to the home root) are kept.
pub fn reset_test_fs() {
    let home = ensure_test_home();
    for sub in [
        ".claude",
        ".codex",
        ".cc-switch",
        ".gemini",
        ".grok",
        ".config",
        ".openclaw",
        "profiles",
    ] {
        let path = home.join(sub);
        if path.exists() {
            if let Err(err) = std::fs::remove_dir_all(&path) {
                eprintln!("failed to clean {}: {}", path.display(), err);
            }
        }
    }
    let _ = std::fs::remove_file(home.join(".claude.json"));
    let _ = update_settings(AppSettings::default());
}

pub fn enable_codex_official_auth_preservation() {
    update_settings(AppSettings {
        preserve_codex_official_auth_on_switch: true,
        ..Default::default()
    })
    .expect("enable Codex official auth preservation");
}

/// `AppState` backed by an in-memory database. Use this unless the test needs
/// the database file itself (backups, restore, file-level export).
pub fn create_test_state() -> Result<AppState, Box<dyn std::error::Error>> {
    let db = Arc::new(Database::memory()?);
    Ok(AppState::new(db))
}

/// `AppState` backed by `<test home>/.cc-switch/cc-switch.db`.
pub fn create_disk_test_state() -> Result<AppState, Box<dyn std::error::Error>> {
    let db = Arc::new(Database::init()?);
    Ok(AppState::new(db))
}

/// In-memory `AppState` seeded from a legacy `MultiAppConfig`.
pub fn create_test_state_with_config(
    config: &MultiAppConfig,
) -> Result<AppState, Box<dyn std::error::Error>> {
    let db = Arc::new(Database::memory()?);
    db.migrate_from_json(config)?;
    Ok(AppState::new(db))
}

/// Disk-backed variant of [`create_test_state_with_config`], for tests that
/// assert on the database file itself.
pub fn create_disk_test_state_with_config(
    config: &MultiAppConfig,
) -> Result<AppState, Box<dyn std::error::Error>> {
    let db = Arc::new(Database::init()?);
    db.migrate_from_json(config)?;
    Ok(AppState::new(db))
}

/// Path helper used by tests that want a guaranteed-unwritable target, even
/// when the suite runs as root: a regular file used as a parent directory.
pub fn unwritable_path(env: &TestEnv, file_name: &str) -> PathBuf {
    let blocker = env.home().join("not-a-directory");
    std::fs::write(&blocker, b"blocker").expect("create blocker file");
    blocker.join(file_name)
}
