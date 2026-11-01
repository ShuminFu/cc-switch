//! One merge policy for projecting a cc-switch MCP server onto an entry that
//! already exists in an app's live config (CFG-42).
//!
//! * transport keys ([`CORE_KEYS`]) come from cc-switch's DB spec: a core key
//!   the projection no longer carries is removed from the live entry;
//! * every other key already present in the live entry is preserved
//!   (app-native settings such as Codex `startup_timeout_sec` /
//!   `enabled_tools`, OpenCode `timeout`/`oauth`, Hermes `tools`/`sampling`),
//!   so no per-app allowlist needs upkeep;
//! * `enabled` follows cc-switch's toggle: projecting a server means it is
//!   enabled, so an existing `enabled = false` becomes `true` (apps whose
//!   format has no `enabled` key are left without one).
//!
//! Existing keys keep their position; new keys are appended.

use serde_json::Value;

/// Keys whose value is owned by cc-switch's unified spec, across all apps'
/// native shapes.
pub(crate) const CORE_KEYS: &[&str] = &[
    "type",
    "command",
    "args",
    "env",
    "environment",
    "cwd",
    "url",
    "httpUrl",
    "headers",
    "http_headers",
];

/// Merge a projected JSON entry onto the existing live entry.
pub(crate) fn merge_live_json_entry(existing: Option<&Value>, projected: Value) -> Value {
    let Some(existing) = existing.and_then(Value::as_object) else {
        return projected;
    };
    let Value::Object(projected) = projected else {
        return projected;
    };
    let mut merged = existing.clone();
    for key in CORE_KEYS {
        if !projected.contains_key(*key) {
            merged.shift_remove(*key);
        }
    }
    if merged.contains_key("enabled") && !projected.contains_key("enabled") {
        merged.insert("enabled".to_string(), Value::Bool(true));
    }
    for (key, value) in projected {
        merged.insert(key, value);
    }
    Value::Object(merged)
}

/// TOML counterpart of [`merge_live_json_entry`] (Codex, Grok Build). The
/// existing table's formatting is kept for every key that is not replaced.
pub(crate) fn merge_live_toml_table(
    existing: Option<&toml_edit::Table>,
    projected: toml_edit::Table,
) -> toml_edit::Table {
    let Some(existing) = existing else {
        return projected;
    };
    let mut merged = existing.clone();
    for key in CORE_KEYS {
        if !projected.contains_key(key) {
            merged.remove(key);
        }
    }
    if merged.contains_key("enabled") && !projected.contains_key("enabled") {
        merged.insert("enabled", toml_edit::value(true));
    }
    for (key, item) in projected.iter() {
        merged.insert(key, item.clone());
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_merge_keeps_app_native_keys_and_enables() {
        let existing = json!({
            "type": "local",
            "command": ["old"],
            "environment": {"A": "1"},
            "timeout": 5000,
            "enabled": false
        });
        let projected = json!({"type": "local", "command": ["new"], "enabled": true});
        let merged = merge_live_json_entry(Some(&existing), projected);
        assert_eq!(
            merged,
            json!({"type": "local", "command": ["new"], "timeout": 5000, "enabled": true})
        );
    }

    #[test]
    fn json_merge_sets_existing_enabled_flag_when_projection_has_none() {
        let existing = json!({"command": "x", "enabled": false, "trust": true});
        let merged = merge_live_json_entry(Some(&existing), json!({"command": "y"}));
        assert_eq!(
            merged,
            json!({"command": "y", "enabled": true, "trust": true})
        );
        // no existing entry: projection as-is
        assert_eq!(
            merge_live_json_entry(None, json!({"command": "y"})),
            json!({"command": "y"})
        );
    }

    #[test]
    fn toml_merge_keeps_timeouts_and_tool_filters() {
        let doc: toml_edit::DocumentMut = "[s]\ncommand = \"old\"\nstartup_timeout_sec = 30 # slow\nenabled_tools = [\"a\"]\nenabled = false\n"
            .parse()
            .unwrap();
        let existing = doc["s"].as_table().unwrap();
        let mut projected = toml_edit::Table::new();
        projected.insert("command", toml_edit::value("new"));
        projected.insert(
            "args",
            toml_edit::value(toml_edit::Array::from_iter(["-y"])),
        );
        let merged = merge_live_toml_table(Some(existing), projected);
        let mut out = toml_edit::DocumentMut::new();
        out.insert("s", toml_edit::Item::Table(merged));
        let text = out.to_string();
        assert!(text.contains("startup_timeout_sec = 30 # slow"), "{text}");
        assert!(text.contains("enabled_tools = [\"a\"]"), "{text}");
        assert!(text.contains("enabled = true"), "{text}");
        assert!(text.contains("command = \"new\""), "{text}");
    }
}
