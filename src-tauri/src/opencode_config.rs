use crate::adapters::json5_doc::Json5Doc;
use crate::adapters::live_file::{
    read_text_lenient, to_json_string, write_live_text, JsonStyle, WriteOptions,
};
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

/// OpenCode config directory and where it came from (CFG-12).
///
/// cc-switch override → `$XDG_CONFIG_HOME/opencode` (OpenCode resolves its
/// global config with xdg-basedir on every platform) → `~/.config/opencode`.
pub fn resolve_opencode_dir() -> (PathBuf, crate::config::ConfigDirSource) {
    crate::config::resolve_tool_dir(
        get_opencode_override_dir(),
        crate::config::tool_env_dir("XDG_CONFIG_HOME")
            .map(|dir| ("XDG_CONFIG_HOME", dir.join("opencode"))),
        || {
            crate::config::get_home_dir()
                .join(".config")
                .join("opencode")
        },
    )
}

pub fn get_opencode_dir() -> PathBuf {
    resolve_opencode_dir().0
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

pub fn read_opencode_config() -> Result<Value, AppError> {
    let path = get_opencode_config_path();

    let Some(live) = read_text_lenient(&path)? else {
        return Ok(json!({
            "$schema": "https://opencode.ai/config.json"
        }));
    };

    json_five::from_str(&live.text).map_err(|e| {
        AppError::Config(format!(
            "Failed to parse OpenCode config: {}: {e}",
            path.display()
        ))
    })
}

/// Write the full OpenCode config.
///
/// `opencode.json(c)` is user-maintained JSONC: instead of re-serialising the
/// whole value (which used to drop every comment and alphabetise all keys),
/// the existing document is patched so that only the keys that actually
/// changed are rewritten (CFG-07). The previous file is backed up first and
/// nothing is written when the content is unchanged.
pub fn write_opencode_config(config: &Value) -> Result<(), AppError> {
    let path = get_opencode_config_path();
    let content = match read_text_lenient(&path)? {
        Some(live) => match Json5Doc::parse(&live.text) {
            Ok(mut doc) => {
                doc.apply(config)?;
                live.style.apply(&doc.render())
            }
            Err(err) => {
                // The round-trip parser is stricter than the reader; fall back
                // to a plain rewrite (the old content is backed up below).
                log::warn!("OpenCode config could not be edited in place ({err}); rewriting it");
                to_json_string(config, &JsonStyle::detect(&live.text))?
            }
        },
        None => to_json_string(config, &JsonStyle::default())?,
    };

    write_live_text(&path, &content, WriteOptions::backup("opencode").secret())?;

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
    let mut full_config = read_opencode_config()?;

    if full_config.get("provider").is_none() {
        full_config["provider"] = json!({});
    }

    if let Some(providers) = full_config
        .get_mut("provider")
        .and_then(|v| v.as_object_mut())
    {
        providers.insert(id.to_string(), config);
    }

    write_opencode_config(&full_config)
}

pub fn remove_provider(id: &str) -> Result<(), AppError> {
    let mut config = read_opencode_config()?;

    if let Some(providers) = config.get_mut("provider").and_then(|v| v.as_object_mut()) {
        providers.remove(id);
    }

    write_opencode_config(&config)
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
    let mut full_config = read_opencode_config()?;

    if full_config.get("mcp").is_none() {
        full_config["mcp"] = json!({});
    }

    if let Some(mcp) = full_config.get_mut("mcp").and_then(|v| v.as_object_mut()) {
        mcp.insert(id.to_string(), config);
    }

    write_opencode_config(&full_config)
}

pub fn remove_mcp_server(id: &str) -> Result<(), AppError> {
    let mut config = read_opencode_config()?;

    if let Some(mcp) = config.get_mut("mcp").and_then(|v| v.as_object_mut()) {
        mcp.remove(id);
    }

    write_opencode_config(&config)
}

pub fn add_plugin(plugin_name: &str) -> Result<(), AppError> {
    let mut config = read_opencode_config()?;
    let normalized_plugin_name = canonicalize_plugin_name(plugin_name);

    let plugins = config.get_mut("plugin").and_then(|v| v.as_array_mut());

    match plugins {
        Some(arr) => {
            // Mutual exclusion: standard OMO and OMO Slim cannot coexist as plugins
            if matches_any_plugin_prefix(&normalized_plugin_name, &STANDARD_OMO_PLUGIN_PREFIXES) {
                arr.retain(|v| {
                    v.as_str()
                        .map(|s| {
                            !matches_any_plugin_prefix(s, &STANDARD_OMO_PLUGIN_PREFIXES)
                                && !matches_any_plugin_prefix(s, &SLIM_OMO_PLUGIN_PREFIXES)
                        })
                        .unwrap_or(true)
                });
            } else if matches_any_plugin_prefix(&normalized_plugin_name, &SLIM_OMO_PLUGIN_PREFIXES)
            {
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

    write_opencode_config(&config)
}

pub fn remove_plugins_by_prefixes(prefixes: &[&str]) -> Result<(), AppError> {
    let mut config = read_opencode_config()?;

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

    write_opencode_config(&config)
}

// TEST-20: opencode.json(c) CRUD on a thread-isolated home (no #[serial]).
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::isolated_home;

    fn write_raw(text: &str) {
        let path = get_opencode_config_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn read_raw() -> String {
        std::fs::read_to_string(get_opencode_config_path()).unwrap()
    }

    #[test]
    fn paths_resolve_under_the_test_home() {
        let home = isolated_home();
        assert_eq!(
            get_opencode_config_path(),
            home.path()
                .join(".config")
                .join("opencode")
                .join("opencode.json")
        );
    }

    #[test]
    fn missing_config_reads_as_schema_stub() {
        let _home = isolated_home();
        let config = read_opencode_config().unwrap();
        assert_eq!(config["$schema"], "https://opencode.ai/config.json");
        assert!(get_providers().unwrap().is_empty());
        assert!(get_mcp_servers().unwrap().is_empty());
    }

    #[test]
    fn provider_crud_roundtrip() {
        let _home = isolated_home();
        set_provider(
            "a",
            json!({"npm": "@ai-sdk/anthropic", "options": {"apiKey": "k1"}}),
        )
        .unwrap();
        set_provider("b", json!({"npm": "@ai-sdk/openai-compatible"})).unwrap();
        let providers = get_providers().unwrap();
        assert_eq!(providers.len(), 2);
        assert_eq!(providers["a"]["options"]["apiKey"], "k1");

        set_provider(
            "a",
            json!({"npm": "@ai-sdk/anthropic", "options": {"apiKey": "k2"}}),
        )
        .unwrap();
        assert_eq!(get_providers().unwrap()["a"]["options"]["apiKey"], "k2");

        remove_provider("a").unwrap();
        let providers = get_providers().unwrap();
        assert!(!providers.contains_key("a"));
        assert!(providers.contains_key("b"), "other providers are kept");
    }

    #[test]
    fn typed_providers_skip_malformed_entries() {
        let _home = isolated_home();
        let typed = OpenCodeProviderConfig::default();
        set_typed_provider("good", &typed).unwrap();
        set_provider("bad", json!({"npm": 42})).unwrap();
        let parsed = get_typed_providers().unwrap();
        assert!(parsed.contains_key("good"));
        assert!(
            !parsed.contains_key("bad"),
            "a bad entry does not fail the read"
        );
    }

    #[test]
    fn mcp_server_crud_roundtrip() {
        let _home = isolated_home();
        set_mcp_server("fs", json!({"type": "local", "command": ["npx", "fs"]})).unwrap();
        assert_eq!(get_mcp_servers().unwrap()["fs"]["type"], "local");
        remove_mcp_server("fs").unwrap();
        assert!(get_mcp_servers().unwrap().is_empty());
        // Removing again is a no-op.
        remove_mcp_server("fs").unwrap();
    }

    #[test]
    fn unrelated_keys_and_comments_survive_provider_edits() {
        let _home = isolated_home();
        write_raw(
            "{\n  // keep me\n  \"theme\": \"dark\",\n  \"provider\": {\n    \"a\": { \"npm\": \"x\" }\n  }\n}\n",
        );
        set_provider("b", json!({"npm": "y"})).unwrap();
        let text = read_raw();
        assert!(text.contains("// keep me"), "{text}");
        let config = read_opencode_config().unwrap();
        assert_eq!(config["theme"], "dark");
        assert_eq!(config["provider"]["a"]["npm"], "x");
        assert_eq!(config["provider"]["b"]["npm"], "y");
    }

    #[test]
    fn invalid_config_is_an_error_not_a_panic() {
        let _home = isolated_home();
        write_raw("{ \"provider\": ");
        assert!(read_opencode_config().is_err());
        assert!(get_providers().is_err());
        assert!(
            set_provider("a", json!({})).is_err(),
            "never overwrite unparsable files"
        );
        assert_eq!(read_raw(), "{ \"provider\": ");
    }

    #[test]
    fn add_plugin_canonicalises_and_is_idempotent() {
        let _home = isolated_home();
        add_plugin("oh-my-opencode@1.2.3").unwrap();
        add_plugin("oh-my-openagent@1.2.3").unwrap();
        let config = read_opencode_config().unwrap();
        assert_eq!(config["plugin"], json!(["oh-my-openagent@1.2.3"]));
    }

    #[test]
    fn standard_and_slim_omo_plugins_are_mutually_exclusive() {
        let _home = isolated_home();
        write_raw("{\"plugin\": [\"other\", \"oh-my-opencode-slim@1\"]}");
        add_plugin("oh-my-openagent").unwrap();
        assert_eq!(
            read_opencode_config().unwrap()["plugin"],
            json!(["other", "oh-my-openagent"])
        );
        add_plugin("oh-my-opencode-slim@2").unwrap();
        assert_eq!(
            read_opencode_config().unwrap()["plugin"],
            json!(["other", "oh-my-opencode-slim@2"])
        );
    }

    #[test]
    fn remove_plugins_by_prefix_drops_the_empty_array() {
        let _home = isolated_home();
        write_raw("{\"plugin\": [\"oh-my-openagent@3\", \"oh-my-openagentx\"]}");
        remove_plugins_by_prefixes(&["oh-my-openagent"]).unwrap();
        // `oh-my-openagentx` is a different plugin, not a version of the prefix.
        assert_eq!(
            read_opencode_config().unwrap()["plugin"],
            json!(["oh-my-openagentx"])
        );
        remove_plugins_by_prefixes(&["oh-my-openagentx"]).unwrap();
        assert!(read_opencode_config().unwrap().get("plugin").is_none());
    }

    #[test]
    fn plugin_prefix_matching_rules() {
        assert!(matches_plugin_prefix("oh-my-opencode", "oh-my-opencode"));
        assert!(matches_plugin_prefix(
            "oh-my-opencode@1.0",
            "oh-my-opencode"
        ));
        assert!(!matches_plugin_prefix(
            "oh-my-opencode-slim",
            "oh-my-opencode"
        ));
        assert_eq!(
            canonicalize_plugin_name("oh-my-opencode"),
            "oh-my-openagent"
        );
        assert_eq!(
            canonicalize_plugin_name("oh-my-opencode-slim@1"),
            "oh-my-opencode-slim@1"
        );
    }
}
