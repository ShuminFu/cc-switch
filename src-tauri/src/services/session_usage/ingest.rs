//! Shared writer for the session-log usage importers (Claude, Codex, Gemini, OpenCode).
//!
//! Every importer parses its own source into [`SessionUsageRow`]s and hands them to
//! [`commit_session_rows`], which owns:
//! - cross-source dedup (existing `request_id` / matching proxy log fingerprint),
//! - pricing lookup, memoized per model for the whole sync run ([`PricingCache`]),
//! - cost calculation,
//! - the single INSERT/UPSERT statement text (prepared once and cached),
//! - one transaction per file/session that also advances the sync cursor, so rows
//!   and cursor commit together and a failed row leaves the cursor untouched.

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::calculator::{CostCalculator, ModelPricing};
use crate::proxy::usage::parser::TokenUsage;
use crate::services::usage_stats::{
    find_model_pricing, has_matching_proxy_usage_log, session_row_has_matching_proxy_sql, DedupKey,
};
use rusqlite::{params, Connection};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::SystemTime;

/// Constant columns that identify one session-log source.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SessionSource {
    /// `proxy_request_logs.app_type`
    pub app_type: &'static str,
    /// `proxy_request_logs.provider_id`
    pub provider_id: &'static str,
    /// `proxy_request_logs.data_source` and `provider_type`
    pub data_source: &'static str,
    /// What to do when a row with the same `request_id` already exists.
    pub policy: WritePolicy,
}

/// Conflict handling for a `request_id` that is already stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WritePolicy {
    /// Rows are immutable once written (Codex deltas, completed OpenCode messages).
    InsertOnly,
    /// Streaming snapshots of one message: a later snapshot replaces the stored row
    /// only when it carries more output tokens (Claude).
    KeepMaxOutput,
    /// Full re-reads may carry corrected values: replace whenever tokens or model differ (Gemini).
    ReplaceChanged,
}

/// One usage row produced by a session-log parser.
#[derive(Debug, Clone)]
pub(crate) struct SessionUsageRow {
    pub request_id: String,
    pub model: String,
    /// Model name used for the pricing lookup (normally equal to `model`).
    pub pricing_model: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_creation_tokens: u32,
    pub session_id: Option<String>,
    pub created_at: i64,
    /// Cost already computed by the tool itself (OpenCode); stored as total cost.
    pub precomputed_total_cost: Option<f64>,
}

/// Outcome of writing one row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteOutcome {
    Inserted,
    Updated,
    Skipped,
}

/// Per-run memo of `model -> pricing` so each model is resolved once per sync pass.
#[derive(Default)]
pub(crate) struct PricingCache {
    entries: HashMap<String, Option<ModelPricing>>,
}

impl PricingCache {
    pub(crate) fn get(&mut self, conn: &Connection, model: &str) -> Option<ModelPricing> {
        if let Some(cached) = self.entries.get(model) {
            return cached.clone();
        }
        let pricing = find_model_pricing(conn, model);
        self.entries.insert(model.to_string(), pricing.clone());
        pricing
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Sync-cursor update committed in the same transaction as the rows.
pub(crate) struct CursorUpdate<'a> {
    pub key: &'a str,
    pub last_modified: i64,
    pub last_offset: i64,
    /// Byte position after the last consumed line plus a hash of the file
    /// prefix, letting the next sync `seek` instead of re-reading (SESS-28).
    pub byte: Option<ByteCursor>,
}

/// Byte-level resume point of a sync cursor (schema v18).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ByteCursor {
    pub offset: i64,
    pub prefix_hash: String,
}

/// Parse an RFC 3339 timestamp to unix seconds, falling back to "now".
pub(crate) fn created_at_from_rfc3339(timestamp: Option<&str>) -> i64 {
    timestamp
        .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
        .map(|dt| dt.timestamp())
        .unwrap_or_else(now_secs)
}

pub(crate) fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The one INSERT statement for session rows; only the conflict clause differs per policy.
macro_rules! session_insert_sql {
    ($($conflict:tt)*) => {
        concat!(
            "INSERT INTO proxy_request_logs (
                request_id, provider_id, app_type, model, request_model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
                latency_ms, first_token_ms, status_code, error_message, session_id,
                provider_type, is_streaming, cost_multiplier, created_at, data_source
            ) VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                      0, NULL, 200, NULL, ?14, ?15, 1, '1.0', ?16, ?15)
            ON CONFLICT(request_id) ",
            $($conflict)*
        )
    };
}

macro_rules! session_update_set {
    () => {
        "DO UPDATE SET
            model = excluded.model,
            request_model = excluded.request_model,
            input_tokens = excluded.input_tokens,
            output_tokens = excluded.output_tokens,
            cache_read_tokens = excluded.cache_read_tokens,
            cache_creation_tokens = excluded.cache_creation_tokens,
            input_cost_usd = excluded.input_cost_usd,
            output_cost_usd = excluded.output_cost_usd,
            cache_read_cost_usd = excluded.cache_read_cost_usd,
            cache_creation_cost_usd = excluded.cache_creation_cost_usd,
            total_cost_usd = excluded.total_cost_usd "
    };
}

const INSERT_ONLY_SQL: &str = session_insert_sql!("DO NOTHING");
const KEEP_MAX_OUTPUT_SQL: &str = session_insert_sql!(
    session_update_set!(),
    "WHERE excluded.output_tokens > proxy_request_logs.output_tokens"
);
const REPLACE_CHANGED_SQL: &str = session_insert_sql!(
    session_update_set!(),
    "WHERE excluded.input_tokens != proxy_request_logs.input_tokens
        OR excluded.output_tokens != proxy_request_logs.output_tokens
        OR excluded.cache_read_tokens != proxy_request_logs.cache_read_tokens
        OR excluded.cache_creation_tokens != proxy_request_logs.cache_creation_tokens
        OR excluded.model != proxy_request_logs.model"
);

fn write_sql(policy: WritePolicy) -> &'static str {
    match policy {
        WritePolicy::InsertOnly => INSERT_ONLY_SQL,
        WritePolicy::KeepMaxOutput => KEEP_MAX_OUTPUT_SQL,
        WritePolicy::ReplaceChanged => REPLACE_CHANGED_SQL,
    }
}

/// The dedup triggers only run AFTER INSERT; an updated row re-checks its own
/// `dedup_hidden` flag with the same predicate (hiding is sticky, never un-hidden).
fn rehide_sql() -> &'static str {
    static SQL: OnceLock<String> = OnceLock::new();
    SQL.get_or_init(|| {
        format!(
            "UPDATE proxy_request_logs SET dedup_hidden = 1
             WHERE request_id = ?1 AND dedup_hidden = 0 AND {}",
            session_row_has_matching_proxy_sql("proxy_request_logs")
        )
    })
}

fn request_id_exists(conn: &Connection, request_id: &str) -> Result<bool, AppError> {
    conn.prepare_cached("SELECT EXISTS(SELECT 1 FROM proxy_request_logs WHERE request_id = ?1)")
        .and_then(|mut stmt| stmt.query_row(params![request_id], |row| row.get::<_, bool>(0)))
        .map_err(|e| AppError::Database(format!("查询 request_id 失败: {e}")))
}

/// Write one row on an open connection/transaction.
pub(crate) fn write_session_usage_row(
    conn: &Connection,
    source: &SessionSource,
    row: &SessionUsageRow,
    pricing: &mut PricingCache,
) -> Result<WriteOutcome, AppError> {
    let exists = request_id_exists(conn, &row.request_id)?;
    if exists && source.policy == WritePolicy::InsertOnly {
        return Ok(WriteOutcome::Skipped);
    }
    if !exists {
        let dedup_key = DedupKey {
            app_type: source.app_type,
            model: &row.model,
            input_tokens: row.input_tokens,
            output_tokens: row.output_tokens,
            cache_read_tokens: row.cache_read_tokens,
            cache_creation_tokens: row.cache_creation_tokens,
            created_at: row.created_at,
        };
        if has_matching_proxy_usage_log(conn, &dedup_key)? {
            return Ok(WriteOutcome::Skipped);
        }
    }

    let costs = compute_costs(conn, source, row, pricing);

    let changed = conn
        .prepare_cached(write_sql(source.policy))
        .and_then(|mut stmt| {
            stmt.execute(params![
                row.request_id,
                source.provider_id,
                source.app_type,
                row.model,
                row.input_tokens,
                row.output_tokens,
                row.cache_read_tokens,
                row.cache_creation_tokens,
                costs[0],
                costs[1],
                costs[2],
                costs[3],
                costs[4],
                row.session_id,
                source.data_source,
                row.created_at,
            ])
        })
        .map_err(|e| AppError::Database(format!("写入 {} 会话用量失败: {e}", source.app_type)))?;

    if changed == 0 {
        return Ok(WriteOutcome::Skipped);
    }
    if !exists {
        return Ok(WriteOutcome::Inserted);
    }

    conn.prepare_cached(rehide_sql())
        .and_then(|mut stmt| stmt.execute(params![row.request_id]))
        .map_err(|e| AppError::Database(format!("更新会话用量去重标记失败: {e}")))?;
    Ok(WriteOutcome::Updated)
}

/// `[input, output, cache_read, cache_creation, total]` as decimal strings.
fn compute_costs(
    conn: &Connection,
    source: &SessionSource,
    row: &SessionUsageRow,
    pricing: &mut PricingCache,
) -> [String; 5] {
    if let Some(total) = row.precomputed_total_cost.filter(|cost| *cost > 0.0) {
        // The tool's own cost is an aggregate that cannot be split per dimension.
        return [
            "0".into(),
            "0".into(),
            "0".into(),
            "0".into(),
            total.to_string(),
        ];
    }
    let Some(model_pricing) = pricing.get(conn, &row.pricing_model) else {
        return std::array::from_fn(|_| "0".to_string());
    };
    let usage = TokenUsage {
        input_tokens: row.input_tokens,
        output_tokens: row.output_tokens,
        cache_read_tokens: row.cache_read_tokens,
        cache_creation_tokens: row.cache_creation_tokens,
        model: Some(row.model.clone()),
        message_id: None,
    };
    let cost = CostCalculator::calculate_for_app(
        source.app_type,
        &usage,
        &model_pricing,
        Decimal::from(1),
    );
    [
        cost.input_cost.to_string(),
        cost.output_cost.to_string(),
        cost.cache_read_cost.to_string(),
        cost.cache_creation_cost.to_string(),
        cost.total_cost.to_string(),
    ]
}

/// Write `rows` and (optionally) advance a sync cursor in one transaction.
///
/// Returns `(imported, skipped)` where `imported` counts inserted and updated rows.
/// Any write error rolls the whole batch back, so the cursor is not advanced past rows
/// that failed and the next sync retries them (rows are idempotent on `request_id`).
pub(crate) fn commit_session_rows(
    db: &Database,
    source: &SessionSource,
    rows: &[SessionUsageRow],
    pricing: &mut PricingCache,
    cursor: Option<CursorUpdate<'_>>,
) -> Result<(u32, u32), AppError> {
    if rows.is_empty() && cursor.is_none() {
        return Ok((0, 0));
    }

    let mut conn = lock_conn!(db.conn);
    let tx = conn
        .transaction()
        .map_err(|e| AppError::Database(format!("开启会话同步事务失败: {e}")))?;

    let mut imported = 0u32;
    let mut skipped = 0u32;
    for row in rows {
        match write_session_usage_row(&tx, source, row, pricing)? {
            WriteOutcome::Inserted | WriteOutcome::Updated => imported += 1,
            WriteOutcome::Skipped => skipped += 1,
        }
    }

    if let Some(cursor) = cursor {
        update_sync_state_on(&tx, cursor.key, cursor.last_modified, cursor.last_offset)?;
        if let Some(byte) = &cursor.byte {
            update_byte_cursor_on(&tx, cursor.key, byte)?;
        }
    }

    tx.commit()
        .map_err(|e| AppError::Database(format!("提交会话同步事务失败: {e}")))?;
    drop(conn);

    if imported > 0 {
        crate::usage_events::notify_log_recorded();
    }
    Ok((imported, skipped))
}

/// Upsert one `session_log_sync` row on an open connection.
pub(crate) fn update_sync_state_on(
    conn: &Connection,
    key: &str,
    last_modified: i64,
    last_offset: i64,
) -> Result<(), AppError> {
    conn.prepare_cached(
        "INSERT OR REPLACE INTO session_log_sync (file_path, last_modified, last_line_offset, last_synced_at)
         VALUES (?1, ?2, ?3, ?4)",
    )
    .and_then(|mut stmt| stmt.execute(params![key, last_modified, last_offset, now_secs()]))
    .map_err(|e| AppError::Database(format!("更新同步状态失败: {e}")))?;
    Ok(())
}

/// Record the byte resume point for `key` (row written by [`update_sync_state_on`]).
pub(crate) fn update_byte_cursor_on(
    conn: &Connection,
    key: &str,
    byte: &ByteCursor,
) -> Result<(), AppError> {
    conn.prepare_cached(
        "UPDATE session_log_sync SET byte_offset = ?2, prefix_hash = ?3 WHERE file_path = ?1",
    )
    .and_then(|mut stmt| stmt.execute(params![key, byte.offset, byte.prefix_hash]))
    .map_err(|e| AppError::Database(format!("更新字节游标失败: {e}")))?;
    Ok(())
}

/// Byte resume points keyed by file path (only rows that have one).
pub(crate) fn load_byte_cursors(db: &Database) -> Result<HashMap<String, ByteCursor>, AppError> {
    let conn = lock_conn!(db.conn);
    let mut stmt = conn
        .prepare(
            "SELECT file_path, byte_offset, prefix_hash FROM session_log_sync
             WHERE byte_offset > 0 AND prefix_hash IS NOT NULL",
        )
        .map_err(|e| AppError::Database(format!("读取字节游标失败: {e}")))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                ByteCursor {
                    offset: row.get(1)?,
                    prefix_hash: row.get(2)?,
                },
            ))
        })
        .map_err(|e| AppError::Database(format!("读取字节游标失败: {e}")))?;
    let mut cursors = HashMap::new();
    for row in rows {
        let (key, cursor) = row.map_err(|e| AppError::Database(e.to_string()))?;
        cursors.insert(key, cursor);
    }
    Ok(cursors)
}

/// All sync cursors keyed by file path, loaded with one statement per pass.
pub(crate) type SyncCursors = HashMap<String, (i64, i64)>;

pub(crate) fn load_sync_cursors(db: &Database) -> Result<SyncCursors, AppError> {
    let conn = lock_conn!(db.conn);
    let mut stmt = conn
        .prepare("SELECT file_path, last_modified, last_line_offset FROM session_log_sync")
        .map_err(|e| AppError::Database(format!("读取同步状态失败: {e}")))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                (row.get::<_, i64>(1)?, row.get::<_, i64>(2)?),
            ))
        })
        .map_err(|e| AppError::Database(format!("读取同步状态失败: {e}")))?;
    let mut cursors = HashMap::new();
    for row in rows {
        let (key, state) = row.map_err(|e| AppError::Database(e.to_string()))?;
        cursors.insert(key, state);
    }
    Ok(cursors)
}

/// Delete cursors under `root` whose file no longer exists (SESS-46).
///
/// Only keys that are real paths below `root` and were not discovered in this pass are
/// considered, and each is checked on disk before removal.
pub(crate) fn prune_missing_cursors(
    db: &Database,
    root: &std::path::Path,
    cursors: &SyncCursors,
    discovered: &std::collections::HashSet<String>,
) -> Result<usize, AppError> {
    let root_str = root.to_string_lossy();
    let stale: Vec<&String> = cursors
        .keys()
        .filter(|key| key.starts_with(root_str.as_ref()) && !discovered.contains(*key))
        .filter(|key| !std::path::Path::new(key.as_str()).exists())
        .collect();
    if stale.is_empty() {
        return Ok(0);
    }
    let mut conn = lock_conn!(db.conn);
    let tx = conn
        .transaction()
        .map_err(|e| AppError::Database(e.to_string()))?;
    for key in &stale {
        tx.prepare_cached("DELETE FROM session_log_sync WHERE file_path = ?1")
            .and_then(|mut stmt| stmt.execute(params![key]))
            .map_err(|e| AppError::Database(format!("清理同步状态失败: {e}")))?;
    }
    tx.commit().map_err(|e| AppError::Database(e.to_string()))?;
    Ok(stale.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SOURCE: SessionSource = SessionSource {
        app_type: "claude",
        provider_id: "_session",
        data_source: "session_log",
        policy: WritePolicy::KeepMaxOutput,
    };

    fn row(request_id: &str, output: u32) -> SessionUsageRow {
        SessionUsageRow {
            request_id: request_id.to_string(),
            model: "claude-sonnet-4-5".to_string(),
            pricing_model: "claude-sonnet-4-5".to_string(),
            input_tokens: 10,
            output_tokens: output,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            session_id: Some("s".to_string()),
            created_at: 1_000,
            precomputed_total_cost: None,
        }
    }

    fn output_of(db: &Database, request_id: &str) -> i64 {
        let conn = db.conn.lock().unwrap();
        conn.query_row(
            "SELECT output_tokens FROM proxy_request_logs WHERE request_id = ?1",
            [request_id],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn keep_max_output_updates_only_on_larger_snapshot() -> Result<(), AppError> {
        let db = Database::memory()?;
        let mut pricing = PricingCache::default();
        assert_eq!(
            commit_session_rows(&db, &TEST_SOURCE, &[row("r1", 5)], &mut pricing, None)?,
            (1, 0)
        );
        assert_eq!(
            commit_session_rows(&db, &TEST_SOURCE, &[row("r1", 50)], &mut pricing, None)?,
            (1, 0)
        );
        assert_eq!(output_of(&db, "r1"), 50);
        // A smaller (older) snapshot never overwrites a larger one.
        assert_eq!(
            commit_session_rows(&db, &TEST_SOURCE, &[row("r1", 7)], &mut pricing, None)?,
            (0, 1)
        );
        assert_eq!(output_of(&db, "r1"), 50);
        Ok(())
    }

    #[test]
    fn insert_only_never_updates() -> Result<(), AppError> {
        let db = Database::memory()?;
        let source = SessionSource {
            policy: WritePolicy::InsertOnly,
            ..TEST_SOURCE
        };
        let mut pricing = PricingCache::default();
        commit_session_rows(&db, &source, &[row("r1", 5)], &mut pricing, None)?;
        assert_eq!(
            commit_session_rows(&db, &source, &[row("r1", 50)], &mut pricing, None)?,
            (0, 1)
        );
        assert_eq!(output_of(&db, "r1"), 5);
        Ok(())
    }

    #[test]
    fn pricing_is_resolved_once_per_model() -> Result<(), AppError> {
        let db = Database::memory()?;
        let mut pricing = PricingCache::default();
        let rows: Vec<_> = (0..20).map(|i| row(&format!("r{i}"), 5)).collect();
        commit_session_rows(&db, &TEST_SOURCE, &rows, &mut pricing, None)?;
        assert_eq!(pricing.len(), 1);
        Ok(())
    }

    #[test]
    fn rows_and_cursor_commit_together() -> Result<(), AppError> {
        let db = Database::memory()?;
        let mut pricing = PricingCache::default();
        commit_session_rows(
            &db,
            &TEST_SOURCE,
            &[row("r1", 5)],
            &mut pricing,
            Some(CursorUpdate {
                key: "/tmp/x.jsonl",
                last_modified: 42,
                last_offset: 3,
                byte: None,
            }),
        )?;
        let cursors = load_sync_cursors(&db)?;
        assert_eq!(cursors.get("/tmp/x.jsonl"), Some(&(42, 3)));
        Ok(())
    }
}
