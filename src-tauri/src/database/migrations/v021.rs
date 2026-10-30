//! v20 -> v21：`proxy_live_backup.raw_snapshot` 接管前 live 文件的原始字节（CCS-152）
//!
//! - 新增可为 NULL 的 `raw_snapshot` 列：接管时与备份一起记录 live 文件的原文，
//!   崩溃恢复 / 关闭接管时据此逐字节还原（保留用户的缩进、注释与键顺序）；
//! - `save_live_backup` 用 `INSERT OR REPLACE` 整行重写，备份一旦被热切换等路径
//!   改写，快照随之清空，恢复回退为按备份 JSON 重写。
//!
//! 只新增可为 NULL 的列，已有列的含义不变；`proxy_live_backup` 不参与同步导出，
//! 因此属于增量变更，不升级 `DB_COMPAT_VERSION`。

use crate::database::Database;
use crate::error::AppError;
use rusqlite::Connection;

pub(super) fn apply(conn: &Connection) -> Result<(), AppError> {
    // 只含部分表的测试库 / 损坏库没有 proxy_live_backup：与其他步骤一样跳过
    if !Database::table_exists(conn, "proxy_live_backup")? {
        return Ok(());
    }
    Database::add_column_if_missing(conn, "proxy_live_backup", "raw_snapshot", "TEXT")?;
    Ok(())
}
