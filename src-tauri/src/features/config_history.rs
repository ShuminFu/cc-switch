//! Config time machine (PRODUCT-21): per-provider version history with diff
//! and rollback, plus the audit hook for provider mutations (PRODUCT-25).
//!
//! Each provider has one JSON file under
//! `~/.cc-switch/history/providers/<app>/<id>.json` holding up to
//! [`MAX_VERSIONS`] snapshots. Secrets are masked before they are stored
//! (`<redacted:xxxxxxxx>`, see [`super::redact`]), so the history never holds
//! an API key outside the database or the secrets vault; a rollback keeps the
//! provider's current key at every masked position and reports the positions
//! it could not fill (a deleted provider must have its key re-entered).
//!
//! Mutations are captured by wrapping the service call in
//! [`track_provider_change`] / [`track_switch`]: the provider row is read
//! before and after, so every path through the wrapped call (including the
//! live-config backfill a switch performs on the previous provider) yields
//! one history entry and one audit event.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::AppHandle;

use super::audit_log::{self, AuditEvent};
use super::redact;
use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::Provider;
use crate::store::AppState;

pub const MAX_VERSIONS: usize = 50;
const ENTITY_PROVIDER: &str = "provider";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub version_id: String,
    pub ts: i64,
    /// `baseline` (state before the first recorded change), `create`,
    /// `update`, `delete`, `restore`.
    pub action: String,
    pub source: String,
    /// Masked entity JSON after the change; `None` once deleted.
    pub snapshot: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryFile {
    entity_type: String,
    app: String,
    entity_id: String,
    name: String,
    entries: Vec<HistoryEntry>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryChange {
    /// JSON pointer of the changed leaf.
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryVersion {
    #[serde(flatten)]
    pub entry: HistoryEntry,
    /// Differences from the previous (older) version.
    pub changes: Vec<HistoryChange>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistorySummary {
    pub entity_type: String,
    pub app: String,
    pub entity_id: String,
    pub name: String,
    pub versions: usize,
    pub last_ts: i64,
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreOutcome {
    /// The provider did not exist any more and was re-created.
    pub recreated: bool,
    /// Secret fields that could not be restored and must be re-entered.
    pub missing_secrets: Vec<String>,
}

fn history_root() -> PathBuf {
    crate::config::get_app_config_dir().join("history")
}

fn safe_segment(raw: &str) -> String {
    use sha2::{Digest, Sha256};
    let readable: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(48)
        .collect();
    let digest = Sha256::digest(raw.as_bytes());
    let short: String = digest[..4].iter().map(|b| format!("{b:02x}")).collect();
    format!("{readable}-{short}")
}

fn entity_path(root: &Path, entity_type: &str, app: &str, id: &str) -> PathBuf {
    root.join(format!("{entity_type}s"))
        .join(safe_segment(app))
        .join(format!("{}.json", safe_segment(id)))
}

fn load(path: &Path) -> Option<HistoryFile> {
    let bytes = std::fs::read(path).ok()?;
    match serde_json::from_slice(&bytes) {
        Ok(file) => Some(file),
        Err(e) => {
            log::warn!("[History] ignoring unreadable {}: {e}", path.display());
            None
        }
    }
}

fn save(path: &Path, file: &HistoryFile) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    let body =
        serde_json::to_string_pretty(file).map_err(|source| AppError::JsonSerialize { source })?;
    crate::config::atomic_write_secret(path, body.as_bytes())
}

fn new_version_id(entries: &[HistoryEntry], ts: i64) -> String {
    let mut n = 0;
    loop {
        let id = format!("{ts}-{n}");
        if !entries.iter().any(|e| e.version_id == id) {
            return id;
        }
        n += 1;
    }
}

/// Record one change in `root`. Returns whether a version was added.
#[allow(clippy::too_many_arguments)]
fn record_in(
    root: &Path,
    entity_type: &str,
    app: &str,
    id: &str,
    name: &str,
    action: &str,
    source: &str,
    before: Option<&Value>,
    after: Option<&Value>,
) -> Result<bool, AppError> {
    let masked_before = before.map(|v| redact::mask(v).0);
    let masked_after = after.map(|v| redact::mask(v).0);
    if masked_before == masked_after && action != "restore" {
        return Ok(false);
    }
    let path = entity_path(root, entity_type, app, id);
    let mut file = load(&path).unwrap_or_else(|| HistoryFile {
        entity_type: entity_type.to_string(),
        app: app.to_string(),
        entity_id: id.to_string(),
        ..Default::default()
    });
    file.name = name.to_string();
    let ts = chrono::Utc::now().timestamp_millis();
    let last_snapshot = file.entries.last().map(|e| e.snapshot.clone());
    if let Some(before) = masked_before {
        if last_snapshot.as_ref() != Some(&Some(before.clone())) {
            let version_id = new_version_id(&file.entries, ts);
            file.entries.push(HistoryEntry {
                version_id,
                ts,
                action: "baseline".to_string(),
                source: "system".to_string(),
                snapshot: Some(before),
            });
        }
    }
    let version_id = new_version_id(&file.entries, ts);
    file.entries.push(HistoryEntry {
        version_id,
        ts,
        action: action.to_string(),
        source: source.to_string(),
        snapshot: masked_after,
    });
    if file.entries.len() > MAX_VERSIONS {
        let excess = file.entries.len() - MAX_VERSIONS;
        file.entries.drain(..excess);
    }
    save(&path, &file)?;
    Ok(true)
}

fn flatten(value: &Value, pointer: &str, out: &mut Vec<(String, Value)>) {
    match value {
        Value::Object(map) if !map.is_empty() => {
            for (k, v) in map {
                let seg = k.replace('~', "~0").replace('/', "~1");
                flatten(v, &format!("{pointer}/{seg}"), out);
            }
        }
        Value::Array(items) if !items.is_empty() => {
            for (i, v) in items.iter().enumerate() {
                flatten(v, &format!("{pointer}/{i}"), out);
            }
        }
        other => out.push((pointer.to_string(), other.clone())),
    }
}

/// Leaf-level differences between two snapshots.
pub fn diff(before: Option<&Value>, after: Option<&Value>) -> Vec<HistoryChange> {
    let mut old = Vec::new();
    let mut new = Vec::new();
    if let Some(v) = before {
        flatten(v, "", &mut old);
    }
    if let Some(v) = after {
        flatten(v, "", &mut new);
    }
    let old_map: std::collections::HashMap<_, _> = old.iter().cloned().collect();
    let new_map: std::collections::HashMap<_, _> = new.iter().cloned().collect();
    let mut changes = Vec::new();
    for (path, value) in &old {
        match new_map.get(path) {
            Some(n) if n == value => {}
            n => changes.push(HistoryChange {
                path: path.clone(),
                before: Some(value.clone()),
                after: n.cloned(),
            }),
        }
    }
    for (path, value) in &new {
        if !old_map.contains_key(path) {
            changes.push(HistoryChange {
                path: path.clone(),
                before: None,
                after: Some(value.clone()),
            });
        }
    }
    changes
}

fn versions_in(root: &Path, entity_type: &str, app: &str, id: &str) -> Vec<HistoryVersion> {
    let Some(file) = load(&entity_path(root, entity_type, app, id)) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(file.entries.len());
    let mut previous: Option<Value> = None;
    for entry in file.entries {
        let changes = diff(previous.as_ref(), entry.snapshot.as_ref());
        previous = entry.snapshot.clone();
        out.push(HistoryVersion { entry, changes });
    }
    out.reverse();
    out
}

fn summaries_in(root: &Path) -> Vec<HistorySummary> {
    let mut out = Vec::new();
    let Ok(types) = std::fs::read_dir(root) else {
        return out;
    };
    for type_dir in types.flatten() {
        let Ok(apps) = std::fs::read_dir(type_dir.path()) else {
            continue;
        };
        for app_dir in apps.flatten() {
            let Ok(files) = std::fs::read_dir(app_dir.path()) else {
                continue;
            };
            for f in files.flatten() {
                let Some(file) = load(&f.path()) else {
                    continue;
                };
                let Some(last) = file.entries.last() else {
                    continue;
                };
                out.push(HistorySummary {
                    entity_type: file.entity_type.clone(),
                    app: file.app.clone(),
                    entity_id: file.entity_id.clone(),
                    name: file.name.clone(),
                    versions: file.entries.len(),
                    last_ts: last.ts,
                    deleted: last.snapshot.is_none(),
                });
            }
        }
    }
    out.sort_by_key(|s| std::cmp::Reverse(s.last_ts));
    out
}

fn provider_value(provider: &Provider) -> Option<Value> {
    serde_json::to_value(provider).ok()
}

/// Record a provider change in the history and the audit log. Never fails
/// the mutation that already happened.
pub fn record_provider(
    app: &AppType,
    id: &str,
    action: &str,
    source: &str,
    before: Option<&Provider>,
    after: Option<&Provider>,
) {
    let before_v = before.and_then(provider_value);
    let after_v = after.and_then(provider_value);
    let name = after.or(before).map(|p| p.name.clone()).unwrap_or_default();
    match record_in(
        &history_root(),
        ENTITY_PROVIDER,
        app.as_str(),
        id,
        &name,
        action,
        source,
        before_v.as_ref(),
        after_v.as_ref(),
    ) {
        Ok(false) if action == "update" => return,
        Ok(_) => {}
        Err(e) => log::warn!("[History] could not record {action} of a provider: {e}"),
    }
    audit_log::record(
        AuditEvent::new(source, &format!("provider.{action}"), ENTITY_PROVIDER, id)
            .app(app.as_str())
            .states(before_v.as_ref(), after_v.as_ref()),
    );
}

fn read_provider(state: &AppState, app: &AppType, id: &str) -> Option<Provider> {
    state.db.get_provider_by_id(id, app.as_str()).ok().flatten()
}

/// Run a provider add/update/delete and record it. `before_id` is the id the
/// row had before the call (differs from `after_id` when an additive-mode
/// provider key is renamed).
pub fn track_provider_change<T>(
    state: &AppState,
    app: &AppType,
    before_id: &str,
    after_id: &str,
    source: &str,
    f: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let before = read_provider(state, app, before_id);
    let result = f()?;
    let after = read_provider(state, app, after_id);
    let action = match (&before, &after) {
        (None, Some(_)) => "create",
        (Some(_), None) => "delete",
        _ => "update",
    };
    if before_id != after_id {
        record_provider(app, before_id, "delete", source, before.as_ref(), None);
        record_provider(app, after_id, "create", source, None, after.as_ref());
    } else {
        record_provider(
            app,
            after_id,
            action,
            source,
            before.as_ref(),
            after.as_ref(),
        );
    }
    Ok(result)
}

/// Run a provider switch and record it: one `provider.switch` audit event,
/// plus a `backfill` history entry when the switch rewrote the previous
/// provider from its live config.
pub fn track_switch<T>(
    state: &AppState,
    app: &AppType,
    target_id: &str,
    source: &str,
    f: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let previous_id = state.db.get_current_provider(app.as_str()).ok().flatten();
    let previous_before = previous_id
        .as_deref()
        .and_then(|id| read_provider(state, app, id));
    let result = f()?;
    if let (Some(id), Some(before)) = (previous_id.as_deref(), previous_before.as_ref()) {
        let after = read_provider(state, app, id);
        if after.as_ref().and_then(provider_value) != provider_value(before) {
            record_provider(app, id, "update", "backfill", Some(before), after.as_ref());
        }
    }
    let mut event =
        AuditEvent::new(source, "provider.switch", ENTITY_PROVIDER, target_id).app(app.as_str());
    if let Some(prev) = previous_id.filter(|p| p != target_id) {
        event = event.detail(format!("from {prev}"));
    }
    audit_log::record(event);
    Ok(result)
}

fn restore_in(
    state: &AppState,
    root: &Path,
    app: &AppType,
    id: &str,
    version_id: &str,
) -> Result<RestoreOutcome, AppError> {
    let file = load(&entity_path(root, ENTITY_PROVIDER, app.as_str(), id))
        .ok_or_else(|| AppError::Message(format!("No history for provider {id}")))?;
    let entry = file
        .entries
        .iter()
        .find(|e| e.version_id == version_id)
        .ok_or_else(|| AppError::Message(format!("Unknown version {version_id}")))?;
    let mut snapshot = entry.snapshot.clone().ok_or_else(|| {
        AppError::Message("This version records the deletion; pick an earlier one".to_string())
    })?;
    let current = read_provider(state, app, id);
    let current_v = current.as_ref().and_then(provider_value);
    let missing_secrets = redact::fill_masked(&mut snapshot, current_v.as_ref());
    let mut provider: Provider = serde_json::from_value(snapshot)
        .map_err(|e| AppError::Message(format!("Version {version_id} is not a provider: {e}")))?;
    provider.id = id.to_string();
    let recreated = current.is_none();
    if recreated {
        crate::services::ProviderService::add(state, app.clone(), provider.clone(), true)?;
    } else {
        crate::services::ProviderService::update(state, app.clone(), None, provider.clone())?;
    }
    let after = read_provider(state, app, id);
    let before_v = current_v;
    let after_v = after.as_ref().and_then(provider_value);
    if let Err(e) = record_in(
        root,
        ENTITY_PROVIDER,
        app.as_str(),
        id,
        &provider.name,
        "restore",
        "ui",
        before_v.as_ref(),
        after_v.as_ref(),
    ) {
        log::warn!("[History] could not record restore: {e}");
    }
    audit_log::record(
        AuditEvent::new("ui", "provider.restore", ENTITY_PROVIDER, id)
            .app(app.as_str())
            .states(before_v.as_ref(), after_v.as_ref())
            .detail(format!("version {version_id}")),
    );
    Ok(RestoreOutcome {
        recreated,
        missing_secrets,
    })
}

fn parse_app(app: &str) -> Result<AppType, String> {
    AppType::from_str(app).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_config_history() -> Vec<HistorySummary> {
    summaries_in(&history_root())
}

/// Versions of one provider, newest first, each with its diff from the
/// previous version.
#[tauri::command]
pub fn get_provider_history(app: String, id: String) -> Result<Vec<HistoryVersion>, String> {
    let app = parse_app(&app)?;
    Ok(versions_in(
        &history_root(),
        ENTITY_PROVIDER,
        app.as_str(),
        &id,
    ))
}

#[tauri::command]
pub async fn restore_provider_version(
    handle: AppHandle,
    app: String,
    id: String,
    #[allow(non_snake_case)] versionId: String,
) -> Result<RestoreOutcome, String> {
    let app = parse_app(&app)?;
    crate::commands::run_blocking_with_state(handle, move |state| {
        restore_in(state, &history_root(), &app, &id, &versionId).map_err(|e| e.to_string())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;

    fn provider(id: &str, url: &str, key: &str) -> Provider {
        Provider::with_id(
            id.to_string(),
            format!("Provider {id}"),
            json!({ "env": { "ANTHROPIC_BASE_URL": url, "ANTHROPIC_AUTH_TOKEN": key } }),
            None,
        )
    }

    #[test]
    fn three_edits_yield_three_diffs_without_secrets_on_disk() {
        let root = tempfile::tempdir().unwrap();
        let mut prev = serde_json::to_value(provider("p", "https://a", "sk-secret-0")).unwrap();
        for i in 1..=3 {
            let next = serde_json::to_value(provider(
                "p",
                &format!("https://a{i}"),
                &format!("sk-secret-{i}"),
            ))
            .unwrap();
            assert!(record_in(
                root.path(),
                "provider",
                "claude",
                "p",
                "P",
                "update",
                "ui",
                Some(&prev),
                Some(&next)
            )
            .unwrap());
            prev = next;
        }
        let versions = versions_in(root.path(), "provider", "claude", "p");
        // baseline + three updates, newest first.
        assert_eq!(versions.len(), 4);
        assert_eq!(versions[3].entry.action, "baseline");
        for v in &versions[..3] {
            assert_eq!(v.entry.action, "update");
            assert!(v
                .changes
                .iter()
                .any(|c| c.path == "/settingsConfig/env/ANTHROPIC_BASE_URL"));
        }
        let on_disk =
            std::fs::read_to_string(entity_path(root.path(), "provider", "claude", "p")).unwrap();
        assert!(!on_disk.contains("sk-secret"));
        let summaries = summaries_in(root.path());
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].versions, 4);
        assert!(!summaries[0].deleted);
    }

    #[test]
    fn unchanged_updates_are_not_recorded_and_history_is_bounded() {
        let root = tempfile::tempdir().unwrap();
        let v = json!({ "a": 1 });
        assert!(!record_in(
            root.path(),
            "provider",
            "codex",
            "x",
            "X",
            "update",
            "ui",
            Some(&v),
            Some(&v)
        )
        .unwrap());
        for i in 0..(MAX_VERSIONS + 10) {
            let next = json!({ "a": i + 2 });
            record_in(
                root.path(),
                "provider",
                "codex",
                "x",
                "X",
                "update",
                "ui",
                None,
                Some(&next),
            )
            .unwrap();
        }
        assert_eq!(
            versions_in(root.path(), "provider", "codex", "x").len(),
            MAX_VERSIONS
        );
    }

    #[test]
    fn diff_reports_added_removed_and_changed_leaves() {
        let changes = diff(
            Some(&json!({ "a": 1, "b": { "c": "x" } })),
            Some(&json!({ "a": 2, "d": true })),
        );
        let paths: Vec<_> = changes.iter().map(|c| c.path.as_str()).collect();
        assert!(paths.contains(&"/a"));
        assert!(paths.contains(&"/b/c"));
        assert!(paths.contains(&"/d"));
        assert_eq!(diff(None, Some(&json!({"a": 1})))[0].before, None);
    }

    fn state() -> AppState {
        AppState::new(Arc::new(crate::database::Database::memory().unwrap()))
    }

    #[test]
    // Provider writes update the process-wide settings cache.
    #[serial_test::serial]
    fn rollback_restores_the_version_and_keeps_the_current_key() {
        let _home = crate::test_support::isolated_home();
        let state = state();
        let app = AppType::Claude;
        // Not current (another provider is), so no live write is needed.
        state
            .db
            .save_provider("claude", &provider("other", "https://o", "k"))
            .unwrap();
        state.db.set_current_provider("claude", "other").unwrap();
        state
            .db
            .save_provider("claude", &provider("p", "https://v1", "sk-1"))
            .unwrap();
        let root = tempfile::tempdir().unwrap();

        let before = read_provider(&state, &app, "p").unwrap();
        let v1 = provider_value(&before).unwrap();
        state
            .db
            .save_provider("claude", &provider("p", "https://v2", "sk-2"))
            .unwrap();
        let v2 = provider_value(&read_provider(&state, &app, "p").unwrap()).unwrap();
        record_in(
            root.path(),
            "provider",
            "claude",
            "p",
            "P",
            "update",
            "ui",
            Some(&v1),
            Some(&v2),
        )
        .unwrap();

        let versions = versions_in(root.path(), "provider", "claude", "p");
        let baseline = versions.last().unwrap().entry.version_id.clone();
        let outcome = restore_in(&state, root.path(), &app, "p", &baseline).unwrap();
        assert!(!outcome.recreated);
        assert!(outcome.missing_secrets.is_empty());
        let restored = read_provider(&state, &app, "p").unwrap();
        assert_eq!(
            restored.settings_config["env"]["ANTHROPIC_BASE_URL"],
            "https://v1"
        );
        // The masked key was filled from the current row, not the masked marker.
        assert_eq!(
            restored.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"],
            "sk-2"
        );
        assert_eq!(
            versions_in(root.path(), "provider", "claude", "p")[0]
                .entry
                .action,
            "restore"
        );
    }

    #[test]
    // Provider writes update the process-wide settings cache.
    #[serial_test::serial]
    fn restoring_a_deleted_provider_recreates_it_and_reports_the_missing_key() {
        let _home = crate::test_support::isolated_home();
        let state = state();
        let app = AppType::Claude;
        state
            .db
            .save_provider("claude", &provider("other", "https://o", "k"))
            .unwrap();
        state.db.set_current_provider("claude", "other").unwrap();
        let root = tempfile::tempdir().unwrap();
        let gone = provider_value(&provider("p", "https://v1", "sk-1")).unwrap();
        record_in(
            root.path(),
            "provider",
            "claude",
            "p",
            "P",
            "delete",
            "ui",
            Some(&gone),
            None,
        )
        .unwrap();
        let versions = versions_in(root.path(), "provider", "claude", "p");
        assert!(versions[0].entry.snapshot.is_none());
        assert!(restore_in(
            &state,
            root.path(),
            &app,
            "p",
            &versions[0].entry.version_id
        )
        .is_err());
        let outcome = restore_in(
            &state,
            root.path(),
            &app,
            "p",
            &versions[1].entry.version_id,
        )
        .unwrap();
        assert!(outcome.recreated);
        assert_eq!(
            outcome.missing_secrets,
            vec!["/settingsConfig/env/ANTHROPIC_AUTH_TOKEN".to_string()]
        );
        assert!(read_provider(&state, &app, "p").is_some());
    }
}
