//! 数据库备份和恢复
//!
//! 提供 SQL 导出/导入和二进制快照备份功能。

use super::{lock_conn, Database, SCHEMA_VERSION};
use crate::config::get_app_config_dir;
use crate::error::AppError;
use chrono::{Local, Utc};
use rusqlite::backup::Backup;
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use rusqlite::limits::Limit;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use std::fs;
use std::path::{Component, Path, PathBuf};
use tempfile::NamedTempFile;

const CC_SWITCH_SQL_EXPORT_HEADER: &str = "-- CC Switch SQLite 导出";

/// Tables whose data rows are skipped when exporting for WebDAV sync.
const SYNC_SKIP_TABLES: &[&str] = &[
    "proxy_request_logs",
    "stream_check_logs",
    "provider_health",
    "proxy_live_backup",
    "usage_daily_rollups",
    "session_log_sync",
];

/// Tables whose local data is preserved (restored from local snapshot) during WebDAV import.
/// Excludes ephemeral tables like provider_health that can safely rebuild at runtime.
const SYNC_PRESERVE_TABLES: &[&str] = &[
    "proxy_request_logs",
    "stream_check_logs",
    "proxy_live_backup",
    "usage_daily_rollups",
    "session_log_sync",
];

/// Settings rows that belong to this install only: never exported, and kept
/// from the local database on SQL import, sync import and backup restore.
const LOCAL_ONLY_SETTING_KEYS: &[&str] = &[crate::proxy::inbound_auth::INBOUND_TOKEN_SETTING_KEY];

/// PRAGMAs that an imported SQL file may run; `dump_sql` only emits these.
const SQL_IMPORT_ALLOWED_PRAGMAS: &[&str] = &["foreign_keys", "user_version"];

/// A database backup entry for the UI
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntry {
    pub filename: String,
    pub size_bytes: u64,
    pub created_at: String, // ISO 8601
}

/// A backup that this app can restore (its `user_version` is not newer than
/// [`SCHEMA_VERSION`]); listed on the db_version_too_new recovery screen.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestorableBackup {
    pub filename: String,
    pub size_bytes: u64,
    pub created_at: String, // ISO 8601
    pub db_version: i32,
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
        // 更新版本的导出在任何备份/写入之前拒绝，避免部分导入
        Self::ensure_sql_dump_version_supported(sql_content)?;

        // 导入前备份现有数据库
        let backup_path = self.backup_database_file()?;

        let local_settings = self.read_local_only_settings()?;

        let local_snapshot = if preserve_tables.is_empty() {
            None
        } else {
            Some(self.snapshot_to_memory()?)
        };

        // 在临时数据库执行导入，确保失败不会污染主库
        let temp_file = NamedTempFile::new().map_err(|e| AppError::IoContext {
            context: "创建临时数据库文件失败".to_string(),
            source: e,
        })?;
        let temp_path = temp_file.path().to_path_buf();
        let temp_conn =
            Connection::open(&temp_path).map_err(|e| AppError::Database(e.to_string()))?;

        // 导入文件内容不可信（头部可伪造，同步的 SHA256 也来自同一远端）：
        // 禁止 ATTACH（含 VACUUM INTO 内部的 ATTACH），并只放行导出格式所需的语句
        temp_conn.set_limit(Limit::SQLITE_LIMIT_ATTACHED, 0);
        temp_conn.authorizer(Some(Self::authorize_sql_import));
        let import_result = temp_conn.execute_batch(sql_content);
        temp_conn.authorizer(None::<fn(AuthContext<'_>) -> Authorization>);
        import_result.map_err(|e| AppError::Database(format!("执行 SQL 导入失败: {e}")))?;
        // 头部可伪造：以实际执行后的 user_version 再校验一次
        Self::ensure_dump_version_supported(Self::get_user_version(&temp_conn)?)?;

        // 补齐缺失表/索引并进行基础校验
        Self::create_tables_on_conn(&temp_conn)?;
        Self::apply_schema_migrations_on_conn(&temp_conn)?;
        Self::validate_basic_state(&temp_conn)?;
        if let Some(local_snapshot) = local_snapshot.as_ref() {
            Self::restore_tables(local_snapshot, &temp_conn, preserve_tables)?;
        }
        Self::write_local_only_settings(&temp_conn, &local_settings)?;

        // 使用 Backup 将临时库原子写回主库
        {
            let mut main_conn = lock_conn!(self.conn);
            let backup = Backup::new(&temp_conn, &mut main_conn)
                .map_err(|e| AppError::Database(e.to_string()))?;
            backup
                .step(-1)
                .map_err(|e| AppError::Database(e.to_string()))?;
        }
        // Backup API 整库覆盖不经过 update hook，需手动让运行时配置快照失效
        self.mark_config_changed();

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

    /// SQL 导入期间的授权回调：仅允许 `dump_sql` 产生的语句类型
    /// （事务、CREATE TABLE/INDEX、INSERT 及白名单 PRAGMA；REINDEX 只重建已有索引），其余一律拒绝。
    fn authorize_sql_import(ctx: AuthContext<'_>) -> Authorization {
        match ctx.action {
            AuthAction::Transaction { .. }
            | AuthAction::CreateTable { .. }
            | AuthAction::CreateIndex { .. }
            | AuthAction::Insert { .. }
            | AuthAction::Read { .. }
            | AuthAction::Select
            | AuthAction::Reindex { .. }
            | AuthAction::Function { .. } => Authorization::Allow,
            // CREATE TABLE/INDEX 内部更新 sqlite_master；用户语句直接改写它需要
            // writable_schema，而该 PRAGMA 不在白名单内
            AuthAction::Update { table_name, .. }
                if table_name.eq_ignore_ascii_case("sqlite_master") =>
            {
                Authorization::Allow
            }
            AuthAction::Pragma { pragma_name, .. }
                if SQL_IMPORT_ALLOWED_PRAGMAS
                    .iter()
                    .any(|allowed| allowed.eq_ignore_ascii_case(pragma_name)) =>
            {
                Authorization::Allow
            }
            _ => Authorization::Deny,
        }
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

    /// 读取 `dump_sql` 写入的 `PRAGMA user_version=N;`（缺失时返回 None）
    fn sql_dump_user_version(sql: &str) -> Option<i32> {
        sql.lines().find_map(|line| {
            let line = line.trim();
            let (name, value) = line
                .strip_prefix("PRAGMA ")
                .or_else(|| line.strip_prefix("pragma "))?
                .split_once('=')?;
            if !name.trim().eq_ignore_ascii_case("user_version") {
                return None;
            }
            value.trim().trim_end_matches(';').trim().parse().ok()
        })
    }

    /// SQL 导出（含同步快照 db.sql）来自更新的 schema 时返回本地化错误。
    ///
    /// 仅检查文本，不触碰数据库或 Skills；同步导入在替换任何本地数据前调用，
    /// 保证不会出现部分导入。
    pub(crate) fn ensure_sql_dump_version_supported(sql: &str) -> Result<(), AppError> {
        match Self::sql_dump_user_version(sql) {
            Some(version) => Self::ensure_dump_version_supported(version),
            None => Ok(()),
        }
    }

    fn ensure_dump_version_supported(version: i32) -> Result<(), AppError> {
        if version <= SCHEMA_VERSION {
            return Ok(());
        }
        Err(AppError::localized(
            "backup.sql.newer_schema",
            format!(
                "该数据来自更新版本的 CC Switch（数据库版本 {version}），当前应用仅支持 {SCHEMA_VERSION}，请升级应用后再导入。本地数据未做任何修改。"
            ),
            format!(
                "This data was exported by a newer version of CC Switch (database version {version}); this app supports up to {SCHEMA_VERSION}. Update the app before importing it. No local data was changed."
            ),
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

    /// 生成一致性快照备份，返回备份文件路径（不存在主库时返回 None）
    pub(crate) fn backup_database_file(&self) -> Result<Option<PathBuf>, AppError> {
        let db_path = get_app_config_dir().join("cc-switch.db");
        if !db_path.exists() {
            return Ok(None);
        }

        let (_, backup_path) = {
            let conn = lock_conn!(self.conn);
            Self::snapshot_connection_to_backup(&conn)?
        };
        Ok(Some(backup_path))
    }

    /// 迁移前备份：在任何 DDL（create_tables / schema 迁移）之前调用。
    ///
    /// 已有数据库且 `user_version < SCHEMA_VERSION`（含 v0）时，用 SQLite Online Backup API
    /// 生成快照（WAL 中尚未 checkpoint 的写入也会包含在内）；旧版 `config.json` 仍存在时
    /// 一并复制为 `<backup_id>.config.json`。
    ///
    /// 返回备份 ID（无需备份时返回 None），供迁移失败时提示用户恢复。
    pub(crate) fn create_premigration_backup(
        conn: &Connection,
        db_existed: bool,
    ) -> Result<Option<String>, AppError> {
        if !db_existed {
            // JSON → SQLite 升级的 config.json 备份由启动流程在加载配置前完成
            // （见 backup_legacy_config_json），此时数据库还是空的，无需快照
            return Ok(None);
        }
        let version = Self::get_user_version(conn)?;
        if version >= SCHEMA_VERSION {
            return Ok(None);
        }
        log::info!("Creating pre-migration database backup (v{version} → v{SCHEMA_VERSION})");
        let (backup_id, _) = Self::snapshot_connection_to_backup(conn)?;
        // 数据库快照已完成，config.json 复制失败不应让调用方丢失备份 ID
        if let Err(e) = Self::copy_legacy_config_json(&backup_id) {
            log::warn!("Pre-migration config.json backup failed: {e}");
        }
        Ok(Some(backup_id))
    }

    /// JSON → SQLite 升级前备份旧版 `config.json`（原样复制为 `<backup_id>.config.json`）。
    ///
    /// 必须在 `MultiAppConfig::load()` 之前调用：加载时会就地升级并重写 config.json
    /// （如 v3.6 → v3.7 MCP 结构迁移）。config.json 不存在时返回 None。
    pub fn backup_legacy_config_json() -> Result<Option<String>, AppError> {
        let backup_dir = get_app_config_dir().join("backups");
        if !get_app_config_dir().join("config.json").exists() {
            return Ok(None);
        }
        fs::create_dir_all(&backup_dir).map_err(|e| AppError::io(&backup_dir, e))?;
        let (backup_id, _) = Self::next_backup_id(&backup_dir);
        Self::copy_legacy_config_json(&backup_id)?;
        Ok(Some(backup_id))
    }

    /// 若 config.json 存在，复制到 `backups/<backup_id>.config.json`
    fn copy_legacy_config_json(backup_id: &str) -> Result<(), AppError> {
        let app_dir = get_app_config_dir();
        let json_path = app_dir.join("config.json");
        if !json_path.exists() {
            return Ok(());
        }
        let backup_dir = app_dir.join("backups");
        fs::create_dir_all(&backup_dir).map_err(|e| AppError::io(&backup_dir, e))?;
        let dest = backup_dir.join(format!("{backup_id}.config.json"));
        log::info!("Backing up legacy config.json to {}", dest.display());
        fs::copy(&json_path, &dest).map_err(|e| AppError::io(&dest, e))?;
        Ok(())
    }

    /// 生成一个未被占用的备份 ID（`db_backup_YYYYMMDD_HHMMSS[_N]`）及对应的 `.db` 路径。
    /// 同名的 `.db` 或 `.config.json` 已存在时都会追加序号。
    fn next_backup_id(backup_dir: &Path) -> (String, PathBuf) {
        let base_id = format!("db_backup_{}", Local::now().format("%Y%m%d_%H%M%S"));
        let mut backup_id = base_id.clone();
        let mut counter = 1;
        loop {
            let backup_path = backup_dir.join(format!("{backup_id}.db"));
            let json_path = backup_dir.join(format!("{backup_id}.config.json"));
            if !backup_path.exists() && !json_path.exists() {
                return (backup_id, backup_path);
            }
            backup_id = format!("{base_id}_{counter}");
            counter += 1;
        }
    }

    /// 使用 SQLite Online Backup API 将 `conn` 快照到 backups 目录，返回 (备份 ID, 路径)
    fn snapshot_connection_to_backup(conn: &Connection) -> Result<(String, PathBuf), AppError> {
        let backup_dir = get_app_config_dir().join("backups");
        fs::create_dir_all(&backup_dir).map_err(|e| AppError::io(&backup_dir, e))?;

        let (backup_id, backup_path) = Self::next_backup_id(&backup_dir);
        {
            let mut dest_conn =
                Connection::open(&backup_path).map_err(|e| AppError::Database(e.to_string()))?;
            Self::copy_database(conn, &mut dest_conn)?;
            // Backup API 会连同库头复制 WAL 标记；备份文件独立存放、常以只读方式打开，
            // 切回 DELETE 以免留下 -wal / -shm 文件
            super::journal::set_journal_mode(&dest_conn, "delete")?;
        }

        Self::cleanup_db_backups(&backup_dir)?;
        Ok((backup_id, backup_path))
    }

    /// 清理旧的数据库备份，保留最新的 N 个
    fn cleanup_db_backups(dir: &Path) -> Result<(), AppError> {
        let retain = crate::settings::effective_backup_retain_count();
        let entries = match fs::read_dir(dir) {
            Ok(iter) => iter
                .filter_map(|entry| entry.ok())
                .filter(|entry| {
                    entry
                        .path()
                        .extension()
                        .map(|ext| ext == "db")
                        .unwrap_or(false)
                })
                .collect::<Vec<_>>(),
            Err(_) => return Ok(()),
        };

        if entries.len() <= retain {
            return Ok(());
        }

        let remove_count = entries.len().saturating_sub(retain);
        let mut sorted = entries;
        sorted.sort_by_key(|entry| entry.metadata().and_then(|m| m.modified()).ok());

        for entry in sorted.into_iter().take(remove_count) {
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
    fn dump_sql(conn: &Connection, skip_tables: &[&str]) -> Result<String, AppError> {
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

        // 导出 schema
        let mut stmt = conn
            .prepare(
                "SELECT type, name, tbl_name, sql
                 FROM sqlite_master
                 WHERE sql NOT NULL AND type IN ('table','index','trigger','view')
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

            // 跳过 SQLite 内部对象（如 sqlite_sequence）
            if name.starts_with("sqlite_") {
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

            // 本安装专属的 settings 行（如代理入站 token）不导出
            let settings_key_idx = (table == "settings")
                .then(|| columns.iter().position(|c| c == "key"))
                .flatten();

            let mut stmt = conn
                .prepare(&format!("SELECT * FROM \"{table}\""))
                .map_err(|e| AppError::Database(e.to_string()))?;
            let mut rows = stmt
                .query([])
                .map_err(|e| AppError::Database(e.to_string()))?;

            while let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
                if let Some(key_idx) = settings_key_idx {
                    if let ValueRef::Text(key) = row
                        .get_ref(key_idx)
                        .map_err(|e| AppError::Database(e.to_string()))?
                    {
                        if LOCAL_ONLY_SETTING_KEYS
                            .iter()
                            .any(|local| local.as_bytes() == key)
                        {
                            continue;
                        }
                    }
                }

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

    /// 读取本安装专属的 settings 行（键, 本机值）
    fn read_local_only_settings(&self) -> Result<Vec<(&'static str, Option<String>)>, AppError> {
        LOCAL_ONLY_SETTING_KEYS
            .iter()
            .map(|key| Ok((*key, self.get_setting(key)?)))
            .collect()
    }

    /// 把本机原值写回目标库：本机没有时删除导入进来的值，避免沿用其他安装的 token
    fn write_local_only_settings(
        conn: &Connection,
        settings: &[(&'static str, Option<String>)],
    ) -> Result<(), AppError> {
        if !Self::table_exists(conn, "settings")? {
            return Ok(());
        }
        for (key, value) in settings {
            match value {
                Some(value) => conn.execute(
                    "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
                    rusqlite::params![key, value],
                ),
                None => conn.execute(
                    "DELETE FROM settings WHERE key = ?1",
                    rusqlite::params![key],
                ),
            }
            .map_err(|e| AppError::Database(format!("保留本机设置 {key} 失败: {e}")))?;
        }
        Ok(())
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

    /// List the backups this app can restore (`user_version <= SCHEMA_VERSION`),
    /// newest first. Used by the db_version_too_new startup path, where these are
    /// the pre-migration snapshots taken before a newer app upgraded the database.
    /// Unreadable backups are skipped.
    pub fn list_restorable_backups() -> Result<Vec<RestorableBackup>, AppError> {
        let backup_dir = get_app_config_dir().join("backups");
        let entries = Self::list_backups()?
            .into_iter()
            .filter_map(|entry| {
                let path = backup_dir.join(&entry.filename);
                let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                    .map_err(|e| log::warn!("Skipping unreadable backup {}: {e}", entry.filename))
                    .ok()?;
                let db_version = Self::get_user_version(&conn)
                    .map_err(|e| log::warn!("Skipping unreadable backup {}: {e}", entry.filename))
                    .ok()?;
                (db_version <= SCHEMA_VERSION).then_some(RestorableBackup {
                    filename: entry.filename,
                    size_bytes: entry.size_bytes,
                    created_at: entry.created_at,
                    db_version,
                })
            })
            .collect();
        Ok(entries)
    }

    /// Downgrade restore for the db_version_too_new recovery screen: replace the
    /// on-disk database (too new for this app, so never opened by [`Database::init`])
    /// with a restorable backup.
    ///
    /// The backup is migrated and validated in a temp database first. The newer
    /// database is kept as `backups/db_newer_v{N}_<timestamp>.sqlite` (not a `.db`
    /// file, so backup retention never prunes it and it is not listed as a backup).
    /// Returns that preserved filename.
    pub fn restore_backup_over_newer_db(filename: &str) -> Result<String, AppError> {
        validate_backup_filename(filename)?;

        let app_dir = get_app_config_dir();
        let backup_dir = app_dir.join("backups");
        let backup_path = backup_dir.join(filename);
        let db_path = app_dir.join("cc-switch.db");

        if !backup_path.exists() {
            return Err(AppError::InvalidInput(format!(
                "Backup file not found: {filename}"
            )));
        }

        let source_conn =
            Connection::open_with_flags(&backup_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(|e| AppError::Database(e.to_string()))?;
        let backup_version = Self::get_user_version(&source_conn)?;
        if backup_version > SCHEMA_VERSION {
            return Err(AppError::localized(
                "error.backupFromNewerVersion",
                format!(
                    "该备份来自更新版本的 CC Switch（数据库版本 {backup_version}），当前应用仅支持 {SCHEMA_VERSION}，请升级应用后再恢复。"
                ),
                format!(
                    "This backup was created by a newer version of CC Switch (database version {backup_version}); this app supports up to {SCHEMA_VERSION}. Please update the app before restoring it."
                ),
            ));
        }

        let temp_file = NamedTempFile::new().map_err(|e| AppError::IoContext {
            context: "创建临时数据库文件失败".to_string(),
            source: e,
        })?;
        let mut temp_conn =
            Connection::open(temp_file.path()).map_err(|e| AppError::Database(e.to_string()))?;
        Self::copy_database(&source_conn, &mut temp_conn)?;
        drop(source_conn);

        Self::create_tables_on_conn(&temp_conn)?;
        Self::apply_schema_migrations_on_conn(&temp_conn)?;
        Self::validate_basic_state(&temp_conn)?;
        Self::ensure_model_pricing_seeded_on_conn(&temp_conn)?;

        fs::create_dir_all(&backup_dir).map_err(|e| AppError::io(&backup_dir, e))?;
        let mut preserved_name = String::new();
        if db_path.exists() {
            let newer_conn =
                Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                    .map_err(|e| AppError::Database(e.to_string()))?;
            let newer_version = Self::get_user_version(&newer_conn)?;
            let base = format!(
                "db_newer_v{newer_version}_{}",
                Local::now().format("%Y%m%d_%H%M%S")
            );
            preserved_name = format!("{base}.sqlite");
            let mut counter = 1;
            while backup_dir.join(&preserved_name).exists() {
                preserved_name = format!("{base}_{counter}.sqlite");
                counter += 1;
            }
            let preserved_path = backup_dir.join(&preserved_name);
            let mut preserved_conn =
                Connection::open(&preserved_path).map_err(|e| AppError::Database(e.to_string()))?;
            Self::copy_database(&newer_conn, &mut preserved_conn)?;
            super::journal::set_journal_mode(&preserved_conn, "delete")?;
        }

        {
            let mut main_conn =
                Connection::open(&db_path).map_err(|e| AppError::Database(e.to_string()))?;
            Self::copy_database(&temp_conn, &mut main_conn)?;
        }

        log::info!(
            "Database restored from backup {filename} (v{backup_version}); newer database kept as {preserved_name}"
        );
        Ok(preserved_name)
    }

    /// Restore database from a backup file. Returns the safety backup ID.
    pub fn restore_from_backup(&self, filename: &str) -> Result<String, AppError> {
        // Security: validate filename to prevent path traversal
        validate_backup_filename(filename)?;

        let backup_dir = get_app_config_dir().join("backups");
        let backup_path = backup_dir.join(filename);

        if !backup_path.exists() {
            return Err(AppError::InvalidInput(format!(
                "Backup file not found: {filename}"
            )));
        }

        // Step 1: Open the backup read-only and reject newer schemas before touching anything
        let source_conn =
            Connection::open_with_flags(&backup_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(|e| AppError::Database(e.to_string()))?;
        let backup_version = Self::get_user_version(&source_conn)?;
        if backup_version > SCHEMA_VERSION {
            return Err(AppError::localized(
                "error.backupFromNewerVersion",
                format!(
                    "该备份来自更新版本的 CC Switch（数据库版本 {backup_version}），当前应用仅支持 {SCHEMA_VERSION}，请升级应用后再恢复。"
                ),
                format!(
                    "This backup was created by a newer version of CC Switch (database version {backup_version}); this app supports up to {SCHEMA_VERSION}. Please update the app before restoring it."
                ),
            ));
        }

        let local_settings = self.read_local_only_settings()?;

        // Step 2: Copy the backup into a temp database and migrate/validate it there,
        // so a broken backup never replaces the main database
        let temp_file = NamedTempFile::new().map_err(|e| AppError::IoContext {
            context: "创建临时数据库文件失败".to_string(),
            source: e,
        })?;
        let mut temp_conn =
            Connection::open(temp_file.path()).map_err(|e| AppError::Database(e.to_string()))?;
        Self::copy_database(&source_conn, &mut temp_conn)?;
        drop(source_conn);

        Self::create_tables_on_conn(&temp_conn)?;
        Self::apply_schema_migrations_on_conn(&temp_conn)?;
        Self::validate_basic_state(&temp_conn)?;
        Self::ensure_model_pricing_seeded_on_conn(&temp_conn)?;
        Self::write_local_only_settings(&temp_conn, &local_settings)?;

        // Step 3: Create safety backup of current database
        let safety_backup = self.backup_database_file()?;
        let safety_id = safety_backup
            .as_ref()
            .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
            .unwrap_or_default();

        // Step 4: Write the validated temp database into the main database;
        // roll back to the safety backup if the copy fails midway
        let restore_result = {
            let mut main_conn = lock_conn!(self.conn);
            Self::copy_database(&temp_conn, &mut main_conn)
        };
        // Backup API 整库覆盖不经过 update hook，需手动让运行时配置快照失效
        // （成功与回滚两条路径都可能已改写主库内容）
        self.mark_config_changed();
        if let Err(err) = restore_result {
            log::error!("Restoring backup {filename} failed, rolling back: {err}");
            if let Some(safety_path) = safety_backup.as_ref() {
                let rollback =
                    Connection::open_with_flags(safety_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                        .map_err(|e| AppError::Database(e.to_string()))
                        .and_then(|safety_conn| {
                            let mut main_conn = lock_conn!(self.conn);
                            Self::copy_database(&safety_conn, &mut main_conn)
                        });
                self.mark_config_changed();
                if let Err(rollback_err) = rollback {
                    log::error!("Rolling back to safety backup {safety_id} failed: {rollback_err}");
                }
            }
            return Err(err);
        }

        log::info!("Database restored from backup: {filename}, safety backup: {safety_id}");
        Ok(safety_id)
    }

    /// 使用 SQLite Backup API 将 `source` 完整复制到 `dest`
    fn copy_database(source: &Connection, dest: &mut Connection) -> Result<(), AppError> {
        let backup = Backup::new(source, dest).map_err(|e| AppError::Database(e.to_string()))?;
        backup
            .step(-1)
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// Rename a backup file. Returns the new filename.
    pub fn rename_backup(old_filename: &str, new_name: &str) -> Result<String, AppError> {
        // Validate old filename (path traversal + .db suffix)
        validate_backup_filename(old_filename)?;

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
        if validate_backup_filename(&new_filename).is_err() {
            return Err(AppError::InvalidInput(
                "Invalid characters in new name".to_string(),
            ));
        }

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
        validate_backup_filename(filename)?;

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

/// 校验备份文件名只能指向 backups 目录下的单个 `.db` 文件。
///
/// 除了 `..` 和路径分隔符外，还拒绝 `:`（Windows 盘符相对路径如 `C:x.db`
/// 会让 `backup_dir.join()` 丢弃 backups 目录）和 NUL；允许非 ASCII 名称（如中文）。
fn validate_backup_filename(filename: &str) -> Result<(), AppError> {
    let invalid = || AppError::InvalidInput("Invalid backup filename".to_string());

    if filename.contains("..")
        || filename.contains('/')
        || filename.contains('\\')
        || filename.contains(':')
        || filename.contains('\0')
        || !filename.ends_with(".db")
    {
        return Err(invalid());
    }

    let mut components = Path::new(filename).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(name)), None) if name == std::ffi::OsStr::new(filename) => Ok(()),
        _ => Err(invalid()),
    }
}

#[cfg(test)]
mod tests {
    use super::Database;
    use crate::error::AppError;
    use crate::settings::{update_settings, AppSettings};
    use serial_test::serial;

    #[test]
    fn proxy_inbound_token_stays_with_this_install_across_export_and_import() -> Result<(), AppError>
    {
        use crate::proxy::inbound_auth::{get_or_create_inbound_token, INBOUND_TOKEN_SETTING_KEY};

        let remote_db = Database::memory()?;
        let remote_token = get_or_create_inbound_token(&remote_db)?;
        remote_db.set_setting("remote_marker", "1")?;
        {
            let conn = crate::database::lock_conn!(remote_db.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('remote-provider', 'claude', 'Remote Provider', '{}', '{}')",
                [],
            )?;
        }
        let full_sql = remote_db.export_sql_string()?;
        let sync_sql = remote_db.export_sql_string_for_sync()?;
        assert!(!full_sql.contains(&remote_token), "full export leaks token");
        assert!(!sync_sql.contains(&remote_token), "sync export leaks token");

        // An import keeps the local token, so the running proxy guard and the
        // live configs written from the DB keep agreeing.
        let local_db = Database::memory()?;
        let local_token = get_or_create_inbound_token(&local_db)?;
        local_db.import_sql_string_for_sync(&sync_sql)?;
        assert_eq!(get_or_create_inbound_token(&local_db)?, local_token);
        assert_eq!(local_db.get_setting("remote_marker")?.as_deref(), Some("1"));
        local_db.import_sql_string(&full_sql)?;
        assert_eq!(get_or_create_inbound_token(&local_db)?, local_token);

        // A foreign token inside an older export is not adopted.
        let legacy_sql = full_sql.replacen(
            "COMMIT;",
            &format!(
                "INSERT INTO \"settings\" (\"key\", \"value\") VALUES ('{INBOUND_TOKEN_SETTING_KEY}', '{remote_token}');\nCOMMIT;"
            ),
            1,
        );
        let fresh_db = Database::memory()?;
        fresh_db.import_sql_string(&legacy_sql)?;
        assert_eq!(fresh_db.get_setting(INBOUND_TOKEN_SETTING_KEY)?, None);
        assert_ne!(get_or_create_inbound_token(&fresh_db)?, remote_token);
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
    fn sync_import_preserves_local_session_log_sync_cursors() -> Result<(), AppError> {
        let remote_db = Database::memory()?;
        {
            let conn = crate::database::lock_conn!(remote_db.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('remote-provider', 'claude', 'Remote Provider', '{}', '{}')",
                [],
            )?;
            conn.execute(
                "INSERT INTO session_log_sync (file_path, last_modified, last_line_offset, last_synced_at)
                 VALUES ('/remote/only.jsonl', 111, 11, 1111),
                        ('/shared/session.jsonl', 999, 99, 9999)",
                [],
            )?;
        }
        let remote_sql = remote_db.export_sql_string_for_sync()?;
        assert!(
            !remote_sql.contains("INSERT INTO \"session_log_sync\""),
            "sync export should not include session_log_sync rows"
        );

        let local_db = Database::memory()?;
        {
            let conn = crate::database::lock_conn!(local_db.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('local-provider', 'claude', 'Local Provider', '{}', '{}')",
                [],
            )?;
            conn.execute(
                "INSERT INTO session_log_sync (file_path, last_modified, last_line_offset, last_synced_at)
                 VALUES ('/local/only.jsonl', 222, 22, 2222),
                        ('/shared/session.jsonl', 333, 33, 3333)",
                [],
            )?;
        }

        // Even a snapshot that still carries remote cursor rows (e.g. produced by an
        // older version) must not replace the local cursors.
        let full_remote_sql = remote_db.export_sql_string()?;
        assert!(full_remote_sql.contains("INSERT INTO \"session_log_sync\""));

        for sql in [&remote_sql, &full_remote_sql] {
            local_db.import_sql_string_for_sync(sql)?;

            let conn = crate::database::lock_conn!(local_db.conn);
            let remote_provider_exists: i64 = conn.query_row(
                "SELECT COUNT(*) FROM providers WHERE id = 'remote-provider' AND app_type = 'claude'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(
                remote_provider_exists, 1,
                "remote config should be imported"
            );

            let mut stmt = conn.prepare(
                "SELECT file_path, last_modified, last_line_offset, last_synced_at
                 FROM session_log_sync ORDER BY file_path",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            assert_eq!(
                rows,
                vec![
                    ("/local/only.jsonl".to_string(), 222, 22, 2222),
                    ("/shared/session.jsonl".to_string(), 333, 33, 3333),
                ],
                "local session_log_sync cursors should be preserved"
            );
        }

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

    #[test]
    fn sql_import_rejects_attach_vacuum_into_and_unsafe_statements() -> Result<(), AppError> {
        let dir = tempfile::tempdir().expect("create temp dir");
        let attach_target = dir.path().join("pwned.db");
        let vacuum_target = dir.path().join("v.db");
        let header = "-- CC Switch SQLite 导出\n";

        let db = Database::memory()?;
        db.set_setting("local_marker", "kept")?;
        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('p1', 'claude', 'Provider', '{}', '{}')",
                [],
            )?;
        }

        let attach_sql = format!(
            "{header}ATTACH DATABASE '{}' AS x; CREATE TABLE x.t(a);",
            attach_target.display()
        );
        let vacuum_sql = format!("{header}VACUUM INTO '{}';", vacuum_target.display());
        let cases = [
            attach_sql,
            vacuum_sql,
            format!("{header}CREATE TABLE t(a); CREATE TRIGGER tr AFTER INSERT ON t BEGIN SELECT 1; END;"),
            format!("{header}CREATE VIEW v AS SELECT 1;"),
            format!("{header}PRAGMA writable_schema=ON;"),
            format!("{header}CREATE TABLE t(a); DELETE FROM t;"),
        ];

        for sql in &cases {
            assert!(
                db.import_sql_string(sql).is_err(),
                "import should reject: {sql}"
            );
            assert!(
                db.import_sql_string_for_sync(sql).is_err(),
                "sync import should reject: {sql}"
            );
        }

        assert!(!attach_target.exists(), "ATTACH created {attach_target:?}");
        assert!(
            !vacuum_target.exists(),
            "VACUUM INTO created {vacuum_target:?}"
        );
        assert_eq!(
            db.get_setting("local_marker")?.as_deref(),
            Some("kept"),
            "rejected import must not touch the main database"
        );

        // A genuine export (PRAGMA, transaction, CREATE TABLE/INDEX, INSERT) still imports.
        let export = db.export_sql_string()?;
        let fresh_db = Database::memory()?;
        fresh_db.import_sql_string(&export)?;
        assert_eq!(
            fresh_db.get_setting("local_marker")?.as_deref(),
            Some("kept")
        );
        Ok(())
    }

    fn assert_invalid_filename(result: Result<impl std::fmt::Debug, AppError>) {
        match result {
            Err(AppError::InvalidInput(msg)) => {
                assert!(
                    msg.starts_with("Invalid"),
                    "expected filename validation error, got: {msg}"
                )
            }
            other => panic!("expected InvalidInput, got: {other:?}"),
        }
    }

    #[test]
    fn validate_backup_filename_rejects_drive_relative_and_nul_names() {
        for name in ["C:x.db", "C:..x.db", "foo:bar.db", "\0.db", "a\0b.db"] {
            assert_invalid_filename(super::validate_backup_filename(name));
        }
        for name in [
            "../x.db", "a/b.db", "a\\b.db", "/abs.db", "x.sql", ".db.bak", "",
        ] {
            assert_invalid_filename(super::validate_backup_filename(name));
        }
        for name in ["db_backup_20260101_000000.db", "我的备份.db", "a b.db"] {
            super::validate_backup_filename(name).expect(name);
        }
    }

    #[test]
    #[serial]
    fn backup_commands_reject_colon_filenames_without_touching_files() -> Result<(), AppError> {
        let home = tempfile::tempdir().expect("tempdir");
        let _paths = crate::paths::scoped(crate::paths::Paths::with_home(home.path()));
        let backup_dir = crate::config::get_app_config_dir().join("backups");
        std::fs::create_dir_all(&backup_dir).expect("create backups dir");
        let ok_path = backup_dir.join("ok.db");
        std::fs::write(&ok_path, b"x").expect("write ok backup");

        let db = Database::memory()?;
        for name in ["C:x.db", "C:..x.db", "foo:bar.db", "\0.db"] {
            assert_invalid_filename(db.restore_from_backup(name));
            assert_invalid_filename(Database::delete_backup(name));
            assert_invalid_filename(Database::rename_backup(name, "renamed"));
            assert_invalid_filename(Database::rename_backup("ok.db", name));
        }
        assert_invalid_filename(Database::rename_backup("ok.db", "C:evil"));

        // On Unix a ':' is a legal filename character, so this file really exists;
        // it must still be rejected instead of being deleted.
        #[cfg(unix)]
        {
            let colon_path = backup_dir.join("foo:bar.db");
            std::fs::write(&colon_path, b"x").expect("write colon backup");
            assert_invalid_filename(Database::delete_backup("foo:bar.db"));
            assert!(colon_path.exists(), "colon-named file must not be deleted");
        }

        // Non-ASCII names stay supported.
        assert_eq!(Database::rename_backup("ok.db", "我的备份")?, "我的备份.db");
        assert!(backup_dir.join("我的备份.db").exists());
        Database::delete_backup("我的备份.db")?;
        assert!(!backup_dir.join("我的备份.db").exists());
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn drive_relative_names_are_not_single_normal_components_on_windows() {
        use std::path::{Component, Path};

        for name in ["C:x.db", "C:..x.db"] {
            let first = Path::new(name).components().next();
            assert!(
                matches!(first, Some(Component::Prefix(_))),
                "{name} should parse as a drive prefix"
            );
            let joined = Path::new(r"D:\backups").join(name);
            assert!(!joined.starts_with(r"D:\backups"));
            assert_invalid_filename(super::validate_backup_filename(name));
        }
    }
}
