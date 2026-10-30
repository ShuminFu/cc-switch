//! 跨源去重标记：`proxy_request_logs.superseded_by`
//!
//! 会话日志行（[`UsageSource::SESSIONS`]）若在时间窗口内有指纹相同的 2xx 代理日志，
//! 就是重复计量，Dashboard 不计入。过去每个查询都用相关 `EXISTS` 子查询现算；
//! 现在把结果存进 `superseded_by`（覆盖它的代理行 `request_id`，NULL 表示有效行）：
//!
//! - 触发器在写入时维护该列（新会话行、新代理行、改动指纹列、删除代理行），
//!   因此所有写入路径（含测试里的原始 INSERT、同步恢复）都无需额外处理；
//! - 升级前已有的行由后台回填补算：按 `request_id` 每次 [`BACKFILL_CHUNK_ROWS`] 行，
//!   每块与游标在同一事务内提交，崩溃后从游标继续；
//! - 回填完成前查询仍回落到 `EXISTS`（[`effective_usage_log_filter`]），结果不变。
//!
//! 触发器不进入 SQL 导出（导入授权不允许 CREATE TRIGGER），导入后由
//! [`ensure_triggers`] 重建；回填状态表与 `proxy_request_logs` 一起留在本设备。

use super::usage::{
    data_source_expr, session_sources_sql_list, SESSION_PROXY_DEDUP_WINDOW_SECONDS,
};
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::time::Duration;

/// 回填状态表（每台设备独立，随 `proxy_request_logs` 一起保留）
pub(crate) const BACKFILL_STATE_TABLE: &str = "usage_backfill_state";

/// `usage_backfill_state.name`：superseded_by 回填
const SUPERSEDED_BACKFILL: &str = "superseded_by";

/// 每块回填的行数（按 `request_id` 计，含非会话行）
pub(crate) const BACKFILL_CHUNK_ROWS: u32 = 10_000;

/// 两块之间释放连接锁的间隔，让代理写入与 Dashboard 查询插队
const BACKFILL_CHUNK_PAUSE: Duration = Duration::from_millis(20);

const TRIGGER_AFTER_INSERT: &str = "trg_request_logs_superseded_ai";
const TRIGGER_AFTER_UPDATE: &str = "trg_request_logs_superseded_au";
const TRIGGER_AFTER_DELETE: &str = "trg_request_logs_superseded_ad";

/// 指纹相关的列：改动它们会改变去重结果
const FINGERPRINT_COLUMNS: &str = "request_id, app_type, data_source, status_code, model, \
     input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens, created_at";

/// 回填进度（也是前端事件的载荷）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupersededBackfillProgress {
    /// 已处理的行数（含之前运行中已提交的块）
    pub processed: u64,
    /// 预计总行数（已处理 + 本次运行开始时游标之后的行）
    pub total: u64,
    pub done: bool,
}

/// 会话行 `session` 被代理行 `proxy` 覆盖的匹配条件（不含"会话行属于会话来源"）。
///
/// 与 [`super::usage::has_matching_proxy_usage_log`] 同口径；`session` / `proxy`
/// 可以是表别名、表名或触发器里的 `NEW` / `OLD`。
fn proxy_match_conditions(session: &str, proxy: &str) -> String {
    let session_ds = data_source_expr(session);
    let proxy_ds = data_source_expr(proxy);
    format!(
        "{proxy_ds} = 'proxy'
                  AND {proxy}.app_type = {session}.app_type
                  AND {proxy}.status_code >= 200
                  AND {proxy}.status_code < 300
                  AND {proxy}.input_tokens = {session}.input_tokens
                  AND {proxy}.output_tokens = {session}.output_tokens
                  AND {proxy}.cache_read_tokens = {session}.cache_read_tokens
                  AND (
                      {proxy}.cache_creation_tokens = {session}.cache_creation_tokens
                      OR (
                          {session}.cache_creation_tokens = 0
                          AND {session_ds} IN ('codex_session', 'gemini_session', 'opencode_session')
                      )
                  )
                  AND {proxy}.created_at BETWEEN
                      {session}.created_at - {SESSION_PROXY_DEDUP_WINDOW_SECONDS}
                      AND {session}.created_at + {SESSION_PROXY_DEDUP_WINDOW_SECONDS}
                  AND (
                      LOWER({proxy}.model) = LOWER({session}.model)
                      OR LOWER({proxy}.model) = 'unknown'
                      OR LOWER({session}.model) = 'unknown'
                  )"
    )
}

/// `session` 是会话来源行
fn is_session_row(session: &str) -> String {
    format!(
        "{} IN ({})",
        data_source_expr(session),
        session_sources_sql_list()
    )
}

/// 标量子查询：覆盖会话行 `session` 的任一代理行 `request_id`（没有则 NULL）
fn superseding_proxy_id(session: &str) -> String {
    let conditions = proxy_match_conditions(session, "proxy_dedup");
    format!(
        "(SELECT proxy_dedup.request_id
          FROM proxy_request_logs proxy_dedup
          WHERE {conditions}
          LIMIT 1)"
    )
}

/// 相关 `EXISTS` 形式的有效行条件：回填完成前（以及没有该列的旧/测试库）使用
pub(crate) fn superseded_exists_filter(log_alias: &str) -> String {
    let session_row = is_session_row(log_alias);
    let conditions = proxy_match_conditions(log_alias, "proxy_dedup");
    format!(
        "NOT (
            {session_row}
            AND EXISTS (
                SELECT 1
                FROM proxy_request_logs proxy_dedup
                WHERE {conditions}
            )
        )"
    )
}

/// Dashboard 查询的有效行条件：排除已被代理日志覆盖的会话行。
///
/// 回填完成后直接读 `superseded_by`，否则回落到 [`superseded_exists_filter`]。
pub(crate) fn effective_usage_log_filter(
    conn: &Connection,
    log_alias: &str,
) -> Result<String, AppError> {
    if is_backfill_complete(conn)? {
        Ok(format!("{log_alias}.superseded_by IS NULL"))
    } else {
        Ok(superseded_exists_filter(log_alias))
    }
}

/// `superseded_by` 回填是否已完成（没有状态表的库视为未完成）
pub(crate) fn is_backfill_complete(conn: &Connection) -> Result<bool, AppError> {
    if !Database::table_exists(conn, BACKFILL_STATE_TABLE)? {
        return Ok(false);
    }
    let done: Option<bool> = conn
        .query_row(
            &format!("SELECT done FROM {BACKFILL_STATE_TABLE} WHERE name = ?1"),
            params![SUPERSEDED_BACKFILL],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| AppError::Database(format!("读取去重回填状态失败: {e}")))?;
    Ok(done.unwrap_or(false))
}

/// `proxy_request_logs` 具备去重所需的全部列（只含部分列的测试库 / 损坏库返回 false）
fn has_fingerprint_columns(conn: &Connection) -> Result<bool, AppError> {
    if !Database::table_exists(conn, "proxy_request_logs")? {
        return Ok(false);
    }
    for column in FINGERPRINT_COLUMNS.split(',').map(str::trim) {
        if !Database::has_column(conn, "proxy_request_logs", column)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// v19 迁移：新增 `superseded_by` 列、索引、回填状态表与维护触发器。
///
/// 不在这里回填（大表会阻塞启动）：空表直接标记完成，否则交给后台回填。
pub(crate) fn install(conn: &Connection) -> Result<(), AppError> {
    if !has_fingerprint_columns(conn)? {
        return Ok(());
    }
    Database::add_column_if_missing(conn, "proxy_request_logs", "superseded_by", "TEXT")?;
    // 触发器按「指向某代理行的会话行」重新判定，需要这个索引；绝大多数行为 NULL，用部分索引
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_request_logs_superseded_by
         ON proxy_request_logs(superseded_by) WHERE superseded_by IS NOT NULL",
        [],
    )
    .map_err(|e| AppError::Database(format!("创建 superseded_by 索引失败: {e}")))?;
    conn.execute(
        &format!(
            "CREATE TABLE IF NOT EXISTS {BACKFILL_STATE_TABLE} (
                name TEXT PRIMARY KEY,
                cursor TEXT,
                processed INTEGER NOT NULL DEFAULT 0,
                done INTEGER NOT NULL DEFAULT 0,
                updated_at INTEGER
            )"
        ),
        [],
    )
    .map_err(|e| AppError::Database(format!("创建回填状态表失败: {e}")))?;
    conn.execute(
        &format!(
            "INSERT OR IGNORE INTO {BACKFILL_STATE_TABLE} (name, cursor, processed, done, updated_at)
             VALUES (?1, NULL, 0, NOT EXISTS (SELECT 1 FROM proxy_request_logs),
                     CAST(strftime('%s', 'now') AS INTEGER))"
        ),
        params![SUPERSEDED_BACKFILL],
    )
    .map_err(|e| AppError::Database(format!("写入回填状态失败: {e}")))?;
    ensure_triggers(conn)
}

/// 创建维护 `superseded_by` 的触发器（幂等；没有该列的库跳过）。
///
/// 每次迁移结束和 SQL 导入后都会调用：SQL 导出不含触发器。
pub(crate) fn ensure_triggers(conn: &Connection) -> Result<(), AppError> {
    if !has_fingerprint_columns(conn)?
        || !Database::has_column(conn, "proxy_request_logs", "superseded_by")?
    {
        return Ok(());
    }

    let recompute_pointing_to = |proxy_id: &str| {
        format!(
            "UPDATE proxy_request_logs
             SET superseded_by = {superseding}
             WHERE superseded_by = {proxy_id};",
            superseding = superseding_proxy_id("proxy_request_logs"),
        )
    };
    let mark_sessions_covered_by_new = format!(
        "UPDATE proxy_request_logs
         SET superseded_by = NEW.request_id
         WHERE superseded_by IS NULL
           AND {session_row}
           AND {conditions};",
        session_row = is_session_row("proxy_request_logs"),
        conditions = proxy_match_conditions("proxy_request_logs", "NEW"),
    );
    let new_is_session = is_session_row("NEW");
    let new_superseding = superseding_proxy_id("NEW");
    let old_ds = data_source_expr("OLD");

    // 写入 superseded_by 本身不在 UPDATE OF 列表里，不会再次触发；
    // recursive_triggers 默认关闭，REPLACE 删除旧行不触发 DELETE 触发器，
    // 因此 INSERT 时也重新判定指向同一 request_id 的会话行。
    let sql = format!(
        "CREATE TRIGGER IF NOT EXISTS {TRIGGER_AFTER_INSERT}
         AFTER INSERT ON proxy_request_logs
         BEGIN
             {recompute_new}
             UPDATE proxy_request_logs
             SET superseded_by = {new_superseding}
             WHERE request_id = NEW.request_id AND {new_is_session};
             {mark_sessions_covered_by_new}
         END;

         CREATE TRIGGER IF NOT EXISTS {TRIGGER_AFTER_UPDATE}
         AFTER UPDATE OF {FINGERPRINT_COLUMNS} ON proxy_request_logs
         BEGIN
             {recompute_old}
             UPDATE proxy_request_logs
             SET superseded_by = CASE WHEN {new_is_session} THEN {new_superseding} ELSE NULL END
             WHERE request_id = NEW.request_id;
             {mark_sessions_covered_by_new}
         END;

         CREATE TRIGGER IF NOT EXISTS {TRIGGER_AFTER_DELETE}
         AFTER DELETE ON proxy_request_logs
         WHEN {old_ds} = 'proxy'
         BEGIN
             {recompute_old}
         END;",
        recompute_new = recompute_pointing_to("NEW.request_id"),
        recompute_old = recompute_pointing_to("OLD.request_id"),
    );
    conn.execute_batch(&sql)
        .map_err(|e| AppError::Database(format!("创建 superseded_by 触发器失败: {e}")))
}

/// 删除维护触发器：整表原样恢复（值已自洽）时避免逐行重算
pub(crate) fn drop_triggers(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(&format!(
        "DROP TRIGGER IF EXISTS {TRIGGER_AFTER_INSERT};
         DROP TRIGGER IF EXISTS {TRIGGER_AFTER_UPDATE};
         DROP TRIGGER IF EXISTS {TRIGGER_AFTER_DELETE};"
    ))
    .map_err(|e| AppError::Database(format!("删除 superseded_by 触发器失败: {e}")))
}

/// 读取回填状态：`(cursor, processed, done)`；没有状态行时返回 None
fn read_state(conn: &Connection) -> Result<Option<(Option<String>, u64, bool)>, AppError> {
    if !Database::table_exists(conn, BACKFILL_STATE_TABLE)? {
        return Ok(None);
    }
    conn.query_row(
        &format!("SELECT cursor, processed, done FROM {BACKFILL_STATE_TABLE} WHERE name = ?1"),
        params![SUPERSEDED_BACKFILL],
        |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, i64>(1)?.max(0) as u64,
                row.get::<_, bool>(2)?,
            ))
        },
    )
    .optional()
    .map_err(|e| AppError::Database(format!("读取去重回填状态失败: {e}")))
}

/// 游标之后剩余的行数
fn remaining_rows(conn: &Connection, cursor: Option<&str>) -> Result<u64, AppError> {
    conn.query_row(
        "SELECT COUNT(*) FROM proxy_request_logs WHERE ?1 IS NULL OR request_id > ?1",
        params![cursor],
        |row| row.get::<_, i64>(0),
    )
    .map(|n| n.max(0) as u64)
    .map_err(|e| AppError::Database(format!("统计待回填行数失败: {e}")))
}

/// 回填一块：游标之后按 `request_id` 取至多 `chunk_rows` 行，重算其中会话行的
/// `superseded_by`，并在同一事务里推进游标。没有剩余行时标记完成。
///
/// 返回 `(本块行数, 是否已完成)`；状态表不存在时返回 `(0, true)`。
pub(crate) fn backfill_chunk(conn: &Connection, chunk_rows: u32) -> Result<(u64, bool), AppError> {
    let Some((cursor, _, done)) = read_state(conn)? else {
        return Ok((0, true));
    };
    if done {
        return Ok((0, true));
    }

    conn.execute("SAVEPOINT superseded_backfill;", [])
        .map_err(|e| AppError::Database(format!("开启回填 savepoint 失败: {e}")))?;
    let result = (|| {
        let (last_id, rows): (Option<String>, i64) = conn
            .query_row(
                "SELECT MAX(request_id), COUNT(*) FROM (
                     SELECT request_id FROM proxy_request_logs
                     WHERE ?1 IS NULL OR request_id > ?1
                     ORDER BY request_id
                     LIMIT ?2
                 )",
                params![cursor, chunk_rows],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| AppError::Database(format!("读取回填块失败: {e}")))?;

        let Some(last_id) = last_id else {
            conn.execute(
                &format!(
                    "UPDATE {BACKFILL_STATE_TABLE}
                     SET done = 1, updated_at = CAST(strftime('%s', 'now') AS INTEGER)
                     WHERE name = ?1"
                ),
                params![SUPERSEDED_BACKFILL],
            )
            .map_err(|e| AppError::Database(format!("标记回填完成失败: {e}")))?;
            return Ok((0, true));
        };

        conn.execute(
            &format!(
                "UPDATE proxy_request_logs
                 SET superseded_by = {superseding}
                 WHERE (?1 IS NULL OR request_id > ?1) AND request_id <= ?2
                   AND {session_row}",
                superseding = superseding_proxy_id("proxy_request_logs"),
                session_row = is_session_row("proxy_request_logs"),
            ),
            params![cursor, last_id],
        )
        .map_err(|e| AppError::Database(format!("回填 superseded_by 失败: {e}")))?;

        conn.execute(
            &format!(
                "UPDATE {BACKFILL_STATE_TABLE}
                 SET cursor = ?2, processed = processed + ?3,
                     updated_at = CAST(strftime('%s', 'now') AS INTEGER)
                 WHERE name = ?1"
            ),
            params![SUPERSEDED_BACKFILL, last_id, rows],
        )
        .map_err(|e| AppError::Database(format!("推进回填游标失败: {e}")))?;
        Ok((rows.max(0) as u64, false))
    })();

    match result {
        Ok(outcome) => {
            conn.execute("RELEASE superseded_backfill;", [])
                .map_err(|e| AppError::Database(format!("提交回填 savepoint 失败: {e}")))?;
            Ok(outcome)
        }
        Err(e) => {
            conn.execute("ROLLBACK TO superseded_backfill;", []).ok();
            conn.execute("RELEASE superseded_backfill;", []).ok();
            Err(e)
        }
    }
}

impl Database {
    /// 后台回填 `superseded_by`，直到完成（已完成时立即返回）。
    ///
    /// 每块单独加锁、提交后暂停片刻，不会长时间占用连接；每块之后调用
    /// `on_progress`。中途退出（崩溃、关闭）后再次调用会从持久化游标继续。
    pub(crate) fn run_superseded_backfill(
        &self,
        mut on_progress: impl FnMut(SupersededBackfillProgress),
    ) -> Result<SupersededBackfillProgress, AppError> {
        let (mut processed, total) = {
            let conn = lock_conn!(self.conn);
            match read_state(&conn)? {
                None | Some((_, _, true)) => {
                    return Ok(SupersededBackfillProgress {
                        processed: 0,
                        total: 0,
                        done: true,
                    });
                }
                Some((cursor, processed, false)) => {
                    let remaining = remaining_rows(&conn, cursor.as_deref())?;
                    (processed, processed + remaining)
                }
            }
        };
        log::info!("开始回填 superseded_by：已处理 {processed} 行，共约 {total} 行");

        loop {
            let (rows, done) = {
                let conn = lock_conn!(self.conn);
                backfill_chunk(&conn, BACKFILL_CHUNK_ROWS)?
            };
            processed += rows;
            let progress = SupersededBackfillProgress {
                processed,
                // 回填期间写入的新行可能让已处理数超过开始时的估计
                total: total.max(processed),
                done,
            };
            on_progress(progress);
            if done {
                log::info!("superseded_by 回填完成：共处理 {processed} 行");
                return Ok(progress);
            }
            std::thread::sleep(BACKFILL_CHUNK_PAUSE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::time::Instant;

    /// 确定性伪随机数（LCG），避免引入 rand 依赖
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self, bound: u64) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (self.0 >> 33) % bound
        }

        fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
            items[self.next(items.len() as u64) as usize]
        }
    }

    /// 写入一批代理 / 会话混合行：取值集合很小，保证大量指纹碰撞与擦边不匹配
    /// （窗口边界、`unknown` 模型、大小写、缺失 cache_creation、非 2xx）。
    fn insert_synthetic_rows(
        conn: &Connection,
        rng: &mut Lcg,
        prefix: &str,
        rows: usize,
        base_ts: i64,
        span_secs: i64,
    ) -> Result<(), AppError> {
        let mut stmt = conn.prepare(
            "INSERT INTO proxy_request_logs (
                request_id, provider_id, app_type, model, request_model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd,
                total_cost_usd, latency_ms, status_code, created_at, data_source
            ) VALUES (?1, 'p1', ?2, ?3, ?3, ?4, ?5, ?6, ?7, '0', '0', '0', '0', '0.01', 100, ?8, ?9, ?10)",
        )?;
        for i in 0..rows {
            let app = rng.pick(&["claude", "codex", "gemini", "opencode"]);
            let is_session = rng.next(2) == 0;
            let data_source = if is_session {
                match app {
                    "claude" => "session_log",
                    "codex" => "codex_session",
                    "gemini" => "gemini_session",
                    _ => "opencode_session",
                }
            } else {
                "proxy"
            };
            let status = if rng.next(8) == 0 { 500 } else { 200 };
            let created_at = base_ts + rng.next(span_secs as u64) as i64;
            stmt.execute(params![
                format!("{prefix}-{i:07}"),
                app,
                rng.pick(&["model-a", "MODEL-A", "model-b", "unknown"]),
                [10_i64, 20][rng.next(2) as usize],
                [1_i64, 2][rng.next(2) as usize],
                [0_i64, 5][rng.next(2) as usize],
                [0_i64, 3][rng.next(2) as usize],
                status,
                created_at,
                data_source,
            ])?;
        }
        Ok(())
    }

    fn ids_where(conn: &Connection, filter: &str) -> Result<BTreeSet<String>, AppError> {
        let mut stmt = conn.prepare(&format!(
            "SELECT l.request_id FROM proxy_request_logs l WHERE {filter}"
        ))?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut ids = BTreeSet::new();
        for row in rows {
            ids.insert(row?);
        }
        Ok(ids)
    }

    /// 列口径与 EXISTS 口径给出同一组有效行；返回被排除的行数
    fn assert_parity(conn: &Connection) -> Result<usize, AppError> {
        let all = ids_where(conn, "1")?;
        let by_exists = ids_where(conn, &superseded_exists_filter("l"))?;
        let by_column = ids_where(conn, "l.superseded_by IS NULL")?;
        assert_eq!(
            by_column, by_exists,
            "superseded_by must match the EXISTS logic"
        );
        Ok(all.len() - by_exists.len())
    }

    /// 模拟升级前已有的数据：清空标记并把回填状态重置为未开始
    fn reset_backfill(conn: &Connection) -> Result<(), AppError> {
        conn.execute_batch(&format!(
            "UPDATE proxy_request_logs SET superseded_by = NULL;
             UPDATE {BACKFILL_STATE_TABLE} SET cursor = NULL, processed = 0, done = 0;"
        ))?;
        Ok(())
    }

    fn run_to_completion(conn: &Connection, chunk_rows: u32) -> Result<u64, AppError> {
        let mut processed = 0;
        loop {
            let (rows, done) = backfill_chunk(conn, chunk_rows)?;
            processed += rows;
            if done {
                return Ok(processed);
            }
        }
    }

    #[test]
    fn superseded_by_fresh_database_is_complete_and_triggers_match_exists() -> Result<(), AppError>
    {
        let db = Database::memory()?;
        let conn = lock_conn!(db.conn);
        assert!(
            is_backfill_complete(&conn)?,
            "empty table needs no backfill"
        );
        assert_eq!(
            effective_usage_log_filter(&conn, "l")?,
            "l.superseded_by IS NULL"
        );

        let mut rng = Lcg(7);
        insert_synthetic_rows(&conn, &mut rng, "r", 3_000, 1_000_000, 4 * 3600)?;
        let excluded = assert_parity(&conn)?;
        assert!(
            excluded > 100,
            "fixture must exercise dedup, got {excluded}"
        );
        Ok(())
    }

    #[test]
    fn superseded_by_triggers_follow_updates_and_deletes() -> Result<(), AppError> {
        let db = Database::memory()?;
        let conn = lock_conn!(db.conn);
        let mut rng = Lcg(11);
        insert_synthetic_rows(&conn, &mut rng, "r", 2_000, 1_000_000, 2 * 3600)?;
        assert_parity(&conn)?;

        // 删除一部分代理行（与归档裁剪同路径）：指向它们的会话行重新判定
        conn.execute(
            "DELETE FROM proxy_request_logs
             WHERE COALESCE(data_source, 'proxy') = 'proxy' AND created_at < 1003600",
            [],
        )?;
        assert_parity(&conn)?;

        // 改代理行状态码、改会话行 token（会话导入升级行的路径）
        conn.execute(
            "UPDATE proxy_request_logs SET status_code = 503
             WHERE COALESCE(data_source, 'proxy') = 'proxy' AND request_id LIKE '%3'",
            [],
        )?;
        conn.execute(
            "UPDATE proxy_request_logs SET input_tokens = input_tokens + 10
             WHERE data_source <> 'proxy' AND request_id LIKE '%7'",
            [],
        )?;
        assert_parity(&conn)?;

        // INSERT OR REPLACE 同一 request_id 的代理行（REPLACE 不触发 DELETE 触发器）
        conn.execute(
            "INSERT OR REPLACE INTO proxy_request_logs
                (request_id, provider_id, app_type, model, input_tokens, output_tokens,
                 cache_read_tokens, cache_creation_tokens, total_cost_usd, latency_ms,
                 status_code, created_at, data_source)
             SELECT request_id, provider_id, app_type, 'model-z', 999, 999, 0, 0, '0', 1,
                    200, created_at, 'proxy'
             FROM proxy_request_logs
             WHERE request_id IN (SELECT superseded_by FROM proxy_request_logs
                                  WHERE superseded_by IS NOT NULL LIMIT 20)",
            [],
        )?;
        assert_parity(&conn)?;
        Ok(())
    }

    #[test]
    fn superseded_backfill_matches_exists_and_queries_switch_when_done() -> Result<(), AppError> {
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            let mut rng = Lcg(23);
            insert_synthetic_rows(&conn, &mut rng, "r", 2_500, 1_000_000, 3 * 3600)?;
        }
        let before = db.get_usage_summary(None, None, None, None, None)?;

        {
            let conn = lock_conn!(db.conn);
            reset_backfill(&conn)?;
            assert!(!is_backfill_complete(&conn)?);
            // 回填完成前查询回落到 EXISTS：结果不变
            assert!(effective_usage_log_filter(&conn, "l")?.contains("EXISTS"));
        }
        let during = db.get_usage_summary(None, None, None, None, None)?;
        assert_eq!(during.total_requests, before.total_requests);

        let mut events = Vec::new();
        let finished = db.run_superseded_backfill(|p| events.push(p))?;
        assert!(finished.done);
        assert_eq!(finished.processed, 2_500);
        assert_eq!(events.last().copied(), Some(finished));

        let conn = lock_conn!(db.conn);
        assert!(is_backfill_complete(&conn)?);
        assert_eq!(
            effective_usage_log_filter(&conn, "l")?,
            "l.superseded_by IS NULL"
        );
        assert_parity(&conn)?;
        drop(conn);
        let after = db.get_usage_summary(None, None, None, None, None)?;
        assert_eq!(after.total_requests, before.total_requests);
        assert_eq!(after.total_cost, before.total_cost);
        Ok(())
    }

    #[test]
    fn superseded_backfill_resumes_from_cursor_after_kill() -> Result<(), AppError> {
        let dir = tempfile::tempdir().map_err(|e| AppError::Database(e.to_string()))?;
        let path = dir.path().join("cc-switch.db");
        let total_rows = 2_345_u64;
        {
            let conn = Connection::open(&path)?;
            Database::apply_schema_migrations_on_conn(&conn)?;
            let mut rng = Lcg(31);
            insert_synthetic_rows(&conn, &mut rng, "r", total_rows as usize, 1_000_000, 3600)?;
            reset_backfill(&conn)?;

            // 两块提交成功
            assert_eq!(backfill_chunk(&conn, 500)?, (500, false));
            assert_eq!(backfill_chunk(&conn, 500)?, (500, false));
            // 第三块进行到一半时进程被杀：外层事务没有提交
            conn.execute_batch("BEGIN;")?;
            backfill_chunk(&conn, 500)?;
            // 连接直接丢弃，等同崩溃
        }

        let conn = Connection::open(&path)?;
        Database::apply_schema_migrations_on_conn(&conn)?;
        let (cursor, processed, done) = read_state(&conn)?.expect("state row");
        assert!(!done);
        assert_eq!(processed, 1_000, "the uncommitted chunk must not count");
        assert_eq!(cursor.as_deref(), Some("r-0000999"));
        // 已提交块之外的行仍未回填
        let filled_past_cursor: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs
             WHERE request_id > 'r-0000999' AND superseded_by IS NOT NULL",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(filled_past_cursor, 0);

        let resumed = run_to_completion(&conn, 500)?;
        assert_eq!(
            resumed,
            total_rows - 1_000,
            "resume continues after the cursor"
        );
        let (_, processed, done) = read_state(&conn)?.expect("state row");
        assert!(done);
        assert_eq!(processed, total_rows);
        assert_parity(&conn)?;
        Ok(())
    }

    #[test]
    fn superseded_migration_from_v18_defers_backfill() -> Result<(), AppError> {
        let conn = Connection::open_in_memory()?;
        Database::apply_schema_migrations_up_to(&conn, 18)?;
        let mut rng = Lcg(41);
        insert_synthetic_rows(&conn, &mut rng, "r", 1_500, 1_000_000, 3600)?;

        Database::apply_schema_migrations_on_conn(&conn)?;
        assert!(
            !is_backfill_complete(&conn)?,
            "existing rows need the backfill"
        );
        let marked: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs WHERE superseded_by IS NOT NULL",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(marked, 0, "the migration itself must not backfill");

        // 回填期间写入的新行由触发器维护
        insert_synthetic_rows(&conn, &mut rng, "s", 500, 1_000_000, 3600)?;
        run_to_completion(&conn, BACKFILL_CHUNK_ROWS)?;
        assert!(is_backfill_complete(&conn)?);
        assert_parity(&conn)?;
        Ok(())
    }

    #[test]
    fn superseded_triggers_survive_sql_export_and_import() -> Result<(), AppError> {
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES ('p1', 'claude', 'Provider', '{}', '{}')",
                [],
            )?;
            let mut rng = Lcg(53);
            insert_synthetic_rows(&conn, &mut rng, "r", 800, 1_000_000, 3600)?;
        }
        let sql = db.export_sql_string()?;
        assert!(!sql.contains("CREATE TRIGGER"), "triggers are not exported");

        let trigger_count = |conn: &Connection| -> Result<i64, AppError> {
            Ok(conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger'
                 AND name LIKE 'trg_request_logs_superseded_%'",
                [],
                |row| row.get(0),
            )?)
        };

        // 完整导入：日志与回填状态随导出一起到达，触发器由迁移框架重建
        let imported = Database::memory()?;
        imported.import_sql_string(&sql)?;
        {
            let conn = lock_conn!(imported.conn);
            assert!(is_backfill_complete(&conn)?);
            assert_eq!(
                trigger_count(&conn)?,
                3,
                "import must recreate the triggers"
            );
            let mut rng = Lcg(59);
            insert_synthetic_rows(&conn, &mut rng, "s", 800, 1_000_000, 3600)?;
            assert_parity(&conn)?;
        }

        // 同步导入：本地日志连同 superseded_by 与回填状态原样保留
        let local_before = {
            let conn = lock_conn!(imported.conn);
            ids_where(&conn, "l.superseded_by IS NULL")?
        };
        imported.import_sql_string_for_sync(&db.export_sql_string_for_sync()?)?;
        let conn = lock_conn!(imported.conn);
        assert!(is_backfill_complete(&conn)?);
        assert_eq!(trigger_count(&conn)?, 3);
        assert_eq!(ids_where(&conn, "l.superseded_by IS NULL")?, local_before);
        assert_parity(&conn)?;
        Ok(())
    }

    /// 1M 行合成库的性能验收（耗时较长，手动运行）：
    /// `cargo test --manifest-path src-tauri/Cargo.toml superseded_benchmark -- --ignored --nocapture`
    #[test]
    #[ignore = "benchmark: builds a 1M-row database"]
    fn superseded_benchmark_one_million_rows() -> Result<(), AppError> {
        const ROWS: usize = 1_000_000;
        let dir = tempfile::tempdir().map_err(|e| AppError::Database(e.to_string()))?;
        let path = dir.path().join("bench.db");
        let now = chrono::Local::now().timestamp();
        {
            let conn = Connection::open(&path)?;
            Database::apply_schema_migrations_up_to(&conn, 18)?;
            let mut rng = Lcg(97);
            conn.execute_batch("BEGIN;")?;
            // 30 天内均匀分布（更早的行会被启动时的归档裁剪）
            insert_synthetic_rows(&conn, &mut rng, "r", ROWS, now - 30 * 86_400, 30 * 86_400)?;
            conn.execute_batch("COMMIT;")?;
        }

        let conn = Connection::open(&path)?;
        let started = Instant::now();
        Database::apply_schema_migrations_on_conn(&conn)?;
        let migration = started.elapsed();
        println!("v18 -> v19 migration on {ROWS} rows: {migration:?}");
        drop(conn);

        let conn = Connection::open(&path)?;
        let db = Database {
            conn: std::sync::Mutex::new(conn),
            config_generation: Default::default(),
        };
        let today_start = now - now.rem_euclid(86_400);
        let summary_p95 = |label: &str| -> Result<Duration, AppError> {
            let mut samples = Vec::new();
            for _ in 0..20 {
                let t = Instant::now();
                db.get_usage_summary(Some(today_start), Some(now), None, None, None)?;
                samples.push(t.elapsed());
            }
            samples.sort();
            let p95 = samples[(samples.len() * 95).div_ceil(100) - 1];
            println!("summary (today) p95 {label}: {p95:?}");
            Ok(p95)
        };
        summary_p95("before backfill (EXISTS)")?;

        let started = Instant::now();
        let progress = db.run_superseded_backfill(|_| {})?;
        println!(
            "backfill of {} rows: {:?}",
            progress.processed,
            started.elapsed()
        );
        let p95 = summary_p95("after backfill (superseded_by)")?;
        let started = Instant::now();
        db.get_usage_summary(None, None, None, None, None)?;
        println!("summary (all 30 days): {:?}", started.elapsed());

        let conn = lock_conn!(db.conn);
        assert_parity(&conn)?;
        assert!(
            p95 <= Duration::from_millis(50),
            "summary p95 {p95:?} > 50ms"
        );
        Ok(())
    }
}
