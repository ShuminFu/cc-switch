//! Locating Codex's per-thread state SQLite databases.
//!
//! Codex stores thread metadata in `state_5.sqlite`, normally inside the Codex
//! config dir (`CODEX_HOME` / `~/.codex`). The SQLite location can be moved with
//! the `sqlite_home` key in `config.toml` or the `CODEX_SQLITE_HOME` env var;
//! when set, a second DB lives there. Both history migration and the session
//! list's title lookup need the same resolution, so it lives here once.
//!
//! Path resolution mirrors upstream Codex (openai/codex `codex-rs`):
//! - `sqlite_home` in `config.toml` is an `AbsolutePathBuf`; a relative value
//!   is resolved against the directory containing that `config.toml` (the
//!   Codex home), after `~` / `~/` expansion, and normalized lexically
//!   (`config/src/loader/mod.rs` `resolve_relative_paths_in_config_toml`,
//!   `utils/absolute-path/src/lib.rs` `resolve_path_against_base`).
//! - `CODEX_SQLITE_HOME` is resolved against the Codex session's cwd
//!   (`core/src/config/mod.rs` `resolve_sqlite_home_env`). We cannot know that
//!   cwd, so a relative env value falls back to our own process cwd.

use std::path::{Component, Path, PathBuf};

use toml_edit::DocumentMut;

use crate::config::get_home_dir;

/// Filename of Codex's per-thread state database. Codex bumps the version
/// number across releases; update this single source of truth when a new state
/// DB version ships.
pub(crate) const CODEX_STATE_DB_FILENAME: &str = "state_5.sqlite";

/// Env var that overrides the Codex SQLite state directory.
const CODEX_SQLITE_HOME_ENV: &str = "CODEX_SQLITE_HOME";

/// Resolve every candidate `state_5.sqlite` path: the config-dir DB plus, when
/// Codex is configured to keep its SQLite state elsewhere, that DB too.
///
/// `config_dir` is the Codex config dir (`~/.codex`); `config_text` is the raw
/// `config.toml` contents, used to detect a `sqlite_home` override.
pub(crate) fn codex_state_db_paths(config_dir: &Path, config_text: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    push_unique_path(&mut paths, config_dir.join(CODEX_STATE_DB_FILENAME));
    // Codex lets SQLite state move away from CODEX_HOME; config takes precedence.
    if let Some(sqlite_home) = sqlite_home_from_codex_config(config_dir, config_text) {
        push_unique_path(&mut paths, sqlite_home.join(CODEX_STATE_DB_FILENAME));
    } else if let Some(sqlite_home) = sqlite_home_from_env() {
        push_unique_path(&mut paths, sqlite_home.join(CODEX_STATE_DB_FILENAME));
    }
    paths
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.contains(&path) {
        paths.push(path);
    }
}

/// `sqlite_home` from `config.toml`. Like Codex, a relative value is resolved
/// against `config_dir` (the directory holding `config.toml`), not the cwd.
fn sqlite_home_from_codex_config(config_dir: &Path, config_text: &str) -> Option<PathBuf> {
    let doc = config_text.parse::<DocumentMut>().ok()?;
    let raw = doc.get("sqlite_home")?.as_str()?.trim();
    if raw.is_empty() {
        return None;
    }
    let expanded = resolve_user_path(raw);
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        config_dir.join(expanded)
    };
    Some(normalize_lexically(&absolute))
}

/// Drop `.` and resolve `..` without touching the filesystem, matching Codex's
/// `absolutize::normalize_path`, so e.g. `sqlite_home = "."` dedupes against
/// the config-dir DB.
fn normalize_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

fn sqlite_home_from_env() -> Option<PathBuf> {
    let raw = std::env::var(CODEX_SQLITE_HOME_ENV).ok()?;
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    Some(resolve_user_path(raw))
}

/// `~` / `~/…` (and `~\…` on Windows) expansion, as in Codex's
/// `AbsolutePathBuf::maybe_expand_home_directory`. `~user` is not expanded.
fn resolve_user_path(raw: &str) -> PathBuf {
    if raw == "~" {
        return get_home_dir();
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return get_home_dir().join(rest.trim_start_matches('/'));
    }
    if cfg!(windows) {
        if let Some(rest) = raw.strip_prefix("~\\") {
            return get_home_dir().join(rest.trim_start_matches('\\'));
        }
    }
    PathBuf::from(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn includes_config_sqlite_home() {
        let temp = tempdir().expect("tempdir");
        let sqlite_home = temp.path().join("sqlite-home");
        // 用 TOML 字面量字符串(单引号)承载路径：Windows 路径含反斜杠，basic string(双引号)
        // 会把 `\U`/`\s` 等当作非法转义导致解析失败。
        let config_text = format!("sqlite_home = '{}'\n", sqlite_home.display());

        let paths = codex_state_db_paths(temp.path(), &config_text);

        assert_eq!(
            paths,
            vec![
                temp.path().join(CODEX_STATE_DB_FILENAME),
                sqlite_home.join(CODEX_STATE_DB_FILENAME),
            ]
        );
    }

    #[test]
    fn relative_config_sqlite_home_resolves_against_config_dir() {
        let config_dir = Path::new("/codex-home");
        assert_eq!(
            sqlite_home_from_codex_config(config_dir, "sqlite_home = 'state'\n"),
            Some(PathBuf::from("/codex-home/state"))
        );
        assert_eq!(
            sqlite_home_from_codex_config(config_dir, "sqlite_home = './a/../b'\n"),
            Some(PathBuf::from("/codex-home/b"))
        );
        assert_eq!(
            sqlite_home_from_codex_config(config_dir, "sqlite_home = '../shared'\n"),
            Some(PathBuf::from("/shared"))
        );
    }

    #[test]
    fn config_sqlite_home_expands_tilde() {
        let config_dir = Path::new("/codex-home");
        assert_eq!(
            sqlite_home_from_codex_config(config_dir, "sqlite_home = '~'\n"),
            Some(get_home_dir())
        );
        assert_eq!(
            sqlite_home_from_codex_config(config_dir, "sqlite_home = '~/codex-db'\n"),
            Some(get_home_dir().join("codex-db"))
        );
        // `~user` is not a home reference; it is a relative directory name.
        assert_eq!(
            sqlite_home_from_codex_config(config_dir, "sqlite_home = '~other'\n"),
            Some(PathBuf::from("/codex-home/~other"))
        );
    }

    #[test]
    fn config_sqlite_home_dot_dedupes_with_config_dir_db() {
        let temp = tempdir().expect("tempdir");
        let paths = codex_state_db_paths(temp.path(), "sqlite_home = '.'\n");
        assert_eq!(paths, vec![temp.path().join(CODEX_STATE_DB_FILENAME)]);
    }

    #[test]
    fn missing_or_blank_config_sqlite_home_is_ignored() {
        let config_dir = Path::new("/codex-home");
        assert_eq!(sqlite_home_from_codex_config(config_dir, ""), None);
        assert_eq!(
            sqlite_home_from_codex_config(config_dir, "sqlite_home = '  '\n"),
            None
        );
        assert_eq!(
            sqlite_home_from_codex_config(config_dir, "sqlite_home = 1\n"),
            None
        );
    }
}
