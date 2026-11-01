//! Restore a compatible backup from the "database too new" screen (PRODUCT-72).
//!
//! That screen runs before the database is opened, so `AppState` is not
//! managed and the regular `restore_db_backup` command cannot be used. These
//! commands work on files only:
//!
//! * [`list_recovery_backups`] lists `~/.cc-switch/backups/*.db` with each
//!   file's schema version, so the screen can offer the ones this build can
//!   open (typically the pre-migration backup the newer build took);
//! * [`restore_backup_for_recovery`] keeps the too-new database (and its WAL)
//!   as `backups/db_too_new_v<N>_<ts>.db`, which rotation never deletes,
//!   puts the chosen backup in place atomically and restarts the app. The
//!   chosen backup is renamed to a user-named file so rotation never deletes
//!   it either.
//!
//! Both refuse to run unless startup stopped on `db_version_too_new`, so they
//! can never replace a database the running app has open.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;

use crate::error::AppError;

const DB_FILE: &str = "cc-switch.db";

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryBackup {
    pub filename: String,
    pub size_bytes: u64,
    /// RFC 3339 modification time.
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<i32>,
    /// This build can open the file.
    pub compatible: bool,
}

fn in_recovery_mode() -> bool {
    crate::init_status::get_init_error()
        .is_some_and(|p| p.kind.as_deref() == Some("db_version_too_new"))
}

fn read_schema_version(path: &Path) -> Option<i32> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    let ok: String = conn
        .query_row("PRAGMA quick_check", [], |r| r.get(0))
        .ok()?;
    if ok != "ok" {
        return None;
    }
    conn.query_row("PRAGMA user_version", [], |r| r.get(0)).ok()
}

fn is_compatible(version: Option<i32>, supported: i32) -> bool {
    version.is_some_and(|v| v >= 1 && v <= supported)
}

fn list_in(config_dir: &Path, supported: i32) -> Vec<RecoveryBackup> {
    let dir = config_dir.join("backups");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<RecoveryBackup> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "db"))
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            let created_at = meta
                .modified()
                .ok()
                .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339())
                .unwrap_or_default();
            let schema_version = read_schema_version(&e.path());
            Some(RecoveryBackup {
                filename: e.file_name().to_string_lossy().to_string(),
                size_bytes: meta.len(),
                created_at,
                schema_version,
                compatible: is_compatible(schema_version, supported),
            })
        })
        .collect();
    out.sort_by(|a, b| {
        b.compatible
            .cmp(&a.compatible)
            .then_with(|| b.created_at.cmp(&a.created_at))
    });
    out
}

fn validate_name(filename: &str) -> Result<(), AppError> {
    let plain = !filename.is_empty()
        && !filename.contains(['/', '\\'])
        && !filename.contains("..")
        && filename.ends_with(".db");
    if plain {
        Ok(())
    } else {
        Err(AppError::Message(format!(
            "Invalid backup name: {filename}"
        )))
    }
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

/// Swap the chosen backup in. Returns the name the too-new database was
/// saved under.
fn restore_in(config_dir: &Path, filename: &str, supported: i32) -> Result<String, AppError> {
    validate_name(filename)?;
    let backups = config_dir.join("backups");
    let source = backups.join(filename);
    if !source.is_file() {
        return Err(AppError::Message(format!("Backup not found: {filename}")));
    }
    let version = read_schema_version(&source);
    if !is_compatible(version, supported) {
        return Err(AppError::Message(format!(
            "{filename} cannot be opened by this version (schema {version:?}, supported {supported})"
        )));
    }

    let db_path = config_dir.join(DB_FILE);
    let current_version = read_schema_version(&db_path).unwrap_or_default();
    let saved_name = format!(
        "db_too_new_v{current_version}_{}.db",
        chrono::Local::now().format("%Y%m%d_%H%M%S")
    );
    let saved = backups.join(&saved_name);
    if db_path.exists() {
        std::fs::copy(&db_path, &saved).map_err(|e| AppError::io(&saved, e))?;
        for suffix in ["-wal", "-shm"] {
            let side = sidecar(&db_path, suffix);
            if side.exists() {
                let target = sidecar(&saved, suffix);
                std::fs::copy(&side, &target).map_err(|e| AppError::io(&target, e))?;
            }
        }
    }

    // Stage the backup next to the database, then replace it atomically.
    let staged = config_dir.join(format!("{DB_FILE}.recovery-tmp"));
    std::fs::copy(&source, &staged).map_err(|e| AppError::io(&staged, e))?;
    // The newer database's WAL must not be replayed onto the restored file.
    for suffix in ["-wal", "-shm"] {
        let side = sidecar(&db_path, suffix);
        if side.exists() {
            std::fs::remove_file(&side).map_err(|e| AppError::io(&side, e))?;
        }
    }
    if let Err(e) = std::fs::rename(&staged, &db_path) {
        let _ = std::fs::remove_file(&staged);
        return Err(AppError::io(&db_path, e));
    }

    // Keep the backup that is now live out of rotation.
    let stem = filename.trim_end_matches(".db");
    let pinned = backups.join(format!("{stem}-restored.db"));
    if !pinned.exists() {
        if let Err(e) = std::fs::rename(&source, &pinned) {
            log::warn!("[Recovery] could not pin {filename}: {e}");
        }
    }
    super::audit_log::record(
        super::audit_log::AuditEvent::new("ui", "database.recoveryRestore", "database", DB_FILE)
            .detail(format!(
                "restored {filename}; kept the newer database as {saved_name}"
            )),
    );
    Ok(saved_name)
}

#[tauri::command]
pub fn list_recovery_backups() -> Result<Vec<RecoveryBackup>, String> {
    if !in_recovery_mode() {
        return Err("Not in database recovery mode".to_string());
    }
    Ok(list_in(
        &crate::config::get_app_config_dir(),
        crate::database::SCHEMA_VERSION,
    ))
}

/// Restore `filename` and restart. Returns the name the newer database was
/// saved under (the restart follows shortly after).
#[tauri::command]
pub fn restore_backup_for_recovery<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    filename: String,
) -> Result<String, String> {
    if !in_recovery_mode() {
        return Err("Not in database recovery mode".to_string());
    }
    let saved = restore_in(
        &crate::config::get_app_config_dir(),
        &filename,
        crate::database::SCHEMA_VERSION,
    )
    .map_err(|e| e.to_string())?;
    log::info!("[Recovery] restored a compatible backup; restarting");
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(400));
        app.restart();
    });
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_db(path: &Path, version: i32, marker: &str) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(&format!(
            "CREATE TABLE t (v TEXT); INSERT INTO t VALUES ('{marker}'); PRAGMA user_version = {version};"
        ))
        .unwrap();
    }

    fn marker(path: &Path) -> String {
        Connection::open(path)
            .unwrap()
            .query_row("SELECT v FROM t", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn lists_compatible_backups_first_and_restores_one() {
        let _home = crate::test_support::isolated_home();
        let dir = tempfile::tempdir().unwrap();
        let backups = dir.path().join("backups");
        std::fs::create_dir_all(&backups).unwrap();
        make_db(&dir.path().join(DB_FILE), 23, "newer");
        std::fs::write(sidecar(&dir.path().join(DB_FILE), "-wal"), b"").unwrap();
        make_db(
            &backups.join("db_premigration_20260101_000000.db"),
            22,
            "older",
        );
        make_db(
            &backups.join("db_backup_20260102_000000.db"),
            23,
            "too-new-backup",
        );
        std::fs::write(backups.join("broken.db"), b"not sqlite").unwrap();

        let listed = list_in(dir.path(), 22);
        assert_eq!(listed[0].filename, "db_premigration_20260101_000000.db");
        assert!(listed[0].compatible);
        assert_eq!(listed[0].schema_version, Some(22));
        assert_eq!(listed.iter().filter(|b| b.compatible).count(), 1);

        assert!(restore_in(dir.path(), "db_backup_20260102_000000.db", 22).is_err());
        assert!(restore_in(dir.path(), "broken.db", 22).is_err());
        assert!(restore_in(dir.path(), "../cc-switch.db", 22).is_err());

        let saved = restore_in(dir.path(), "db_premigration_20260101_000000.db", 22).unwrap();
        assert_eq!(marker(&dir.path().join(DB_FILE)), "older");
        assert_eq!(marker(&backups.join(&saved)), "newer");
        assert!(saved.starts_with("db_too_new_v23_"));
        assert!(!sidecar(&dir.path().join(DB_FILE), "-wal").exists());
        // The restored backup is renamed so rotation never removes it, and
        // the saved copy is not a rotated kind either.
        assert!(backups
            .join("db_premigration_20260101_000000-restored.db")
            .exists());
        assert!(crate::database::backup::BackupKind::from_filename(&saved).is_none());
        assert!(crate::database::backup::BackupKind::from_filename(
            "db_premigration_20260101_000000-restored.db"
        )
        .is_none());
    }
}
