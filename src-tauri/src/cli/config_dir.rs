//! Config directory resolution for the CLI and the headless proxy.
//!
//! The GUI keeps its "custom data directory" override in the Tauri store file
//! `<app data dir>/com.ccswitch.desktop/app_paths.json` (see `app_store.rs`).
//! The CLI has no `AppHandle`, so it reads that file directly and feeds the
//! result into the same process-wide override cache that
//! [`crate::config::get_app_config_dir`] consults. Every later path lookup
//! (database, settings.json, backups, lock file) therefore resolves exactly as
//! it does in the GUI.
//!
//! Precedence: `--config-dir` flag > `CC_SWITCH_CONFIG_DIR` env var > GUI store
//! override > default (`~/.cc-switch`).

use std::path::{Path, PathBuf};

use crate::app_store::{
    resolve_path, set_app_config_dir_override_for_process, STORE_KEY_APP_CONFIG_DIR,
};

/// Tauri bundle identifier (tauri.conf.json `identifier`).
pub const APP_IDENTIFIER: &str = "com.ccswitch.desktop";
/// Store file the GUI writes the config dir override to.
pub const STORE_FILE_NAME: &str = "app_paths.json";
/// Environment variable that overrides the config dir for CLI / headless runs.
pub const CONFIG_DIR_ENV: &str = "CC_SWITCH_CONFIG_DIR";

/// Where a resolved config dir came from (shown by `--json` outputs and errors).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigDirSource {
    Flag,
    Env,
    GuiStore,
    Default,
}

impl ConfigDirSource {
    pub fn as_str(self) -> &'static str {
        match self {
            ConfigDirSource::Flag => "flag",
            ConfigDirSource::Env => "env",
            ConfigDirSource::GuiStore => "gui-store",
            ConfigDirSource::Default => "default",
        }
    }
}

/// Platform app-data directory that holds the Tauri store, i.e. what
/// `app.path().app_data_dir()` returns in the GUI.
///
/// When `CC_SWITCH_TEST_HOME` is set (tests), it is derived from that home so a
/// test never reads the real user's store file.
pub fn gui_store_path() -> Option<PathBuf> {
    let data_dir = match crate::config::test_home_override() {
        Some(home) => {
            if cfg!(target_os = "macos") {
                home.join("Library").join("Application Support")
            } else if cfg!(windows) {
                home.join("AppData").join("Roaming")
            } else {
                home.join(".local").join("share")
            }
        }
        None => dirs::data_dir()?,
    };
    Some(data_dir.join(APP_IDENTIFIER).join(STORE_FILE_NAME))
}

/// Read the override from a store file. Mirrors `app_store::read_override_from_store`:
/// blank values and directories that do not exist are ignored.
pub fn read_store_override(store_file: &Path) -> Option<PathBuf> {
    let raw = std::fs::read_to_string(store_file).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let path_str = value.get(STORE_KEY_APP_CONFIG_DIR)?.as_str()?.trim();
    if path_str.is_empty() {
        return None;
    }
    let path = resolve_path(path_str);
    path.exists().then_some(path)
}

/// Pure resolution step (no side effects), exposed for tests.
pub fn resolve_override(
    flag: Option<&Path>,
    env_value: Option<&str>,
    store_file: Option<&Path>,
) -> (Option<PathBuf>, ConfigDirSource) {
    if let Some(flag) = flag {
        return (
            Some(resolve_path(&flag.to_string_lossy())),
            ConfigDirSource::Flag,
        );
    }
    if let Some(env) = env_value.map(str::trim).filter(|v| !v.is_empty()) {
        return (Some(resolve_path(env)), ConfigDirSource::Env);
    }
    if let Some(found) = store_file.and_then(read_store_override) {
        return (Some(found), ConfigDirSource::GuiStore);
    }
    (None, ConfigDirSource::Default)
}

/// Resolve the override from flag/env/store and install it for this process.
/// Returns the effective config dir and its source.
pub fn apply(flag: Option<&Path>) -> (PathBuf, ConfigDirSource) {
    let env_value = std::env::var(CONFIG_DIR_ENV).ok();
    let store = gui_store_path();
    let (override_dir, source) = resolve_override(flag, env_value.as_deref(), store.as_deref());
    set_app_config_dir_override_for_process(override_dir);
    (crate::config::get_app_config_dir(), source)
}

/// Clear any override installed by [`apply`] (used by tests).
pub fn reset() {
    set_app_config_dir_override_for_process(None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_wins_over_env_and_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("app_paths.json");
        std::fs::write(
            &store,
            format!(
                r#"{{"app_config_dir_override": "{}"}}"#,
                dir.path().display().to_string().replace('\\', "\\\\")
            ),
        )
        .unwrap();
        let (path, source) =
            resolve_override(Some(Path::new("/flag/dir")), Some("/env/dir"), Some(&store));
        assert_eq!(path, Some(PathBuf::from("/flag/dir")));
        assert_eq!(source, ConfigDirSource::Flag);

        let (path, source) = resolve_override(None, Some("/env/dir"), Some(&store));
        assert_eq!(path, Some(PathBuf::from("/env/dir")));
        assert_eq!(source, ConfigDirSource::Env);

        let (path, source) = resolve_override(None, Some("  "), Some(&store));
        assert_eq!(path.as_deref(), Some(dir.path()));
        assert_eq!(source, ConfigDirSource::GuiStore);
    }

    #[test]
    fn store_override_ignores_missing_dirs_and_garbage() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("app_paths.json");
        std::fs::write(
            &store,
            r#"{"app_config_dir_override": "/definitely/not/here/ccs"}"#,
        )
        .unwrap();
        assert_eq!(read_store_override(&store), None);
        std::fs::write(&store, "not json").unwrap();
        assert_eq!(read_store_override(&store), None);
        std::fs::write(&store, r#"{"app_config_dir_override": 3}"#).unwrap();
        assert_eq!(read_store_override(&store), None);
        let (path, source) = resolve_override(None, None, Some(&store));
        assert_eq!(path, None);
        assert_eq!(source, ConfigDirSource::Default);
    }
}
