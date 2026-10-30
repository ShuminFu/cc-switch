//! v16 -> v17：MCP / Skill 应用启用状态改用关联表，并去掉 proxy_config 的 app_type CHECK
//!
//! - 新增 `mcp_server_apps(server_id, app_type)` 与 `skill_apps(skill_id, app_type)`，
//!   每个启用的应用一行；从旧的 `enabled_<slug>` 列回填。新增应用不再需要加列。
//! - 旧的 `enabled_<slug>` 列保留，DAO 继续双写一个版本（混合版本同步、降级时仍可读）。
//! - `proxy_config` 按原列重建，去掉 `CHECK (app_type IN (...))`，新应用可以有自己的行。
//!
//! 只新增表并放宽约束，列与取值含义不变，较旧的同代构建仍能读取旧列，
//! 因此按 docs/dev/DB_COMPATIBILITY.md 属于增量变更，不升级 `DB_COMPAT_VERSION`。

use crate::database::Database;
use crate::error::AppError;
use rusqlite::Connection;

/// v17 时主表上已有的每应用旧列（`enabled_<slug>`），用于一次性回填
const BACKFILL_APPS: &[&str] = &[
    "claude",
    "codex",
    "gemini",
    "grokbuild",
    "opencode",
    "hermes",
];

/// proxy_config 的全部列（顺序与 v14 重建后的结构一致）
const PROXY_CONFIG_COLUMNS: &[&str] = &[
    "app_type",
    "proxy_enabled",
    "listen_address",
    "listen_port",
    "enable_logging",
    "enabled",
    "auto_failover_enabled",
    "max_retries",
    "streaming_first_byte_timeout",
    "streaming_idle_timeout",
    "non_streaming_timeout",
    "circuit_failure_threshold",
    "circuit_success_threshold",
    "circuit_timeout_seconds",
    "circuit_error_rate_threshold",
    "circuit_min_requests",
    "default_cost_multiplier",
    "pricing_model_source",
    "live_takeover_active",
    "created_at",
    "updated_at",
];

pub(super) fn apply(conn: &Connection) -> Result<(), AppError> {
    create_join_table(conn, "mcp_server_apps", "server_id", "mcp_servers")?;
    create_join_table(conn, "skill_apps", "skill_id", "skills")?;
    drop_proxy_config_app_check(conn)?;
    Ok(())
}

/// 创建关联表并从主表的 `enabled_<slug>` 列回填（主表或某列不存在时跳过）
fn create_join_table(
    conn: &Connection,
    table: &str,
    owner_column: &str,
    owner_table: &str,
) -> Result<(), AppError> {
    conn.execute(
        &format!(
            "CREATE TABLE IF NOT EXISTS {table} (
                {owner_column} TEXT NOT NULL,
                app_type TEXT NOT NULL,
                PRIMARY KEY ({owner_column}, app_type)
            )"
        ),
        [],
    )
    .map_err(|e| AppError::Database(format!("创建 {table} 表失败: {e}")))?;

    if !Database::table_exists(conn, owner_table)? {
        return Ok(());
    }
    for slug in BACKFILL_APPS {
        let column = format!("enabled_{slug}");
        if !Database::has_column(conn, owner_table, &column)? {
            continue;
        }
        conn.execute(
            &format!(
                "INSERT OR IGNORE INTO {table} ({owner_column}, app_type)
                 SELECT id, ?1 FROM {owner_table} WHERE {column} = 1"
            ),
            [slug],
        )
        .map_err(|e| AppError::Database(format!("回填 {table}（{slug}）失败: {e}")))?;
    }
    Ok(())
}

/// 按原列重建 proxy_config，去掉 app_type 的 CHECK 约束
fn drop_proxy_config_app_check(conn: &Connection) -> Result<(), AppError> {
    if !Database::table_exists(conn, "proxy_config")?
        || !Database::has_column(conn, "proxy_config", "app_type")?
    {
        return Ok(());
    }

    conn.execute("DROP TABLE IF EXISTS proxy_config_v17", [])
        .map_err(|e| AppError::Database(e.to_string()))?;
    conn.execute(
        "CREATE TABLE proxy_config_v17 (
            app_type TEXT PRIMARY KEY,
            proxy_enabled INTEGER NOT NULL DEFAULT 0,
            listen_address TEXT NOT NULL DEFAULT '127.0.0.1',
            listen_port INTEGER NOT NULL DEFAULT 15721,
            enable_logging INTEGER NOT NULL DEFAULT 1,
            enabled INTEGER NOT NULL DEFAULT 0,
            auto_failover_enabled INTEGER NOT NULL DEFAULT 0,
            max_retries INTEGER NOT NULL DEFAULT 3,
            streaming_first_byte_timeout INTEGER NOT NULL DEFAULT 60,
            streaming_idle_timeout INTEGER NOT NULL DEFAULT 120,
            non_streaming_timeout INTEGER NOT NULL DEFAULT 600,
            circuit_failure_threshold INTEGER NOT NULL DEFAULT 4,
            circuit_success_threshold INTEGER NOT NULL DEFAULT 2,
            circuit_timeout_seconds INTEGER NOT NULL DEFAULT 60,
            circuit_error_rate_threshold REAL NOT NULL DEFAULT 0.6,
            circuit_min_requests INTEGER NOT NULL DEFAULT 10,
            default_cost_multiplier TEXT NOT NULL DEFAULT '1',
            pricing_model_source TEXT NOT NULL DEFAULT 'response',
            live_takeover_active INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
        [],
    )
    .map_err(|e| AppError::Database(format!("创建 proxy_config_v17 失败: {e}")))?;

    // 只复制旧表中存在的列，其余取新表默认值
    let mut columns = Vec::new();
    for column in PROXY_CONFIG_COLUMNS {
        if Database::has_column(conn, "proxy_config", column)? {
            columns.push(format!("\"{column}\""));
        }
    }
    let columns = columns.join(", ");
    conn.execute(
        &format!("INSERT INTO proxy_config_v17 ({columns}) SELECT {columns} FROM proxy_config"),
        [],
    )
    .map_err(|e| AppError::Database(format!("复制 proxy_config 失败: {e}")))?;

    conn.execute("DROP TABLE proxy_config", [])
        .map_err(|e| AppError::Database(e.to_string()))?;
    conn.execute("ALTER TABLE proxy_config_v17 RENAME TO proxy_config", [])
        .map_err(|e| AppError::Database(e.to_string()))?;
    Ok(())
}
