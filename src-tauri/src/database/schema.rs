//! Schema 辅助方法与模型定价入口
//!
//! 表结构由 `migrations/` 中的版本化迁移步骤创建和演进；这里只保留迁移步骤
//! 共用的辅助方法（user_version、表/列探测、补列）。内置定价数据与 seed / 修复
//! 逻辑在 [`crate::pricing::catalog`]。

use super::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::{params, Connection};

impl Database {
    /// 确保模型定价表具备默认数据。仅在启动 / 恢复数据库时调用，读取定价列表时不再调用，
    /// 否则会把用户刚删除的内置定价又插回来。
    pub fn ensure_model_pricing_seeded(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        Self::ensure_model_pricing_seeded_on_conn(&conn)
    }

    pub(crate) fn ensure_model_pricing_seeded_on_conn(conn: &Connection) -> Result<(), AppError> {
        // 每次启动都执行 INSERT OR IGNORE，增量追加新模型（墓碑行不会被恢复）；
        // 只把来源为 builtin 的行更新到当前目录价，用户改过的行不动。
        crate::pricing::catalog::sync(conn)
    }

    /// 删除模型定价。内置定价写墓碑（`deleted = 1`），之后的目录同步与重启都不会恢复它；
    /// 同时仍在 settings 中记录删除标记，供只认识该标记的旧版本使用。自定义定价直接删除。
    pub fn delete_model_pricing(&self, model_id: &str) -> Result<(), AppError> {
        if crate::pricing::catalog::is_builtin(model_id) {
            crate::settings::set_builtin_model_pricing_deleted(model_id, true)?;
        }
        let conn = lock_conn!(self.conn);
        if crate::pricing::catalog::tombstone(&conn, model_id)? {
            return Ok(());
        }
        conn.execute(
            "DELETE FROM model_pricing WHERE model_id = ?1",
            params![model_id],
        )
        .map_err(|e| AppError::Database(format!("删除模型定价失败: {e}")))?;
        Ok(())
    }

    /// 保存用户新增或编辑的模型定价：行标记为 `user`，之后的目录更新不再覆盖；
    /// 覆盖同名墓碑（用户重新添加被删除过的内置定价），并清除 settings 中的删除标记。
    pub fn save_user_model_pricing(
        &self,
        model_id: &str,
        display_name: &str,
        price: crate::pricing::catalog::CatalogPrice,
    ) -> Result<(), AppError> {
        crate::settings::set_builtin_model_pricing_deleted(model_id, false)?;
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO model_pricing (
                model_id, display_name, input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million,
                source, catalog_version, updated_at, deleted
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8, 0)",
            params![
                model_id,
                display_name,
                price.input,
                price.output,
                price.cache_read,
                price.cache_creation,
                crate::pricing::catalog::SOURCE_USER,
                chrono::Utc::now().timestamp()
            ],
        )
        .map_err(|e| AppError::Database(format!("更新模型定价失败: {e}")))?;
        Ok(())
    }

    // --- 辅助方法 ---

    pub(crate) fn get_user_version(conn: &Connection) -> Result<i32, AppError> {
        conn.query_row("PRAGMA user_version;", [], |row| row.get(0))
            .map_err(|e| AppError::Database(format!("读取 user_version 失败: {e}")))
    }

    pub(crate) fn set_user_version(conn: &Connection, version: i32) -> Result<(), AppError> {
        if version < 0 {
            return Err(AppError::Database("user_version 不能为负数".to_string()));
        }
        let sql = format!("PRAGMA user_version = {version};");
        conn.execute(&sql, [])
            .map_err(|e| AppError::Database(format!("写入 user_version 失败: {e}")))?;
        Ok(())
    }

    pub(super) fn create_request_logs_usage_indexes_if_supported(
        conn: &Connection,
    ) -> Result<(), AppError> {
        if !Self::table_exists(conn, "proxy_request_logs")? {
            return Ok(());
        }

        let has_app_type = Self::has_column(conn, "proxy_request_logs", "app_type")?;
        let has_created_at = Self::has_column(conn, "proxy_request_logs", "created_at")?;
        if has_app_type && has_created_at {
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_request_logs_app_created_at
                 ON proxy_request_logs(app_type, created_at DESC)",
                [],
            )
            .map_err(|e| AppError::Database(format!("创建使用量应用时间索引失败: {e}")))?;
        }

        let required_columns = [
            "app_type",
            "data_source",
            "input_tokens",
            "output_tokens",
            "cache_read_tokens",
            "created_at",
            "cache_creation_tokens",
        ];
        for column in required_columns {
            if !Self::has_column(conn, "proxy_request_logs", column)? {
                return Ok(());
            }
        }

        conn.execute("DROP INDEX IF EXISTS idx_request_logs_dedup_lookup", [])
            .map_err(|e| AppError::Database(format!("删除旧使用量去重索引失败: {e}")))?;

        // 查询层为了兼容历史 NULL data_source 行，会使用
        // COALESCE(data_source, 'proxy')。普通 data_source 索引无法匹配该表达式，
        // 会让跨源去重子查询退化成大量扫描；表达式索引让 SQLite 能按同一表达式查找。
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_logs_dedup_lookup_expr
             ON proxy_request_logs(app_type, COALESCE(data_source, 'proxy'), input_tokens,
                                   output_tokens, cache_read_tokens, created_at,
                                   cache_creation_tokens)",
            [],
        )
        .map_err(|e| AppError::Database(format!("创建使用量去重表达式索引失败: {e}")))?;
        Ok(())
    }

    fn validate_identifier(s: &str, kind: &str) -> Result<(), AppError> {
        if s.is_empty() {
            return Err(AppError::Database(format!("{kind} 不能为空")));
        }
        if !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(AppError::Database(format!(
                "非法{kind}: {s}，仅允许字母、数字和下划线"
            )));
        }
        Ok(())
    }

    pub(crate) fn table_exists(conn: &Connection, table: &str) -> Result<bool, AppError> {
        Self::validate_identifier(table, "表名")?;

        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table'")
            .map_err(|e| AppError::Database(format!("读取表名失败: {e}")))?;
        let mut rows = stmt
            .query([])
            .map_err(|e| AppError::Database(format!("查询表名失败: {e}")))?;
        while let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
            let name: String = row
                .get(0)
                .map_err(|e| AppError::Database(format!("解析表名失败: {e}")))?;
            if name.eq_ignore_ascii_case(table) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(crate) fn has_column(
        conn: &Connection,
        table: &str,
        column: &str,
    ) -> Result<bool, AppError> {
        Self::validate_identifier(table, "表名")?;
        Self::validate_identifier(column, "列名")?;

        let sql = format!("PRAGMA table_info(\"{table}\");");
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| AppError::Database(format!("读取表结构失败: {e}")))?;
        let mut rows = stmt
            .query([])
            .map_err(|e| AppError::Database(format!("查询表结构失败: {e}")))?;
        while let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
            let name: String = row
                .get(1)
                .map_err(|e| AppError::Database(format!("读取列名失败: {e}")))?;
            if name.eq_ignore_ascii_case(column) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(super) fn add_column_if_missing(
        conn: &Connection,
        table: &str,
        column: &str,
        definition: &str,
    ) -> Result<bool, AppError> {
        Self::validate_identifier(table, "表名")?;
        Self::validate_identifier(column, "列名")?;

        if !Self::table_exists(conn, table)? {
            return Err(AppError::Database(format!(
                "表 {table} 不存在，无法添加列 {column}"
            )));
        }
        if Self::has_column(conn, table, column)? {
            return Ok(false);
        }

        let sql = format!("ALTER TABLE \"{table}\" ADD COLUMN \"{column}\" {definition};");
        conn.execute(&sql, [])
            .map_err(|e| AppError::Database(format!("为表 {table} 添加列 {column} 失败: {e}")))?;
        log::info!("已为表 {table} 添加缺失列 {column}");
        Ok(true)
    }
}
