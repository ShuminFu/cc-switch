//! Row versioning and tombstones for cloud sync (DB-20, first increment).
//!
//! Sync still ships whole-database snapshots, but every synced entity row now
//! carries `sync_rev` (incremented on every change) and `sync_updated_at`
//! (Unix milliseconds), and deletions are recorded in the device-local
//! `sync_tombstones` table. Both are maintained by the app's own triggers, so
//! every write path (DAO, migrations, services) is covered without touching
//! each call site. A later increment can merge snapshots row by row (higher
//! `sync_updated_at` wins, tombstones delete) without another schema change.
//!
//! Wire compatibility: the columns are part of the existing table dumps
//! (older clients import them as extra columns and ignore them) and the
//! tombstone table is neither exported nor synced, so the sync
//! `DB_COMPAT_VERSION` does not change.

use super::Database;
use crate::error::AppError;
use rusqlite::Connection;

/// Schema version that introduced the row versions and tombstones.
pub(crate) const SYNC_ROWS_SCHEMA_VERSION: i32 = 22;

/// Device-local table of deleted entity rows.
pub(crate) const TOMBSTONE_TABLE: &str = "sync_tombstones";

/// Tombstones older than this are purged by the periodic maintenance.
pub(crate) const TOMBSTONE_RETENTION_DAYS: i64 = 90;

/// Synced entity tables and the SQL expression of their row key (over `r.`).
const VERSIONED_TABLES: &[(&str, &str)] = &[
    ("providers", "r.app_type || '/' || r.id"),
    ("mcp_servers", "r.id"),
    ("prompts", "r.app_type || '/' || r.id"),
    ("skills", "r.id"),
    ("profiles", "r.id"),
];

/// Current Unix time in milliseconds, as an SQL expression.
const NOW_MS_SQL: &str = "CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)";

fn db_err(context: &'static str) -> impl Fn(rusqlite::Error) -> AppError {
    move |e| AppError::Database(format!("{context}: {e}"))
}

pub(crate) fn add_row_version_columns(conn: &Connection) -> Result<(), AppError> {
    for (table, _) in VERSIONED_TABLES {
        if !Database::table_exists(conn, table)? {
            continue;
        }
        let added_rev = !Database::has_column(conn, table, "sync_rev")?;
        if added_rev {
            conn.execute(
                &format!("ALTER TABLE \"{table}\" ADD COLUMN sync_rev INTEGER NOT NULL DEFAULT 0"),
                [],
            )
            .map_err(db_err("添加 sync_rev 列失败"))?;
        }
        if !Database::has_column(conn, table, "sync_updated_at")? {
            conn.execute(
                &format!(
                    "ALTER TABLE \"{table}\" ADD COLUMN sync_updated_at INTEGER NOT NULL DEFAULT 0"
                ),
                [],
            )
            .map_err(db_err("添加 sync_updated_at 列失败"))?;
        }
        // Existing rows start at revision 1, stamped with the upgrade time.
        conn.execute(
            &format!(
                "UPDATE \"{table}\" SET sync_rev = 1, sync_updated_at = {NOW_MS_SQL}
                 WHERE sync_rev = 0"
            ),
            [],
        )
        .map_err(db_err("初始化行版本失败"))?;
    }
    Ok(())
}

pub(crate) fn ensure_tombstone_table(conn: &Connection) -> Result<(), AppError> {
    conn.execute(
        &format!(
            "CREATE TABLE IF NOT EXISTS {TOMBSTONE_TABLE} (
                table_name TEXT NOT NULL,
                row_key TEXT NOT NULL,
                deleted_at INTEGER NOT NULL,
                PRIMARY KEY (table_name, row_key)
            )"
        ),
        [],
    )
    .map_err(db_err("创建 sync_tombstones 表失败"))?;
    Ok(())
}

/// Create the app's row-version triggers (idempotent). Tables that do not
/// have the columns yet are skipped.
pub(crate) fn create_row_version_triggers(conn: &Connection) -> Result<(), AppError> {
    if !Database::table_exists(conn, TOMBSTONE_TABLE)? {
        return Ok(());
    }
    for (table, key) in VERSIONED_TABLES {
        if !Database::table_exists(conn, table)?
            || !Database::has_column(conn, table, "sync_rev")?
            || !Database::has_column(conn, table, "sync_updated_at")?
        {
            continue;
        }
        let new_key = key.replace("r.", "NEW.");
        let old_key = key.replace("r.", "OLD.");
        conn.execute_batch(&format!(
            "CREATE TRIGGER IF NOT EXISTS trg_sync_{table}_insert AFTER INSERT ON \"{table}\"
             BEGIN
                 UPDATE \"{table}\" SET
                     sync_rev = CASE WHEN NEW.sync_rev > 0 THEN NEW.sync_rev ELSE 1 END,
                     sync_updated_at = CASE WHEN NEW.sync_updated_at > 0 THEN NEW.sync_updated_at
                                            ELSE {NOW_MS_SQL} END
                 WHERE rowid = NEW.rowid AND (NEW.sync_rev = 0 OR NEW.sync_updated_at = 0);
                 DELETE FROM {TOMBSTONE_TABLE}
                 WHERE table_name = '{table}' AND row_key = {new_key};
             END;
             CREATE TRIGGER IF NOT EXISTS trg_sync_{table}_update AFTER UPDATE ON \"{table}\"
             WHEN NEW.sync_rev = OLD.sync_rev
             BEGIN
                 UPDATE \"{table}\" SET sync_rev = OLD.sync_rev + 1, sync_updated_at = {NOW_MS_SQL}
                 WHERE rowid = NEW.rowid;
             END;
             CREATE TRIGGER IF NOT EXISTS trg_sync_{table}_delete AFTER DELETE ON \"{table}\"
             BEGIN
                 INSERT INTO {TOMBSTONE_TABLE} (table_name, row_key, deleted_at)
                 VALUES ('{table}', {old_key}, {NOW_MS_SQL})
                 ON CONFLICT(table_name, row_key) DO UPDATE SET deleted_at = excluded.deleted_at;
             END;"
        ))
        .map_err(db_err("创建行版本触发器失败"))?;
    }
    Ok(())
}

/// A recorded deletion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncTombstone {
    pub table_name: String,
    pub row_key: String,
    pub deleted_at: i64,
}

/// Retention windows of the periodic maintenance (DB-25). `0` = keep forever.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsageRetention {
    pub request_log_days: i64,
    pub stream_check_days: i64,
}

impl Default for UsageRetention {
    fn default() -> Self {
        Self {
            request_log_days: 30,
            stream_check_days: 7,
        }
    }
}

/// Settings keys holding the retention windows (device-local).
pub(crate) const USAGE_RETENTION_DAYS_KEY: &str = "usage_retention_days";
pub(crate) const STREAM_CHECK_RETENTION_DAYS_KEY: &str = "stream_check_retention_days";

/// Upper bound accepted for a retention window (about 10 years).
const MAX_RETENTION_DAYS: i64 = 3650;

impl Database {
    /// Deletions recorded since `since_ms` (Unix ms), oldest first.
    #[allow(dead_code)]
    pub fn sync_tombstones_since(&self, since_ms: i64) -> Result<Vec<SyncTombstone>, AppError> {
        let conn = super::lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(&format!(
                "SELECT table_name, row_key, deleted_at FROM {TOMBSTONE_TABLE}
                 WHERE deleted_at >= ?1 ORDER BY deleted_at, table_name, row_key"
            ))
            .map_err(db_err("读取 sync_tombstones 失败"))?;
        let rows = stmt
            .query_map([since_ms], |row| {
                Ok(SyncTombstone {
                    table_name: row.get(0)?,
                    row_key: row.get(1)?,
                    deleted_at: row.get(2)?,
                })
            })
            .map_err(db_err("读取 sync_tombstones 失败"))?;
        rows.collect::<Result<_, _>>()
            .map_err(db_err("读取 sync_tombstones 失败"))
    }

    /// Delete tombstones older than `older_than_days`. Returns the count.
    pub(crate) fn purge_sync_tombstones(&self, older_than_days: i64) -> Result<u64, AppError> {
        let conn = super::lock_conn!(self.conn);
        if !Database::table_exists(&conn, TOMBSTONE_TABLE)? {
            return Ok(0);
        }
        let cutoff = chrono::Utc::now().timestamp_millis() - older_than_days * 86_400_000;
        let deleted = conn
            .execute(
                &format!("DELETE FROM {TOMBSTONE_TABLE} WHERE deleted_at < ?1"),
                [cutoff],
            )
            .map_err(db_err("清理 sync_tombstones 失败"))?;
        Ok(deleted as u64)
    }

    /// Retention windows from settings; missing or invalid values fall back to
    /// the defaults (30 days of request logs, 7 days of stream checks).
    pub fn usage_retention(&self) -> UsageRetention {
        let defaults = UsageRetention::default();
        let read = |key: &str, default: i64| -> i64 {
            match self.get_setting(key) {
                Ok(Some(v)) => v
                    .trim()
                    .parse::<i64>()
                    .ok()
                    .filter(|d| (0..=MAX_RETENTION_DAYS).contains(d))
                    .unwrap_or(default),
                _ => default,
            }
        };
        UsageRetention {
            request_log_days: read(USAGE_RETENTION_DAYS_KEY, defaults.request_log_days),
            stream_check_days: read(STREAM_CHECK_RETENTION_DAYS_KEY, defaults.stream_check_days),
        }
    }

    /// Store the retention windows (`0` = never prune).
    #[allow(dead_code)]
    pub fn set_usage_retention(&self, retention: UsageRetention) -> Result<(), AppError> {
        for (key, days) in [
            (USAGE_RETENTION_DAYS_KEY, retention.request_log_days),
            (STREAM_CHECK_RETENTION_DAYS_KEY, retention.stream_check_days),
        ] {
            if !(0..=MAX_RETENTION_DAYS).contains(&days) {
                return Err(AppError::InvalidInput(format!(
                    "保留天数必须在 0..={MAX_RETENTION_DAYS} 之间: {days}"
                )));
            }
            self.set_setting(key, &days.to_string())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::lock_conn;

    /// `(sync_rev, sync_updated_at)` of one entity row.
    type RowVersion = (i64, i64);

    fn version(db: &Database, sql: &str) -> RowVersion {
        let conn = db.conn.lock().unwrap();
        conn.query_row(sql, [], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
    }

    #[test]
    fn writes_bump_row_versions_and_deletes_leave_tombstones() -> Result<(), AppError> {
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config) VALUES ('p', 'claude', 'P', '{}')",
                [],
            )?;
        }
        let q = "SELECT sync_rev, sync_updated_at FROM providers WHERE id = 'p'";
        let (rev1, at1) = version(&db, q);
        assert_eq!(rev1, 1);
        assert!(at1 > 1_600_000_000_000, "ms timestamp: {at1}");

        {
            let conn = lock_conn!(db.conn);
            conn.execute("UPDATE providers SET name = 'P2' WHERE id = 'p'", [])?;
        }
        let (rev2, at2) = version(&db, q);
        assert_eq!(rev2, 2);
        assert!(at2 >= at1);

        {
            let conn = lock_conn!(db.conn);
            conn.execute("DELETE FROM providers WHERE id = 'p'", [])?;
        }
        let tombstones = db.sync_tombstones_since(0)?;
        assert_eq!(tombstones.len(), 1);
        assert_eq!(tombstones[0].table_name, "providers");
        assert_eq!(tombstones[0].row_key, "claude/p");

        // Re-creating the row clears its tombstone.
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config) VALUES ('p', 'claude', 'P', '{}')",
                [],
            )?;
        }
        assert!(db.sync_tombstones_since(0)?.is_empty());

        assert_eq!(db.purge_sync_tombstones(0)?, 0);
        Ok(())
    }

    /// The wire format is unchanged: the snapshot carries the row versions in
    /// the existing tables and no tombstone table, and an import keeps this
    /// device's tombstones and re-creates the triggers.
    #[test]
    fn sync_snapshot_round_trip_keeps_versions_and_local_tombstones() -> Result<(), AppError> {
        let remote = Database::memory()?;
        {
            let conn = lock_conn!(remote.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, sync_rev, sync_updated_at)
                 VALUES ('r', 'claude', 'R', '{}', 7, 1700000000000)",
                [],
            )?;
        }
        let sql = remote.export_sql_string_for_sync()?;
        assert!(
            !sql.contains(TOMBSTONE_TABLE),
            "tombstones are not exported"
        );

        let local = Database::memory()?;
        {
            let conn = lock_conn!(local.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config) VALUES ('gone', 'claude', 'G', '{}')",
                [],
            )?;
            conn.execute("DELETE FROM providers WHERE id = 'gone'", [])?;
        }
        local.import_sql_string_for_sync(&sql)?;

        assert_eq!(
            version(
                &local,
                "SELECT sync_rev, sync_updated_at FROM providers WHERE id = 'r'"
            ),
            (7, 1_700_000_000_000)
        );
        assert_eq!(local.sync_tombstones_since(0)?.len(), 1);
        {
            let conn = lock_conn!(local.conn);
            conn.execute("UPDATE providers SET name = 'R2' WHERE id = 'r'", [])?;
        }
        assert_eq!(
            version(
                &local,
                "SELECT sync_rev, sync_updated_at FROM providers WHERE id = 'r'"
            )
            .0,
            8,
            "triggers are active after the import"
        );
        Ok(())
    }

    #[test]
    fn retention_settings_default_validate_and_round_trip() -> Result<(), AppError> {
        let db = Database::memory()?;
        assert_eq!(db.usage_retention(), UsageRetention::default());
        db.set_usage_retention(UsageRetention {
            request_log_days: 0,
            stream_check_days: 3,
        })?;
        assert_eq!(
            db.usage_retention(),
            UsageRetention {
                request_log_days: 0,
                stream_check_days: 3
            }
        );
        assert!(db
            .set_usage_retention(UsageRetention {
                request_log_days: -1,
                stream_check_days: 3
            })
            .is_err());
        Ok(())
    }
}
