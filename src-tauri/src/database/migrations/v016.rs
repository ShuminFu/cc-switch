//! v15 -> v16：把原先每次启动都由 `create_tables` 执行的修复改为编号步骤
//!
//! v15 及更早的应用在每次启动时用 `create_tables` 补建索引、修复开发版数据，
//! 错误被吞掉。v16 起这些修复只在这一步执行一次，错误向上传播：
//! - 使用量查询索引（v8 引入，但 v8/v9 发布版尚未创建）；
//! - 额度窗口索引 `idx_request_logs_provider_created_at`（CCS-095，v15 之后新增）；
//! - 未发布开发版遗留的全局 `current_profile_id` 标记改为按应用分组；
//! - proxy_config 每应用默认行。
//!
//! 全部为增量变更，不影响 `DB_COMPAT_VERSION`。

use crate::database::Database;
use crate::error::AppError;
use rusqlite::Connection;

pub(super) fn apply(conn: &Connection) -> Result<(), AppError> {
    if Database::table_exists(conn, "proxy_request_logs")? {
        Database::create_request_logs_usage_indexes_if_supported(conn)?;
        // 额度检查按 provider 查 [start, end) 时间窗：三列索引让 created_at 范围走索引 seek
        let mut has_window_columns = true;
        for column in ["provider_id", "app_type", "created_at"] {
            has_window_columns &= Database::has_column(conn, "proxy_request_logs", column)?;
        }
        if has_window_columns {
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_request_logs_provider_created_at
                 ON proxy_request_logs(provider_id, app_type, created_at)",
                [],
            )
            .map_err(|e| AppError::Database(format!("创建额度窗口索引失败: {e}")))?;
        }
    }

    // 修复跑过未发布开发版的库：current 标记曾是全局 key，现按应用分组
    if Database::table_exists(conn, "settings")? {
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value)
             SELECT 'current_profile_id_claude', value FROM settings
             WHERE key = 'current_profile_id'",
            [],
        )
        .map_err(|e| AppError::Database(format!("迁移全局 profile 标记失败: {e}")))?;
        conn.execute("DELETE FROM settings WHERE key = 'current_profile_id'", [])
            .map_err(|e| AppError::Database(format!("删除全局 profile 标记失败: {e}")))?;
    }

    if Database::table_exists(conn, "proxy_config")?
        && Database::has_column(conn, "proxy_config", "app_type")?
    {
        Database::seed_proxy_config_rows(conn)?;
    }

    Ok(())
}
