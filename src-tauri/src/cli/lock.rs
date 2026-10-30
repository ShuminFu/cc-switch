//! Cross-process "database writer" lock.
//!
//! SQLite (WAL + busy_timeout) already serialises individual statements, but a
//! provider switch or a proxy takeover is a multi-step operation that also
//! rewrites live CLI config files and the takeover backups. Two processes doing
//! that at once (GUI + `ccs`, or two `ccs` runs) can interleave and leave the
//! live files out of sync with the database. The writer lock makes that
//! impossible: whoever writes holds an OS advisory lock on
//! `<config dir>/cc-switch.lock` for as long as it runs.
//!
//! * The GUI takes it at launch (`main.rs`) and keeps it until exit.
//! * The headless proxy (`ccs proxy start` / `cc-switch --headless`) holds it
//!   while it runs.
//! * `ccs switch` / `ccs import` hold it for the duration of the command.
//!
//! Read-only commands (`list`, `current`, `usage`, `export`, `proxy status`)
//! never take it, so they work while the GUI is open.
//!
//! The lock is released by the OS when the holder exits, even after a crash, so
//! a stale lock file never blocks anyone. Holder details (pid, owner) are kept
//! in a sibling `cc-switch.lock.json` purely for error messages.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const LOCK_FILE_NAME: &str = "cc-switch.lock";
pub const LOCK_INFO_FILE_NAME: &str = "cc-switch.lock.json";

/// Who holds the writer lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LockOwner {
    /// The desktop app (window or tray).
    Gui,
    /// A foreground headless proxy (`ccs proxy start` or `cc-switch --headless`).
    HeadlessProxy,
    /// A short-lived `ccs` write command.
    Cli,
}

impl LockOwner {
    pub fn describe(self) -> &'static str {
        match self {
            LockOwner::Gui => "the CC Switch desktop app",
            LockOwner::HeadlessProxy => "a headless CC Switch proxy",
            LockOwner::Cli => "another ccs command",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockHolder {
    pub owner: LockOwner,
    pub pid: u32,
    pub started_at: String,
}

/// Returned when another process holds the lock.
#[derive(Debug, Clone)]
pub struct LockHeld {
    pub lock_path: PathBuf,
    /// Best-effort holder details; `None` if the info file is missing/unreadable
    /// (e.g. written by an older build).
    pub holder: Option<LockHolder>,
}

impl LockHeld {
    pub fn owner(&self) -> Option<LockOwner> {
        self.holder.as_ref().map(|h| h.owner)
    }

    /// One-line, user-facing explanation.
    pub fn message(&self) -> String {
        match &self.holder {
            Some(holder) => format!(
                "The CC Switch database is in use by {} (pid {}).",
                holder.owner.describe(),
                holder.pid
            ),
            None => format!(
                "The CC Switch database is in use by another process (lock: {}).",
                self.lock_path.display()
            ),
        }
    }
}

/// Held writer lock; released on drop (or when the process exits).
#[derive(Debug)]
pub struct WriterLock {
    file: File,
    info_path: PathBuf,
}

impl WriterLock {
    /// Try to take the lock without blocking.
    pub fn try_acquire(config_dir: &Path, owner: LockOwner) -> Result<WriterLock, AcquireError> {
        crate::config::ensure_private_dir(config_dir)
            .map_err(|e| AcquireError::Io(e.to_string()))?;
        let lock_path = config_dir.join(LOCK_FILE_NAME);
        let info_path = config_dir.join(LOCK_INFO_FILE_NAME);
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(|e| AcquireError::Io(format!("{}: {e}", lock_path.display())))?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(AcquireError::Held(LockHeld {
                    holder: read_holder(&info_path),
                    lock_path,
                }));
            }
            Err(TryLockError::Error(e)) => {
                return Err(AcquireError::Io(format!("{}: {e}", lock_path.display())));
            }
        }
        let holder = LockHolder {
            owner,
            pid: std::process::id(),
            started_at: chrono::Utc::now().to_rfc3339(),
        };
        if let Ok(json) = serde_json::to_vec_pretty(&holder) {
            let _ = std::fs::write(&info_path, json);
        }
        Ok(WriterLock { file, info_path })
    }

    /// Leak the lock so it is held until the process exits (GUI).
    pub fn hold_until_exit(self) {
        std::mem::forget(self);
    }
}

impl Drop for WriterLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.info_path);
        let _ = self.file.unlock();
    }
}

#[derive(Debug, Clone)]
pub enum AcquireError {
    Held(LockHeld),
    Io(String),
}

/// Who currently holds the lock, without taking it. `None` = free.
pub fn current_holder(config_dir: &Path) -> Option<LockHeld> {
    let lock_path = config_dir.join(LOCK_FILE_NAME);
    if !lock_path.exists() {
        return None;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock_path)
        .ok()?;
    match file.try_lock() {
        Ok(()) => {
            let _ = file.unlock();
            None
        }
        Err(TryLockError::WouldBlock) => Some(LockHeld {
            holder: read_holder(&config_dir.join(LOCK_INFO_FILE_NAME)),
            lock_path,
        }),
        Err(TryLockError::Error(_)) => None,
    }
}

fn read_holder(info_path: &Path) -> Option<LockHolder> {
    let raw = std::fs::read(info_path).ok()?;
    serde_json::from_slice(&raw).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_acquire_reports_holder_and_release_frees_it() {
        let dir = tempfile::tempdir().unwrap();
        let first = WriterLock::try_acquire(dir.path(), LockOwner::Gui).expect("first lock");
        match WriterLock::try_acquire(dir.path(), LockOwner::Cli) {
            Err(AcquireError::Held(held)) => {
                assert_eq!(held.owner(), Some(LockOwner::Gui));
                assert_eq!(held.holder.as_ref().unwrap().pid, std::process::id());
                assert!(held.message().contains("desktop app"));
            }
            other => panic!("expected Held, got {other:?}"),
        }
        assert_eq!(
            current_holder(dir.path()).and_then(|h| h.owner()),
            Some(LockOwner::Gui)
        );
        drop(first);
        assert!(current_holder(dir.path()).is_none());
        let again = WriterLock::try_acquire(dir.path(), LockOwner::Cli).expect("re-acquire");
        drop(again);
    }

    #[test]
    fn missing_lock_file_means_free() {
        let dir = tempfile::tempdir().unwrap();
        assert!(current_holder(dir.path()).is_none());
    }
}
