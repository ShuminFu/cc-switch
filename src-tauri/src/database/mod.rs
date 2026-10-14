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
//! ```text
//! database/
//! ├── mod.rs        - Database 结构体 + 初始化
//! ├── schema.rs     - 表结构定义 + Schema 迁移
//! ├── backup.rs     - SQL 导入导出 + 快照备份
//! ├── migration.rs  - JSON → SQLite 数据迁移
//! └── dao/          - 数据访问对象
//!     ├── providers.rs
//!     ├── mcp.rs
//!     ├── prompts.rs
//!     ├── skills.rs
//!     └── settings.rs
//! ```

pub(crate) mod backup;
mod dao;
mod migration;
mod schema;

#[cfg(test)]
mod tests;

// DAO 类型导出供外部使用
pub(crate) use dao::providers_seed::{
    is_official_seed_id, CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID, CODEX_OFFICIAL_PROVIDER_ID,
};
pub(crate) use dao::proxy::{
    validate_cost_multiplier, validate_pricing_source, PRICING_SOURCE_REQUEST,
    PRICING_SOURCE_RESPONSE,
};
pub use dao::FailoverQueueItem;
pub use dao::Profile;

use crate::config::get_app_config_dir;
use crate::error::AppError;
use crate::proxy::usage::logger::UsageLogEntry;
use crate::proxy::usage::writer::{self, Enqueued, UsageLogWriter};
use rusqlite::{hooks::Action, Connection, OpenFlags};
use serde::Serialize;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

// DAO 方法通过 impl Database 提供，无需额外导出

/// 当前 Schema 版本号
/// 每次修改表结构时递增，并在 schema.rs 中添加相应的迁移逻辑
pub(crate) const SCHEMA_VERSION: i32 = 15;

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

/// How long a connection waits on a locked database before giving up with
/// `SQLITE_BUSY`. With WAL readers never block the writer, but two writers
/// (e.g. another process, or a checkpoint) can still briefly contend.
pub(crate) const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Per-connection settings for connections to the on-disk database.
fn configure_file_connection(conn: &Connection) -> Result<(), AppError> {
    conn.busy_timeout(BUSY_TIMEOUT)
        .map_err(|e| AppError::Database(format!("设置 busy_timeout 失败: {e}")))
}

/// Switch the on-disk database to WAL journaling so readers no longer block
/// the writer (and vice versa). `journal_mode = WAL` is persistent in the file;
/// `synchronous = NORMAL` is per-connection and is the recommended pairing for
/// WAL (durable across application crashes, may lose the last transactions on
/// power loss, never corrupts).
///
/// Returns whether WAL is active. Some file systems (e.g. certain network
/// mounts) can't do WAL; we then stay on the rollback journal and keep the
/// default `synchronous = FULL`.
pub(crate) fn enable_wal(conn: &Connection) -> Result<bool, AppError> {
    let mode: String = conn
        .query_row("PRAGMA journal_mode = WAL;", [], |row| row.get(0))
        .map_err(|e| AppError::Database(format!("启用 WAL 失败: {e}")))?;
    if !mode.eq_ignore_ascii_case("wal") {
        log::warn!("SQLite WAL 不可用，继续使用 journal_mode={mode}");
        return Ok(false);
    }
    conn.execute_batch("PRAGMA synchronous = NORMAL;")
        .map_err(|e| AppError::Database(format!("设置 synchronous 失败: {e}")))?;
    Ok(true)
}

/// 数据库连接封装
///
/// 使用 Mutex 包装 Connection 以支持在多线程环境（如 Tauri State）中共享。
/// rusqlite::Connection 本身不是 Sync 的，因此需要这层包装。
pub struct Database {
    /// Background writer for proxy usage logs, started on first use. Declared
    /// first so it is dropped (draining its queue into `conn`) before the
    /// other fields. `Some(None)` means the thread could not be spawned and
    /// logs are written synchronously instead.
    usage_writer: OnceLock<Option<UsageLogWriter>>,
    /// The writer connection. Shared (`Arc`) with the usage-log writer thread.
    pub(crate) conn: Arc<Mutex<Connection>>,
    /// Read-only connections for heavy, read-only queries (usage dashboard,
    /// request logs). `None` for in-memory databases, which can't be shared
    /// between connections; [`Database::read_conn`] then falls back to `conn`.
    read_pool: Option<ReadPool>,
}

/// Number of read-only connections opened next to the writer connection. The
/// dashboard fires a handful of stats queries at once; two lets them overlap
/// without holding many file handles.
const READ_POOL_SIZE: usize = 2;

/// A tiny fixed-size pool of read-only connections to the WAL database.
///
/// Under WAL, readers see the last committed state as of the start of their
/// statement and never block (nor are blocked by) the writer connection.
struct ReadPool {
    conns: Vec<Mutex<Connection>>,
    next: AtomicUsize,
}

impl ReadPool {
    fn open(db_path: &Path, size: usize) -> Result<Self, AppError> {
        let mut conns = Vec::with_capacity(size);
        for _ in 0..size {
            let conn = Connection::open_with_flags(
                db_path,
                OpenFlags::SQLITE_OPEN_READ_ONLY
                    | OpenFlags::SQLITE_OPEN_URI
                    | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )
            .map_err(|e| AppError::Database(format!("打开只读连接失败: {e}")))?;
            configure_file_connection(&conn)?;
            conn.execute_batch("PRAGMA query_only = ON;")
                .map_err(|e| AppError::Database(format!("设置只读连接失败: {e}")))?;
            conns.push(Mutex::new(conn));
        }
        Ok(Self {
            conns,
            next: AtomicUsize::new(0),
        })
    }

    fn get(&self) -> Result<MutexGuard<'_, Connection>, AppError> {
        // Prefer an idle connection; if all are busy, queue on one in
        // round-robin order. Waiting here never involves the writer's lock.
        for conn in &self.conns {
            if let Ok(guard) = conn.try_lock() {
                return Ok(guard);
            }
        }
        let idx = self.next.fetch_add(1, Ordering::Relaxed) % self.conns.len();
        self.conns[idx]
            .lock()
            .map_err(|e| AppError::Database(format!("Mutex lock failed: {e}")))
    }
}

fn register_db_change_hook(conn: &Connection) {
    conn.update_hook(Some(
        |action: Action, _database: &str, table: &str, _row_id: i64| match action {
            Action::SQLITE_INSERT | Action::SQLITE_UPDATE | Action::SQLITE_DELETE => {
                crate::services::webdav_auto_sync::notify_db_changed(table);
                crate::services::s3_auto_sync::notify_db_changed(table);
            }
            _ => {}
        },
    ));
}

/// Strip group/other permission bits from `path` on Unix (dir → ≤0700,
/// file → ≤0600). The app config dir holds the SQLite DB, its backups and the
/// log file, all of which can contain API keys; with a typical 0022 umask and
/// a world-traversable home directory they would otherwise be readable by
/// other local users. Best effort: failures are logged, never fatal.
pub(crate) fn restrict_to_owner(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let Ok(meta) = std::fs::metadata(path) else {
            return;
        };
        let mode = meta.permissions().mode();
        let restricted = mode & !0o077;
        if restricted != mode {
            if let Err(e) =
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(restricted))
            {
                log::warn!("无法收紧权限 {}: {e}", path.display());
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

impl Database {
    /// 初始化数据库连接并创建表
    ///
    /// 数据库文件位于 `~/.cc-switch/cc-switch.db`
    pub fn init() -> Result<Self, AppError> {
        let db_path = get_app_config_dir().join("cc-switch.db");

        // 确保父目录存在
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        }

        // 数据库、备份与日志中包含 API Key，限制为仅当前用户可访问
        // 目录只在默认位置（~/.cc-switch）时收紧，避免改动用户自定义目录的权限
        if crate::app_store::get_app_config_dir_override().is_none() {
            if let Some(parent) = db_path.parent() {
                restrict_to_owner(parent);
            }
        }

        Self::open_at(&db_path)
    }

    /// Open (creating if needed), migrate and configure the database file at
    /// `db_path`. [`Database::init`] calls this with the app's default path.
    pub(crate) fn open_at(db_path: &Path) -> Result<Self, AppError> {
        let db_exists = db_path.exists();

        let conn = Connection::open(db_path).map_err(|e| AppError::Database(e.to_string()))?;
        configure_file_connection(&conn)?;

        restrict_to_owner(db_path);

        // 启用外键约束
        conn.execute("PRAGMA foreign_keys = ON;", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        if !db_exists {
            // For a brand-new database, configure incremental auto-vacuum
            // before creating any tables so no rebuild is needed later.
            conn.execute("PRAGMA auto_vacuum = INCREMENTAL;", [])
                .map_err(|e| AppError::Database(e.to_string()))?;
        }
        register_db_change_hook(&conn);

        let mut db = Self {
            usage_writer: OnceLock::new(),
            conn: Arc::new(Mutex::new(conn)),
            read_pool: None,
        };
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
                if let Err(e) = db.backup_database_file() {
                    log::warn!("Pre-migration backup failed, continuing migration: {e}");
                }
            }
        }

        db.apply_schema_migrations()?;
        if let Err(e) = db.ensure_incremental_auto_vacuum() {
            log::warn!("Failed to ensure incremental auto-vacuum: {e}");
        }

        // Switch to WAL only after the (one-time, legacy) auto-vacuum rebuild
        // above, so that rebuild keeps running exactly as it always has. On
        // every later start the file is already in WAL mode and this only
        // re-applies the per-connection `synchronous` setting.
        {
            let conn = lock_conn!(db.conn);
            enable_wal(&conn)?;
        }

        db.ensure_model_pricing_seeded()?;

        // Startup cleanup: prune old logs and reclaim space
        if let Err(e) = db.cleanup_old_stream_check_logs(7) {
            log::warn!("Startup stream_check_logs cleanup failed: {e}");
        }
        if let Err(e) = db.rollup_and_prune(30) {
            log::warn!("Startup rollup_and_prune failed: {e}");
        }
        // Reclaim disk space after cleanup
        let wal = {
            let conn = lock_conn!(db.conn);
            if let Err(e) = conn.execute_batch("PRAGMA incremental_vacuum;") {
                log::warn!("Startup incremental vacuum failed: {e}");
            }
            Self::is_wal(&conn)?
        };

        // Readers on separate connections only help (and are only safe to
        // open read-only) once the file is in WAL mode and fully migrated.
        if wal {
            match ReadPool::open(db_path, READ_POOL_SIZE) {
                Ok(pool) => db.read_pool = Some(pool),
                Err(e) => log::warn!("只读连接池不可用，统计查询将使用主连接: {e}"),
            }
        }

        Ok(db)
    }

    fn is_wal(conn: &Connection) -> Result<bool, AppError> {
        let mode: String = conn
            .query_row("PRAGMA journal_mode;", [], |row| row.get(0))
            .map_err(|e| AppError::Database(format!("读取 journal_mode 失败: {e}")))?;
        Ok(mode.eq_ignore_ascii_case("wal"))
    }

    /// Lock a connection for read-only queries that don't need to observe
    /// uncommitted writes (usage stats, trends, request logs).
    ///
    /// On the file database this is one of the read-only WAL connections, so
    /// a long aggregate query neither waits for nor blocks the writer
    /// connection (`self.conn`). It sees everything committed before the
    /// statement starts. For in-memory databases it is the main connection.
    ///
    /// Never write through the returned connection (it is opened read-only
    /// and `query_only`), and never use it for reads that must see the
    /// effects of a transaction still open on `self.conn`.
    pub(crate) fn read_conn(&self) -> Result<MutexGuard<'_, Connection>, AppError> {
        match &self.read_pool {
            Some(pool) => pool.get(),
            None => Ok(lock_conn!(self.conn)),
        }
    }

    /// Fold the WAL back into the main database file and truncate it, so the
    /// `.db` file on disk is self-contained (e.g. before the app exits or the
    /// user copies the config directory by hand). Best effort: a checkpoint
    /// that can't complete because a reader is active is not an error; the WAL
    /// is still valid and SQLite recovers from it on the next open.
    ///
    /// No-op for in-memory / non-WAL databases.
    pub fn checkpoint_wal(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        Self::checkpoint_wal_on_conn(&conn)
    }

    pub(crate) fn checkpoint_wal_on_conn(conn: &Connection) -> Result<(), AppError> {
        if !Self::is_wal(conn)? {
            return Ok(());
        }
        let (busy, _log, _ckpt): (i64, i64, i64) = conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE);", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .map_err(|e| AppError::Database(format!("WAL checkpoint 失败: {e}")))?;
        if busy != 0 {
            log::debug!("WAL checkpoint 未能完全截断（存在活动读者），稍后会自动完成");
        }
        Ok(())
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

    /// 创建内存数据库（用于测试）
    pub fn memory() -> Result<Self, AppError> {
        let conn = Connection::open_in_memory().map_err(|e| AppError::Database(e.to_string()))?;

        // 启用外键约束
        conn.execute("PRAGMA foreign_keys = ON;", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute("PRAGMA auto_vacuum = INCREMENTAL;", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        register_db_change_hook(&conn);

        let db = Self {
            usage_writer: OnceLock::new(),
            conn: Arc::new(Mutex::new(conn)),
            read_pool: None,
        };
        db.create_tables()?;
        db.ensure_model_pricing_seeded()?;

        Ok(db)
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
            self.backup_database_file()?;
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

    /// Queue a proxy usage log for the background writer. Never blocks: if
    /// the queue is full the entry is dropped (with a warning).
    pub(crate) fn enqueue_usage_log(&self, entry: UsageLogEntry) {
        let writer =
            self.usage_writer
                .get_or_init(|| match UsageLogWriter::spawn(Arc::clone(&self.conn)) {
                    Ok(w) => Some(w),
                    Err(e) => {
                        log::error!("[USG-004] 无法启动使用量日志写线程，改为同步写入: {e}");
                        None
                    }
                });
        let entry = match writer {
            Some(writer) => match writer.enqueue(entry) {
                Enqueued::Queued | Enqueued::Dropped => return,
                Enqueued::WriterGone(entry) => *entry,
            },
            None => entry,
        };
        writer::write_batch(&self.conn, vec![entry]);
    }

    /// Wait (up to `timeout`) until every usage log queued so far has been
    /// committed. Returns `false` on timeout. Cheap when nothing was queued.
    pub fn flush_usage_logs(&self, timeout: Duration) -> bool {
        match self.usage_writer.get() {
            Some(Some(writer)) => writer.flush(timeout),
            _ => true,
        }
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
