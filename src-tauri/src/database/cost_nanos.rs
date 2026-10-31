//! Exact integer costs (DB-14).
//!
//! Costs are written as decimal TEXT (`rust_decimal` at the API edge) and used
//! to be aggregated with `SUM(CAST(total_cost_usd AS REAL))`, accumulating
//! binary-float error over thousands of rows. Since schema v26,
//! `proxy_request_logs` and `usage_daily_rollups` carry
//! `total_cost_nanos INTEGER` (USD × 10⁹) and aggregates sum that integer.
//!
//! The column is maintained by the app's own triggers from the TEXT value,
//! so every writer (proxy logger, session import, cost backfill, rollups) is
//! covered without a dual-write in each of them, and the TEXT column stays
//! the source of truth (and the fallback for readers). Triggers are not part
//! of SQL exports; [`ensure_cost_nanos_triggers`] recreates them after an
//! import and recomputes the column when they were missing, so rows imported
//! from a dump (or written by an older build) are never left stale.

use crate::error::AppError;
use rusqlite::Connection;

/// Tables carrying `total_cost_usd` + `total_cost_nanos`.
pub(crate) const COST_NANOS_TABLES: &[&str] = &["proxy_request_logs", "usage_daily_rollups"];

/// SQL turning the decimal TEXT `col` into integer nano-dollars without a
/// REAL round-trip: plain decimals are split at the point (fraction padded,
/// or truncated, to 9 digits). Signed or exponent forms (older
/// `CAST(REAL AS TEXT)` rollups) fall back to rounding through REAL.
pub(crate) fn cost_nanos_sql(col: &str) -> String {
    format!(
        "(CASE
            WHEN {col} IS NULL OR {col} = '' THEN 0
            WHEN {col} GLOB '*[eE]*' OR {col} GLOB '[-+]*'
                THEN CAST(ROUND(CAST({col} AS REAL) * 1000000000) AS INTEGER)
            WHEN instr({col}, '.') = 0 THEN CAST({col} AS INTEGER) * 1000000000
            ELSE CAST(substr({col}, 1, instr({col}, '.') - 1) AS INTEGER) * 1000000000
                + CAST(substr(substr({col}, instr({col}, '.') + 1) || '000000000', 1, 9) AS INTEGER)
        END)"
    )
}

/// SQL rendering integer nano-dollars `expr` as exact decimal TEXT with 9
/// fraction digits, e.g. `425730` → `0.000425730`.
pub(crate) fn nanos_to_decimal_text_sql(expr: &str) -> String {
    format!(
        "(CASE WHEN {expr} < 0 THEN '-' ELSE '' END
          || printf('%d.%09d', abs({expr}) / 1000000000, abs({expr}) % 1000000000))"
    )
}

fn has_column(conn: &Connection, table: &str, column: &str) -> Result<bool, AppError> {
    conn.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?1)"),
        [column],
        |row| row.get(0),
    )
    .map_err(|e| AppError::Database(e.to_string()))
}

fn trigger_exists(conn: &Connection, name: &str) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'trigger' AND name = ?1)",
        [name],
        |row| row.get(0),
    )
    .map_err(|e| AppError::Database(e.to_string()))
}

/// Recompute `total_cost_nanos` for every row of `table`.
pub(crate) fn backfill_cost_nanos(conn: &Connection, table: &str) -> Result<usize, AppError> {
    conn.execute(
        &format!(
            "UPDATE {table} SET total_cost_nanos = {}",
            cost_nanos_sql("total_cost_usd")
        ),
        [],
    )
    .map_err(|e| AppError::Database(format!("回填 {table}.total_cost_nanos 失败: {e}")))
}

/// Create the maintenance triggers on every table that has the column. When a
/// trigger was missing (fresh v26 migration, or an SQL import, which drops all
/// triggers), the column is recomputed for the whole table first.
pub(crate) fn ensure_cost_nanos_triggers(conn: &Connection) -> Result<(), AppError> {
    for table in COST_NANOS_TABLES {
        if !has_column(conn, table, "total_cost_nanos")? {
            continue;
        }
        let insert = format!("trg_{table}_cost_nanos_insert");
        let update = format!("trg_{table}_cost_nanos_update");
        if trigger_exists(conn, &insert)? && trigger_exists(conn, &update)? {
            continue;
        }
        let expr = cost_nanos_sql("NEW.total_cost_usd");
        conn.execute_batch(&format!(
            "CREATE TRIGGER IF NOT EXISTS {insert} AFTER INSERT ON {table}
             BEGIN
                 UPDATE {table} SET total_cost_nanos = {expr} WHERE rowid = NEW.rowid;
             END;
             CREATE TRIGGER IF NOT EXISTS {update} AFTER UPDATE OF total_cost_usd ON {table}
             BEGIN
                 UPDATE {table} SET total_cost_nanos = {expr} WHERE rowid = NEW.rowid;
             END;"
        ))
        .map_err(|e| AppError::Database(format!("创建 {table} 成本触发器失败: {e}")))?;
        backfill_cost_nanos(conn, table)?;
    }
    Ok(())
}
