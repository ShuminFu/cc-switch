//! Locating Codex's per-thread state SQLite databases.
//!
//! Codex stores thread metadata in `state_5.sqlite`, normally inside the Codex
//! config dir (`CODEX_HOME` / `~/.codex`). The SQLite location can be moved with
//! the `sqlite_home` key in `config.toml` or the `CODEX_SQLITE_HOME` env var;
//! when set, a second DB lives there. Both history migration and the session
//! list's title lookup need the same resolution, so it lives here once.

use std::path::{Path, PathBuf};

use toml_edit::DocumentMut;

use crate::config::get_home_dir;

/// Filename of the newest Codex per-thread state database this build knows.
/// Codex bumps the version number across releases; [`codex_state_db_paths`]
/// picks up a newer `state_<N>.sqlite` on its own (DB-41), this constant is
/// the fallback name when a directory has none yet.
pub(crate) const CODEX_STATE_DB_FILENAME: &str = "state_5.sqlite";

/// Version number in [`CODEX_STATE_DB_FILENAME`].
const KNOWN_STATE_DB_VERSION: u32 = 5;

/// Version of a `state_<N>.sqlite` filename.
fn state_db_version(file_name: &str) -> Option<u32> {
    let digits = file_name.strip_prefix("state_")?.strip_suffix(".sqlite")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// The state DB inside `dir`: the highest-versioned `state_<N>.sqlite` that
/// exists, or `dir/state_5.sqlite` when there is none (callers skip missing
/// files). A version newer than this build knows is used but logged, since
/// its schema may have drifted (consumers keep their table/column guards).
pub(crate) fn state_db_in(dir: &Path) -> PathBuf {
    let newest = std::fs::read_dir(dir).ok().and_then(|entries| {
        entries
            .flatten()
            .filter(|entry| entry.file_type().map(|t| t.is_file()).unwrap_or(false))
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                state_db_version(&name).map(|version| (version, entry.path()))
            })
            .max_by_key(|(version, _)| *version)
    });
    match newest {
        Some((version, path)) => {
            if version > KNOWN_STATE_DB_VERSION {
                log::warn!(
                    "Codex state DB {} is newer than the known state_{KNOWN_STATE_DB_VERSION}.sqlite; using it",
                    path.display()
                );
            }
            path
        }
        None => dir.join(CODEX_STATE_DB_FILENAME),
    }
}

/// Env var that overrides the Codex SQLite state directory.
const CODEX_SQLITE_HOME_ENV: &str = "CODEX_SQLITE_HOME";

/// Resolve every candidate state DB path: the config-dir DB plus, when
/// Codex is configured to keep its SQLite state elsewhere, that DB too.
///
/// `config_dir` is the Codex config dir (`~/.codex`); `config_text` is the raw
/// `config.toml` contents, used to detect a `sqlite_home` override.
pub(crate) fn codex_state_db_paths(config_dir: &Path, config_text: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    push_unique_path(&mut paths, state_db_in(config_dir));
    // Codex lets SQLite state move away from CODEX_HOME; config takes precedence.
    if let Some(sqlite_home) = sqlite_home_from_codex_config(config_text) {
        push_unique_path(&mut paths, state_db_in(&sqlite_home));
    } else if let Some(sqlite_home) = sqlite_home_from_env() {
        push_unique_path(&mut paths, state_db_in(&sqlite_home));
    }
    paths
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.contains(&path) {
        paths.push(path);
    }
}

fn sqlite_home_from_codex_config(config_text: &str) -> Option<PathBuf> {
    let doc = config_text.parse::<DocumentMut>().ok()?;
    let raw = doc.get("sqlite_home")?.as_str()?.trim();
    if raw.is_empty() {
        return None;
    }
    Some(resolve_user_path(raw))
}

fn sqlite_home_from_env() -> Option<PathBuf> {
    let raw = std::env::var(CODEX_SQLITE_HOME_ENV).ok()?;
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    Some(resolve_user_path(raw))
}

fn resolve_user_path(raw: &str) -> PathBuf {
    if raw == "~" {
        return get_home_dir();
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return get_home_dir().join(rest);
    }
    if let Some(rest) = raw.strip_prefix("~\\") {
        return get_home_dir().join(rest);
    }
    PathBuf::from(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// DB-41: a newer Codex state DB is picked up without a code change.
    #[test]
    fn resolves_the_highest_state_db_version_in_a_dir() {
        let temp = tempdir().expect("tempdir");
        assert_eq!(
            state_db_in(temp.path()),
            temp.path().join(CODEX_STATE_DB_FILENAME),
            "no state DB yet: the known name is the (missing) candidate"
        );

        std::fs::write(temp.path().join("state_6.sqlite"), b"").unwrap();
        assert_eq!(state_db_in(temp.path()), temp.path().join("state_6.sqlite"));

        for name in [
            "state_5.sqlite",
            "state_12.sqlite",
            "state_x.sqlite",
            "state_99.sqlite-wal",
        ] {
            std::fs::write(temp.path().join(name), b"").unwrap();
        }
        std::fs::create_dir(temp.path().join("state_40.sqlite")).unwrap();
        assert_eq!(
            state_db_in(temp.path()),
            temp.path().join("state_12.sqlite")
        );
        assert_eq!(state_db_version("state_.sqlite"), None);
        assert_eq!(state_db_version("state_7.sqlite"), Some(7));
    }

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
}
