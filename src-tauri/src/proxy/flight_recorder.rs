//! In-memory flight recorder of recent proxy requests (PROXY-21, first increment).
//!
//! Keeps the last [`CAPACITY`] client requests with their attempt timeline:
//! which provider was tried, the outcome, the upstream status and a short,
//! redacted error excerpt. Nothing is written to disk and no request or
//! response bodies or headers are kept, so the recorder holds no credentials.
//! Error excerpts go through the same secret redaction as session search.
//!
//! Exposed through `list_recent_proxy_requests` and
//! `export_recent_proxy_requests` (JSONL). Replay and full-body capture are
//! not implemented yet.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use serde::Serialize;

/// Requests kept in memory.
pub const CAPACITY: usize = 200;
/// Longest error excerpt kept per attempt (characters).
pub const MAX_ERROR_CHARS: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AttemptResult {
    /// The provider answered and the response was returned to the client.
    Served,
    /// The attempt failed and the next provider was tried.
    Failover,
    /// The attempt failed with an error that is not retried.
    Aborted,
    /// Skipped before sending (budget or concurrency limit).
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptRecord {
    pub provider_id: String,
    pub provider_name: String,
    pub result: AttemptResult,
    pub status: Option<u16>,
    pub error: Option<String>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRecord {
    /// Monotonic per process.
    pub id: u64,
    pub app_type: String,
    pub method: String,
    /// Path only; the query string is dropped (it can carry API keys).
    pub endpoint: String,
    /// Unix milliseconds.
    pub started_at: i64,
    pub duration_ms: u64,
    pub success: bool,
    pub attempts: Vec<AttemptRecord>,
}

/// Bounded ring buffer of [`RequestRecord`]s.
pub struct FlightRecorder {
    next_id: AtomicU64,
    records: Mutex<VecDeque<RequestRecord>>,
    capacity: usize,
}

impl FlightRecorder {
    pub fn new(capacity: usize) -> Self {
        Self {
            next_id: AtomicU64::new(1),
            records: Mutex::new(VecDeque::with_capacity(capacity.min(CAPACITY))),
            capacity,
        }
    }

    /// Store a finished request (assigns its id, evicts the oldest when full).
    pub fn record(&self, mut record: RequestRecord) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        record.id = id;
        let mut records = self.records.lock().unwrap_or_else(|e| e.into_inner());
        while records.len() >= self.capacity {
            records.pop_front();
        }
        records.push_back(record);
        id
    }

    /// Newest first.
    pub fn recent(&self) -> Vec<RequestRecord> {
        let records = self.records.lock().unwrap_or_else(|e| e.into_inner());
        records.iter().rev().cloned().collect()
    }

    /// One JSON object per line, oldest first.
    pub fn export_jsonl(&self) -> String {
        let records = self.records.lock().unwrap_or_else(|e| e.into_inner());
        let mut out = String::new();
        for record in records.iter() {
            if let Ok(line) = serde_json::to_string(record) {
                out.push_str(&line);
                out.push('\n');
            }
        }
        out
    }
}

/// The process-wide recorder used by the forwarder.
pub fn global() -> &'static FlightRecorder {
    static RECORDER: OnceLock<FlightRecorder> = OnceLock::new();
    RECORDER.get_or_init(|| FlightRecorder::new(CAPACITY))
}

/// Path of `endpoint` without its query string.
pub fn endpoint_path(endpoint: &str) -> String {
    endpoint.split('?').next().unwrap_or_default().to_string()
}

/// Truncated, secret-redacted error excerpt.
pub fn error_excerpt(message: &str) -> String {
    // Redact before truncating so a key cut in half cannot slip through.
    crate::session_manager::index::redact_secrets(message)
        .chars()
        .take(MAX_ERROR_CHARS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(app: &str) -> RequestRecord {
        RequestRecord {
            id: 0,
            app_type: app.to_string(),
            method: "POST".to_string(),
            endpoint: "/v1/messages".to_string(),
            started_at: 0,
            duration_ms: 5,
            success: false,
            attempts: vec![AttemptRecord {
                provider_id: "p1".to_string(),
                provider_name: "P1".to_string(),
                result: AttemptResult::Failover,
                status: Some(500),
                error: Some("boom".to_string()),
                duration_ms: 5,
            }],
        }
    }

    #[test]
    fn keeps_the_newest_records_up_to_capacity() {
        let recorder = FlightRecorder::new(3);
        for i in 0..5 {
            recorder.record(request(&format!("app{i}")));
        }
        let recent = recorder.recent();
        let apps: Vec<&str> = recent.iter().map(|r| r.app_type.as_str()).collect();
        assert_eq!(apps, ["app4", "app3", "app2"]);
        assert_eq!(recent[0].id, 5);
    }

    #[test]
    fn export_is_jsonl_in_camel_case() {
        let recorder = FlightRecorder::new(10);
        recorder.record(request("claude"));
        recorder.record(request("codex"));
        let jsonl = recorder.export_jsonl();
        let lines: Vec<serde_json::Value> = jsonl
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["appType"], "claude");
        assert_eq!(lines[0]["attempts"][0]["result"], "failover");
        assert_eq!(lines[0]["attempts"][0]["status"], 500);
    }

    #[test]
    fn endpoint_and_errors_drop_credentials() {
        assert_eq!(
            endpoint_path("/v1beta/models/x:generateContent?key=AIzaSECRET"),
            "/v1beta/models/x:generateContent"
        );
        let excerpt =
            error_excerpt("invalid key sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789");
        assert!(!excerpt.contains("abcdefghijklmnopqrstuvwxyz"), "{excerpt}");
        assert_eq!(
            error_excerpt(&"x".repeat(2000)).chars().count(),
            MAX_ERROR_CHARS
        );
    }
}
