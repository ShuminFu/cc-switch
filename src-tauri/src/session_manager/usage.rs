//! Per-session token and cost totals (SESS-17).
//!
//! Session syncs (Claude/Codex/Gemini/OpenCode) and the proxy store the
//! session id on every `proxy_request_logs` row, so a session's usage is one
//! `GROUP BY session_id`. Costs are summed the way the Usage Dashboard sums
//! them (exact integer `total_cost_nanos` sums, DB-14; skipping `dedup_hidden` duplicates of
//! proxy rows), so the two agree. Rows already
//! folded into daily rollups by log retention carry no session id and are not
//! counted.

use std::collections::HashMap;

use serde::Serialize;

use crate::database::Database;
use crate::error::AppError;

/// Largest batch `summaries` accepts in one call.
pub const MAX_SESSION_IDS: usize = 2000;
const CHUNK: usize = 500;

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionUsageSummary {
    pub session_id: String,
    pub request_count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub total_cost_usd: f64,
    /// Distinct models, sorted.
    pub models: Vec<String>,
    /// Unix seconds of the first and last request.
    pub first_request_at: Option<i64>,
    pub last_request_at: Option<i64>,
}

/// Totals for every id in `session_ids` that has usage rows. Ids without
/// usage are absent from the map (the UI shows "—").
pub fn summaries(
    db: &Database,
    session_ids: &[String],
) -> Result<HashMap<String, SessionUsageSummary>, AppError> {
    let mut ids: Vec<&String> = session_ids.iter().filter(|s| !s.is_empty()).collect();
    ids.sort();
    ids.dedup();
    ids.truncate(MAX_SESSION_IDS);
    let mut out = HashMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    let conn = db.read_conn()?;
    for chunk in ids.chunks(CHUNK) {
        let placeholders = vec!["?"; chunk.len()].join(",");
        let sql = format!(
            "SELECT session_id, COUNT(*),
                    COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0),
                    COALESCE(SUM(cache_read_tokens), 0), COALESCE(SUM(cache_creation_tokens), 0),
                    (COALESCE(SUM(total_cost_nanos), 0) / 1e9),
                    GROUP_CONCAT(DISTINCT model), MIN(created_at), MAX(created_at)
             FROM proxy_request_logs
             WHERE session_id IN ({placeholders}) AND dedup_hidden = 0
             GROUP BY session_id"
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| AppError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(chunk.iter()), |row| {
                let models: Option<String> = row.get(7)?;
                let mut models: Vec<String> = models
                    .unwrap_or_default()
                    .split(',')
                    .filter(|m| !m.is_empty())
                    .map(str::to_string)
                    .collect();
                models.sort();
                Ok(SessionUsageSummary {
                    session_id: row.get(0)?,
                    request_count: row.get::<_, i64>(1)?.max(0) as u64,
                    input_tokens: row.get::<_, i64>(2)?.max(0) as u64,
                    output_tokens: row.get::<_, i64>(3)?.max(0) as u64,
                    cache_read_tokens: row.get::<_, i64>(4)?.max(0) as u64,
                    cache_creation_tokens: row.get::<_, i64>(5)?.max(0) as u64,
                    total_cost_usd: row.get(6)?,
                    models,
                    first_request_at: row.get(8)?,
                    last_request_at: row.get(9)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;
        for row in rows {
            let summary = row.map_err(|e| AppError::Database(e.to_string()))?;
            out.insert(summary.session_id.clone(), summary);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insert(db: &Database, id: &str, session: Option<&str>, model: &str, cost: &str, at: i64) {
        let conn = db.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO proxy_request_logs (
                request_id, provider_id, app_type, model, input_tokens, output_tokens,
                cache_read_tokens, cache_creation_tokens, total_cost_usd, latency_ms,
                status_code, session_id, created_at)
             VALUES (?1, 'p1', 'claude', ?2, 100, 20, 5, 1, ?3, 10, 200, ?4, ?5)",
            rusqlite::params![id, model, cost, session, at],
        )
        .unwrap();
    }

    #[test]
    fn sums_tokens_and_cost_per_session() {
        let db = Database::memory().unwrap();
        insert(&db, "r1", Some("s1"), "claude-sonnet-4", "0.10", 100);
        insert(&db, "r2", Some("s1"), "claude-haiku-4", "0.05", 300);
        insert(&db, "r3", Some("s2"), "claude-sonnet-4", "1.00", 200);
        insert(&db, "r4", None, "claude-sonnet-4", "9.00", 200);

        let ids = vec!["s1".to_string(), "s2".to_string(), "missing".to_string()];
        let map = summaries(&db, &ids).unwrap();
        assert_eq!(map.len(), 2);
        let s1 = &map["s1"];
        assert_eq!(s1.request_count, 2);
        assert_eq!(
            (s1.input_tokens, s1.output_tokens, s1.cache_read_tokens),
            (200, 40, 10)
        );
        assert!((s1.total_cost_usd - 0.15).abs() < 1e-9);
        assert_eq!(s1.models, ["claude-haiku-4", "claude-sonnet-4"]);
        assert_eq!(
            (s1.first_request_at, s1.last_request_at),
            (Some(100), Some(300))
        );
        assert!(!map.contains_key("missing"));
        assert!(summaries(&db, &[]).unwrap().is_empty());
    }
}
