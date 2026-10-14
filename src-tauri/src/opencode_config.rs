use crate::config::write_json_file;
use crate::error::AppError;
use crate::provider::OpenCodeProviderConfig;
use crate::settings::get_opencode_override_dir;
use indexmap::IndexMap;
use serde_json::{json, Map, Value};
use std::path::PathBuf;

const STANDARD_OMO_PLUGIN_PREFIXES: [&str; 2] = ["oh-my-openagent", "oh-my-opencode"];
const SLIM_OMO_PLUGIN_PREFIXES: [&str; 1] = ["oh-my-opencode-slim"];

fn matches_plugin_prefix(plugin_name: &str, prefix: &str) -> bool {
    plugin_name == prefix
        || plugin_name
            .strip_prefix(prefix)
            .map(|suffix| suffix.starts_with('@'))
            .unwrap_or(false)
}

fn matches_any_plugin_prefix(plugin_name: &str, prefixes: &[&str]) -> bool {
    prefixes
        .iter()
        .any(|prefix| matches_plugin_prefix(plugin_name, prefix))
}

fn canonicalize_plugin_name(plugin_name: &str) -> String {
    if let Some(suffix) = plugin_name.strip_prefix("oh-my-opencode") {
        if suffix.is_empty() || suffix.starts_with('@') {
            return format!("oh-my-openagent{suffix}");
        }
    }
    plugin_name.to_string()
}

pub fn get_opencode_dir() -> PathBuf {
    if let Some(override_dir) = get_opencode_override_dir() {
        return override_dir;
    }

    crate::config::get_home_dir()
        .join(".config")
        .join("opencode")
}

pub fn get_opencode_config_path() -> PathBuf {
    get_opencode_dir().join("opencode.json")
}

/// 获取 OpenCode SQLite 数据库路径
/// 优先级: OPENCODE_DB 环境变量 > XDG_DATA_HOME > ~/.local/share/opencode
pub fn get_opencode_db_path() -> PathBuf {
    // 支持 OPENCODE_DB 环境变量覆盖（忽略空字符串）
    if let Ok(custom_path) = std::env::var("OPENCODE_DB") {
        if !custom_path.is_empty() {
            let path = PathBuf::from(&custom_path);
            if path.is_absolute() {
                return path;
            }
            // 相对路径基于数据目录
            return get_opencode_data_dir().join(path);
        }
    }

    get_opencode_data_dir().join("opencode.db")
}

fn get_opencode_data_dir() -> PathBuf {
    // 尊重 XDG_DATA_HOME（按 XDG 规范，空字符串视为未设置）
    if let Ok(xdg_data) = std::env::var("XDG_DATA_HOME") {
        if !xdg_data.is_empty() {
            return PathBuf::from(xdg_data).join("opencode");
        }
    }

    // OpenCode 使用 xdg-basedir，不遵守 macOS/Windows 平台约定，
    // 所有平台默认都落在 ~/.local/share/opencode
    crate::config::get_home_dir()
        .join(".local")
        .join("share")
        .join("opencode")
}

#[allow(dead_code)]
pub fn get_opencode_env_path() -> PathBuf {
    get_opencode_dir().join(".env")
}

pub fn read_opencode_config() -> Result<Value, AppError> {
    let path = get_opencode_config_path();

    if !path.exists() {
        return Ok(json!({
            "$schema": "https://opencode.ai/config.json"
        }));
    }

    let content = std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    json5::from_str(&content).map_err(|e| {
        AppError::Config(format!(
            "Failed to parse OpenCode config: {}: {e}",
            path.display()
        ))
    })
}

pub fn write_opencode_config(config: &Value) -> Result<(), AppError> {
    let path = get_opencode_config_path();
    write_json_file(&path, config)?;

    log::debug!("OpenCode config written to {path:?}");
    Ok(())
}

pub fn get_providers() -> Result<Map<String, Value>, AppError> {
    let config = read_opencode_config()?;
    Ok(config
        .get("provider")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

pub fn set_provider(id: &str, config: Value) -> Result<(), AppError> {
    let full_config = read_opencode_config()?;
    write_opencode_config(&with_section_entry(full_config, "provider", id, config))
}

pub fn remove_provider(id: &str) -> Result<(), AppError> {
    let config = read_opencode_config()?;
    write_opencode_config(&without_section_entry(config, "provider", id))
}

pub fn get_typed_providers() -> Result<IndexMap<String, OpenCodeProviderConfig>, AppError> {
    let providers = get_providers()?;
    let mut result = IndexMap::new();

    for (id, value) in providers {
        match serde_json::from_value::<OpenCodeProviderConfig>(value.clone()) {
            Ok(config) => {
                result.insert(id, config);
            }
            Err(e) => {
                log::warn!("Failed to parse provider '{id}': {e}");
            }
        }
    }

    Ok(result)
}

pub fn set_typed_provider(id: &str, config: &OpenCodeProviderConfig) -> Result<(), AppError> {
    let value = serde_json::to_value(config).map_err(|e| AppError::JsonSerialize { source: e })?;
    set_provider(id, value)
}

pub fn get_mcp_servers() -> Result<Map<String, Value>, AppError> {
    let config = read_opencode_config()?;
    Ok(config
        .get("mcp")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

pub fn set_mcp_server(id: &str, config: Value) -> Result<(), AppError> {
    let full_config = read_opencode_config()?;
    write_opencode_config(&with_section_entry(full_config, "mcp", id, config))
}

pub fn remove_mcp_server(id: &str) -> Result<(), AppError> {
    let config = read_opencode_config()?;
    write_opencode_config(&without_section_entry(config, "mcp", id))
}

pub fn add_plugin(plugin_name: &str) -> Result<(), AppError> {
    let config = read_opencode_config()?;
    write_opencode_config(&apply_plugin(config, plugin_name))
}

pub fn remove_plugins_by_prefixes(prefixes: &[&str]) -> Result<(), AppError> {
    let config = read_opencode_config()?;
    write_opencode_config(&strip_plugins_by_prefixes(config, prefixes))
}

// ── Pure config transforms ──────────────────────────────────
//
// The file-backed functions above are thin read → transform → write wrappers
// around these, so the merge rules can be unit-tested without touching disk.

/// Insert (or replace) `config[section][id] = value`, creating `section` as an
/// empty object when absent. An existing non-object `section` is left as-is.
fn with_section_entry(mut config: Value, section: &str, id: &str, value: Value) -> Value {
    if config.get(section).is_none() {
        config[section] = json!({});
    }

    if let Some(entries) = config.get_mut(section).and_then(|v| v.as_object_mut()) {
        entries.insert(id.to_string(), value);
    }

    config
}

/// Remove `config[section][id]` if present. The (possibly now empty) section
/// object is kept.
fn without_section_entry(mut config: Value, section: &str, id: &str) -> Value {
    if let Some(entries) = config.get_mut(section).and_then(|v| v.as_object_mut()) {
        entries.remove(id);
    }

    config
}

/// Add `plugin_name` (canonicalized) to the `plugin` array.
///
/// Standard OMO and OMO Slim are mutually exclusive: adding either one first
/// removes every existing entry from both families (any version). Duplicates
/// are not added twice. A missing or non-array `plugin` value is replaced by a
/// single-element array.
fn apply_plugin(mut config: Value, plugin_name: &str) -> Value {
    let normalized_plugin_name = canonicalize_plugin_name(plugin_name);

    match config.get_mut("plugin").and_then(|v| v.as_array_mut()) {
        Some(arr) => {
            let is_omo_family =
                matches_any_plugin_prefix(&normalized_plugin_name, &STANDARD_OMO_PLUGIN_PREFIXES)
                    || matches_any_plugin_prefix(
                        &normalized_plugin_name,
                        &SLIM_OMO_PLUGIN_PREFIXES,
                    );
            if is_omo_family {
                arr.retain(|v| {
                    v.as_str()
                        .map(|s| {
                            !matches_any_plugin_prefix(s, &STANDARD_OMO_PLUGIN_PREFIXES)
                                && !matches_any_plugin_prefix(s, &SLIM_OMO_PLUGIN_PREFIXES)
                        })
                        .unwrap_or(true)
                });
            }

            let already_exists = arr
                .iter()
                .any(|v| v.as_str() == Some(normalized_plugin_name.as_str()));
            if !already_exists {
                arr.push(Value::String(normalized_plugin_name));
            }
        }
        None => {
            config["plugin"] = json!([normalized_plugin_name]);
        }
    }

    config
}

/// Drop every string `plugin` entry matching one of `prefixes` (exact name or
/// `name@version`); non-string entries are kept. Removes the `plugin` key
/// entirely when the array ends up empty.
fn strip_plugins_by_prefixes(mut config: Value, prefixes: &[&str]) -> Value {
    if let Some(arr) = config.get_mut("plugin").and_then(|v| v.as_array_mut()) {
        arr.retain(|v| {
            v.as_str()
                .map(|s| !matches_any_plugin_prefix(s, prefixes))
                .unwrap_or(true)
        });

        if arr.is_empty() {
            config.as_object_mut().map(|obj| obj.remove("plugin"));
        }
    }

    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_prefix_matches_exact_name_or_version_suffix_only() {
        assert!(matches_plugin_prefix("oh-my-opencode", "oh-my-opencode"));
        assert!(matches_plugin_prefix(
            "oh-my-opencode@1.2.3",
            "oh-my-opencode"
        ));
        assert!(matches_plugin_prefix(
            "oh-my-opencode@latest",
            "oh-my-opencode"
        ));
        // A longer package name sharing the prefix is a different plugin
        assert!(!matches_plugin_prefix(
            "oh-my-opencode-slim",
            "oh-my-opencode"
        ));
        assert!(!matches_plugin_prefix(
            "oh-my-opencode-slim@1.0",
            "oh-my-opencode"
        ));
        assert!(!matches_plugin_prefix("oh-my-open", "oh-my-opencode"));
        assert!(!matches_plugin_prefix("", "oh-my-opencode"));
    }

    #[test]
    fn standard_and_slim_prefix_sets_are_disjoint() {
        for name in ["oh-my-opencode", "oh-my-openagent@2.0.0"] {
            assert!(matches_any_plugin_prefix(
                name,
                &STANDARD_OMO_PLUGIN_PREFIXES
            ));
            assert!(!matches_any_plugin_prefix(name, &SLIM_OMO_PLUGIN_PREFIXES));
        }
        let slim = "oh-my-opencode-slim@0.3.1";
        assert!(matches_any_plugin_prefix(slim, &SLIM_OMO_PLUGIN_PREFIXES));
        assert!(!matches_any_plugin_prefix(
            slim,
            &STANDARD_OMO_PLUGIN_PREFIXES
        ));
    }

    #[test]
    fn canonicalize_renames_legacy_package_but_keeps_slim_and_others() {
        assert_eq!(
            canonicalize_plugin_name("oh-my-opencode"),
            "oh-my-openagent"
        );
        assert_eq!(
            canonicalize_plugin_name("oh-my-opencode@1.2.3"),
            "oh-my-openagent@1.2.3"
        );
        assert_eq!(
            canonicalize_plugin_name("oh-my-opencode-slim@1.0"),
            "oh-my-opencode-slim@1.0"
        );
        assert_eq!(
            canonicalize_plugin_name("oh-my-openagent"),
            "oh-my-openagent"
        );
        assert_eq!(canonicalize_plugin_name("other-plugin"), "other-plugin");
    }

    fn plugins(config: &Value) -> Vec<&str> {
        config["plugin"]
            .as_array()
            .expect("plugin array")
            .iter()
            .map(|v| v.as_str().unwrap_or("<non-string>"))
            .collect()
    }

    #[test]
    fn apply_plugin_creates_array_when_missing_or_not_an_array() {
        let out = apply_plugin(json!({"$schema": "x"}), "foo@1");
        assert_eq!(out, json!({"$schema": "x", "plugin": ["foo@1"]}));

        let out = apply_plugin(json!({"plugin": "oops"}), "foo");
        assert_eq!(out["plugin"], json!(["foo"]));
    }

    #[test]
    fn apply_plugin_appends_once_and_preserves_other_entries() {
        let cfg = json!({"plugin": ["a", 42, "b"]});
        let out = apply_plugin(cfg, "c");
        assert_eq!(out["plugin"], json!(["a", 42, "b", "c"]));

        let out = apply_plugin(out, "c");
        assert_eq!(out["plugin"], json!(["a", 42, "b", "c"]));
    }

    #[test]
    fn apply_plugin_canonicalizes_legacy_name() {
        let out = apply_plugin(json!({}), "oh-my-opencode@1.2.3");
        assert_eq!(plugins(&out), vec!["oh-my-openagent@1.2.3"]);
    }

    #[test]
    fn apply_plugin_standard_replaces_any_omo_variant() {
        let cfg = json!({"plugin": [
            "keep-me",
            "oh-my-opencode@0.9",
            "oh-my-openagent@1.0",
            "oh-my-opencode-slim@0.3",
            42
        ]});
        let out = apply_plugin(cfg, "oh-my-openagent@2.0");
        assert_eq!(out["plugin"], json!(["keep-me", 42, "oh-my-openagent@2.0"]));
    }

    #[test]
    fn apply_plugin_slim_replaces_standard_omo() {
        let cfg = json!({"plugin": ["oh-my-openagent@2.0", "other"]});
        let out = apply_plugin(cfg, "oh-my-opencode-slim@0.4");
        assert_eq!(plugins(&out), vec!["other", "oh-my-opencode-slim@0.4"]);
    }

    #[test]
    fn apply_plugin_non_omo_does_not_evict_omo() {
        let cfg = json!({"plugin": ["oh-my-openagent", "oh-my-opencode-slim"]});
        let out = apply_plugin(cfg, "oh-my-opencode-extra");
        assert_eq!(
            plugins(&out),
            vec![
                "oh-my-openagent",
                "oh-my-opencode-slim",
                "oh-my-opencode-extra"
            ]
        );
    }

    #[test]
    fn strip_plugins_removes_matching_entries_only() {
        let cfg = json!({"plugin": [
            "oh-my-opencode-slim@0.3",
            "oh-my-openagent@1",
            "oh-my-opencode-slimmer",
            7
        ]});
        let out = strip_plugins_by_prefixes(cfg, &SLIM_OMO_PLUGIN_PREFIXES);
        assert_eq!(
            out["plugin"],
            json!(["oh-my-openagent@1", "oh-my-opencode-slimmer", 7])
        );
    }

    #[test]
    fn strip_plugins_drops_key_when_array_becomes_empty() {
        let cfg = json!({"$schema": "x", "plugin": ["oh-my-opencode", "oh-my-openagent@3"]});
        let out = strip_plugins_by_prefixes(cfg, &STANDARD_OMO_PLUGIN_PREFIXES);
        assert_eq!(out, json!({"$schema": "x"}));
    }

    #[test]
    fn strip_plugins_leaves_config_without_plugin_array_untouched() {
        for cfg in [json!({"a": 1}), json!({"plugin": "str"})] {
            let out = strip_plugins_by_prefixes(cfg.clone(), &STANDARD_OMO_PLUGIN_PREFIXES);
            assert_eq!(out, cfg);
        }
    }

    #[test]
    fn section_entry_insert_creates_section_and_replaces_existing() {
        let out = with_section_entry(json!({}), "provider", "p1", json!({"a": 1}));
        assert_eq!(out, json!({"provider": {"p1": {"a": 1}}}));

        let out = with_section_entry(out, "provider", "p1", json!({"b": 2}));
        let out = with_section_entry(out, "provider", "p2", json!({}));
        assert_eq!(out, json!({"provider": {"p1": {"b": 2}, "p2": {}}}));
    }

    #[test]
    fn section_entry_insert_ignores_non_object_section() {
        let cfg = json!({"mcp": ["not", "an", "object"]});
        let out = with_section_entry(cfg.clone(), "mcp", "srv", json!({}));
        assert_eq!(out, cfg);
    }

    #[test]
    fn section_entry_remove_keeps_empty_section_and_other_keys() {
        let cfg = json!({"mcp": {"srv": {}}, "provider": {"srv": {}}});
        let out = without_section_entry(cfg, "mcp", "srv");
        assert_eq!(out, json!({"mcp": {}, "provider": {"srv": {}}}));

        let untouched = json!({"x": 1});
        assert_eq!(
            without_section_entry(untouched.clone(), "mcp", "srv"),
            untouched
        );
    }
}
