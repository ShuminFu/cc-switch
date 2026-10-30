//! v17 -> v18：模型定价来源（provenance）与删除墓碑（CCS-158）
//!
//! - `model_pricing` 新增 `source`（`builtin` / `user`，默认 `user`）、`catalog_version`、
//!   `updated_at` 与 `deleted`（墓碑，默认 0）四列；
//! - 已有行按内置目录标注来源：仍等于目录当前价或旧价的内置行为 `builtin`，
//!   其余（改过价格的内置行、自定义模型）为 `user`，之后的目录更新不再覆盖它们。
//!
//! settings 中 P1 的删除标记由启动时的目录同步并入墓碑（见 [`crate::pricing::catalog`]），
//! 这样较旧设备上传的 v17 快照被导入并重新迁移后，墓碑也能恢复。
//!
//! 只新增带默认值的列，已有列的含义不变；较旧的构建不会打开 v18 数据库
//! （见 docs/dev/DB_COMPATIBILITY.md），因此属于增量变更，不升级 `DB_COMPAT_VERSION`。

use crate::database::Database;
use crate::error::AppError;
use rusqlite::Connection;

pub(super) fn apply(conn: &Connection) -> Result<(), AppError> {
    // 只含部分表的测试库 / 损坏库没有 model_pricing：与其他步骤一样跳过
    if !Database::table_exists(conn, "model_pricing")? {
        return Ok(());
    }
    Database::add_column_if_missing(
        conn,
        "model_pricing",
        "source",
        "TEXT NOT NULL DEFAULT 'user'",
    )?;
    Database::add_column_if_missing(conn, "model_pricing", "catalog_version", "INTEGER")?;
    Database::add_column_if_missing(conn, "model_pricing", "updated_at", "INTEGER")?;
    Database::add_column_if_missing(
        conn,
        "model_pricing",
        "deleted",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    crate::pricing::catalog::classify_provenance(conn)
}
