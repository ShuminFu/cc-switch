//! Session quarantine ("recently deleted", SESS-41).
//!
//! Deleting a file-based session used to `remove_file` / `remove_dir_all` the
//! transcript (and, for Claude, the sidecar directory with subagents and tool
//! results), so one mis-click destroyed history the source CLI needs for
//! `--resume`. Deletion now moves the session's paths into
//! `<app config dir>/session-trash/<entry id>/` together with a
//! `manifest.json` recording the original locations; [`restore`] moves them
//! back and entries older than [`TRASH_TTL`] are purged on the next delete.
//!
//! SQLite-backed providers (OpenCode db, Hermes db) still delete rows.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Trashed sessions are kept this long.
pub const TRASH_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const MANIFEST: &str = "manifest.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TrashedItem {
    /// Where the path lived before deletion.
    pub original_path: PathBuf,
    /// Name inside the entry directory.
    pub stored_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TrashEntry {
    pub id: String,
    pub provider_id: String,
    pub session_id: String,
    /// Unix milliseconds.
    pub deleted_at: i64,
    pub items: Vec<TrashedItem>,
}

#[cfg(test)]
thread_local! {
    static TEST_ROOT: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Point this test thread's trash at `root` (tests never touch the real one).
#[cfg(test)]
pub(crate) fn set_test_root(root: Option<PathBuf>) {
    TEST_ROOT.with(|r| *r.borrow_mut() = root);
}

#[cfg(not(test))]
fn trash_root() -> PathBuf {
    crate::config::get_app_config_dir().join("session-trash")
}

#[cfg(test)]
fn trash_root() -> PathBuf {
    TEST_ROOT
        .with(|r| r.borrow().clone())
        .unwrap_or_else(|| std::env::temp_dir().join("cc-switch-test-session-trash"))
}

fn sanitize(component: &str) -> String {
    component
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect()
}

/// Move `path` to `dest`: rename, or copy + remove across filesystems.
fn move_path(path: &Path, dest: &Path) -> std::io::Result<()> {
    match std::fs::rename(path, dest) {
        Ok(()) => Ok(()),
        Err(_) => {
            copy_recursive(path, dest)?;
            if path.is_dir() {
                std::fs::remove_dir_all(path)
            } else {
                std::fs::remove_file(path)
            }
        }
    }
}

fn copy_recursive(src: &Path, dest: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(src)?;
    if meta.is_dir() {
        std::fs::create_dir_all(dest)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &dest.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(src, dest).map(|_| ())
    }
}

/// Move the existing `paths` of one session into a new trash entry. Missing
/// paths are skipped; with nothing to move no entry is created.
pub fn move_to_trash(
    provider_id: &str,
    session_id: &str,
    paths: &[&Path],
) -> Result<Option<String>, String> {
    let existing: Vec<&Path> = paths
        .iter()
        .copied()
        .filter(|p| std::fs::symlink_metadata(p).is_ok())
        .collect();
    if existing.is_empty() {
        return Ok(None);
    }
    let root = trash_root();
    purge_expired_in(&root, TRASH_TTL);

    let deleted_at = chrono::Utc::now().timestamp_millis();
    let id = format!(
        "{deleted_at}-{}-{}-{}",
        sanitize(provider_id),
        sanitize(session_id),
        uuid::Uuid::new_v4().simple()
    );
    let dir = root.join(&id);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create session trash {}: {e}", dir.display()))?;

    let mut entry = TrashEntry {
        id: id.clone(),
        provider_id: provider_id.to_string(),
        session_id: session_id.to_string(),
        deleted_at,
        items: Vec::new(),
    };
    for (index, path) in existing.into_iter().enumerate() {
        let base = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "item".to_string());
        let stored_name = format!("{index}-{base}");
        if let Err(e) = move_path(path, &dir.join(&stored_name)) {
            // Persist what was moved so far so it stays restorable.
            let _ = write_manifest(&dir, &entry);
            return Err(format!(
                "Failed to move {} to the session trash: {e}",
                path.display()
            ));
        }
        entry.items.push(TrashedItem {
            original_path: path.to_path_buf(),
            stored_name,
        });
    }
    write_manifest(&dir, &entry)?;
    Ok(Some(id))
}

fn write_manifest(dir: &Path, entry: &TrashEntry) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(entry).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(MANIFEST), json)
        .map_err(|e| format!("Failed to write session trash manifest: {e}"))
}

fn read_entry(dir: &Path) -> Option<TrashEntry> {
    let data = std::fs::read(dir.join(MANIFEST)).ok()?;
    serde_json::from_slice(&data).ok()
}

/// Trash entries, newest first.
pub fn list() -> Vec<TrashEntry> {
    list_in(&trash_root())
}

fn list_in(root: &Path) -> Vec<TrashEntry> {
    let Ok(read) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut entries: Vec<TrashEntry> = read
        .filter_map(Result::ok)
        .filter_map(|e| read_entry(&e.path()))
        .collect();
    entries.sort_by_key(|e| std::cmp::Reverse(e.deleted_at));
    entries
}

/// Move a trashed session back to its original location. Refuses to
/// overwrite a path that exists again.
pub fn restore(id: &str) -> Result<TrashEntry, String> {
    if id.is_empty() || sanitize(id) != id {
        return Err(format!("Invalid trash entry id: {id}"));
    }
    let dir = trash_root().join(id);
    let entry = read_entry(&dir).ok_or_else(|| format!("Trash entry not found: {id}"))?;
    for item in &entry.items {
        if std::fs::symlink_metadata(&item.original_path).is_ok() {
            return Err(format!(
                "Cannot restore: {} already exists",
                item.original_path.display()
            ));
        }
    }
    for item in &entry.items {
        if let Some(parent) = item.original_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        move_path(&dir.join(&item.stored_name), &item.original_path)
            .map_err(|e| format!("Failed to restore {}: {e}", item.original_path.display()))?;
    }
    let _ = std::fs::remove_dir_all(&dir);
    Ok(entry)
}

fn purge_expired_in(root: &Path, ttl: Duration) {
    let cutoff = chrono::Utc::now().timestamp_millis() - ttl.as_millis() as i64;
    for entry in list_in(root) {
        if entry.deleted_at < cutoff {
            let _ = std::fs::remove_dir_all(root.join(&entry.id));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trash_and_restore_round_trip() {
        let temp = tempfile::tempdir().unwrap();
        set_test_root(Some(temp.path().join("trash")));
        let project = temp.path().join("projects");
        std::fs::create_dir_all(project.join("s1").join("subagents")).unwrap();
        std::fs::write(project.join("s1.jsonl"), "line\n").unwrap();
        std::fs::write(project.join("s1").join("subagents").join("a.jsonl"), "x").unwrap();

        let jsonl = project.join("s1.jsonl");
        let sidecar = project.join("s1");
        let missing = project.join("missing");
        let id = move_to_trash("claude", "s1", &[&sidecar, &jsonl, &missing])
            .unwrap()
            .unwrap();
        assert!(!jsonl.exists() && !sidecar.exists());

        let listed = list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].items.len(), 2);

        restore(&id).unwrap();
        assert_eq!(std::fs::read_to_string(&jsonl).unwrap(), "line\n");
        assert!(sidecar.join("subagents").join("a.jsonl").exists());
        assert!(list().is_empty());
        set_test_root(None);
    }

    #[test]
    fn restore_refuses_to_overwrite_and_rejects_bad_ids() {
        let temp = tempfile::tempdir().unwrap();
        set_test_root(Some(temp.path().join("trash")));
        let file = temp.path().join("s.json");
        std::fs::write(&file, "old").unwrap();
        let id = move_to_trash("gemini", "s", &[&file]).unwrap().unwrap();
        std::fs::write(&file, "new").unwrap();
        assert!(restore(&id).is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "new");
        assert!(restore("../etc").is_err());
        assert!(
            move_to_trash("gemini", "none", &[&temp.path().join("nope")])
                .unwrap()
                .is_none()
        );
        set_test_root(None);
    }

    #[test]
    fn expired_entries_are_purged() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("trash");
        set_test_root(Some(root.clone()));
        let file = temp.path().join("old.jsonl");
        std::fs::write(&file, "x").unwrap();
        move_to_trash("codex", "old", &[&file]).unwrap();
        purge_expired_in(&root, Duration::ZERO);
        std::thread::sleep(Duration::from_millis(5));
        purge_expired_in(&root, Duration::ZERO);
        assert!(list().is_empty());
        set_test_root(None);
    }
}
