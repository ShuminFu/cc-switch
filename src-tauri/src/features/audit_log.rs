//! Local audit log of configuration mutations (PRODUCT-25).
//!
//! Every recorded mutation is one JSON line appended to
//! `~/.cc-switch/audit/audit.jsonl` (owner-only permissions). An event names
//! the actor (`ui`, `cli`, `deeplink`, `tray`, `sync`, `backfill`, `system`),
//! the action, the entity and SHA-256 hashes of the entity before and after
//! the change, never its content, so the log never carries an API key.
//! The file rotates at [`MAX_FILE_BYTES`] into `audit.1.jsonl` …
//! `audit.<ROTATED_FILES>.jsonl`; the oldest is dropped.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;

const DIR_NAME: &str = "audit";
const FILE_NAME: &str = "audit.jsonl";
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
pub const ROTATED_FILES: usize = 3;
const DEFAULT_LIST_LIMIT: usize = 500;
const MAX_LIST_LIMIT: usize = 5000;

/// Serialises appends and rotation within this process.
static WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AuditEvent {
    /// Unix time in milliseconds.
    pub ts: i64,
    pub actor: String,
    /// Dotted action, e.g. `provider.update`, `provider.switch`.
    pub action: String,
    pub entity_type: String,
    pub entity_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_hash: Option<String>,
    /// Short non-secret note (e.g. the restored version id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl AuditEvent {
    pub fn new(actor: &str, action: &str, entity_type: &str, entity_id: &str) -> Self {
        Self {
            ts: chrono::Utc::now().timestamp_millis(),
            actor: actor.to_string(),
            action: action.to_string(),
            entity_type: entity_type.to_string(),
            entity_id: entity_id.to_string(),
            app: None,
            before_hash: None,
            after_hash: None,
            detail: None,
        }
    }

    pub fn app(mut self, app: &str) -> Self {
        self.app = Some(app.to_string());
        self
    }

    pub fn states(mut self, before: Option<&Value>, after: Option<&Value>) -> Self {
        self.before_hash = before.map(super::redact::canonical_hash);
        self.after_hash = after.map(super::redact::canonical_hash);
        self
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

fn audit_dir() -> PathBuf {
    crate::config::get_app_config_dir().join(DIR_NAME)
}

fn current_file(dir: &Path) -> PathBuf {
    dir.join(FILE_NAME)
}

fn rotated_file(dir: &Path, n: usize) -> PathBuf {
    dir.join(format!("audit.{n}.jsonl"))
}

fn rotate_if_needed(dir: &Path) -> std::io::Result<()> {
    let current = current_file(dir);
    let size = std::fs::metadata(&current).map(|m| m.len()).unwrap_or(0);
    if size < MAX_FILE_BYTES {
        return Ok(());
    }
    let _ = std::fs::remove_file(rotated_file(dir, ROTATED_FILES));
    for n in (1..ROTATED_FILES).rev() {
        let from = rotated_file(dir, n);
        if from.exists() {
            std::fs::rename(&from, rotated_file(dir, n + 1))?;
        }
    }
    std::fs::rename(&current, rotated_file(dir, 1))
}

fn append_in(dir: &Path, event: &AuditEvent) -> std::io::Result<()> {
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::fs::create_dir_all(dir)?;
    rotate_if_needed(dir)?;
    let mut line = serde_json::to_string(event).map_err(std::io::Error::other)?;
    line.push('\n');
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(current_file(dir))?;
    file.write_all(line.as_bytes())
}

/// Append one event. Failures are logged and never fail the mutation that
/// was already applied.
pub fn record(event: AuditEvent) {
    if let Err(e) = append_in(&audit_dir(), &event) {
        log::warn!("[Audit] could not append {} event: {e}", event.action);
    }
}

fn read_lines(path: &Path, out: &mut Vec<AuditEvent>) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    out.extend(
        text.lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str::<AuditEvent>(l).ok()),
    );
}

/// All events, oldest first.
fn read_all_in(dir: &Path) -> Vec<AuditEvent> {
    let mut out = Vec::new();
    for n in (1..=ROTATED_FILES).rev() {
        read_lines(&rotated_file(dir, n), &mut out);
    }
    read_lines(&current_file(dir), &mut out);
    out
}

fn list_in(dir: &Path, limit: Option<usize>) -> Vec<AuditEvent> {
    let limit = limit.unwrap_or(DEFAULT_LIST_LIMIT).clamp(1, MAX_LIST_LIMIT);
    let mut all = read_all_in(dir);
    all.reverse();
    all.truncate(limit);
    all
}

fn to_jsonl(events: &[AuditEvent]) -> String {
    let mut out = String::new();
    for e in events {
        if let Ok(line) = serde_json::to_string(e) {
            out.push_str(&line);
            out.push('\n');
        }
    }
    out
}

/// Newest events first.
#[tauri::command]
pub fn list_audit_events(limit: Option<usize>) -> Vec<AuditEvent> {
    list_in(&audit_dir(), limit)
}

/// Save the whole log (oldest first) as JSONL through a save dialog. Returns
/// the saved path, or `None` when cancelled.
#[tauri::command]
pub async fn export_audit_log<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Option<String>, String> {
    let content = to_jsonl(&read_all_in(&audit_dir()));
    let name = format!(
        "cc-switch-audit-{}.jsonl",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );
    super::file_dialog::save_text(&app, &name, "JSON Lines", "jsonl", &content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_append_as_jsonl_and_list_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..3 {
            append_in(
                dir.path(),
                &AuditEvent::new("ui", "provider.update", "provider", &format!("p{i}"))
                    .app("claude"),
            )
            .unwrap();
        }
        let text = std::fs::read_to_string(current_file(dir.path())).unwrap();
        assert_eq!(text.lines().count(), 3);
        let listed = list_in(dir.path(), Some(2));
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].entity_id, "p2");
        assert_eq!(listed[1].entity_id, "p1");
        assert!(to_jsonl(&read_all_in(dir.path())).ends_with('\n'));
    }

    #[test]
    fn hashes_are_recorded_without_content() {
        let secret = serde_json::json!({ "env": { "ANTHROPIC_AUTH_TOKEN": "sk-planted-secret" } });
        let dir = tempfile::tempdir().unwrap();
        append_in(
            dir.path(),
            &AuditEvent::new("ui", "provider.update", "provider", "p").states(None, Some(&secret)),
        )
        .unwrap();
        let text = std::fs::read_to_string(current_file(dir.path())).unwrap();
        assert!(!text.contains("sk-planted-secret"));
        let event = &list_in(dir.path(), None)[0];
        assert_eq!(event.before_hash, None);
        assert_eq!(event.after_hash.as_ref().map(String::len), Some(64));
    }

    #[test]
    fn rotation_keeps_a_bounded_number_of_files() {
        let dir = tempfile::tempdir().unwrap();
        let big = "x".repeat(MAX_FILE_BYTES as usize);
        for i in 0..(ROTATED_FILES + 2) {
            std::fs::write(current_file(dir.path()), &big).unwrap();
            append_in(
                dir.path(),
                &AuditEvent::new("ui", "a", "provider", &i.to_string()),
            )
            .unwrap();
        }
        assert!(rotated_file(dir.path(), ROTATED_FILES).exists());
        assert!(!rotated_file(dir.path(), ROTATED_FILES + 1).exists());
        // Corrupt (oversized filler) lines are skipped; the last event is kept.
        let listed = list_in(dir.path(), None);
        assert_eq!(listed[0].entity_id, (ROTATED_FILES + 1).to_string());
    }

    #[test]
    fn unparsable_lines_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(current_file(dir.path()), "not json\n").unwrap();
        append_in(
            dir.path(),
            &AuditEvent::new("cli", "provider.switch", "provider", "x"),
        )
        .unwrap();
        assert_eq!(list_in(dir.path(), None).len(), 1);
    }
}
