//! v18 -> v19：`proxy_request_logs.superseded_by` 跨源去重标记（CCS-160）
//!
//! - 新增 `superseded_by` 列（覆盖该会话行的代理行 `request_id`，NULL 为有效行）、
//!   部分索引、`usage_backfill_state` 回填状态表与维护触发器；
//! - 已有行不在迁移里回填（大表会阻塞启动），由启动后的后台任务分块完成，
//!   完成前查询仍用 `EXISTS`，见 [`crate::database::repo::superseded`]。
//!
//! 只新增可为 NULL 的列、索引、表与触发器，已有列的含义不变；较旧的构建不会打开
//! v19 数据库（见 docs/dev/DB_COMPATIBILITY.md），因此属于增量变更，不升级 `DB_COMPAT_VERSION`。

use crate::error::AppError;
use rusqlite::Connection;

pub(super) fn apply(conn: &Connection) -> Result<(), AppError> {
    crate::database::repo::superseded::install(conn)
}
