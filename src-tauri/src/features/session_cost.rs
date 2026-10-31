//! Usage attribution by session and project (PRODUCT-14, first increment).
//!
//! Request-log rows carry the CLI `session_id` (from the proxy's session
//! detection and from session-log sync). The session manager knows each
//! session's project directory. Joining the two gives:
//!
//! * per-session totals (shown in the session detail header);
//! * per-project totals for a time window, normalised to the git root, plus
//!   an "unattributed" bucket for rows whose session is unknown, so the
//!   buckets always add up to the dashboard total.
//!
//! Only detail rows are used: rows already folded into daily rollups by the
//! retention job (PRODUCT-31) no longer have a session id.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::State;

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::store::AppState;

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageTotals {
    pub requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub total_cost_usd: f64,
}

impl UsageTotals {
    fn add(&mut self, other: &UsageTotals) {
        self.requests += other.requests;
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
        self.cache_creation_tokens += other.cache_creation_tokens;
        self.total_cost_usd += other.total_cost_usd;
    }
}

const SELECT_TOTALS: &str =
    "COUNT(*), COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0), \
     COALESCE(SUM(cache_read_tokens), 0), COALESCE(SUM(cache_creation_tokens), 0), \
     COALESCE(SUM(CAST(total_cost_usd AS REAL)), 0)";

fn row_totals(row: &rusqlite::Row<'_>, offset: usize) -> rusqlite::Result<UsageTotals> {
    Ok(UsageTotals {
        requests: row.get::<_, i64>(offset)?.max(0) as u64,
        input_tokens: row.get::<_, i64>(offset + 1)?.max(0) as u64,
        output_tokens: row.get::<_, i64>(offset + 2)?.max(0) as u64,
        cache_read_tokens: row.get::<_, i64>(offset + 3)?.max(0) as u64,
        cache_creation_tokens: row.get::<_, i64>(offset + 4)?.max(0) as u64,
        total_cost_usd: row.get::<_, f64>(offset + 5)?,
    })
}

fn db_err(e: rusqlite::Error) -> AppError {
    AppError::Database(e.to_string())
}

/// Totals for one session (`None` when no detail row has that id).
fn session_totals(db: &Database, session_id: &str) -> Result<Option<UsageTotals>, AppError> {
    let conn = lock_conn!(db.conn);
    let totals = conn
        .query_row(
            &format!(
                "SELECT {SELECT_TOTALS} FROM proxy_request_logs \
                 WHERE dedup_hidden = 0 AND session_id = ?1"
            ),
            [session_id],
            |row| row_totals(row, 0),
        )
        .map_err(db_err)?;
    Ok((totals.requests > 0).then_some(totals))
}

/// Totals per session id (rows without a session id under `""`) since
/// `since` (unix seconds).
fn totals_by_session(db: &Database, since: i64) -> Result<HashMap<String, UsageTotals>, AppError> {
    let conn = lock_conn!(db.conn);
    let mut stmt = conn
        .prepare(&format!(
            "SELECT COALESCE(session_id, ''), {SELECT_TOTALS} FROM proxy_request_logs \
             WHERE dedup_hidden = 0 AND created_at >= ?1 GROUP BY COALESCE(session_id, '')"
        ))
        .map_err(db_err)?;
    let rows = stmt
        .query_map([since], |row| {
            Ok((row.get::<_, String>(0)?, row_totals(row, 1)?))
        })
        .map_err(db_err)?;
    let mut out = HashMap::new();
    for row in rows {
        let (id, totals) = row.map_err(db_err)?;
        out.insert(id, totals);
    }
    Ok(out)
}

/// The nearest ancestor that contains `.git`, else the directory itself.
fn project_root(dir: &str, cache: &mut HashMap<String, String>) -> String {
    if let Some(hit) = cache.get(dir) {
        return hit.clone();
    }
    let trimmed = dir.trim_end_matches(['/', '\\']);
    let start = PathBuf::from(if trimmed.is_empty() { dir } else { trimmed });
    let root = start
        .ancestors()
        .find(|p| p.join(".git").exists())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| start.clone());
    let root = root.to_string_lossy().to_string();
    cache.insert(dir.to_string(), root.clone());
    root
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectUsage {
    pub project_dir: String,
    pub sessions: usize,
    #[serde(flatten)]
    pub totals: UsageTotals,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectUsageReport {
    pub since: i64,
    /// Sorted by cost, highest first.
    pub projects: Vec<ProjectUsage>,
    /// Rows whose session has no known project (or no session id).
    pub unattributed: UsageTotals,
    /// Sum of every bucket; equals the request-log total for the window.
    pub total: UsageTotals,
}

fn build_report(
    since: i64,
    by_session: HashMap<String, UsageTotals>,
    session_projects: &HashMap<String, String>,
) -> ProjectUsageReport {
    let mut cache = HashMap::new();
    let mut projects: HashMap<String, ProjectUsage> = HashMap::new();
    let mut unattributed = UsageTotals::default();
    let mut total = UsageTotals::default();
    for (session_id, totals) in by_session {
        total.add(&totals);
        match session_projects.get(&session_id) {
            Some(dir) if !session_id.is_empty() => {
                let root = project_root(dir, &mut cache);
                let entry = projects
                    .entry(root.clone())
                    .or_insert_with(|| ProjectUsage {
                        project_dir: root,
                        sessions: 0,
                        totals: UsageTotals::default(),
                    });
                entry.sessions += 1;
                entry.totals.add(&totals);
            }
            _ => unattributed.add(&totals),
        }
    }
    let mut projects: Vec<ProjectUsage> = projects.into_values().collect();
    projects.sort_by(|a, b| {
        b.totals
            .total_cost_usd
            .partial_cmp(&a.totals.total_cost_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.project_dir.cmp(&b.project_dir))
    });
    ProjectUsageReport {
        since,
        projects,
        unattributed,
        total,
    }
}

#[tauri::command]
pub fn get_session_usage(
    state: State<'_, AppState>,
    #[allow(non_snake_case)] sessionId: String,
) -> Result<Option<UsageTotals>, String> {
    if sessionId.trim().is_empty() {
        return Ok(None);
    }
    session_totals(&state.db, &sessionId).map_err(|e| e.to_string())
}

/// Usage per project over the last `days` days (default 30).
#[tauri::command]
pub async fn get_usage_by_project(
    handle: tauri::AppHandle,
    days: Option<u32>,
) -> Result<ProjectUsageReport, String> {
    let days = i64::from(days.unwrap_or(30).clamp(1, 3650));
    let since = chrono::Utc::now().timestamp() - days * 86_400;
    crate::commands::run_blocking_with_state(handle, move |state| {
        let by_session = totals_by_session(&state.db, since).map_err(|e| e.to_string())?;
        let session_projects: HashMap<String, String> = crate::session_manager::scan_sessions()
            .into_iter()
            .filter_map(|s| s.project_dir.map(|dir| (s.session_id, dir)))
            .collect();
        Ok::<_, String>(build_report(since, by_session, &session_projects))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insert(db: &Database, id: &str, session: Option<&str>, cost: &str, created_at: i64) {
        insert_row(db, id, session, cost, created_at).unwrap();
    }

    fn insert_row(
        db: &Database,
        id: &str,
        session: Option<&str>,
        cost: &str,
        created_at: i64,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO proxy_request_logs (
                request_id, provider_id, app_type, model, input_tokens, output_tokens,
                total_cost_usd, latency_ms, status_code, created_at, session_id
            ) VALUES (?1, 'p', 'claude', 'm', 100, 10, ?2, 1, 200, ?3, ?4)",
            rusqlite::params![id, cost, created_at, session],
        )
        .map_err(db_err)?;
        Ok(())
    }

    #[test]
    fn session_and_project_totals_reconcile_with_the_log_total() {
        let db = Database::memory().unwrap();
        let now = chrono::Utc::now().timestamp();
        insert(&db, "r1", Some("s1"), "0.5", now);
        insert(&db, "r2", Some("s1"), "0.25", now);
        insert(&db, "r3", Some("s2"), "1", now);
        insert(&db, "r4", Some("unknown"), "0.1", now);
        insert(&db, "r5", None, "0.05", now);
        insert(&db, "old", Some("s1"), "9", now - 40 * 86_400);

        let s1 = session_totals(&db, "s1").unwrap().unwrap();
        assert_eq!(s1.requests, 3);
        assert_eq!(s1.input_tokens, 300);
        assert!(session_totals(&db, "nope").unwrap().is_none());

        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(repo.path().join(".git")).unwrap();
        let sub = repo.path().join("crates/app");
        std::fs::create_dir_all(&sub).unwrap();
        let other = tempfile::tempdir().unwrap();
        let projects = HashMap::from([
            ("s1".to_string(), sub.to_string_lossy().to_string()),
            ("s2".to_string(), other.path().to_string_lossy().to_string()),
        ]);
        let since = now - 30 * 86_400;
        let report = build_report(since, totals_by_session(&db, since).unwrap(), &projects);
        assert_eq!(report.projects.len(), 2);
        // s1 is attributed to the git root, not the sub-directory it ran in.
        let s1_project = report
            .projects
            .iter()
            .find(|p| p.project_dir == repo.path().to_string_lossy())
            .unwrap();
        assert_eq!(s1_project.totals.requests, 2);
        assert_eq!(report.projects[0].totals.total_cost_usd, 1.0);
        assert_eq!(report.unattributed.requests, 2);
        assert_eq!(report.total.requests, 5);
        let sum: f64 = report
            .projects
            .iter()
            .map(|p| p.totals.total_cost_usd)
            .sum::<f64>()
            + report.unattributed.total_cost_usd;
        assert!((sum - report.total.total_cost_usd).abs() < 1e-9);
        assert!((report.total.total_cost_usd - 1.9).abs() < 1e-9);
    }
}
