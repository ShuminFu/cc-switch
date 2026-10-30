//! 数据库备份和恢复
//!
//! 提供 SQL 导出/导入和二进制快照备份功能。

use super::{lock_conn, Database, SCHEMA_VERSION};
use crate::config::get_app_config_dir;
use crate::error::AppError;
use chrono::{Local, Utc};
use rusqlite::backup::Backup;
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tempfile::NamedTempFile;

const CC_SWITCH_SQL_EXPORT_HEADER: &str = "-- CC Switch SQLite 导出";

/// Tables whose rows are synced between devices (WebDAV/S3) as-is.
///
/// Every table the app creates must be listed in exactly one of
/// [`SYNC_SYNCED_TABLES`] or [`SYNC_SKIP_TABLES`]; a unit test enforces this so
/// that new tables are classified deliberately.
///
/// `settings` and `proxy_config` are synced, but their device-local keys and
/// columns are kept from the importing device (see [`DEVICE_SETTINGS_KEYS`],
/// [`DEVICE_SETTINGS_KEY_PREFIXES`] and [`PROXY_CONFIG_DEVICE_COLUMNS`]).
pub(crate) const SYNC_SYNCED_TABLES: &[&str] = &[
    "providers",
    "provider_endpoints",
    "mcp_servers",
    "prompts",
    "skills",
    "skill_repos",
    "settings",
    "proxy_config",
    "model_pricing",
    "profiles",
];

/// Tables whose data rows are skipped when exporting for WebDAV sync.
pub(crate) const SYNC_SKIP_TABLES: &[&str] = &[
    "proxy_request_logs",
    "stream_check_logs",
    "provider_health",
    "proxy_live_backup",
    "usage_daily_rollups",
    // Absolute local file paths and byte offsets of this device's session logs.
    "session_log_sync",
];

/// Tables whose local data is preserved (restored from local snapshot) during WebDAV import.
/// Excludes ephemeral tables like provider_health that can safely rebuild at runtime.
pub(crate) const SYNC_PRESERVE_TABLES: &[&str] = &[
    "proxy_request_logs",
    "stream_check_logs",
    "proxy_live_backup",
    "usage_daily_rollups",
    "session_log_sync",
];

/// Tables whose current rows are kept when restoring a local `.db` backup:
/// the original live-config backups must stay consistent with the takeover
/// state of the live files on disk, which a restore does not touch.
const RESTORE_PRESERVE_TABLES: &[&str] = &["proxy_live_backup"];

/// Legacy tables that may appear in SQL exports from older schema versions and
/// are migrated away by `apply_schema_migrations`.
const LEGACY_IMPORT_TABLES: &[&str] = &["failover_queue", "circuit_breaker_config"];

/// `proxy_config` columns that describe this device's proxy/takeover state.
/// They are never taken from a sync download or a restored backup.
const PROXY_CONFIG_DEVICE_COLUMNS: &[&str] = &[
    "enabled",
    "proxy_enabled",
    "listen_address",
    "listen_port",
    "live_takeover_active",
];

/// `proxy_config` columns switched off on imported rows that have no local
/// counterpart, so a download never turns takeover on by itself.
const PROXY_CONFIG_TAKEOVER_FLAGS: &[&str] = &["enabled", "proxy_enabled", "live_takeover_active"];

/// Device-local `settings` keys (exact match).
const DEVICE_SETTINGS_KEYS: &[&str] = &[
    "global_proxy_url",
    "skills_ssot_migration_pending",
    "skills_ssot_migration_snapshot",
];

/// Device-local `settings` key prefixes.
const DEVICE_SETTINGS_KEY_PREFIXES: &[&str] = &["current_profile_id_"];

/// Built-in SQL functions an imported script may reference (expression
/// indexes and column defaults in the app schema).
const IMPORT_ALLOWED_FUNCTIONS: &[&str] = &[
    "coalesce",
    "ifnull",
    "datetime",
    "date",
    "time",
    "strftime",
    "julianday",
    "unixepoch",
    "lower",
    "upper",
    "length",
    "abs",
];

/// PRAGMAs emitted by [`Database::dump_sql`].
const IMPORT_ALLOWED_PRAGMAS: &[&str] = &["foreign_keys", "user_version"];

/// Origin of a database backup file. The kind is encoded in the file name so
/// that rotation can treat each kind separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackupKind {
    /// Periodic automatic backup (`db_backup_<ts>.db`, the historical name).
    Auto,
    /// Created by the user from the backup manager. Never rotated.
    Manual,
    /// Taken right before an import, sync download or restore replaces the DB.
    Safety,
    /// Taken before a schema migration or a database rebuild.
    PreMigration,
}

impl BackupKind {
    const ALL: [BackupKind; 4] = [
        BackupKind::Auto,
        BackupKind::Manual,
        BackupKind::Safety,
        BackupKind::PreMigration,
    ];

    fn prefix(self) -> &'static str {
        match self {
            BackupKind::Auto => "db_backup_",
            BackupKind::Manual => "db_manual_",
            BackupKind::Safety => "db_safety_",
            BackupKind::PreMigration => "db_premigration_",
        }
    }

    /// Manual backups are kept until the user deletes them; the other kinds
    /// are rotated, each within its own pool.
    fn is_rotated(self) -> bool {
        !matches!(self, BackupKind::Manual)
    }

    /// Classify a backup file name. Returns `None` for user-named (renamed)
    /// backups, which are never rotated.
    pub(crate) fn from_filename(filename: &str) -> Option<Self> {
        let stem = filename.strip_suffix(".db")?;
        Self::ALL.into_iter().find(|kind| {
            stem.strip_prefix(kind.prefix())
                .is_some_and(is_backup_timestamp_suffix)
        })
    }
}

/// Matches `YYYYMMDD_HHMMSS`, optionally followed by `_<n>`.
fn is_backup_timestamp_suffix(rest: &str) -> bool {
    let bytes = rest.as_bytes();
    if bytes.len() < 15 {
        return false;
    }
    let digits = |range: std::ops::Range<usize>| bytes[range].iter().all(u8::is_ascii_digit);
    if !digits(0..8) || bytes[8] != b'_' || !digits(9..15) {
        return false;
    }
    match &rest[15..] {
        "" => true,
        tail => tail
            .strip_prefix('_')
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())),
    }
}

/// SQL predicate (on column `key`) selecting device-local settings keys.
fn device_settings_predicate() -> String {
    let keys = DEVICE_SETTINGS_KEYS
        .iter()
        .map(|k| format!("'{k}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut clauses = vec![format!("key IN ({keys})")];
    for prefix in DEVICE_SETTINGS_KEY_PREFIXES {
        let escaped = prefix.replace('_', "\\_");
        clauses.push(format!("key LIKE '{escaped}%' ESCAPE '\\'"));
    }
    clauses.join(" OR ")
}

/// Every table name an imported SQL script may create or insert into.
fn is_known_import_table(name: &str) -> bool {
    SYNC_SYNCED_TABLES
        .iter()
        .chain(SYNC_SKIP_TABLES)
        .chain(LEGACY_IMPORT_TABLES)
        .any(|t| t.eq_ignore_ascii_case(name))
}

/// SQLite's own bookkeeping tables, which it reads and writes while executing
/// CREATE TABLE/INDEX and AUTOINCREMENT inserts. Users cannot create objects
/// with these names, and direct writes to the schema table are refused unless
/// `writable_schema` is on (that PRAGMA is denied).
fn is_sqlite_internal_table(name: &str) -> bool {
    ["sqlite_master", "sqlite_schema", "sqlite_sequence"]
        .iter()
        .any(|t| t.eq_ignore_ascii_case(name))
}

/// Authorizer for imported SQL scripts: only the statement shapes
/// [`Database::dump_sql`] emits are allowed (CREATE TABLE/INDEX and INSERT on
/// app tables, transactions and two PRAGMAs). ATTACH/DETACH, triggers, views,
/// virtual tables, DROP, ALTER, UPDATE/DELETE on app tables, savepoints, other
/// PRAGMAs and non-allow-listed functions are denied. The first denial is
/// recorded in `denied`.
fn import_authorizer(ctx: AuthContext<'_>, denied: &Mutex<Option<String>>) -> Authorization {
    let in_main = matches!(ctx.database_name, None | Some("main"));
    let allowed = match ctx.action {
        AuthAction::CreateTable { table_name } => {
            in_main && (is_known_import_table(table_name) || is_sqlite_internal_table(table_name))
        }
        AuthAction::CreateIndex { table_name, .. } => in_main && is_known_import_table(table_name),
        AuthAction::Insert { table_name } | AuthAction::Read { table_name, .. } => {
            in_main && (is_known_import_table(table_name) || is_sqlite_internal_table(table_name))
        }
        AuthAction::Update { table_name, .. } | AuthAction::Delete { table_name } => {
            in_main && is_sqlite_internal_table(table_name)
        }
        // Reads are confined to app tables in `main` (ATTACH is denied) and
        // functions still go through the allow-list.
        AuthAction::Select => true,
        // CREATE INDEX populates the new index through an internal REINDEX.
        AuthAction::Reindex { .. } => in_main,
        AuthAction::Transaction { .. } => true,
        AuthAction::Pragma {
            pragma_name,
            pragma_value,
        } => {
            pragma_value.is_some()
                && IMPORT_ALLOWED_PRAGMAS
                    .iter()
                    .any(|p| p.eq_ignore_ascii_case(pragma_name))
        }
        AuthAction::Function { function_name } => IMPORT_ALLOWED_FUNCTIONS
            .iter()
            .any(|f| f.eq_ignore_ascii_case(function_name)),
        _ => false,
    };

    if allowed {
        Authorization::Allow
    } else {
        if let Ok(mut slot) = denied.lock() {
            if slot.is_none() {
                *slot = Some(format!("{:?}", ctx.action));
            }
        }
        Authorization::Deny
    }
}

/// A database backup entry for the UI
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntry {
    pub filename: String,
    pub size_bytes: u64,
    pub created_at: String, // ISO 8601
}

impl Database {
    /// 导出为 SQLite 兼容的 SQL 文本（内存字符串，完整导出）
    pub fn export_sql_string(&self) -> Result<String, AppError> {
        let snapshot = self.snapshot_to_memory()?;
        Self::dump_sql(&snapshot, &[])
    }

    /// Export SQL for sync (WebDAV), skipping local-only tables' data
    pub fn export_sql_string_for_sync(&self) -> Result<String, AppError> {
        let snapshot = self.snapshot_to_memory()?;
        // Device-local settings (outbound proxy URL, profile pointers, one-shot
        // migration state) never leave this device.
        if Self::table_exists(&snapshot, "settings")? {
            snapshot
                .execute(
                    &format!("DELETE FROM settings WHERE {}", device_settings_predicate()),
                    [],
                )
                .map_err(|e| AppError::Database(format!("清理设备本地设置失败: {e}")))?;
        }
        Self::dump_sql(&snapshot, SYNC_SKIP_TABLES)
    }

    /// 导出为 SQLite 兼容的 SQL 文本
    pub fn export_sql(&self, target_path: &Path) -> Result<(), AppError> {
        let dump = self.export_sql_string()?;

        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        }

        crate::config::atomic_write(target_path, dump.as_bytes())
    }

    /// 从 SQL 文件导入，返回生成的备份 ID（若无备份则为空字符串）
    pub fn import_sql(&self, source_path: &Path) -> Result<String, AppError> {
        if !source_path.exists() {
            return Err(AppError::InvalidInput(format!(
                "SQL 文件不存在: {}",
                source_path.display()
            )));
        }

        let sql_raw = fs::read_to_string(source_path).map_err(|e| AppError::io(source_path, e))?;
        let sql_content = sql_raw.trim_start_matches('\u{feff}');
        self.import_sql_string(sql_content)
    }

    /// 从 SQL 字符串导入，返回生成的备份 ID（若无备份则为空字符串）
    pub fn import_sql_string(&self, sql_raw: &str) -> Result<String, AppError> {
        self.import_sql_string_inner(sql_raw, &[])
    }

    /// Import SQL generated for sync, then restore local-only tables from the
    /// current device snapshot before replacing the main database.
    pub(crate) fn import_sql_string_for_sync(&self, sql_raw: &str) -> Result<String, AppError> {
        self.import_sql_string_inner(sql_raw, SYNC_PRESERVE_TABLES)
    }

    fn import_sql_string_inner(
        &self,
        sql_raw: &str,
        preserve_tables: &[&str],
    ) -> Result<String, AppError> {
        let sql_content = sql_raw.trim_start_matches('\u{feff}');
        Self::validate_cc_switch_sql_export(sql_content)?;

        // 在临时数据库执行导入，确保失败不会污染主库
        let temp_file = NamedTempFile::new().map_err(|e| AppError::IoContext {
            context: "创建临时数据库文件失败".to_string(),
            source: e,
        })?;
        let temp_path = temp_file.path().to_path_buf();
        let temp_conn =
            Connection::open(&temp_path).map_err(|e| AppError::Database(e.to_string()))?;

        Self::execute_import_script(&temp_conn, sql_content)?;

        // Drop anything the dump may have smuggled in (triggers/views) BEFORE
        // re-creating the schema, so the app's own triggers (e.g. the
        // dedup_hidden maintenance triggers) are recreated and kept.
        Self::drop_untrusted_schema_objects(&temp_conn)?;

        // 补齐缺失表/索引并进行基础校验
        Self::create_tables_on_conn(&temp_conn)?;
        Self::apply_schema_migrations_on_conn(&temp_conn)?;
        Self::validate_basic_state(&temp_conn)?;

        // 导入前备份现有数据库（校验通过后再备份，失败的导入不会占用备份轮换名额）
        let backup_path = self.backup_database_file_of_kind(BackupKind::Safety, None)?;

        if !preserve_tables.is_empty() {
            let local_snapshot = self.snapshot_to_memory()?;
            Self::restore_tables(&local_snapshot, &temp_conn, preserve_tables)?;
            Self::restore_device_local_state(&local_snapshot, &temp_conn)?;
        }

        // 使用 Backup 将临时库原子写回主库
        {
            let mut main_conn = lock_conn!(self.conn);
            let backup = Backup::new(&temp_conn, &mut main_conn)
                .map_err(|e| AppError::Database(e.to_string()))?;
            backup
                .step(-1)
                .map_err(|e| AppError::Database(e.to_string()))?;
        }

        let backup_id = backup_path
            .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
            .unwrap_or_default();

        Ok(backup_id)
    }

    /// 创建内存快照以避免长时间持有数据库锁
    pub(crate) fn snapshot_to_memory(&self) -> Result<Connection, AppError> {
        let conn = lock_conn!(self.conn);
        let mut snapshot =
            Connection::open_in_memory().map_err(|e| AppError::Database(e.to_string()))?;

        {
            let backup =
                Backup::new(&conn, &mut snapshot).map_err(|e| AppError::Database(e.to_string()))?;
            backup
                .step(-1)
                .map_err(|e| AppError::Database(e.to_string()))?;
        }

        Ok(snapshot)
    }

    fn validate_cc_switch_sql_export(sql: &str) -> Result<(), AppError> {
        let trimmed = sql.trim_start();
        if trimmed.starts_with(CC_SWITCH_SQL_EXPORT_HEADER) {
            return Ok(());
        }

        Err(AppError::localized(
            "backup.sql.invalid_format",
            "仅支持导入由 CC Switch 导出的 SQL 备份文件。",
            "Only SQL backups exported by CC Switch are supported.",
        ))
    }

    fn restore_tables(
        source_conn: &Connection,
        target_conn: &Connection,
        tables: &[&str],
    ) -> Result<(), AppError> {
        for table in tables {
            if !Self::table_exists(source_conn, table)? || !Self::table_exists(target_conn, table)?
            {
                continue;
            }

            let columns = Self::get_table_columns(source_conn, table)?;
            if columns.is_empty() {
                continue;
            }

            target_conn
                .execute(&format!("DELETE FROM \"{table}\""), [])
                .map_err(|e| AppError::Database(format!("清空表 {table} 失败: {e}")))?;

            let placeholders = (1..=columns.len())
                .map(|idx| format!("?{idx}"))
                .collect::<Vec<_>>()
                .join(", ");
            let cols = columns
                .iter()
                .map(|column| format!("\"{column}\""))
                .collect::<Vec<_>>()
                .join(", ");
            let insert_sql = format!("INSERT INTO \"{table}\" ({cols}) VALUES ({placeholders})");

            let mut stmt = source_conn
                .prepare(&format!("SELECT * FROM \"{table}\""))
                .map_err(|e| AppError::Database(format!("读取表 {table} 失败: {e}")))?;
            let mut rows = stmt
                .query([])
                .map_err(|e| AppError::Database(format!("查询表 {table} 数据失败: {e}")))?;

            while let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
                let mut values = Vec::with_capacity(columns.len());
                for idx in 0..columns.len() {
                    values.push(
                        row.get::<_, rusqlite::types::Value>(idx)
                            .map_err(|e| AppError::Database(e.to_string()))?,
                    );
                }

                target_conn
                    .execute(&insert_sql, rusqlite::params_from_iter(values.iter()))
                    .map_err(|e| AppError::Database(format!("恢复表 {table} 数据失败: {e}")))?;
            }
        }

        Ok(())
    }

    /// Run an imported SQL script under [`import_authorizer`], so that only the
    /// statement shapes CC Switch exports can execute.
    fn execute_import_script(conn: &Connection, sql: &str) -> Result<(), AppError> {
        let denied = Arc::new(Mutex::new(None::<String>));
        let denied_hook = Arc::clone(&denied);
        conn.authorizer(Some(move |ctx: AuthContext<'_>| {
            import_authorizer(ctx, &denied_hook)
        }));
        let result = conn.execute_batch(sql);
        conn.authorizer(None::<fn(AuthContext<'_>) -> Authorization>);

        match result {
            Ok(()) => Ok(()),
            Err(err) => {
                let denied = denied.lock().ok().and_then(|slot| slot.clone());
                match denied {
                    Some(action) => Err(AppError::localized(
                        "backup.sql.forbidden_statement",
                        format!("SQL 备份包含不允许的语句，已拒绝导入: {action}"),
                        format!("The SQL backup contains a statement that is not allowed and was rejected: {action}"),
                    )),
                    None => Err(AppError::Database(format!("执行 SQL 导入失败: {err}"))),
                }
            }
        }
    }

    /// Drop every trigger and view a backup or dump carries so they cannot
    /// persist into the main database. Callers run this BEFORE
    /// `create_tables_on_conn`, which recreates the app's own triggers
    /// (the `dedup_hidden` maintenance triggers).
    fn drop_untrusted_schema_objects(conn: &Connection) -> Result<(), AppError> {
        let objects: Vec<(String, String)> = {
            let mut stmt = conn
                .prepare(
                    "SELECT type, name FROM sqlite_master
                     WHERE type IN ('trigger', 'view') AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\'",
                )
                .map_err(|e| AppError::Database(e.to_string()))?;
            let rows = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .map_err(|e| AppError::Database(e.to_string()))?;
            rows.collect::<Result<_, _>>()
                .map_err(|e| AppError::Database(e.to_string()))?
        };
        for (kind, name) in objects {
            let keyword = if kind == "view" { "VIEW" } else { "TRIGGER" };
            let quoted = name.replace('"', "\"\"");
            log::warn!("Dropping unexpected {kind} '{name}' from imported database");
            conn.execute(&format!("DROP {keyword} IF EXISTS \"{quoted}\""), [])
                .map_err(|e| AppError::Database(format!("删除 {kind} {name} 失败: {e}")))?;
        }
        Ok(())
    }

    /// Keep this device's proxy/takeover columns and device-local settings keys
    /// when the rest of the database is replaced (sync download or restore).
    fn restore_device_local_state(local: &Connection, target: &Connection) -> Result<(), AppError> {
        if Self::table_exists(local, "proxy_config")?
            && Self::table_exists(target, "proxy_config")?
            && Self::has_column(local, "proxy_config", "app_type")?
            && Self::has_column(target, "proxy_config", "app_type")?
        {
            let mut columns = Vec::new();
            for column in PROXY_CONFIG_DEVICE_COLUMNS {
                if Self::has_column(local, "proxy_config", column)?
                    && Self::has_column(target, "proxy_config", column)?
                {
                    columns.push(*column);
                }
            }

            // Rows without a local counterpart never arrive switched on.
            let resets = PROXY_CONFIG_TAKEOVER_FLAGS
                .iter()
                .filter(|c| columns.contains(c))
                .map(|c| format!("\"{c}\" = 0"))
                .collect::<Vec<_>>();
            if !resets.is_empty() {
                target
                    .execute(
                        &format!("UPDATE proxy_config SET {}", resets.join(", ")),
                        [],
                    )
                    .map_err(|e| AppError::Database(format!("重置代理接管状态失败: {e}")))?;
            }

            if !columns.is_empty() {
                let select_cols = columns
                    .iter()
                    .map(|c| format!("\"{c}\""))
                    .collect::<Vec<_>>()
                    .join(", ");
                let assignments = columns
                    .iter()
                    .enumerate()
                    .map(|(idx, c)| format!("\"{c}\" = ?{}", idx + 2))
                    .collect::<Vec<_>>()
                    .join(", ");
                let update_sql =
                    format!("UPDATE proxy_config SET {assignments} WHERE app_type = ?1");

                let mut stmt = local
                    .prepare(&format!("SELECT app_type, {select_cols} FROM proxy_config"))
                    .map_err(|e| AppError::Database(e.to_string()))?;
                let mut rows = stmt
                    .query([])
                    .map_err(|e| AppError::Database(e.to_string()))?;
                while let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
                    let mut values = Vec::with_capacity(columns.len() + 1);
                    for idx in 0..=columns.len() {
                        values.push(
                            row.get::<_, rusqlite::types::Value>(idx)
                                .map_err(|e| AppError::Database(e.to_string()))?,
                        );
                    }
                    target
                        .execute(&update_sql, rusqlite::params_from_iter(values.iter()))
                        .map_err(|e| AppError::Database(format!("恢复本机代理配置失败: {e}")))?;
                }
            }
        }

        if Self::table_exists(local, "settings")? && Self::table_exists(target, "settings")? {
            let predicate = device_settings_predicate();
            target
                .execute(&format!("DELETE FROM settings WHERE {predicate}"), [])
                .map_err(|e| AppError::Database(format!("清理设备本地设置失败: {e}")))?;

            let mut stmt = local
                .prepare(&format!(
                    "SELECT key, value FROM settings WHERE {predicate}"
                ))
                .map_err(|e| AppError::Database(e.to_string()))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
                })
                .map_err(|e| AppError::Database(e.to_string()))?;
            for row in rows {
                let (key, value) = row.map_err(|e| AppError::Database(e.to_string()))?;
                target
                    .execute(
                        "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
                        rusqlite::params![key, value],
                    )
                    .map_err(|e| AppError::Database(format!("恢复设备本地设置失败: {e}")))?;
            }
        }

        Ok(())
    }

    /// Periodic backup: create a new backup if the latest one is older than the configured interval
    pub(crate) fn periodic_backup_if_needed(&self) -> Result<(), AppError> {
        let interval_hours = crate::settings::effective_backup_interval_hours();
        if interval_hours > 0 {
            let backup_dir = get_app_config_dir().join("backups");
            if !backup_dir.exists() {
                self.backup_database_file()?;
            } else {
                let latest = fs::read_dir(&backup_dir).ok().and_then(|entries| {
                    entries
                        .filter_map(|e| e.ok())
                        .filter(|e| e.path().extension().map(|ext| ext == "db").unwrap_or(false))
                        .filter_map(|e| e.metadata().ok().and_then(|m| m.modified().ok()))
                        .max()
                });

                let interval_secs = u64::from(interval_hours) * 3600;
                let needs_backup = match latest {
                    None => true,
                    Some(last_modified) => {
                        last_modified.elapsed().unwrap_or_default()
                            > std::time::Duration::from_secs(interval_secs)
                    }
                };

                if needs_backup {
                    log::info!(
                        "Periodic backup: latest backup is older than {interval_hours} hours, creating new backup"
                    );
                    self.backup_database_file()?;
                }
            }
        }

        // Periodic maintenance is always enabled, regardless of auto-backup settings.
        let mut reclaimed_rows = 0u64;
        match self.cleanup_old_stream_check_logs(7) {
            Ok(deleted) => {
                reclaimed_rows += deleted;
            }
            Err(e) => {
                log::warn!("Periodic stream_check_logs cleanup failed: {e}");
            }
        }
        match self.rollup_and_prune(30) {
            Ok(deleted) => {
                reclaimed_rows += deleted;
            }
            Err(e) => {
                log::warn!("Periodic rollup_and_prune failed: {e}");
            }
        }
        if reclaimed_rows > 0 {
            let conn = lock_conn!(self.conn);
            if let Err(e) = conn.execute_batch("PRAGMA incremental_vacuum;") {
                log::warn!("Periodic incremental vacuum failed: {e}");
            }
        }

        Ok(())
    }

    /// 生成一致性快照备份（自动备份），返回备份文件路径（不存在主库时返回 None）
    pub(crate) fn backup_database_file(&self) -> Result<Option<PathBuf>, AppError> {
        self.backup_database_file_of_kind(BackupKind::Auto, None)
    }

    /// 生成指定类型的快照备份。`keep` 指向的文件不会被本次轮换清理
    /// （例如正在恢复的备份）。
    pub(crate) fn backup_database_file_of_kind(
        &self,
        kind: BackupKind,
        keep: Option<&Path>,
    ) -> Result<Option<PathBuf>, AppError> {
        self.backup_database_file_in(
            &get_app_config_dir(),
            crate::settings::effective_backup_retain_count(),
            kind,
            keep,
        )
    }

    /// [`Self::backup_database_file_of_kind`] with an explicit config dir and
    /// retain count.
    pub(crate) fn backup_database_file_in(
        &self,
        config_dir: &Path,
        retain: usize,
        kind: BackupKind,
        keep: Option<&Path>,
    ) -> Result<Option<PathBuf>, AppError> {
        let db_path = config_dir.join("cc-switch.db");
        if !db_path.exists() {
            return Ok(None);
        }

        let backup_dir = db_path
            .parent()
            .ok_or_else(|| AppError::Config("无效的数据库路径".to_string()))?
            .join("backups");

        fs::create_dir_all(&backup_dir).map_err(|e| AppError::io(&backup_dir, e))?;

        let base_id = format!("{}{}", kind.prefix(), Local::now().format("%Y%m%d_%H%M%S"));
        let mut backup_id = base_id.clone();
        let mut backup_path = backup_dir.join(format!("{backup_id}.db"));
        let mut counter = 1;
        while backup_path.exists() {
            backup_id = format!("{base_id}_{counter}");
            backup_path = backup_dir.join(format!("{backup_id}.db"));
            counter += 1;
        }

        {
            let conn = lock_conn!(self.conn);
            let mut dest_conn =
                Connection::open(&backup_path).map_err(|e| AppError::Database(e.to_string()))?;
            let backup = Backup::new(&conn, &mut dest_conn)
                .map_err(|e| AppError::Database(e.to_string()))?;
            backup
                .step(-1)
                .map_err(|e| AppError::Database(e.to_string()))?;
        }

        let mut keep_paths = vec![backup_path.clone()];
        keep_paths.extend(keep.map(Path::to_path_buf));
        Self::cleanup_db_backups(&backup_dir, retain.max(1), kind, &keep_paths)?;
        Ok(Some(backup_path))
    }

    /// 清理旧的数据库备份：仅在同一类型的备份中保留最新的 N 个。
    ///
    /// 手动备份和用户重命名过的备份永远不会被自动清理；`keep` 中的文件也不会被删除。
    fn cleanup_db_backups(
        dir: &Path,
        retain: usize,
        kind: BackupKind,
        keep: &[PathBuf],
    ) -> Result<(), AppError> {
        if !kind.is_rotated() {
            return Ok(());
        }
        let entries = match fs::read_dir(dir) {
            Ok(iter) => iter
                .filter_map(|entry| entry.ok())
                .filter(|entry| {
                    BackupKind::from_filename(&entry.file_name().to_string_lossy()) == Some(kind)
                })
                .collect::<Vec<_>>(),
            Err(_) => return Ok(()),
        };

        if entries.len() <= retain {
            return Ok(());
        }

        let remove_count = entries.len().saturating_sub(retain);
        let mut sorted = entries;
        sorted.sort_by_key(|entry| {
            (
                entry.metadata().and_then(|m| m.modified()).ok(),
                entry.file_name(),
            )
        });

        for entry in sorted
            .into_iter()
            .filter(|entry| !keep.iter().any(|k| k == &entry.path()))
            .take(remove_count)
        {
            if let Err(err) = fs::remove_file(entry.path()) {
                log::warn!("删除旧数据库备份失败 {}: {}", entry.path().display(), err);
            }
        }
        Ok(())
    }

    /// 基础状态校验
    fn validate_basic_state(conn: &Connection) -> Result<(), AppError> {
        let provider_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM providers", [], |row| row.get(0))
            .map_err(|e| AppError::Database(e.to_string()))?;
        let mcp_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM mcp_servers", [], |row| row.get(0))
            .map_err(|e| AppError::Database(e.to_string()))?;

        if provider_count == 0 && mcp_count == 0 {
            return Err(AppError::Config(
                "导入的 SQL 未包含有效的供应商或 MCP 数据".to_string(),
            ));
        }
        Ok(())
    }

    /// 导出数据库为 SQL 文本
    pub(crate) fn dump_sql(conn: &Connection, skip_tables: &[&str]) -> Result<String, AppError> {
        let mut output = String::new();
        let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let user_version: i64 = conn
            .query_row("PRAGMA user_version;", [], |row| row.get(0))
            .unwrap_or(0);

        output.push_str(&format!(
            "-- CC Switch SQLite 导出\n-- 生成时间: {timestamp}\n-- user_version: {user_version}\n"
        ));
        output.push_str("PRAGMA foreign_keys=OFF;\n");
        output.push_str(&format!("PRAGMA user_version={user_version};\n"));
        output.push_str("BEGIN TRANSACTION;\n");

        // 导出 schema。Only tables and indexes: the app's own triggers are
        // recreated by create_tables_on_conn on import, and the import
        // authorizer rejects CREATE TRIGGER / CREATE VIEW.
        let mut stmt = conn
            .prepare(
                "SELECT type, name, tbl_name, sql
                 FROM sqlite_master
                 WHERE sql NOT NULL AND type IN ('table','index')
                 ORDER BY type='table' DESC, name",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut tables = Vec::new();
        let mut rows = stmt
            .query([])
            .map_err(|e| AppError::Database(e.to_string()))?;
        while let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
            let obj_type: String = row.get(0).map_err(|e| AppError::Database(e.to_string()))?;
            let name: String = row.get(1).map_err(|e| AppError::Database(e.to_string()))?;
            let sql: String = row.get(3).map_err(|e| AppError::Database(e.to_string()))?;

            // 跳过 SQLite 内部对象（如 sqlite_sequence）；迁移台账描述的是本地数据库文件的
            // 历史，不导出（导入时由迁移框架按 user_version 回填）
            if name.starts_with("sqlite_") || name == super::schema::MIGRATION_LEDGER_TABLE {
                continue;
            }

            output.push_str(&sql);
            output.push_str(";\n");

            if obj_type == "table" && !name.starts_with("sqlite_") {
                tables.push(name);
            }
        }

        // 导出数据
        for table in tables {
            if skip_tables.iter().any(|t| *t == table) {
                continue;
            }
            let columns = Self::get_table_columns(conn, &table)?;
            if columns.is_empty() {
                continue;
            }

            let mut stmt = conn
                .prepare(&format!("SELECT * FROM \"{table}\""))
                .map_err(|e| AppError::Database(e.to_string()))?;
            let mut rows = stmt
                .query([])
                .map_err(|e| AppError::Database(e.to_string()))?;

            while let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
                let mut values = Vec::with_capacity(columns.len());
                for idx in 0..columns.len() {
                    let value = row
                        .get_ref(idx)
                        .map_err(|e| AppError::Database(e.to_string()))?;
                    values.push(Self::format_sql_value(value)?);
                }

                let cols = columns
                    .iter()
                    .map(|c| format!("\"{c}\""))
                    .collect::<Vec<_>>()
                    .join(", ");
                output.push_str(&format!(
                    "INSERT INTO \"{table}\" ({cols}) VALUES ({});\n",
                    values.join(", ")
                ));
            }
        }

        output.push_str("COMMIT;\nPRAGMA foreign_keys=ON;\n");
        Ok(output)
    }

    /// 获取表的列名列表
    fn get_table_columns(conn: &Connection, table: &str) -> Result<Vec<String>, AppError> {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info(\"{table}\")"))
            .map_err(|e| AppError::Database(e.to_string()))?;
        let iter = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut columns = Vec::new();
        for col in iter {
            columns.push(col.map_err(|e| AppError::Database(e.to_string()))?);
        }
        Ok(columns)
    }

    /// 格式化 SQL 值
    fn format_sql_value(value: ValueRef<'_>) -> Result<String, AppError> {
        match value {
            ValueRef::Null => Ok("NULL".to_string()),
            ValueRef::Integer(i) => Ok(i.to_string()),
            ValueRef::Real(f) => Ok(f.to_string()),
            ValueRef::Text(t) => {
                let text = std::str::from_utf8(t)
                    .map_err(|e| AppError::Database(format!("文本字段不是有效的 UTF-8: {e}")))?;
                let escaped = text.replace('\'', "''");
                Ok(format!("'{escaped}'"))
            }
            ValueRef::Blob(bytes) => {
                let mut s = String::from("X'");
                for b in bytes {
                    use std::fmt::Write;
                    let _ = write!(&mut s, "{b:02X}");
                }
                s.push('\'');
                Ok(s)
            }
        }
    }

    /// List all database backup files, sorted by creation time (newest first)
    pub fn list_backups() -> Result<Vec<BackupEntry>, AppError> {
        let backup_dir = get_app_config_dir().join("backups");
        if !backup_dir.exists() {
            return Ok(vec![]);
        }

        let mut entries: Vec<BackupEntry> = fs::read_dir(&backup_dir)
            .map_err(|e| AppError::io(&backup_dir, e))?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|ext| ext == "db").unwrap_or(false))
            .filter_map(|e| {
                let metadata = e.metadata().ok()?;
                let filename = e.file_name().to_string_lossy().to_string();
                let size_bytes = metadata.len();
                let created_at = metadata
                    .modified()
                    .ok()
                    .map(|t| {
                        let dt: chrono::DateTime<Utc> = t.into();
                        dt.to_rfc3339()
                    })
                    .unwrap_or_default();
                Some(BackupEntry {
                    filename,
                    size_bytes,
                    created_at,
                })
            })
            .collect();

        // Sort by created_at descending (newest first)
        entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(entries)
    }

    /// Restore database from a backup file. Returns the safety backup ID.
    pub fn restore_from_backup(&self, filename: &str) -> Result<String, AppError> {
        self.restore_from_backup_in(
            &get_app_config_dir(),
            crate::settings::effective_backup_retain_count(),
            filename,
        )
    }

    /// [`Self::restore_from_backup`] with an explicit config dir and retain count.
    pub(crate) fn restore_from_backup_in(
        &self,
        config_dir: &Path,
        retain: usize,
        filename: &str,
    ) -> Result<String, AppError> {
        // Security: validate filename to prevent path traversal
        if filename.contains("..")
            || filename.contains('/')
            || filename.contains('\\')
            || !filename.ends_with(".db")
        {
            return Err(AppError::InvalidInput(
                "Invalid backup filename".to_string(),
            ));
        }

        let backup_dir = config_dir.join("backups");
        let backup_path = backup_dir.join(filename);

        if !backup_path.is_file() {
            return Err(AppError::InvalidInput(format!(
                "Backup file not found: {filename}"
            )));
        }

        // Step 1: Copy the backup into a staging database, verify it and migrate
        // it there. The live database is not touched until all checks pass, and
        // later rotation can no longer affect the data being restored.
        let staging_file = NamedTempFile::new().map_err(|e| AppError::IoContext {
            context: "创建临时数据库文件失败".to_string(),
            source: e,
        })?;
        let staging_conn = Self::stage_backup_for_restore(&backup_path, staging_file.path())?;

        // Step 2: Keep this device's proxy/takeover state; the live files on disk
        // are not part of the backup.
        {
            let local_snapshot = self.snapshot_to_memory()?;
            Self::restore_tables(&local_snapshot, &staging_conn, RESTORE_PRESERVE_TABLES)?;
            Self::restore_device_local_state(&local_snapshot, &staging_conn)?;
        }

        // Step 3: Safety backup of the current database. Rotation never prunes
        // the backup being restored.
        let safety_backup = self.backup_database_file_in(
            config_dir,
            retain,
            BackupKind::Safety,
            Some(&backup_path),
        )?;
        let safety_id = safety_backup
            .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
            .unwrap_or_default();

        // Step 4: Replace the main database with the verified staging copy
        {
            let mut main_conn = lock_conn!(self.conn);
            let backup = Backup::new(&staging_conn, &mut main_conn)
                .map_err(|e| AppError::Database(e.to_string()))?;
            backup
                .step(-1)
                .map_err(|e| AppError::Database(e.to_string()))?;
        }

        self.ensure_model_pricing_seeded()?;

        log::info!("Database restored from backup: {filename}, safety backup: {safety_id}");
        Ok(safety_id)
    }

    /// Open `source` read-only (never creating it), check that it is an intact
    /// CC Switch database this app version can read, copy it to `staging_path`
    /// and bring the copy up to the current schema.
    fn stage_backup_for_restore(
        source: &Path,
        staging_path: &Path,
    ) -> Result<Connection, AppError> {
        let invalid = |detail: String| {
            AppError::localized(
                "backup.restore.invalid_backup",
                format!("备份文件无效或已损坏: {detail}"),
                format!("The backup file is invalid or corrupt: {detail}"),
            )
        };

        let source_conn = Connection::open_with_flags(
            source,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| invalid(e.to_string()))?;

        let integrity: String = source_conn
            .query_row("PRAGMA integrity_check(1)", [], |row| row.get(0))
            .map_err(|e| invalid(e.to_string()))?;
        if !integrity.eq_ignore_ascii_case("ok") {
            return Err(invalid(integrity));
        }

        let version = Self::get_user_version(&source_conn).map_err(|e| invalid(e.to_string()))?;
        if version > SCHEMA_VERSION {
            return Err(AppError::localized(
                "backup.restore.version_too_new",
                format!(
                    "备份由更新版本的 CC Switch 创建（数据库版本 {version}，当前支持 {SCHEMA_VERSION}），请先升级应用"
                ),
                format!(
                    "This backup was created by a newer CC Switch (database version {version}, this app supports {SCHEMA_VERSION}). Please update the app first."
                ),
            ));
        }

        if !Self::table_exists(&source_conn, "providers")? {
            return Err(invalid("missing providers table".to_string()));
        }

        let mut staging_conn =
            Connection::open(staging_path).map_err(|e| AppError::Database(e.to_string()))?;
        {
            let backup =
                Backup::new(&source_conn, &mut staging_conn).map_err(|e| invalid(e.to_string()))?;
            backup.step(-1).map_err(|e| invalid(e.to_string()))?;
        }
        drop(source_conn);

        staging_conn
            .execute("PRAGMA foreign_keys = ON;", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        // Drop untrusted triggers/views first; create_tables_on_conn then
        // recreates the app's own triggers.
        Self::drop_untrusted_schema_objects(&staging_conn)?;
        Self::create_tables_on_conn(&staging_conn)?;
        Self::apply_schema_migrations_on_conn(&staging_conn)?;
        Ok(staging_conn)
    }

    /// Rename a backup file. Returns the new filename.
    pub fn rename_backup(old_filename: &str, new_name: &str) -> Result<String, AppError> {
        // Validate old filename (path traversal + .db suffix)
        if old_filename.contains("..")
            || old_filename.contains('/')
            || old_filename.contains('\\')
            || !old_filename.ends_with(".db")
        {
            return Err(AppError::InvalidInput(
                "Invalid backup filename".to_string(),
            ));
        }

        // Clean new name
        let trimmed = new_name.trim();
        if trimmed.is_empty() {
            return Err(AppError::InvalidInput(
                "New name cannot be empty".to_string(),
            ));
        }

        // Length limit (without .db suffix)
        let name_part = trimmed.strip_suffix(".db").unwrap_or(trimmed);
        if name_part.len() > 100 {
            return Err(AppError::InvalidInput(
                "Name too long (max 100 characters)".to_string(),
            ));
        }

        // Prevent path traversal in new name
        if name_part.contains("..")
            || name_part.contains('/')
            || name_part.contains('\\')
            || name_part.contains('\0')
        {
            return Err(AppError::InvalidInput(
                "Invalid characters in new name".to_string(),
            ));
        }

        let new_filename = format!("{name_part}.db");

        let backup_dir = get_app_config_dir().join("backups");
        let old_path = backup_dir.join(old_filename);
        let new_path = backup_dir.join(&new_filename);

        if !old_path.exists() {
            return Err(AppError::InvalidInput(format!(
                "Backup file not found: {old_filename}"
            )));
        }

        if new_path.exists() {
            return Err(AppError::InvalidInput(format!(
                "A backup named '{new_filename}' already exists"
            )));
        }

        fs::rename(&old_path, &new_path).map_err(|e| AppError::io(&old_path, e))?;
        log::info!("Renamed backup: {old_filename} -> {new_filename}");
        Ok(new_filename)
    }

    /// Delete a backup file permanently.
    pub fn delete_backup(filename: &str) -> Result<(), AppError> {
        // Validate filename (path traversal + .db suffix)
        if filename.contains("..")
            || filename.contains('/')
            || filename.contains('\\')
            || !filename.ends_with(".db")
        {
            return Err(AppError::InvalidInput(
                "Invalid backup filename".to_string(),
            ));
        }

        let backup_path = get_app_config_dir().join("backups").join(filename);
        if !backup_path.exists() {
            return Err(AppError::InvalidInput(format!(
                "Backup file not found: {filename}"
            )));
        }

        fs::remove_file(&backup_path).map_err(|e| AppError::io(&backup_path, e))?;
        log::info!("Deleted backup: {filename}");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Database, SYNC_PRESERVE_TABLES, SYNC_SKIP_TABLES, SYNC_SYNCED_TABLES};
    use crate::error::AppError;
    use crate::settings::{update_settings, AppSettings};
    use serial_test::serial;

    fn query_i64(db: &Database, sql: &str) -> i64 {
        let conn = db.conn.lock().unwrap();
        conn.query_row(sql, [], |row| row.get(0)).expect(sql)
    }

    fn query_opt_string(db: &Database, sql: &str) -> Option<String> {
        let conn = db.conn.lock().unwrap();
        conn.query_row(sql, [], |row| row.get(0)).ok()
    }

    fn seed_provider(db: &Database, id: &str) {
        let conn = db.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
             VALUES (?1, 'claude', ?1, '{}', '{}')",
            [id],
        )
        .expect("seed provider");
    }

    #[test]
    fn every_table_is_classified_for_sync() -> Result<(), AppError> {
        let db = Database::memory()?;
        let conn = db.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
        )?;
        let tables = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        assert!(!tables.is_empty());
        // The migration ledger is never exported (see dump_sql).
        for table in tables
            .iter()
            .filter(|t| t.as_str() != crate::database::schema::MIGRATION_LEDGER_TABLE)
        {
            let synced = SYNC_SYNCED_TABLES.contains(&table.as_str());
            let skipped = SYNC_SKIP_TABLES.contains(&table.as_str());
            assert!(
                synced ^ skipped,
                "table `{table}` must be listed in exactly one of SYNC_SYNCED_TABLES / SYNC_SKIP_TABLES"
            );
        }
        for table in SYNC_PRESERVE_TABLES {
            assert!(
                SYNC_SKIP_TABLES.contains(table),
                "preserved table `{table}` must also be skipped on export"
            );
        }
        Ok(())
    }

    #[test]
    fn sync_round_trip_keeps_device_local_state() -> Result<(), AppError> {
        // Device A: takeover on for Claude, custom port, profile pointer, proxy URL.
        let remote_db = Database::memory()?;
        seed_provider(&remote_db, "remote-provider");
        {
            let conn = remote_db.conn.lock().unwrap();
            conn.execute(
                "UPDATE proxy_config SET enabled = 1, proxy_enabled = 1, live_takeover_active = 1,
                     listen_port = 16000, max_retries = 9
                 WHERE app_type = 'claude'",
                [],
            )?;
            conn.execute_batch(
                "INSERT OR REPLACE INTO settings (key, value) VALUES ('current_profile_id_claude', 'p1');
                 INSERT OR REPLACE INTO settings (key, value) VALUES ('global_proxy_url', 'http://user:secret@a-proxy:8080');
                 INSERT OR REPLACE INTO settings (key, value) VALUES ('shared_pref', 'remote');
                 INSERT INTO session_log_sync (file_path, last_modified, last_line_offset, last_synced_at)
                     VALUES ('/home/a/.claude/projects/x.jsonl', 1, 999, 1);",
            )?;
        }
        let remote_sql = remote_db.export_sql_string_for_sync()?;
        assert!(
            !remote_sql.contains("current_profile_id_claude"),
            "profile pointer must not be exported"
        );
        assert!(
            !remote_sql.contains("a-proxy"),
            "outbound proxy URL must not be exported"
        );
        assert!(
            !remote_sql.contains("x.jsonl"),
            "session log offsets must not be exported"
        );

        // Device B: fresh install with its own session-log state.
        let local_db = Database::memory()?;
        seed_provider(&local_db, "local-provider");
        {
            let conn = local_db.conn.lock().unwrap();
            conn.execute_batch(
                "INSERT INTO session_log_sync (file_path, last_modified, last_line_offset, last_synced_at)
                     VALUES ('/home/b/.claude/projects/y.jsonl', 2, 42, 2);
                 INSERT OR REPLACE INTO settings (key, value) VALUES ('global_proxy_url', 'http://b-proxy:3128');",
            )?;
        }
        let local_port = query_i64(
            &local_db,
            "SELECT listen_port FROM proxy_config WHERE app_type = 'claude'",
        );

        local_db.import_sql_string_for_sync(&remote_sql)?;

        assert_eq!(
            query_i64(
                &local_db,
                "SELECT COUNT(*) FROM providers WHERE id = 'remote-provider'"
            ),
            1
        );
        assert_eq!(
            query_i64(
                &local_db,
                "SELECT enabled + proxy_enabled + live_takeover_active FROM proxy_config WHERE app_type = 'claude'"
            ),
            0,
            "takeover flags must stay off on the importing device"
        );
        assert_eq!(
            query_i64(
                &local_db,
                "SELECT listen_port FROM proxy_config WHERE app_type = 'claude'"
            ),
            local_port
        );
        assert_eq!(
            query_i64(
                &local_db,
                "SELECT max_retries FROM proxy_config WHERE app_type = 'claude'"
            ),
            9,
            "shared proxy preferences still sync"
        );
        assert_eq!(
            query_i64(
                &local_db,
                "SELECT COUNT(*) FROM proxy_config WHERE enabled = 1"
            ),
            0,
            "no app is enabled for takeover on startup after the download"
        );
        assert_eq!(
            query_opt_string(
                &local_db,
                "SELECT value FROM settings WHERE key = 'current_profile_id_claude'"
            ),
            None
        );
        assert_eq!(
            query_opt_string(
                &local_db,
                "SELECT value FROM settings WHERE key = 'global_proxy_url'"
            )
            .as_deref(),
            Some("http://b-proxy:3128")
        );
        assert_eq!(
            query_opt_string(
                &local_db,
                "SELECT value FROM settings WHERE key = 'shared_pref'"
            )
            .as_deref(),
            Some("remote")
        );
        assert_eq!(
            query_i64(
                &local_db,
                "SELECT last_line_offset FROM session_log_sync WHERE file_path = '/home/b/.claude/projects/y.jsonl'"
            ),
            42
        );
        assert_eq!(
            query_i64(&local_db, "SELECT COUNT(*) FROM session_log_sync"),
            1
        );
        Ok(())
    }

    #[test]
    fn sync_import_from_older_client_still_keeps_device_state() -> Result<(), AppError> {
        // Older clients export every settings key and session_log_sync rows.
        let remote_db = Database::memory()?;
        seed_provider(&remote_db, "remote-provider");
        {
            let conn = remote_db.conn.lock().unwrap();
            conn.execute_batch(
                "UPDATE proxy_config SET enabled = 1 WHERE app_type = 'claude';
                 INSERT OR REPLACE INTO settings (key, value) VALUES ('current_profile_id_claude', 'p1');
                 INSERT INTO session_log_sync (file_path, last_modified, last_line_offset, last_synced_at)
                     VALUES ('/shared/x.jsonl', 1, 999, 1);",
            )?;
        }
        let snapshot = remote_db.snapshot_to_memory()?;
        let legacy_sql = Database::dump_sql(&snapshot, &["proxy_request_logs"])?;
        assert!(legacy_sql.contains("current_profile_id_claude"));

        let local_db = Database::memory()?;
        seed_provider(&local_db, "local-provider");
        local_db.import_sql_string_for_sync(&legacy_sql)?;

        assert_eq!(
            query_i64(
                &local_db,
                "SELECT enabled FROM proxy_config WHERE app_type = 'claude'"
            ),
            0
        );
        assert_eq!(
            query_opt_string(
                &local_db,
                "SELECT value FROM settings WHERE key = 'current_profile_id_claude'"
            ),
            None
        );
        assert_eq!(
            query_i64(&local_db, "SELECT COUNT(*) FROM session_log_sync"),
            0
        );
        Ok(())
    }

    fn exported_sql_with(extra: &str) -> Result<String, AppError> {
        let db = Database::memory()?;
        seed_provider(&db, "p1");
        let sql = db.export_sql_string()?;
        let marker = "COMMIT;";
        let idx = sql.rfind(marker).expect("export ends with COMMIT");
        Ok(format!("{}{extra}\n{}", &sql[..idx], &sql[idx..]))
    }

    #[test]
    fn sql_import_round_trips_normal_export() -> Result<(), AppError> {
        let source = Database::memory()?;
        seed_provider(&source, "exported-provider");
        let sql = source.export_sql_string()?;

        let target = Database::memory()?;
        target.import_sql_string(&sql)?;
        assert_eq!(
            query_i64(
                &target,
                "SELECT COUNT(*) FROM providers WHERE id = 'exported-provider'"
            ),
            1
        );
        Ok(())
    }

    #[test]
    fn sql_import_rejects_attach() -> Result<(), AppError> {
        let dir = tempfile::tempdir().expect("tempdir");
        let attached = dir.path().join("x.db");
        let attached_sql = attached.to_string_lossy().replace('\'', "''");
        let sql = exported_sql_with(&format!(
            "ATTACH DATABASE '{attached_sql}' AS a; CREATE TABLE a.t(x);"
        ))?;

        let target = Database::memory()?;
        seed_provider(&target, "live-provider");
        let err = target
            .import_sql_string(&sql)
            .expect_err("ATTACH must be rejected");
        assert!(err.to_string().contains("not allowed"), "{err}");
        assert!(!attached.exists(), "ATTACH must not create files");
        assert_eq!(
            query_i64(
                &target,
                "SELECT COUNT(*) FROM providers WHERE id = 'live-provider'"
            ),
            1,
            "live data is untouched"
        );
        Ok(())
    }

    #[test]
    fn sql_import_rejects_triggers_views_and_other_statements() -> Result<(), AppError> {
        let payloads = [
            "CREATE TRIGGER evil AFTER UPDATE ON providers BEGIN UPDATE providers SET name = 'pwned'; END;",
            "CREATE VIEW v AS SELECT * FROM providers;",
            "DROP TABLE prompts;",
            "PRAGMA writable_schema = 1;",
            "UPDATE providers SET name = 'x';",
            "CREATE TABLE unexpected_table (x TEXT);",
            "CREATE TEMP TABLE t (x TEXT);",
        ];
        for payload in payloads {
            let sql = exported_sql_with(payload)?;
            let target = Database::memory()?;
            seed_provider(&target, "live-provider");
            let err = target
                .import_sql_string(&sql)
                .expect_err(&format!("`{payload}` must be rejected"));
            assert!(err.to_string().contains("not allowed"), "{payload}: {err}");
            assert_eq!(
                query_i64(
                    &target,
                    "SELECT COUNT(*) FROM sqlite_master WHERE type IN ('trigger', 'view') \
                     AND name NOT LIKE 'trg_request_logs_dedup_%'"
                ),
                0,
                "only the app's own dedup triggers may exist"
            );
            assert_eq!(
                query_i64(
                    &target,
                    "SELECT COUNT(*) FROM providers WHERE id = 'live-provider'"
                ),
                1
            );
        }
        Ok(())
    }

    #[test]
    fn sync_import_preserves_local_only_tables() -> Result<(), AppError> {
        let remote_db = Database::memory()?;
        {
            let conn = crate::database::lock_conn!(remote_db.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('remote-provider', 'claude', 'Remote Provider', '{}', '{}')",
                [],
            )?;
        }
        let remote_sql = remote_db.export_sql_string_for_sync()?;

        let local_db = Database::memory()?;
        {
            let conn = crate::database::lock_conn!(local_db.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('local-provider', 'claude', 'Local Provider', '{}', '{}')",
                [],
            )?;
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES ('req-1', 'local-provider', 'claude', 'claude-3', 100, 50, '0.01', 120, 200, 1000)",
                [],
            )?;
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model, request_count, success_count,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    total_cost_usd, avg_latency_ms
                ) VALUES ('2026-03-01', 'claude', 'local-provider', 'claude-3', 7, 7, 700, 350, 0, 0, '0.07', 120)",
                [],
            )?;
            conn.execute(
                "INSERT INTO stream_check_logs (
                    provider_id, provider_name, app_type, status, success, message,
                    response_time_ms, http_status, model_used, retry_count, tested_at
                ) VALUES ('local-provider', 'Local Provider', 'claude', 'operational', 1, 'ok', 42, 200, 'claude-3', 0, 1000)",
                [],
            )?;
        }

        local_db.import_sql_string_for_sync(&remote_sql)?;

        let remote_provider_exists: i64 = {
            let conn = crate::database::lock_conn!(local_db.conn);
            conn.query_row(
                "SELECT COUNT(*) FROM providers WHERE id = 'remote-provider' AND app_type = 'claude'",
                [],
                |row| row.get(0),
            )?
        };
        assert_eq!(
            remote_provider_exists, 1,
            "remote config should be imported"
        );

        let (request_logs, rollups, stream_logs): (i64, i64, i64) = {
            let conn = crate::database::lock_conn!(local_db.conn);
            let request_logs =
                conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| {
                    row.get(0)
                })?;
            let rollups =
                conn.query_row("SELECT COUNT(*) FROM usage_daily_rollups", [], |row| {
                    row.get(0)
                })?;
            let stream_logs =
                conn.query_row("SELECT COUNT(*) FROM stream_check_logs", [], |row| {
                    row.get(0)
                })?;
            (request_logs, rollups, stream_logs)
        };
        assert_eq!(request_logs, 1, "local request logs should be preserved");
        assert_eq!(rollups, 1, "local rollups should be preserved");
        assert_eq!(
            stream_logs, 1,
            "local stream check logs should be preserved"
        );

        Ok(())
    }

    #[test]
    #[serial]
    fn periodic_maintenance_runs_even_when_auto_backup_disabled() -> Result<(), AppError> {
        let old_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        let test_home =
            std::env::temp_dir().join("cc-switch-periodic-maintenance-backup-disabled-test");
        let _ = std::fs::remove_dir_all(&test_home);
        std::fs::create_dir_all(&test_home).expect("create test home");
        std::env::set_var("CC_SWITCH_TEST_HOME", &test_home);

        let settings = AppSettings {
            backup_interval_hours: Some(0),
            ..AppSettings::default()
        };
        update_settings(settings).expect("disable auto backup");

        let db = Database::memory()?;
        let now = chrono::Utc::now().timestamp();
        let old_ts = now - 40 * 86400;
        let old_stream_ts = now - 8 * 86400;

        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES ('old-req', 'p1', 'claude', 'claude-3', 100, 50, '0.01', 100, 200, ?1)",
                [old_ts],
            )?;
            conn.execute(
                "INSERT INTO stream_check_logs (
                    provider_id, provider_name, app_type, status, success, message,
                    response_time_ms, http_status, model_used, retry_count, tested_at
                ) VALUES ('p1', 'Provider 1', 'claude', 'operational', 1, 'ok', 42, 200, 'claude-3', 0, ?1)",
                [old_stream_ts],
            )?;
        }

        db.periodic_backup_if_needed()?;

        let (remaining_request_logs, stream_logs, rollups): (i64, i64, i64) = {
            let conn = crate::database::lock_conn!(db.conn);
            let remaining_request_logs =
                conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| {
                    row.get(0)
                })?;
            let stream_logs =
                conn.query_row("SELECT COUNT(*) FROM stream_check_logs", [], |row| {
                    row.get(0)
                })?;
            let rollups =
                conn.query_row("SELECT COUNT(*) FROM usage_daily_rollups", [], |row| {
                    row.get(0)
                })?;
            (remaining_request_logs, stream_logs, rollups)
        };

        assert_eq!(
            remaining_request_logs, 0,
            "old request logs should still be pruned when auto backup is disabled"
        );
        assert_eq!(
            stream_logs, 0,
            "old stream check logs should still be pruned when auto backup is disabled"
        );
        assert_eq!(rollups, 1, "old request logs should be rolled up");

        match old_test_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }

        Ok(())
    }
}
