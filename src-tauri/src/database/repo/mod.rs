//! 仓库层：应用数据库里与使用量相关的 SQL 都在这里
//!
//! - [`usage`]：`proxy_request_logs` 的写入、Dashboard 查询与 [`usage::UsageQuery`]
//! - [`pricing`]：`model_pricing` 表的读写
//! - [`session_cursor`]：`session_log_sync` 导入游标
//!
//! `database/` 之外不写本应用数据库的 SQL（见 `tests::no_sql_outside_database`）。

pub(crate) mod pricing;
pub(crate) mod session_cursor;
pub(crate) mod usage;

#[cfg(test)]
mod tests;
