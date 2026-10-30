//! v19 -> v20：`usage_daily_rollups.total_cost_micros` 整数微美元成本（CCS-161）
//!
//! - 新增 `total_cost_micros INTEGER NOT NULL DEFAULT 0`，按已有 `total_cost_usd`
//!   文本四舍五入到微美元回填；
//! - 之后的汇总以整数相加累计成本，汇总总额与明细行（各自换算为微美元）之和精确相等，
//!   不再有浮点累加误差；`total_cost_usd` 继续写入，取值为微美元的精确十进制文本。
//!
//! 只新增带默认值的列，已有列的取值不变；较旧的构建不会打开 v20 数据库
//! （见 docs/dev/DB_COMPATIBILITY.md），且 rollup 表不参与同步导出，因此属于增量变更，
//! 不升级 `DB_COMPAT_VERSION`。

use crate::database::Database;
use crate::error::AppError;
use crate::services::sql_helpers::cost_micros_sql;
use rusqlite::Connection;

pub(super) fn apply(conn: &Connection) -> Result<(), AppError> {
    // 只含部分表的测试库 / 损坏库没有 usage_daily_rollups：与其他步骤一样跳过
    if !Database::table_exists(conn, "usage_daily_rollups")? {
        return Ok(());
    }
    Database::add_column_if_missing(
        conn,
        "usage_daily_rollups",
        "total_cost_micros",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    // 只含部分列的测试库没有 total_cost_usd：没有可回填的成本
    if !Database::has_column(conn, "usage_daily_rollups", "total_cost_usd")? {
        return Ok(());
    }
    let micros = cost_micros_sql("total_cost_usd");
    conn.execute(
        &format!("UPDATE usage_daily_rollups SET total_cost_micros = {micros}"),
        [],
    )
    .map_err(|e| {
        AppError::Database(format!(
            "回填 usage_daily_rollups.total_cost_micros 失败: {e}"
        ))
    })?;
    Ok(())
}
