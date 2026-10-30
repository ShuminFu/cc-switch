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
use rusqlite::{hooks::Action, Connection};
use serde::Serialize;
use std::sync::Mutex;
use std::time::Duration;

// DAO 方法通过 impl Database 提供，无需额外导出

/// 当前 Schema 版本号
/// 每次修改表结构时递增，并在 schema.rs 中添加相应的迁移逻辑
pub(crate) const SCHEMA_VERSION: i32 = 16;

/// 主库连接的 busy_timeout：其它进程/连接（备份、外部工具）短暂持有写锁时等待而不是立即报错。
pub(crate) const DB_BUSY_TIMEOUT: Duration = Duration::from_secs(5);
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
/// - 所有连接：`foreign_keys=ON`、`busy_timeout`、`cache_size`、`temp_store=MEMORY`。
pub(crate) fn configure_connection(conn: &Connection, file_backed: bool) -> Result<(), AppError> {
    let db_err = |e: rusqlite::Error| AppError::Database(format!("配置数据库连接失败: {e}"));

    conn.busy_timeout(DB_BUSY_TIMEOUT).map_err(db_err)?;
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
/// 使用 Mutex 包装 Connection 以支持在多线程环境（如 Tauri State）中共享。
/// rusqlite::Connection 本身不是 Sync 的，因此需要这层包装。
pub struct Database {
    pub(crate) conn: Mutex<Connection>,
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

impl Database {
    /// 初始化数据库连接并创建表
    ///
    /// 数据库文件位于 `~/.cc-switch/cc-switch.db`
    pub fn init() -> Result<Self, AppError> {
        let db_path = get_app_config_dir().join("cc-switch.db");
        let db_exists = db_path.exists();

        // 确保父目录存在
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        }

        let conn = Connection::open(&db_path).map_err(|e| AppError::Database(e.to_string()))?;

        if !db_exists {
            // For a brand-new database, configure incremental auto-vacuum
            // before creating any tables (and before switching to WAL) so no
            // rebuild is needed later.
            conn.execute("PRAGMA auto_vacuum = INCREMENTAL;", [])
                .map_err(|e| AppError::Database(e.to_string()))?;
        }
        // 外键、WAL、synchronous、busy_timeout、缓存等连接级设置
        configure_connection(&conn, true)?;
        register_db_change_hook(&conn);

        let db = Self {
            conn: Mutex::new(conn),
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
        db.ensure_model_pricing_seeded()?;

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

    /// 创建内存数据库（用于测试）
    pub fn memory() -> Result<Self, AppError> {
        let conn = Connection::open_in_memory().map_err(|e| AppError::Database(e.to_string()))?;

        conn.execute("PRAGMA auto_vacuum = INCREMENTAL;", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        configure_connection(&conn, false)?;
        register_db_change_hook(&conn);

        let db = Self {
            conn: Mutex::new(conn),
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
