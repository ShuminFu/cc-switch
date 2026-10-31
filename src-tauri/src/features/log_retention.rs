//! Request-log retention and privacy controls (PRODUCT-31).
//!
//! Per-request rows in `proxy_request_logs` are rolled up into
//! `usage_daily_rollups` and deleted by the periodic maintenance job once they
//! are older than the retention window (`Database::rollup_and_prune`). The
//! window used to be a hard-coded 30 days; it is now read from
//! `~/.cc-switch/request-log-retention.json`. "Delete logs now" rolls every
//! detail row up immediately, so usage totals are unchanged while the
//! per-request metadata (models, status codes, session ids, error messages) is
//! gone. Request and response bodies are never stored.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::AppError;
use crate::store::AppState;

pub const DEFAULT_RETENTION_DAYS: u32 = 30;
pub const MIN_RETENTION_DAYS: u32 = 1;
pub const MAX_RETENTION_DAYS: u32 = 3650;
const FILE_NAME: &str = "request-log-retention.json";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct RetentionFile {
    detail_retention_days: u32,
}

fn config_path() -> PathBuf {
    crate::config::get_app_config_dir().join(FILE_NAME)
}

fn clamp(days: u32) -> u32 {
    days.clamp(MIN_RETENTION_DAYS, MAX_RETENTION_DAYS)
}

/// Detail-row retention in days (default 30; an unreadable file falls back).
pub fn effective_retention_days() -> i64 {
    let days = std::fs::read(config_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice::<RetentionFile>(&bytes).ok())
        .map(|f| clamp(f.detail_retention_days))
        .unwrap_or(DEFAULT_RETENTION_DAYS);
    i64::from(days)
}

fn save_retention_days(days: u32) -> Result<u32, AppError> {
    let days = clamp(days);
    let body = serde_json::to_string_pretty(&RetentionFile {
        detail_retention_days: days,
    })
    .map_err(|source| AppError::JsonSerialize { source })?;
    crate::config::write_text_file(&config_path(), &body)?;
    Ok(days)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestLogRetention {
    pub retention_days: i64,
    pub min_days: u32,
    pub max_days: u32,
    /// Size of the SQLite database file (bytes), when it exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database_bytes: Option<u64>,
}

fn status() -> RequestLogRetention {
    let db = crate::config::get_app_config_dir().join("cc-switch.db");
    RequestLogRetention {
        retention_days: effective_retention_days(),
        min_days: MIN_RETENTION_DAYS,
        max_days: MAX_RETENTION_DAYS,
        database_bytes: std::fs::metadata(db).ok().map(|m| m.len()),
    }
}

#[tauri::command]
pub fn get_request_log_retention() -> RequestLogRetention {
    status()
}

#[tauri::command]
pub fn set_request_log_retention(days: u32) -> Result<RequestLogRetention, String> {
    save_retention_days(days).map_err(|e| e.to_string())?;
    Ok(status())
}

/// Roll up and delete every per-request row now. Returns the rows deleted.
#[tauri::command]
pub fn prune_request_logs_now(state: State<'_, AppState>) -> Result<u64, String> {
    prune_now(&state.db).map_err(|e| e.to_string())
}

fn prune_now(db: &crate::database::Database) -> Result<u64, AppError> {
    // A zero-day window moves the cutoff to the next local midnight, so every
    // existing detail row is aggregated into the daily rollups first.
    db.rollup_and_prune(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    struct HomeGuard {
        old: Option<std::ffi::OsString>,
        _dir: tempfile::TempDir,
    }
    impl HomeGuard {
        fn new() -> Self {
            let dir = tempfile::TempDir::new().unwrap();
            let old = std::env::var_os("CC_SWITCH_TEST_HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            Self { old, _dir: dir }
        }
    }
    impl Drop for HomeGuard {
        fn drop(&mut self) {
            match self.old.take() {
                Some(v) => std::env::set_var("CC_SWITCH_TEST_HOME", v),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    #[test]
    #[serial]
    fn defaults_to_thirty_days_and_persists_clamped_values() {
        let _home = HomeGuard::new();
        assert_eq!(effective_retention_days(), 30);
        assert_eq!(save_retention_days(7).unwrap(), 7);
        assert_eq!(effective_retention_days(), 7);
        assert_eq!(save_retention_days(0).unwrap(), MIN_RETENTION_DAYS);
        assert_eq!(save_retention_days(99_999).unwrap(), MAX_RETENTION_DAYS);
    }

    #[test]
    #[serial]
    fn corrupt_file_falls_back_to_default() {
        let _home = HomeGuard::new();
        let path = config_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "nope").unwrap();
        assert_eq!(effective_retention_days(), 30);
    }

    fn insert_log(
        db: &crate::database::Database,
        id: &str,
        created_at: i64,
    ) -> Result<(), AppError> {
        let conn = crate::database::lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO proxy_request_logs (
                request_id, provider_id, app_type, model,
                input_tokens, output_tokens, total_cost_usd,
                latency_ms, status_code, created_at
            ) VALUES (?1, 'p1', 'claude', 'claude-3', 100, 50, '0.01', 100, 200, ?2)",
            rusqlite::params![id, created_at],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    fn counts(db: &crate::database::Database) -> Result<(i64, i64), AppError> {
        let conn = crate::database::lock_conn!(db.conn);
        let logs = conn
            .query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |r| r.get(0))
            .unwrap();
        let rolled: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(request_count), 0) FROM usage_daily_rollups",
                [],
                |r| r.get(0),
            )
            .unwrap();
        Ok((logs, rolled))
    }

    #[test]
    #[serial]
    fn maintenance_honours_a_seven_day_window_and_keeps_totals() {
        let _home = HomeGuard::new();
        crate::settings::update_settings(crate::settings::AppSettings {
            backup_interval_hours: Some(0),
            ..crate::settings::AppSettings::default()
        })
        .unwrap();
        save_retention_days(7).unwrap();

        let db = crate::database::Database::memory().unwrap();
        let now = chrono::Utc::now().timestamp();
        insert_log(&db, "old", now - 10 * 86400).unwrap();
        insert_log(&db, "recent", now - 2 * 86400).unwrap();

        db.periodic_backup_if_needed().unwrap();
        // The 10-day-old row is rolled up; the 2-day-old row stays in detail.
        assert_eq!(counts(&db).unwrap(), (1, 1));

        // "Delete logs now" removes the rest while keeping the totals.
        assert_eq!(prune_now(&db).unwrap(), 1);
        assert_eq!(counts(&db).unwrap(), (0, 2));
        crate::settings::update_settings(crate::settings::AppSettings::default()).unwrap();
    }

    #[test]
    fn prune_now_on_empty_db_deletes_nothing() {
        let db = crate::database::Database::memory().unwrap();
        assert_eq!(prune_now(&db).unwrap(), 0);
    }
}
