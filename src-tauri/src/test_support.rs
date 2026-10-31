//! Shared helpers for unit tests (TEST-09, TEST-25).
//!
//! * [`isolated_home`] gives the current test thread its own temporary home
//!   directory through a thread-local seam in `config::get_home_dir()`. While
//!   it is active, path resolution also ignores process-wide overrides (the
//!   settings cache's `*_config_dir` values, `HERMES_HOME`, `LOCALAPPDATA`),
//!   so a test using it needs neither env-var mutation nor `#[serial]`.
//!   Code that hops to another thread (a multi-threaded tokio runtime,
//!   `std::thread::spawn`) does not see the thread-local home; keep such tests
//!   on the env-based helpers.
//! * [`EnvGuard`] sets or removes env vars and restores them on drop, for the
//!   few tests that exercise env-var precedence themselves (those still need
//!   `#[serial]`).
#![allow(dead_code)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// A per-thread temp home; dropping it restores the previous thread home and
/// deletes the directory.
pub(crate) struct IsolatedHome {
    dir: tempfile::TempDir,
    previous: Option<PathBuf>,
}

impl IsolatedHome {
    pub(crate) fn path(&self) -> &Path {
        self.dir.path()
    }
}

impl Drop for IsolatedHome {
    fn drop(&mut self) {
        crate::config::set_thread_test_home(self.previous.take());
    }
}

pub(crate) fn isolated_home() -> IsolatedHome {
    let dir = tempfile::Builder::new()
        .prefix("cc-switch-thread-home-")
        .tempdir()
        .expect("create thread test home");
    let previous = crate::config::set_thread_test_home(Some(dir.path().to_path_buf()));
    IsolatedHome { dir, previous }
}

/// Sets/removes env vars for its lifetime and restores the old values on drop.
pub(crate) struct EnvGuard {
    saved: Vec<(String, Option<OsString>)>,
}

impl EnvGuard {
    /// `vars`: `(name, Some(value))` to set, `(name, None)` to remove.
    pub(crate) fn new(vars: &[(&str, Option<&str>)]) -> Self {
        let mut saved = Vec::new();
        for (name, value) in vars {
            saved.push((name.to_string(), std::env::var_os(name)));
            match value {
                Some(v) => std::env::set_var(name, v),
                None => std::env::remove_var(name),
            }
        }
        Self { saved }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (name, value) in self.saved.drain(..).rev() {
            match value {
                Some(v) => std::env::set_var(&name, v),
                None => std::env::remove_var(&name),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolated_home_is_per_thread_and_restored() {
        let outer = isolated_home();
        assert_eq!(crate::config::get_home_dir(), outer.path());
        assert!(crate::config::thread_isolated());
        {
            let inner = isolated_home();
            assert_eq!(crate::config::get_home_dir(), inner.path());
        }
        assert_eq!(crate::config::get_home_dir(), outer.path());

        let other_thread = std::thread::spawn(crate::config::thread_isolated)
            .join()
            .unwrap();
        assert!(!other_thread, "the seam is thread-local");
        drop(outer);
        assert!(!crate::config::thread_isolated());
    }

    #[test]
    fn isolated_home_ignores_shared_override_dirs() {
        let home = isolated_home();
        assert_eq!(crate::settings::get_opencode_override_dir(), None);
        assert!(crate::opencode_config::get_opencode_dir().starts_with(home.path()));
        assert!(crate::hermes_config::get_hermes_dir().starts_with(home.path()));
    }
}
