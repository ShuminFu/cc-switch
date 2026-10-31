//! 数据库模块 - SQLite 数据持久化
//!
//! 此模块提供应用的核心数据存储功能，包括：
//! - 供应商配置管理
//! - MCP 服务器配置
//! - 提示词管理
//! - Skills 管理
//! - 通用设置存储
//!
//! ## 架构设计
//!
//! See `docs/dev/database.md` for table ownership, sync classification,
//! timestamp units, retention/backup rules and the schema-change checklist.
//!
//! ```text
//! database/
//! ├── mod.rs              - Database 结构体 + 初始化 + 连接配置
//! ├── schema.rs           - 冻结基线 DDL + 迁移注册表 MIGRATIONS
//! ├── backup.rs           - SQL 导入导出 + 快照备份 / 恢复
//! ├── migration.rs        - JSON → SQLite 数据迁移
//! ├── pricing_seed.rs     - 内置模型定价（model_pricing.json）
//! ├── sync_rows.rs        - 行版本 / 删除墓碑 / 保留期设置
//! ├── read_pool.rs        - WAL 只读连接池
//! ├── usage_writer.rs     - 使用量批量写线程
//! ├── request_cache.rs    - 代理请求配置快照
//! └── dao/                - 数据访问对象
//!     ├── providers.rs, providers_seed.rs
//!     ├── mcp.rs, prompts.rs, skills.rs, settings.rs
//!     ├── universal_providers.rs, profiles.rs
//!     ├── proxy.rs, failover.rs, stream_check.rs
//!     └── usage_rollup.rs
//! ```

pub(crate) mod backup;
pub(crate) mod cost_nanos;
mod dao;
mod migration;
pub(crate) mod model_catalog;
mod pricing_seed;
mod read_pool;
mod request_cache;
mod schema;
pub(crate) mod setting_keys;
pub(crate) mod sync_diff;
mod sync_rows;
mod usage_writer;

#[cfg(test)]
mod tests;

// DAO 类型导出供外部使用
pub(crate) use dao::pricing as dao_pricing;
pub use dao::pricing::ModelPricingInfo;
pub(crate) use dao::providers_seed::{
    is_official_seed_id, CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID, CODEX_OFFICIAL_PROVIDER_ID,
};
pub(crate) use dao::proxy::{
    validate_cost_multiplier, validate_pricing_source, PRICING_SOURCE_REQUEST,
    PRICING_SOURCE_RESPONSE,
};
pub use dao::FailoverQueueItem;
pub use dao::Profile;
pub(crate) use read_pool::ReadConn;
pub use request_cache::ProxyRequestSnapshot;
pub use usage_writer::UsageWriterStats;
pub(crate) use usage_writer::WriteJob;

use crate::config::get_app_config_dir;
use crate::error::AppError;
use rusqlite::{hooks::Action, Connection};
use serde::Serialize;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// DAO 方法通过 impl Database 提供，无需额外导出

/// 当前 Schema 版本号：由 schema.rs 的迁移注册表 `MIGRATIONS` 推导（最后一个迁移的版本）。
/// 修改表结构时在注册表末尾追加迁移，不要手动修改这里。
pub(crate) const SCHEMA_VERSION: i32 = schema::LATEST_SCHEMA_VERSION;
/// 同步协议的数据库兼容版本（`services::sync_protocol::DB_COMPAT_VERSION`），同样由注册表推导。
pub(crate) const SYNC_DB_COMPAT_VERSION: u32 = schema::SYNC_DB_COMPAT_VERSION;

/// 主库连接的 busy_timeout：其它进程/连接（备份、外部工具）短暂持有写锁时等待而不是立即报错。
pub(crate) const DB_BUSY_TIMEOUT: Duration = Duration::from_secs(5);
/// Prepared-statement cache per connection (PERF-31): enough for the DAO's hot
/// statements (settings, per-app proxy config, providers, pricing lookups).
pub(crate) const DB_STATEMENT_CACHE_CAPACITY: usize = 64;
/// 页缓存大小（负数 = KiB），约 16 MiB。
const DB_CACHE_SIZE_KIB: i64 = 16_000;
/// WAL 文件在检查点后截断到的上限，避免长时间运行后 -wal 文件无限增长。
const DB_JOURNAL_SIZE_LIMIT_BYTES: i64 = 64 * 1024 * 1024;
/// 设为 `1`/`true` 时不启用 WAL（例如数据库目录位于不支持共享内存的网络盘上）。
const DISABLE_WAL_ENV: &str = "CC_SWITCH_DISABLE_WAL";

fn wal_disabled_by_env() -> bool {
    std::env::var(DISABLE_WAL_ENV)
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

/// 为每个主库连接应用统一的连接级 PRAGMA。
///
/// - 文件库：`journal_mode=WAL`（持久化在库文件中），读写互不阻塞，提交只需一次 fsync；
///   WAL 下 `synchronous=NORMAL` 仍保证崩溃一致性（断电只可能丢最后几个事务）。
///   若 WAL 不可用（或被环境变量禁用）则保持回滚日志 + `synchronous=FULL`，不降低持久性。
/// - 内存库：journal_mode 固定为 memory，只应用其余设置。
/// - 所有连接：`foreign_keys=ON`、`busy_timeout`、`cache_size`、`temp_store=MEMORY`，
///   以及 [`DB_STATEMENT_CACHE_CAPACITY`] 条预编译语句缓存（PERF-31）。
pub(crate) fn configure_connection(conn: &Connection, file_backed: bool) -> Result<(), AppError> {
    let db_err = |e: rusqlite::Error| AppError::Database(format!("配置数据库连接失败: {e}"));

    conn.busy_timeout(DB_BUSY_TIMEOUT).map_err(db_err)?;
    conn.set_prepared_statement_cache_capacity(DB_STATEMENT_CACHE_CAPACITY);
    conn.execute_batch(&format!(
        "PRAGMA foreign_keys = ON;
         PRAGMA cache_size = -{DB_CACHE_SIZE_KIB};
         PRAGMA temp_store = MEMORY;"
    ))
    .map_err(db_err)?;

    let wal_active = if file_backed {
        let requested = if wal_disabled_by_env() {
            "DELETE"
        } else {
            "WAL"
        };
        match conn.query_row(&format!("PRAGMA journal_mode = {requested};"), [], |row| {
            row.get::<_, String>(0)
        }) {
            Ok(mode) => {
                let active = mode.eq_ignore_ascii_case("wal");
                if requested == "WAL" && !active {
                    log::warn!(
                        "SQLite WAL 模式不可用（当前 journal_mode={mode}），继续使用回滚日志"
                    );
                }
                active
            }
            Err(e) => {
                log::warn!("设置 SQLite journal_mode={requested} 失败，保持默认: {e}");
                false
            }
        }
    } else {
        false
    };

    let synchronous = if wal_active { "NORMAL" } else { "FULL" };
    conn.execute_batch(&format!("PRAGMA synchronous = {synchronous};"))
        .map_err(db_err)?;
    if wal_active {
        conn.execute_batch(&format!(
            "PRAGMA journal_size_limit = {DB_JOURNAL_SIZE_LIMIT_BYTES};"
        ))
        .map_err(db_err)?;
    }
    Ok(())
}

/// 安全地序列化 JSON，避免 unwrap panic
pub(crate) fn to_json_string<T: Serialize>(value: &T) -> Result<String, AppError> {
    serde_json::to_string(value)
        .map_err(|e| AppError::Config(format!("JSON serialization failed: {e}")))
}

/// 安全地获取 Mutex 锁，避免 unwrap panic
macro_rules! lock_conn {
    ($mutex:expr) => {
        $mutex
            .lock()
            .map_err(|e| AppError::Database(format!("Mutex lock failed: {}", e)))?
    };
}

// 导出宏供子模块使用
pub(crate) use lock_conn;

/// 数据库连接封装
///
/// - `conn`：主连接（读写）。使用 Mutex 包装以支持在多线程环境（如 Tauri State）中共享；
///   rusqlite::Connection 本身不是 Sync 的。绝大多数 DAO 通过 `lock_conn!` 使用它。
/// - 只读连接池（仅文件库 + WAL）：通过 [`Database::read_conn`] 供仪表盘等耗时只读查询使用，
///   WAL 下与主连接上的写入互不阻塞。
/// - 使用量写线程：代理请求的使用量记录经 [`Database::enqueue_usage_write`] 入队，
///   由专用线程批量写入，请求路径不执行 SQLite。
/// - 请求配置快照缓存：见 [`Database::proxy_request_snapshot`]。
pub struct Database {
    pub(crate) conn: Arc<Mutex<Connection>>,
    read_pool: Option<read_pool::ReadPool>,
    usage_writer: usage_writer::UsageWriter,
    request_cache: request_cache::RequestCache,
    /// Serialises read-modify-write of the universal-provider map (DB-16).
    universal_providers_lock: Mutex<()>,
    /// Whether this database feeds the process-wide model capability snapshot
    /// (PROV-33). Only the app database (`Database::init`) does.
    publishes_model_catalog: bool,
}

/// Whether `model_pricing.input_modalities` exists (v24+).
pub(crate) fn schema_has_input_modalities(conn: &Connection) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('model_pricing') WHERE name = 'input_modalities')",
        [],
        |row| row.get(0),
    )
    .map_err(|e| AppError::Database(e.to_string()))
}

fn register_db_change_hook(conn: &Connection, config_generation: Arc<AtomicU64>) {
    conn.update_hook(Some(
        move |action: Action, database: &str, table: &str, _row_id: i64| match action {
            // DB-43: writes to attached scratch schemas (the sync export
            // snapshot) are not changes to the app's data.
            _ if database != "main" => {}
            Action::SQLITE_INSERT | Action::SQLITE_UPDATE | Action::SQLITE_DELETE => {
                if !request_cache::is_volatile_table(table) {
                    config_generation.fetch_add(1, Ordering::AcqRel);
                }
                crate::services::webdav_auto_sync::notify_db_changed(table);
                crate::services::s3_auto_sync::notify_db_changed(table);
            }
            _ => {}
        },
    ));
}

impl Database {
    /// 初始化数据库连接并创建表
    ///
    /// 数据库文件位于 `~/.cc-switch/cc-switch.db`
    pub fn init() -> Result<Self, AppError> {
        let db_path = get_app_config_dir().join("cc-switch.db");
        let db_exists = db_path.exists();

        // 确保父目录存在
        if let Some(parent) = db_path.parent() {
            // SEC-05: ~/.cc-switch is 0700 and the DB 0600 on Unix.
            crate::config::ensure_private_dir(parent)?;
        }
        crate::config::prepare_private_file(&db_path)?;

        let conn = Connection::open(&db_path).map_err(|e| AppError::Database(e.to_string()))?;
        // DB-30: reject a database written by a newer build before ANY pragma
        // or DDL touches it.
        Self::reject_newer_schema(&conn)?;

        if !db_exists {
            // For a brand-new database, configure incremental auto-vacuum
            // before creating any tables (and before switching to WAL) so no
            // rebuild is needed later.
            conn.execute("PRAGMA auto_vacuum = INCREMENTAL;", [])
                .map_err(|e| AppError::Database(e.to_string()))?;
        }
        // 外键、WAL、synchronous、busy_timeout、缓存等连接级设置
        configure_connection(&conn, true)?;

        let mut db = Self::from_connection(conn);
        db.create_tables()?;

        // Pre-migration backup: only when upgrading from an existing database
        {
            let conn = lock_conn!(db.conn);
            let version = Self::get_user_version(&conn)?;
            drop(conn);
            if version > 0 && version < SCHEMA_VERSION {
                log::info!(
                    "Creating pre-migration database backup (v{version} → v{SCHEMA_VERSION})"
                );
                if let Err(e) =
                    db.backup_database_file_of_kind(backup::BackupKind::PreMigration, None)
                {
                    log::warn!("Pre-migration backup failed, continuing migration: {e}");
                }
            }
        }

        db.apply_schema_migrations()?;
        if let Err(e) = db.ensure_incremental_auto_vacuum() {
            log::warn!("Failed to ensure incremental auto-vacuum: {e}");
        }
        db.publishes_model_catalog = true;
        db.ensure_model_pricing_seeded()?;

        // 迁移完成后再打开只读连接，避免它们缓存迁移前的 schema。
        db.read_pool = Self::open_read_pool(&db_path);

        // 日志清理 / rollup+prune / 成本回填 / incremental_vacuum 不在这里同步执行：
        // `periodic_backup_if_needed()` 会在启动后的后台任务里（lib.rs setup 的 async
        // 块）以及每日定时器中执行同一套维护，init() 只做建表、迁移与定价种子，
        // 避免启动耗时随 proxy_request_logs 规模增长（DB-09）。

        Ok(db)
    }

    /// 读取磁盘上数据库的 `user_version`；仅当它比应用支持的 [`SCHEMA_VERSION`]
    /// 更新时返回 `Some(version)`。
    ///
    /// 用于初始化失败后判断是否为「数据库版本过新（应用过旧，需升级应用）」的可恢复
    /// 场景——此时不应反复弹出无效的重试对话框，而应引导用户在应用内升级。
    pub fn stored_user_version_exceeds_supported(
        db_path: &std::path::Path,
    ) -> Result<Option<i32>, AppError> {
        if !db_path.exists() {
            return Ok(None);
        }
        let conn = Connection::open(db_path).map_err(|e| AppError::Database(e.to_string()))?;
        let version = Self::get_user_version(&conn)?;
        Ok((version > SCHEMA_VERSION).then_some(version))
    }

    /// Fail when `conn` holds a schema newer than this build supports (DB-30).
    /// Runs before anything writes to the file.
    pub(crate) fn reject_newer_schema(conn: &Connection) -> Result<(), AppError> {
        let version = Self::get_user_version(conn)?;
        if version > SCHEMA_VERSION {
            return Err(AppError::Database(format!(
                "数据库版本过新（{version}），当前应用仅支持 {SCHEMA_VERSION}，请升级应用后再尝试。"
            )));
        }
        Ok(())
    }

    /// 创建内存数据库（用于测试）
    pub fn memory() -> Result<Self, AppError> {
        let conn = Connection::open_in_memory().map_err(|e| AppError::Database(e.to_string()))?;

        conn.execute("PRAGMA auto_vacuum = INCREMENTAL;", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        configure_connection(&conn, false)?;

        let db = Self::from_connection(conn);
        db.create_tables()?;
        db.ensure_model_pricing_seeded()?;

        Ok(db)
    }

    /// A scratch in-memory database whose writes are not app data changes:
    /// no auto-sync signal and no proxy-cache invalidation (SVC-27 preview).
    pub(crate) fn memory_detached() -> Result<Self, AppError> {
        let conn = Connection::open_in_memory().map_err(|e| AppError::Database(e.to_string()))?;
        configure_connection(&conn, false)?;
        let db = Self::from_connection_with_hook(conn, false);
        db.create_tables()?;
        Ok(db)
    }

    /// 打开（或创建）指定路径的文件数据库，行为与 [`Database::init`] 的连接设置一致，
    /// 并启用只读连接池。主要用于测试与工具。
    #[allow(dead_code)]
    pub(crate) fn open_file(db_path: &Path) -> Result<Self, AppError> {
        let conn = Connection::open(db_path).map_err(|e| AppError::Database(e.to_string()))?;
        Self::reject_newer_schema(&conn)?;
        conn.execute("PRAGMA auto_vacuum = INCREMENTAL;", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        configure_connection(&conn, true)?;
        let mut db = Self::from_connection(conn);
        db.create_tables()?;
        db.apply_schema_migrations()?;
        db.ensure_model_pricing_seeded()?;
        db.read_pool = Self::open_read_pool(db_path);
        Ok(db)
    }

    fn from_connection(conn: Connection) -> Self {
        Self::from_connection_with_hook(conn, true)
    }

    fn from_connection_with_hook(conn: Connection, change_hook: bool) -> Self {
        let request_cache = request_cache::RequestCache::new();
        if change_hook {
            register_db_change_hook(&conn, request_cache.generation_handle());
        }
        let conn = Arc::new(Mutex::new(conn));
        Self {
            usage_writer: usage_writer::UsageWriter::new(Arc::clone(&conn)),
            conn,
            read_pool: None,
            request_cache,
            universal_providers_lock: Mutex::new(()),
            publishes_model_catalog: false,
        }
    }

    fn open_read_pool(db_path: &Path) -> Option<read_pool::ReadPool> {
        match read_pool::ReadPool::open(db_path, read_pool::READ_POOL_SIZE) {
            Ok(pool) => {
                if pool.is_none() {
                    log::info!("数据库未处于 WAL 模式，只读查询继续使用主连接");
                }
                pool
            }
            Err(e) => {
                log::warn!("打开只读连接池失败，只读查询继续使用主连接: {e}");
                None
            }
        }
    }

    /// 获取一个用于**只读**查询的连接。
    ///
    /// 文件库且处于 WAL 模式时返回只读池中的连接，耗时的聚合查询不会阻塞主连接上的
    /// 写入（代理日志、维护任务），也不会被它们阻塞；否则回落到主连接（与
    /// `lock_conn!(self.conn)` 等价）。只读连接看到的是最近一次已提交的数据。
    /// 不要在返回的连接上执行写语句（只读连接会报错）。
    pub(crate) fn read_conn(&self) -> Result<ReadConn<'_>, AppError> {
        match &self.read_pool {
            Some(pool) => pool.get().map(ReadConn::pooled),
            None => Ok(ReadConn::main(lock_conn!(self.conn))),
        }
    }

    /// Run a read-only closure on a read connection (the WAL read pool when
    /// available, else the main connection) (DB-15). Callers outside
    /// `database/` should prefer this (or a DAO method) over locking `conn`.
    pub(crate) fn read<T>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        let conn = self.read_conn()?;
        f(&conn)
    }

    /// Run a closure on the main (read-write) connection (DB-15). For
    /// all-or-nothing multi-statement writes use [`Self::with_transaction`].
    pub(crate) fn write<T>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        let conn = lock_conn!(self.conn);
        f(&conn)
    }

    /// Config-change counter bumped by the update hook (tests only).
    #[cfg(test)]
    pub(crate) fn config_generation_for_test(&self) -> u64 {
        self.request_cache
            .generation_handle()
            .load(Ordering::Acquire)
    }

    /// Run `f` inside one write transaction on the main connection (SVC-11).
    ///
    /// The transaction commits only when `f` returns `Ok`; an error (or a
    /// panic) rolls back every statement `f` executed, so multi-row saves are
    /// all-or-nothing and fire a single auto-sync signal batch.
    pub(crate) fn with_transaction<T>(
        &self,
        f: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(format!("开启事务失败: {e}")))?;
        let value = f(&tx)?;
        tx.commit()
            .map_err(|e| AppError::Database(format!("提交事务失败: {e}")))?;
        Ok(value)
    }

    /// 把一个写任务交给使用量写线程（非阻塞；队列满时返回错误并丢弃）。
    pub(crate) fn enqueue_usage_write(&self, job: WriteJob) -> Result<(), AppError> {
        self.usage_writer.enqueue(job)
    }

    /// 同步等待所有已入队的使用量写入落库（测试与退出路径使用）。
    pub fn flush_usage_writes(&self) -> Result<(), AppError> {
        self.usage_writer
            .flush(usage_writer::SHUTDOWN_FLUSH_TIMEOUT)
    }

    /// 使用量写线程的统计计数。
    #[allow(dead_code)]
    pub fn usage_writer_stats(&self) -> UsageWriterStats {
        self.usage_writer.stats()
    }

    pub(crate) fn get_auto_vacuum_mode(conn: &Connection) -> Result<i32, AppError> {
        conn.query_row("PRAGMA auto_vacuum;", [], |row| row.get(0))
            .map_err(|e| AppError::Database(format!("读取 auto_vacuum 失败: {e}")))
    }

    fn has_user_tables(conn: &Connection) -> Result<bool, AppError> {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
                [],
                |row| row.get(0),
            )
            .map_err(|e| AppError::Database(format!("读取表数量失败: {e}")))?;
        Ok(count > 0)
    }

    pub(crate) fn ensure_incremental_auto_vacuum_on_conn(
        conn: &Connection,
    ) -> Result<bool, AppError> {
        let mode = Self::get_auto_vacuum_mode(conn)?;
        if mode == 2 {
            return Ok(false);
        }

        let has_tables = Self::has_user_tables(conn)?;
        conn.execute("PRAGMA auto_vacuum = INCREMENTAL;", [])
            .map_err(|e| AppError::Database(format!("设置 auto_vacuum 失败: {e}")))?;

        if !has_tables {
            return Ok(false);
        }

        conn.execute("VACUUM;", [])
            .map_err(|e| AppError::Database(format!("执行 VACUUM 失败: {e}")))?;
        conn.execute("PRAGMA foreign_keys = ON;", [])
            .map_err(|e| AppError::Database(format!("恢复 foreign_keys 失败: {e}")))?;
        Ok(true)
    }

    pub(crate) fn ensure_incremental_auto_vacuum(&self) -> Result<bool, AppError> {
        let mode = {
            let conn = lock_conn!(self.conn);
            Self::get_auto_vacuum_mode(&conn)?
        };
        if mode == 2 {
            return Ok(false);
        }

        let has_tables = {
            let conn = lock_conn!(self.conn);
            Self::has_user_tables(&conn)?
        };
        if has_tables {
            log::info!(
                "Detected auto_vacuum={mode}, rebuilding database to enable incremental vacuum"
            );
            self.backup_database_file_of_kind(backup::BackupKind::PreMigration, None)?;
        }

        let rebuilt = {
            let conn = lock_conn!(self.conn);
            Self::ensure_incremental_auto_vacuum_on_conn(&conn)?
        };

        if rebuilt {
            log::info!("Incremental auto-vacuum enabled after database rebuild");
        } else {
            log::info!("Incremental auto-vacuum configured for new database");
        }

        Ok(rebuilt)
    }

    /// 检查 MCP 服务器表是否为空
    pub fn is_mcp_table_empty(&self) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM mcp_servers", [], |row| row.get(0))
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(count == 0)
    }

    /// 检查提示词表是否为空
    pub fn is_prompts_table_empty(&self) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM prompts", [], |row| row.get(0))
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(count == 0)
    }
}

#[cfg(test)]
mod connection_pragma_tests {
    use super::*;
    use tempfile::TempDir;

    fn pragma_i64(conn: &Connection, name: &str) -> i64 {
        conn.query_row(&format!("PRAGMA {name};"), [], |row| row.get(0))
            .unwrap_or_else(|e| panic!("read PRAGMA {name}: {e}"))
    }

    fn pragma_string(conn: &Connection, name: &str) -> String {
        conn.query_row(&format!("PRAGMA {name};"), [], |row| row.get(0))
            .unwrap_or_else(|e| panic!("read PRAGMA {name}: {e}"))
    }

    #[test]
    fn file_connection_uses_wal_and_tuned_pragmas() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("pragmas.db");
        let conn = Connection::open(&path).expect("open file db");
        configure_connection(&conn, true).expect("configure connection");

        assert_eq!(pragma_string(&conn, "journal_mode"), "wal");
        // synchronous: 1 = NORMAL
        assert_eq!(pragma_i64(&conn, "synchronous"), 1);
        assert_eq!(pragma_i64(&conn, "busy_timeout"), 5000);
        assert_eq!(pragma_i64(&conn, "foreign_keys"), 1);
        assert_eq!(pragma_i64(&conn, "cache_size"), -DB_CACHE_SIZE_KIB);
        // temp_store: 2 = MEMORY
        assert_eq!(pragma_i64(&conn, "temp_store"), 2);
        assert_eq!(
            pragma_i64(&conn, "journal_size_limit"),
            DB_JOURNAL_SIZE_LIMIT_BYTES
        );
        drop(conn);

        // WAL 持久化在库文件中：重新打开的连接（未调用 configure）仍是 WAL。
        let reopened = Connection::open(&path).expect("reopen file db");
        assert_eq!(pragma_string(&reopened, "journal_mode"), "wal");
    }

    #[test]
    fn memory_connection_applies_pragmas_without_wal() {
        let db = Database::memory().expect("memory db");
        let conn = db.conn.lock().expect("lock");
        assert_eq!(pragma_string(&conn, "journal_mode"), "memory");
        // 非 WAL 时保持 FULL（2），不降低回滚日志模式下的持久性
        assert_eq!(pragma_i64(&conn, "synchronous"), 2);
        assert_eq!(pragma_i64(&conn, "busy_timeout"), 5000);
        assert_eq!(pragma_i64(&conn, "foreign_keys"), 1);
        assert_eq!(pragma_i64(&conn, "cache_size"), -DB_CACHE_SIZE_KIB);
        assert_eq!(pragma_i64(&conn, "temp_store"), 2);
        assert_eq!(
            Database::get_auto_vacuum_mode(&conn).expect("auto_vacuum"),
            2
        );
    }

    #[test]
    fn upgraded_file_db_switches_to_wal_and_can_still_enable_incremental_vacuum() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("legacy.db");
        {
            // 旧版本：回滚日志 + auto_vacuum=NONE，且已有表和数据
            let conn = Connection::open(&path).expect("open legacy db");
            conn.execute_batch("PRAGMA auto_vacuum = NONE;")
                .expect("auto_vacuum none");
            Database::create_tables_on_conn(&conn).expect("create tables");
            Database::apply_schema_migrations_on_conn(&conn).expect("migrate");
            assert_eq!(pragma_string(&conn, "journal_mode"), "delete");
        }

        let conn = Connection::open(&path).expect("reopen legacy db");
        configure_connection(&conn, true).expect("configure connection");
        assert_eq!(pragma_string(&conn, "journal_mode"), "wal");

        // 启用 WAL 后仍可通过一次 VACUUM 切换到 INCREMENTAL auto_vacuum
        let rebuilt =
            Database::ensure_incremental_auto_vacuum_on_conn(&conn).expect("enable incremental");
        assert!(rebuilt);
        assert_eq!(
            Database::get_auto_vacuum_mode(&conn).expect("auto_vacuum"),
            2
        );
        assert_eq!(pragma_string(&conn, "journal_mode"), "wal");
        assert_eq!(
            Database::get_user_version(&conn).expect("user_version"),
            SCHEMA_VERSION
        );
    }
}

#[cfg(test)]
mod read_path_tests {
    use super::*;
    use std::sync::mpsc;
    use tempfile::TempDir;

    #[test]
    fn with_transaction_commits_on_ok_and_rolls_back_on_error() {
        let db = Database::memory().expect("memory db");
        db.with_transaction(|tx| {
            tx.execute(
                "INSERT INTO settings (key, value) VALUES ('tx-ok', '1')",
                [],
            )?;
            Ok(())
        })
        .expect("committed");
        let err = db
            .with_transaction(|tx| {
                tx.execute("INSERT INTO settings (key, value) VALUES ('tx-a', '1')", [])?;
                tx.execute("INSERT INTO settings (key, value) VALUES ('tx-b', '1')", [])?;
                Err::<(), _>(AppError::Message("injected".into()))
            })
            .expect_err("closure error propagates");
        assert!(err.to_string().contains("injected"));
        let conn = db.conn.lock().unwrap();
        let keys: Vec<String> = conn
            .prepare("SELECT key FROM settings WHERE key LIKE 'tx-%' ORDER BY key")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(keys, vec!["tx-ok".to_string()]);
    }

    #[test]
    fn memory_db_read_conn_falls_back_to_main_connection() {
        let db = Database::memory().expect("memory db");
        let conn = db.read_conn().expect("read conn");
        assert!(!conn.is_pooled());
    }

    #[test]
    fn file_db_reads_do_not_wait_for_the_main_connection() {
        let dir = TempDir::new().expect("temp dir");
        let db = Arc::new(Database::open_file(&dir.path().join("read.db")).expect("open"));
        db.conn
            .lock()
            .expect("lock")
            .execute(
                "INSERT INTO proxy_request_logs (request_id, provider_id, app_type, model, status_code, latency_ms, created_at)
                 VALUES ('r1', 'p1', 'claude', 'm', 200, 1, 1)",
                [],
            )
            .expect("insert");

        // 主连接被长时间占用（模拟维护任务/写入），只读路径仍立即可用并看到已提交数据
        let main_guard = db.conn.lock().expect("lock main");
        let (tx, rx) = mpsc::channel();
        let reader = Arc::clone(&db);
        let handle = std::thread::spawn(move || {
            let conn = reader.read_conn().expect("read conn");
            assert!(conn.is_pooled());
            let count: i64 = conn
                .query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |r| r.get(0))
                .expect("count");
            // 只读连接拒绝写入
            assert!(conn.execute("DELETE FROM proxy_request_logs", []).is_err());
            drop(conn);
            let summary = reader
                .get_usage_summary(None, None, None, None, None)
                .expect("dashboard summary on read pool");
            tx.send((count, summary.total_requests)).unwrap();
        });
        let (count, total_requests) = rx
            .recv_timeout(Duration::from_secs(10))
            .expect("read path must not block on the main connection");
        drop(main_guard);
        handle.join().unwrap();
        assert_eq!(count, 1);
        assert_eq!(total_requests, 1);
    }

    #[test]
    fn usage_writes_reach_the_file_and_are_visible_to_readers() {
        let dir = TempDir::new().expect("temp dir");
        let db = Database::open_file(&dir.path().join("writer.db")).expect("open");
        for i in 0..20 {
            db.enqueue_usage_write(WriteJob::new("test", move |conn| {
                conn.prepare_cached(
                    "INSERT INTO proxy_request_logs (request_id, provider_id, app_type, model, status_code, latency_ms, created_at)
                     VALUES (?1, 'p', 'claude', 'm', 200, 1, 1)",
                )
                .and_then(|mut stmt| stmt.execute([format!("w{i}")]))
                .map(|_| ())
                .map_err(|e| AppError::Database(e.to_string()))
            }))
            .expect("enqueue");
        }
        db.flush_usage_writes().expect("flush");
        let count: i64 = db
            .read_conn()
            .expect("read conn")
            .query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |r| r.get(0))
            .expect("count");
        assert_eq!(count, 20);
        assert_eq!(db.usage_writer_stats().written, 20);
    }
}
