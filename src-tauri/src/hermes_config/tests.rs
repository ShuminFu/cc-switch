//! Unit tests for `hermes_config` (moved out of hermes_config.rs, TEST-29).

use super::*;
use serial_test::serial;
use std::sync::{Mutex, OnceLock};

fn test_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|err| err.into_inner())
}

/// Run a test with an isolated, thread-local temp home (TEST-09): no env
/// vars or shared settings are touched, so these tests need no `#[serial]`.
fn with_test_home<T>(test_fn: impl FnOnce() -> T) -> T {
    let _home = crate::test_support::isolated_home();
    test_fn()
}

/// Env-var based isolation, for the tests that exercise HERMES_HOME /
/// LOCALAPPDATA / settings precedence themselves (must stay `#[serial]`).
///
/// Saves and restores `CC_SWITCH_TEST_HOME` to avoid interfering with
/// parallel tests in other modules.
fn with_env_test_home<T>(test_fn: impl FnOnce() -> T) -> T {
    let _guard = test_guard();
    let tmp = tempfile::tempdir().unwrap();
    let old_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
    std::env::set_var("CC_SWITCH_TEST_HOME", tmp.path());
    // Neutralize the env vars get_hermes_dir() consults, so an ambient
    // HERMES_HOME / LOCALAPPDATA (e.g. set by a Hermes install) can't make
    // tests escape the temp home. Restored below.
    let old_hermes_home = std::env::var_os("HERMES_HOME");
    let old_local_appdata = std::env::var_os("LOCALAPPDATA");
    std::env::remove_var("HERMES_HOME");
    std::env::remove_var("LOCALAPPDATA");
    let result = test_fn();
    match old_local_appdata {
        Some(value) => std::env::set_var("LOCALAPPDATA", value),
        None => std::env::remove_var("LOCALAPPDATA"),
    }
    match old_hermes_home {
        Some(value) => std::env::set_var("HERMES_HOME", value),
        None => std::env::remove_var("HERMES_HOME"),
    }
    match old_test_home {
        Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
        None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
    }
    result
}

// ---- sanitize_hermes_provider_keys tests ----

#[test]
fn sanitize_rewrites_camel_case_aliases() {
    let mut v = serde_json::json!({
        "name": "test",
        "baseUrl": "https://api.example.com",
        "apiKey": "sk-123",
        "apiMode": "chat_completions",
        "maxTokens": 8192,
        "contextLength": 200000,
    });
    sanitize_hermes_provider_keys(&mut v);
    let obj = v.as_object().unwrap();
    assert_eq!(obj.get("base_url").unwrap(), "https://api.example.com");
    assert_eq!(obj.get("api_key").unwrap(), "sk-123");
    assert_eq!(obj.get("api_mode").unwrap(), "chat_completions");
    assert_eq!(obj.get("max_tokens").unwrap(), 8192);
    assert_eq!(obj.get("context_length").unwrap(), 200000);
    assert!(obj.get("baseUrl").is_none());
    assert!(obj.get("apiKey").is_none());
}

#[test]
fn sanitize_drops_stale_duplicate_when_snake_case_exists() {
    let mut v = serde_json::json!({
        "baseUrl": "https://old.example.com",
        "base_url": "https://new.example.com",
    });
    sanitize_hermes_provider_keys(&mut v);
    let obj = v.as_object().unwrap();
    // snake_case wins; stale camelCase is dropped
    assert_eq!(obj.get("base_url").unwrap(), "https://new.example.com");
    assert!(obj.get("baseUrl").is_none());
}

#[test]
fn sanitize_drops_legacy_api_field() {
    let mut v = serde_json::json!({
        "base_url": "https://api.example.com",
        "api": "openai-completions",
    });
    sanitize_hermes_provider_keys(&mut v);
    let obj = v.as_object().unwrap();
    assert!(obj.get("api").is_none(), "legacy 'api' key must be removed");
    assert!(obj.get("base_url").is_some());
}

#[test]
fn sanitize_preserves_unknown_fields() {
    let mut v = serde_json::json!({
        "base_url": "https://api.example.com",
        "request_timeout_seconds": 300,
        "rate_limit_delay": 1.5,
    });
    sanitize_hermes_provider_keys(&mut v);
    let obj = v.as_object().unwrap();
    // Forward-compat: Hermes' own new fields pass through untouched
    assert_eq!(obj.get("request_timeout_seconds").unwrap(), 300);
    assert_eq!(obj.get("rate_limit_delay").unwrap(), 1.5);
}

#[test]
fn sanitize_noop_on_non_object() {
    let mut v = serde_json::json!(["not", "an", "object"]);
    sanitize_hermes_provider_keys(&mut v);
    assert!(v.is_array());
}

// ---- find_yaml_section_range tests ----

#[test]
fn find_section_in_multi_section_yaml() {
    let yaml = "\
model:
  default: gpt-4
  provider: openai
agent:
  max_turns: 10
custom_providers:
  - name: foo
";
    let (start, end) = find_yaml_section_range(yaml, "agent").unwrap();
    let section = &yaml[start..end];
    assert!(section.starts_with("agent:"));
    assert!(section.contains("max_turns"));
    assert!(!section.contains("custom_providers"));
}

#[test]
fn find_section_at_end_of_file() {
    let yaml = "\
model:
  default: gpt-4
agent:
  max_turns: 10
";
    let (start, end) = find_yaml_section_range(yaml, "agent").unwrap();
    let section = &yaml[start..end];
    assert!(section.starts_with("agent:"));
    assert!(section.contains("max_turns"));
    assert_eq!(end, yaml.len());
}

#[test]
fn find_section_not_found() {
    let yaml = "\
model:
  default: gpt-4
";
    assert!(find_yaml_section_range(yaml, "agent").is_none());
}

#[test]
fn find_section_with_comments_between() {
    let yaml = "\
model:
  default: gpt-4

# This is a comment
  # indented comment

agent:
  max_turns: 10
";
    // model section should span from start to "agent:"
    let (start, end) = find_yaml_section_range(yaml, "model").unwrap();
    let section = &yaml[start..end];
    assert!(section.starts_with("model:"));
    // Comments and blank lines between sections are included in the prior section
    assert!(section.contains("# This is a comment"));
}

#[test]
fn find_section_with_empty_lines() {
    let yaml = "\
model:
  default: gpt-4

agent:
  max_turns: 10
";
    let (start, end) = find_yaml_section_range(yaml, "model").unwrap();
    let section = &yaml[start..end];
    assert!(section.starts_with("model:"));
    // Empty lines don't terminate a section
    assert!(section.contains('\n'));
}

#[test]
fn find_section_does_not_match_substring_key() {
    let yaml = "\
model_extra:
  foo: bar
model:
  default: gpt-4
";
    let (start, _end) = find_yaml_section_range(yaml, "model").unwrap();
    let section = &yaml[start..];
    // Should match "model:", not "model_extra:"
    assert!(section.starts_with("model:"));
    assert!(!section.starts_with("model_extra:"));
}

#[test]
fn find_section_handles_crlf() {
    // Regression for #3633: CRLF line endings must not hide sections.
    let yaml = "model:\r\n  default: gpt-4\r\nagent:\r\n  max_turns: 10\r\n";
    let (start, end) = find_yaml_section_range(yaml, "model").unwrap();
    let section = &yaml[start..end];
    assert!(section.starts_with("model:"));
    assert!(section.contains("default: gpt-4"));
    assert!(!section.contains("agent:"));
}

// ---- deduplicate_top_level_keys tests ----

#[test]
fn dedup_keeps_last_occurrence() {
    // Duplicates come from replace-degraded-to-append, so the last block
    // is the newest data and must win (PyYAML last-wins, like Hermes).
    let yaml = "\
model:
  default: gpt-4
agent:
  max_turns: 10
model:
  default: claude-opus-4-8
";
    let result = deduplicate_top_level_keys(yaml);
    assert_eq!(
        result.lines().filter(|l| *l == "model:").count(),
        1,
        "duplicate model: section was not removed"
    );
    assert!(result.contains("claude-opus-4-8"));
    assert!(!result.contains("gpt-4"));
    assert!(result.contains("max_turns"));
}

#[test]
fn dedup_handles_crlf() {
    let yaml = "model:\r\n  default: gpt-4\r\nagent:\r\n  max_turns: 10\r\nmodel:\r\n  default: claude\r\n";
    let result = deduplicate_top_level_keys(yaml);
    assert_eq!(result.lines().filter(|l| l.trim() == "model:").count(), 1);
    assert!(result.contains("default: claude"));
    assert!(!result.contains("gpt-4"));
}

#[test]
fn dedup_is_identity_without_duplicates() {
    let yaml = "\
# Hermes config
model:
  default: gpt-4

agent:
  max_turns: 10
";
    assert_eq!(deduplicate_top_level_keys(yaml), yaml);
}

#[test]
fn dedup_result_parses_with_last_value() {
    // End-to-end: a config that serde_yaml rejects today must parse after
    // healing, and expose the newest (last) value.
    let yaml = "\
custom_providers:
  - name: old-provider
model:
  default: gpt-4
custom_providers:
  - name: old-provider
  - name: new-provider
";
    let healed = deduplicate_top_level_keys(yaml);
    let value: serde_yaml::Value = serde_yaml::from_str(&healed).unwrap();
    let providers = value
        .get("custom_providers")
        .unwrap()
        .as_sequence()
        .unwrap();
    assert_eq!(providers.len(), 2);
    assert_eq!(
        providers[1].get("name").unwrap().as_str().unwrap(),
        "new-provider"
    );
}

// ---- remove_all_sections tests ----

#[test]
fn remove_all_sections_strips_every_occurrence() {
    let yaml = "\
model:
  default: gpt-4
agent:
  max_turns: 10
model:
  default: claude
model:
  default: gemini
";
    let result = remove_all_sections(yaml, "model");
    assert!(!result.contains("model:"));
    assert!(result.contains("agent:"));
    assert!(result.contains("max_turns"));
}

// ---- replace_yaml_section tests ----

#[test]
fn replace_existing_section() {
    let yaml = "\
model:
  default: gpt-4
  provider: openai
agent:
  max_turns: 10
";
    let new_model = serde_yaml::Value::Mapping({
        let mut m = serde_yaml::Mapping::new();
        m.insert(
            serde_yaml::Value::String("default".to_string()),
            serde_yaml::Value::String("claude-opus-4-8".to_string()),
        );
        m.insert(
            serde_yaml::Value::String("provider".to_string()),
            serde_yaml::Value::String("anthropic".to_string()),
        );
        m
    });

    let result = replace_yaml_section(yaml, "model", &new_model).unwrap();
    // The result should still contain the agent section
    assert!(result.contains("agent:"));
    assert!(result.contains("max_turns"));
    // And the model section should be updated
    assert!(result.contains("claude-opus-4-8"));
    assert!(result.contains("anthropic"));
    assert!(!result.contains("gpt-4"));
    assert!(!result.contains("openai"));
}

#[test]
fn replace_section_in_crlf_config_replaces_in_place() {
    // Regression for #3633: on CRLF configs every "replace" used to
    // degrade into an append, piling up duplicate sections.
    let yaml = "model:\r\n  default: gpt-4\r\nagent:\r\n  max_turns: 10\r\n";
    let new_model = serde_yaml::Value::Mapping({
        let mut m = serde_yaml::Mapping::new();
        m.insert(
            serde_yaml::Value::String("default".to_string()),
            serde_yaml::Value::String("claude-opus-4-8".to_string()),
        );
        m
    });

    let result = replace_yaml_section(yaml, "model", &new_model).unwrap();
    assert_eq!(
        result.lines().filter(|l| l.trim() == "model:").count(),
        1,
        "model: must be replaced in place, not appended"
    );
    assert!(result.contains("claude-opus-4-8"));
    assert!(!result.contains("gpt-4"));
    assert!(result.contains("max_turns"));
}

#[test]
fn replace_section_removes_residual_duplicates() {
    // A config already broken by the append bug: replacing the section
    // must also clean the stale duplicate copies after it.
    let yaml = "\
model:
  default: gpt-4
agent:
  max_turns: 10
model:
  default: stale-copy
";
    let new_model = serde_yaml::Value::Mapping({
        let mut m = serde_yaml::Mapping::new();
        m.insert(
            serde_yaml::Value::String("default".to_string()),
            serde_yaml::Value::String("claude-opus-4-8".to_string()),
        );
        m
    });

    let result = replace_yaml_section(yaml, "model", &new_model).unwrap();
    assert_eq!(result.lines().filter(|l| *l == "model:").count(), 1);
    assert!(result.contains("claude-opus-4-8"));
    assert!(!result.contains("stale-copy"));
    assert!(result.contains("agent:"));
    // The healed output must be valid YAML again
    let parsed: Result<serde_yaml::Value, _> = serde_yaml::from_str(&result);
    assert!(parsed.is_ok());
}

#[test]
fn append_new_section() {
    let yaml = "\
model:
  default: gpt-4
";
    let new_agent = serde_yaml::Value::Mapping({
        let mut m = serde_yaml::Mapping::new();
        m.insert(
            serde_yaml::Value::String("max_turns".to_string()),
            serde_yaml::Value::Number(serde_yaml::Number::from(50)),
        );
        m
    });

    let result = replace_yaml_section(yaml, "agent", &new_agent).unwrap();
    assert!(result.contains("model:"));
    assert!(result.contains("gpt-4"));
    assert!(result.contains("agent:"));
    assert!(result.contains("max_turns: 50"));
}

// ---- CFG-08: comment / line-ending preservation ----

const COMMENTED_HERMES: &str = "\
# Hermes config maintained by hand
model:
  default: gpt-4 # the default model
  provider: openai

# providers used by the team
custom_providers:
  # primary gateway
  - name: alpha
    base_url: https://alpha.example/v1
    api_key: sk-alpha # rotate monthly
  # backup gateway
  - name: beta
    base_url: https://beta.example/v1

# MCP servers
mcp_servers:
  # filesystem access
  fs:
    command: npx
    args: [\"-y\", \"@mcp/fs\"]
  # web search
  search:
    url: https://search.example/mcp

# This is a comment about the agent section
defaults: &defaults
  max_turns: 10
agent:
  <<: *defaults
";

fn yaml(text: &str) -> serde_yaml::Value {
    serde_yaml::from_str(text).unwrap()
}

#[test]
fn replace_keeps_comment_preceding_next_section() {
    let new_model = yaml("default: claude\nprovider: anthropic\n");
    let result = replace_yaml_section(COMMENTED_HERMES, "model", &new_model).unwrap();
    assert!(result.contains("# providers used by the team"), "{result}");
    assert!(result.contains("# This is a comment about the agent section"));
    assert!(result.contains("default: claude"));
    // the untouched anchor/merge-key sections are byte-identical
    assert!(result.ends_with("defaults: &defaults\n  max_turns: 10\nagent:\n  <<: *defaults\n"));
}

#[test]
fn replace_sequence_section_keeps_unchanged_items_and_comments() {
    let mut providers = yaml(COMMENTED_HERMES)["custom_providers"].clone();
    let seq = providers.as_sequence_mut().unwrap();
    seq[1]["base_url"] = serde_yaml::Value::String("https://beta2.example/v1".into());
    let result = replace_yaml_section(COMMENTED_HERMES, "custom_providers", &providers).unwrap();
    // untouched entry verbatim, including inline comment
    assert!(result.contains("  # primary gateway\n  - name: alpha\n    base_url: https://alpha.example/v1\n    api_key: sk-alpha # rotate monthly\n"), "{result}");
    // edited entry keeps the comment above it
    assert!(
        result.contains("  # backup gateway\n  - name: beta\n"),
        "{result}"
    );
    assert!(result.contains("https://beta2.example/v1"));
    assert!(result.contains("# MCP servers\nmcp_servers:"));
    let parsed = yaml(&result);
    assert_eq!(parsed["custom_providers"], providers);
}

#[test]
fn replace_mapping_section_only_touches_changed_servers() {
    let mut servers = yaml(COMMENTED_HERMES)["mcp_servers"].clone();
    let map = servers.as_mapping_mut().unwrap();
    map.remove("search");
    map.insert(
        serde_yaml::Value::String("git".into()),
        yaml("command: uvx\nargs: [mcp-git]\n"),
    );
    let result = replace_yaml_section(COMMENTED_HERMES, "mcp_servers", &servers).unwrap();
    assert!(
        result.contains(
            "  # filesystem access\n  fs:\n    command: npx\n    args: [\"-y\", \"@mcp/fs\"]\n"
        ),
        "{result}"
    );
    assert!(!result.contains("search"));
    assert!(!result.contains("# web search"));
    assert!(result.contains("  git:\n"));
    assert!(result.contains("# This is a comment about the agent section"));
    assert_eq!(yaml(&result)["mcp_servers"], servers);
}

#[test]
fn replace_unchanged_section_is_identity() {
    let current = yaml(COMMENTED_HERMES);
    for key in ["model", "custom_providers", "mcp_servers"] {
        let result = replace_yaml_section(COMMENTED_HERMES, key, &current[key]).unwrap();
        assert_eq!(result, COMMENTED_HERMES, "section {key}");
    }
}

#[test]
fn replace_in_crlf_file_keeps_crlf() {
    let crlf = COMMENTED_HERMES.replace('\n', "\r\n");
    let mut servers = yaml(&crlf)["mcp_servers"].clone();
    servers.as_mapping_mut().unwrap().insert(
        serde_yaml::Value::String("git".into()),
        yaml("command: uvx\n"),
    );
    let result = replace_yaml_section(&crlf, "mcp_servers", &servers).unwrap();
    assert!(
        !result.replace("\r\n", "").contains('\n'),
        "mixed line endings: {result:?}"
    );
    assert!(result.contains("  git:\r\n    command: uvx\r\n"));

    let new_model = yaml("default: claude\n");
    let result = replace_yaml_section(&crlf, "model", &new_model).unwrap();
    assert!(!result.replace("\r\n", "").contains('\n'), "{result:?}");
    let appended = replace_yaml_section(&crlf, "memory", &yaml("enabled: true\n")).unwrap();
    assert!(!appended.replace("\r\n", "").contains('\n'), "{appended:?}");
}

#[test]
fn replace_section_in_empty_file() {
    let yaml = "";
    let new_model = serde_yaml::Value::Mapping({
        let mut m = serde_yaml::Mapping::new();
        m.insert(
            serde_yaml::Value::String("default".to_string()),
            serde_yaml::Value::String("gpt-4".to_string()),
        );
        m
    });

    let result = replace_yaml_section(yaml, "model", &new_model).unwrap();
    assert!(result.contains("model:"));
    assert!(result.contains("gpt-4"));
    assert!(result.ends_with('\n'));
}

// ---- Provider CRUD via mock config ----

#[test]
fn provider_crud_roundtrip() {
    with_test_home(|| {
        // Initially no providers
        let providers = get_providers().unwrap();
        assert!(providers.is_empty());

        // Add a provider
        let config = serde_json::json!({
            "base_url": "https://openrouter.ai/api/v1",
            "api_key": "sk-or-test"
        });
        set_provider("openrouter", config).unwrap();

        let providers = get_providers().unwrap();
        assert_eq!(providers.len(), 1);
        assert!(providers.contains_key("openrouter"));

        let provider = get_provider("openrouter").unwrap().unwrap();
        assert_eq!(provider["base_url"], "https://openrouter.ai/api/v1");
        assert_eq!(provider["name"], "openrouter");

        // Update the provider
        let config2 = serde_json::json!({
            "base_url": "https://openrouter.ai/api/v2",
            "api_key": "sk-or-updated"
        });
        set_provider("openrouter", config2).unwrap();

        let provider = get_provider("openrouter").unwrap().unwrap();
        assert_eq!(provider["base_url"], "https://openrouter.ai/api/v2");

        // Remove the provider
        remove_provider("openrouter").unwrap();
        let providers = get_providers().unwrap();
        assert!(providers.is_empty());
    });
}

#[test]
fn set_provider_preserves_unknown_fields_on_update() {
    // Hermes keeps adding provider-level fields (e.g.
    // `request_timeout_seconds`, `key_env`). Users may set those via
    // Hermes Web UI; a later CC Switch edit must not strip them — set_provider
    // carries over any existing on-disk fields that the UI payload didn't
    // submit.
    with_test_home(|| {
        let yaml = "\
custom_providers:
  - name: acme
    base_url: https://old.example.com
    api_key: sk-old
    request_timeout_seconds: 300
    key_env: ACME_API_KEY
";
        let config_path = get_hermes_config_path();
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        fs::write(&config_path, yaml).unwrap();

        let update = serde_json::json!({
            "base_url": "https://new.example.com",
            "api_key": "sk-new"
        });
        set_provider("acme", update).unwrap();

        let provider = get_provider("acme").unwrap().unwrap();
        assert_eq!(provider["base_url"], "https://new.example.com");
        assert_eq!(provider["api_key"], "sk-new");
        assert_eq!(provider["request_timeout_seconds"], 300);
        assert_eq!(provider["key_env"], "ACME_API_KEY");
    });
}

#[test]
fn get_providers_surfaces_providers_dict_as_read_only() {
    with_test_home(|| {
        let yaml = "\
_config_version: 19
custom_providers:
  - name: mine
    base_url: https://mine.example.com
    api_key: sk-mine
providers:
  anthropic:
    base_url: https://api.anthropic.com
    api_key: sk-ant
    model: claude-opus-4.6
  ollama-local:
    base_url: http://localhost:11434/v1
    request_timeout_seconds: 300
";
        let config_path = get_hermes_config_path();
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        fs::write(&config_path, yaml).unwrap();

        let providers = get_providers().unwrap();
        assert_eq!(providers.len(), 3);

        let mine = providers.get("mine").unwrap();
        assert_eq!(mine[PROVIDER_SOURCE_FIELD], PROVIDER_SOURCE_CUSTOM_LIST);

        let anthropic = providers.get("anthropic").unwrap();
        assert_eq!(anthropic[PROVIDER_SOURCE_FIELD], PROVIDER_SOURCE_DICT);
        assert_eq!(anthropic["provider_key"], "anthropic");
        assert_eq!(anthropic["base_url"], "https://api.anthropic.com");

        let ollama = providers.get("ollama-local").unwrap();
        assert_eq!(ollama[PROVIDER_SOURCE_FIELD], PROVIDER_SOURCE_DICT);
        // Forward-compat fields from the dict pass through untouched
        assert_eq!(ollama["request_timeout_seconds"], 300);
    });
}

#[test]
fn get_providers_list_wins_on_name_collision() {
    with_test_home(|| {
        let yaml = "\
_config_version: 19
custom_providers:
  - name: shared
    base_url: https://writable.example.com
providers:
  shared:
    base_url: https://overlay.example.com
";
        let config_path = get_hermes_config_path();
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        fs::write(&config_path, yaml).unwrap();

        let providers = get_providers().unwrap();
        assert_eq!(providers.len(), 1);
        let shared = providers.get("shared").unwrap();
        assert_eq!(shared["base_url"], "https://writable.example.com");
        assert_eq!(shared[PROVIDER_SOURCE_FIELD], PROVIDER_SOURCE_CUSTOM_LIST);
    });
}

#[test]
fn set_provider_rejects_dict_only_entries() {
    with_test_home(|| {
        let yaml = "\
_config_version: 19
providers:
  anthropic:
    base_url: https://api.anthropic.com
    model: claude-opus-4.6
";
        let config_path = get_hermes_config_path();
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        fs::write(&config_path, yaml).unwrap();

        let update = serde_json::json!({ "base_url": "https://hacked.example.com" });
        let err = set_provider("anthropic", update).unwrap_err();
        assert!(
            format!("{err}").contains("providers:"),
            "error message should point user at providers dict: {err}"
        );
    });
}

#[test]
fn remove_provider_rejects_dict_only_entries() {
    with_test_home(|| {
        let yaml = "\
_config_version: 19
providers:
  anthropic:
    base_url: https://api.anthropic.com
";
        let config_path = get_hermes_config_path();
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        fs::write(&config_path, yaml).unwrap();

        assert!(remove_provider("anthropic").is_err());
    });
}

#[test]
fn sanitize_strips_ui_only_markers() {
    let mut v = serde_json::json!({
        "base_url": "https://api.example.com",
        "_cc_source": "providers_dict",
        "provider_key": "anthropic",
    });
    sanitize_hermes_provider_keys(&mut v);
    let obj = v.as_object().unwrap();
    assert!(obj.get("_cc_source").is_none());
    assert!(obj.get("provider_key").is_none());
    assert!(obj.get("base_url").is_some());
}

#[test]
fn get_providers_heals_legacy_camel_case_on_read() {
    // A DB may still hold records from older DeepLink imports that wrote
    // camelCase fields into `settings_config`. The read path must surface
    // them in Hermes' native snake_case so UI editors aren't lying to users.
    with_test_home(|| {
        let yaml = "\
custom_providers:
  - name: legacy
    baseUrl: https://legacy.example.com
    apiKey: sk-legacy
    apiMode: chat_completions
    api: openai-completions
";
        let config_path = get_hermes_config_path();
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        fs::write(&config_path, yaml).unwrap();

        let provider = get_provider("legacy").unwrap().unwrap();
        assert_eq!(provider["base_url"], "https://legacy.example.com");
        assert_eq!(provider["api_key"], "sk-legacy");
        assert_eq!(provider["api_mode"], "chat_completions");
        assert!(provider.get("baseUrl").is_none());
        assert!(provider.get("apiKey").is_none());
        assert!(provider.get("api").is_none());
    });
}

// ---- Model config tests ----

#[test]
fn model_config_roundtrip() {
    with_test_home(|| {
        // Initially none
        assert!(get_model_config().unwrap().is_none());

        let model = HermesModelConfig {
            default: Some("anthropic/claude-opus-4-8".to_string()),
            provider: Some("openrouter".to_string()),
            base_url: Some("https://openrouter.ai/api/v1".to_string()),
            context_length: Some(200000),
            max_tokens: None,
            extra: HashMap::new(),
        };
        set_model_config(&model).unwrap();

        let read_model = get_model_config().unwrap().unwrap();
        assert_eq!(
            read_model.default.as_deref(),
            Some("anthropic/claude-opus-4-8")
        );
        assert_eq!(read_model.provider.as_deref(), Some("openrouter"));
        assert_eq!(read_model.context_length, Some(200000));
    });
}

// ---- yaml_to_json / json_to_yaml ----

#[test]
fn yaml_json_conversion_roundtrip() {
    let json = serde_json::json!({
        "name": "test",
        "count": 42,
        "nested": {
            "flag": true
        }
    });
    let yaml = json_to_yaml(&json).unwrap();
    let back = yaml_to_json(&yaml).unwrap();
    assert_eq!(json, back);
}

// ---- models array ↔ dict transforms ----

#[test]
fn models_array_to_dict_strips_id_and_preserves_order() {
    let arr = vec![
        serde_json::json!({ "id": "foo", "context_length": 100 }),
        serde_json::json!({ "id": "bar", "max_tokens": 2000 }),
        serde_json::json!({ "id": "baz" }),
    ];
    let dict = models_array_to_dict(arr);
    let obj = dict.as_object().unwrap();
    let keys: Vec<&String> = obj.keys().collect();
    assert_eq!(keys, vec!["foo", "bar", "baz"]);
    assert_eq!(obj["foo"]["context_length"], 100);
    assert_eq!(obj["bar"]["max_tokens"], 2000);
    assert!(obj["baz"].as_object().unwrap().is_empty());
    // id must not leak into values
    assert!(obj["foo"].get("id").is_none());
}

#[test]
fn models_array_to_dict_drops_empty_and_missing_ids() {
    let arr = vec![
        serde_json::json!({ "id": "", "context_length": 1 }),
        serde_json::json!({ "id": "   ", "context_length": 2 }),
        serde_json::json!({ "context_length": 3 }),
        serde_json::json!({ "id": "kept" }),
    ];
    let dict = models_array_to_dict(arr);
    let obj = dict.as_object().unwrap();
    assert_eq!(obj.len(), 1);
    assert!(obj.contains_key("kept"));
}

#[test]
fn models_dict_to_array_reinjects_id_and_preserves_order() {
    let mut map = serde_json::Map::new();
    map.insert(
        "alpha".to_string(),
        serde_json::json!({ "context_length": 10 }),
    );
    map.insert("beta".to_string(), serde_json::json!({ "max_tokens": 20 }));
    map.insert("gamma".to_string(), serde_json::Value::Null);
    let arr = models_dict_to_array(map);
    let list = arr.as_array().unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(list[0]["id"], "alpha");
    assert_eq!(list[0]["context_length"], 10);
    assert_eq!(list[1]["id"], "beta");
    assert_eq!(list[2]["id"], "gamma");
}

#[test]
fn provider_with_models_array_writes_dict_to_yaml() {
    with_test_home(|| {
        let config = serde_json::json!({
            "base_url": "https://api.example.com/v1",
            "api_key": "sk-test",
            "api_mode": "chat_completions",
            "models": [
                { "id": "model-a", "context_length": 200000, "max_tokens": 32000 },
                { "id": "model-b", "context_length": 100000 },
            ]
        });
        set_provider("demo", config).unwrap();

        // Read raw YAML to verify the on-disk shape is a sequence under `custom_providers:`.
        let raw = fs::read_to_string(get_hermes_config_path()).unwrap();
        let yaml: serde_yaml::Value = serde_yaml::from_str(&raw).unwrap();
        let providers = yaml
            .get("custom_providers")
            .and_then(|v| v.as_sequence())
            .unwrap();
        let provider = &providers[0];
        assert_eq!(
            provider.get("name").and_then(|v| v.as_str()),
            Some("demo"),
            "entry should carry a name field"
        );
        assert_eq!(
            provider.get("model").and_then(|v| v.as_str()),
            Some("model-a"),
            "entry should carry a singular `model:` field set to the first model id \
                 so Hermes runtime/picker reads it"
        );
        let models = provider.get("models").and_then(|v| v.as_mapping()).unwrap();
        assert_eq!(models.len(), 2);
        assert!(models.contains_key(serde_yaml::Value::String("model-a".into())));
        assert!(models.contains_key(serde_yaml::Value::String("model-b".into())));
        let model_a = models
            .get(serde_yaml::Value::String("model-a".into()))
            .unwrap();
        assert_eq!(
            model_a
                .get("context_length")
                .and_then(|v| v.as_u64())
                .unwrap(),
            200000
        );
        // id should not leak into each model value
        assert!(model_a.get("id").is_none());
    });
}

#[test]
fn provider_models_roundtrip_array_dict_array_preserves_order() {
    with_test_home(|| {
        let input = serde_json::json!({
            "base_url": "https://api.example.com/v1",
            "api_key": "sk-test",
            "models": [
                { "id": "first", "context_length": 1 },
                { "id": "second", "context_length": 2 },
                { "id": "third", "context_length": 3 },
            ]
        });
        set_provider("order", input).unwrap();

        let providers = get_providers().unwrap();
        let provider = providers.get("order").unwrap();
        let models = provider.get("models").and_then(|v| v.as_array()).unwrap();
        let ids: Vec<&str> = models
            .iter()
            .map(|m| m.get("id").and_then(|v| v.as_str()).unwrap())
            .collect();
        assert_eq!(ids, vec!["first", "second", "third"]);
        assert_eq!(models[0].get("context_length").unwrap(), 1);
    });
}

#[test]
fn provider_without_models_is_unaffected() {
    with_test_home(|| {
        let input = serde_json::json!({
            "base_url": "https://api.example.com/v1",
            "api_key": "sk-test"
        });
        set_provider("simple", input).unwrap();
        let providers = get_providers().unwrap();
        let provider = providers.get("simple").unwrap();
        assert!(provider.get("models").is_none());
        assert!(
            provider.get("model").is_none(),
            "singular `model:` should not appear when no models are declared"
        );
    });
}

// ---- apply_switch_defaults ----

#[test]
fn apply_switch_defaults_sets_default_and_provider() {
    with_test_home(|| {
        let settings = serde_json::json!({
            "base_url": "https://api.example.com/v1",
            "models": [
                { "id": "primary-model", "context_length": 200000 },
                { "id": "fallback", "context_length": 100000 },
            ]
        });
        apply_switch_defaults("demo", &settings).unwrap();

        let model = get_model_config().unwrap().unwrap();
        assert_eq!(model.default.as_deref(), Some("primary-model"));
        assert_eq!(model.provider.as_deref(), Some("demo"));
    });
}

#[test]
fn apply_switch_defaults_preserves_user_context_length() {
    with_test_home(|| {
        // User previously set a custom context_length via the Model panel.
        let initial = HermesModelConfig {
            default: Some("old-model".to_string()),
            provider: Some("old-provider".to_string()),
            base_url: Some("https://user-override.example.com".to_string()),
            context_length: Some(131072),
            max_tokens: Some(16384),
            extra: HashMap::new(),
        };
        set_model_config(&initial).unwrap();

        let settings = serde_json::json!({
            "models": [{ "id": "new-model" }]
        });
        apply_switch_defaults("new-provider", &settings).unwrap();

        let model = get_model_config().unwrap().unwrap();
        assert_eq!(model.default.as_deref(), Some("new-model"));
        assert_eq!(model.provider.as_deref(), Some("new-provider"));
        // User-customized fields must survive the switch.
        assert_eq!(
            model.base_url.as_deref(),
            Some("https://user-override.example.com")
        );
        assert_eq!(model.context_length, Some(131072));
        assert_eq!(model.max_tokens, Some(16384));
    });
}

#[test]
fn apply_switch_defaults_updates_provider_even_without_models() {
    with_test_home(|| {
        // Seed an existing `model:` section — the user was already running
        // some provider before this switch.
        let initial = HermesModelConfig {
            default: Some("legacy-default".to_string()),
            provider: Some("legacy-provider".to_string()),
            ..Default::default()
        };
        set_model_config(&initial).unwrap();

        // New provider has no `models` list — previously this would no-op
        // and leave `model.provider` pointing at the legacy provider,
        // causing "switch succeeds but has no effect" bug.
        let settings = serde_json::json!({
            "base_url": "https://api.example.com/v1"
        });
        apply_switch_defaults("bare", &settings).unwrap();

        let model = get_model_config().unwrap().unwrap();
        assert_eq!(model.provider.as_deref(), Some("bare"));
        assert_eq!(model.default.as_deref(), Some("legacy-default"));
    });
}

#[test]
fn apply_switch_defaults_keeps_old_default_when_first_model_id_is_blank() {
    with_test_home(|| {
        let initial = HermesModelConfig {
            default: Some("prev-default".to_string()),
            provider: Some("prev-provider".to_string()),
            ..Default::default()
        };
        set_model_config(&initial).unwrap();

        let settings = serde_json::json!({
            "models": [{ "id": "   " }, { "id": "real" }]
        });
        apply_switch_defaults("edge", &settings).unwrap();

        let model = get_model_config().unwrap().unwrap();
        // Provider always updates.
        assert_eq!(model.provider.as_deref(), Some("edge"));
        // First entry's id is whitespace-only → blank → fall back to old default
        // (we intentionally don't scan past the first entry for a default).
        assert_eq!(model.default.as_deref(), Some("prev-default"));
    });
}

// ---- memory file tests ----

#[test]
fn read_memory_returns_empty_when_file_missing() {
    with_test_home(|| {
        let memory = read_memory(MemoryKind::Memory).unwrap();
        let user = read_memory(MemoryKind::User).unwrap();
        assert!(memory.is_empty());
        assert!(user.is_empty());
    });
}

#[test]
fn write_then_read_memory_round_trip() {
    with_test_home(|| {
        let blob = "> note\n§\nfirst entry\n§\nsecond entry\n";
        write_memory(MemoryKind::Memory, blob).unwrap();
        assert_eq!(read_memory(MemoryKind::Memory).unwrap(), blob);

        // Writing USER.md doesn't clobber MEMORY.md.
        write_memory(MemoryKind::User, "user profile").unwrap();
        assert_eq!(read_memory(MemoryKind::Memory).unwrap(), blob);
        assert_eq!(read_memory(MemoryKind::User).unwrap(), "user profile");
    });
}

#[test]
fn memory_limits_fall_back_to_defaults_when_config_missing() {
    with_test_home(|| {
        let limits = read_memory_limits().unwrap();
        let defaults = HermesMemoryLimits::default();
        assert_eq!(limits.memory, defaults.memory);
        assert_eq!(limits.user, defaults.user);
        assert_eq!(limits.memory_enabled, defaults.memory_enabled);
        assert_eq!(limits.user_enabled, defaults.user_enabled);
    });
}

#[test]
fn set_memory_enabled_preserves_other_fields() {
    // Flipping one toggle must preserve character budgets and external
    // provider settings the user configured via Hermes Web UI — otherwise
    // a CC Switch toggle would silently wipe those fields.
    with_test_home(|| {
        let yaml = "\
memory:
  memory_char_limit: 4096
  user_char_limit: 2048
  memory_enabled: true
  user_profile_enabled: true
  provider: mem0
";
        let config_path = get_hermes_config_path();
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        fs::write(&config_path, yaml).unwrap();

        set_memory_enabled(MemoryKind::Memory, false).unwrap();

        let limits = read_memory_limits().unwrap();
        assert!(!limits.memory_enabled, "toggle applied");
        assert!(limits.user_enabled, "unrelated toggle untouched");
        assert_eq!(limits.memory, 4096, "budgets preserved");
        assert_eq!(limits.user, 2048);

        // Verify the external provider field survived the section replacement.
        let config = read_hermes_config().unwrap();
        let provider = config
            .get("memory")
            .and_then(|v| v.get("provider"))
            .and_then(|v| v.as_str());
        assert_eq!(provider, Some("mem0"));
    });
}

#[test]
fn memory_limits_read_from_config_yaml() {
    with_test_home(|| {
        let yaml = "\
memory:
  memory_char_limit: 4096
  user_char_limit: 2048
  memory_enabled: false
  user_profile_enabled: true
";
        let config_path = get_hermes_config_path();
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        fs::write(&config_path, yaml).unwrap();

        let limits = read_memory_limits().unwrap();
        assert_eq!(limits.memory, 4096);
        assert_eq!(limits.user, 2048);
        assert!(!limits.memory_enabled);
        assert!(limits.user_enabled);
    });
}

#[test]
fn memory_limits_ignore_top_level_keys() {
    // Regression guard: Hermes nests memory settings under `memory:`, so
    // identically-named keys at the top level must be ignored rather than
    // silently consumed.
    with_test_home(|| {
        let yaml = "\
memory_char_limit: 9999
user_char_limit: 9999
memory_enabled: false
user_profile_enabled: false
";
        let config_path = get_hermes_config_path();
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        fs::write(&config_path, yaml).unwrap();

        let limits = read_memory_limits().unwrap();
        let defaults = HermesMemoryLimits::default();
        assert_eq!(limits.memory, defaults.memory);
        assert_eq!(limits.user, defaults.user);
        assert_eq!(limits.memory_enabled, defaults.memory_enabled);
        assert_eq!(limits.user_enabled, defaults.user_enabled);
    });
}

#[test]
fn memory_kind_deserializes_from_lowercase_strings() {
    let memory: MemoryKind = serde_json::from_str("\"memory\"").unwrap();
    let user: MemoryKind = serde_json::from_str("\"user\"").unwrap();
    assert_eq!(memory, MemoryKind::Memory);
    assert_eq!(user, MemoryKind::User);
    assert!(serde_json::from_str::<MemoryKind>("\"bogus\"").is_err());
}

// ---- get_hermes_dir resolution (platform default + HERMES_HOME) ----

#[test]
#[serial]
fn hermes_home_env_takes_precedence_over_platform_default() {
    with_env_test_home(|| {
        // Clear any settings override so resolution reaches the HERMES_HOME branch.
        let mut s = crate::settings::get_settings();
        s.hermes_config_dir = None;
        crate::settings::update_settings(s).unwrap();

        let old = std::env::var_os("HERMES_HOME");
        let custom = std::env::temp_dir().join("ccs-hermes-home-precedence");
        std::env::set_var("HERMES_HOME", &custom);

        let dir = get_hermes_dir();

        match old {
            Some(v) => std::env::set_var("HERMES_HOME", v),
            None => std::env::remove_var("HERMES_HOME"),
        }

        assert_eq!(
            dir, custom,
            "HERMES_HOME should take precedence over the platform default, got {dir:?}"
        );
    });
}

#[test]
#[serial]
fn settings_override_takes_precedence_over_hermes_home() {
    with_env_test_home(|| {
        let custom = tempfile::tempdir().unwrap();
        let custom_path = custom.path().to_path_buf();

        let mut s = crate::settings::get_settings();
        s.hermes_config_dir = Some(custom_path.to_string_lossy().to_string());
        crate::settings::update_settings(s).unwrap();

        let old = std::env::var_os("HERMES_HOME");
        std::env::set_var("HERMES_HOME", "/tmp/should-be-ignored");

        let dir = get_hermes_dir();

        match old {
            Some(v) => std::env::set_var("HERMES_HOME", v),
            None => std::env::remove_var("HERMES_HOME"),
        }
        let mut s = crate::settings::get_settings();
        s.hermes_config_dir = None;
        crate::settings::update_settings(s).unwrap();

        assert_eq!(
            dir, custom_path,
            "settings override should win over HERMES_HOME, got {dir:?}"
        );
    });
}

#[test]
#[serial]
fn blank_hermes_home_falls_through_to_platform_default() {
    with_env_test_home(|| {
        let mut s = crate::settings::get_settings();
        s.hermes_config_dir = None;
        crate::settings::update_settings(s).unwrap();

        let old = std::env::var_os("HERMES_HOME");
        std::env::set_var("HERMES_HOME", "   ");

        let dir = get_hermes_dir();

        match old {
            Some(v) => std::env::set_var("HERMES_HOME", v),
            None => std::env::remove_var("HERMES_HOME"),
        }

        // Blank HERMES_HOME is ignored (matches Hermes' `.strip()` non-empty check),
        // so resolution must reach the platform default, never the literal blank path.
        assert_ne!(dir, PathBuf::from("   "));
        assert_eq!(dir, default_hermes_dir());
    });
}

#[test]
#[serial]
fn default_hermes_dir_without_override_is_platform_correct() {
    with_env_test_home(|| {
        let mut s = crate::settings::get_settings();
        s.hermes_config_dir = None;
        crate::settings::update_settings(s).unwrap();

        let old = std::env::var_os("HERMES_HOME");
        std::env::remove_var("HERMES_HOME");

        let dir = get_hermes_dir();

        if let Some(v) = old {
            std::env::set_var("HERMES_HOME", v);
        }

        #[cfg(target_os = "windows")]
        assert!(
            dir.ends_with("hermes") && dir.to_string_lossy().to_lowercase().contains("local"),
            "Windows default should be %LOCALAPPDATA%\\hermes, got {dir:?}"
        );
        #[cfg(not(target_os = "windows"))]
        assert!(
            dir.ends_with(".hermes"),
            "Unix default should be ~/.hermes, got {dir:?}"
        );
    });
}

#[test]
fn windows_local_hermes_dir_uses_localappdata_when_set() {
    let local = std::ffi::OsString::from("C:\\Users\\tester\\AppData\\Local");
    let home = Path::new("C:\\Users\\tester");
    // Uses LOCALAPPDATA (ignoring home) and appends `hermes`. Build the expected
    // path with `join` so the separator matches the host OS.
    assert_eq!(
        windows_local_hermes_dir(Some(local.as_os_str()), home),
        PathBuf::from(&local).join("hermes"),
    );
}

#[test]
fn windows_local_hermes_dir_falls_back_to_home_when_localappdata_missing_or_empty() {
    let home = Path::new("C:\\Users\\tester");
    let expected = home.join("AppData").join("Local").join("hermes");
    assert_eq!(windows_local_hermes_dir(None, home), expected);
    let empty = std::ffi::OsString::from("");
    assert_eq!(
        windows_local_hermes_dir(Some(empty.as_os_str()), home),
        expected,
    );
}

#[test]
fn windows_local_hermes_dir_trims_localappdata() {
    // Mirror Hermes' `os.environ.get("LOCALAPPDATA", "").strip()`.
    let home = Path::new("C:\\Users\\tester");
    // Whitespace-only -> treated as unset, fall back to <home>\AppData\Local.
    let blank = std::ffi::OsString::from("   ");
    assert_eq!(
        windows_local_hermes_dir(Some(blank.as_os_str()), home),
        home.join("AppData").join("Local").join("hermes"),
    );
    // Padded value -> trimmed before use.
    let padded = std::ffi::OsString::from("  C:\\Custom\\Local  ");
    assert_eq!(
        windows_local_hermes_dir(Some(padded.as_os_str()), home),
        PathBuf::from("C:\\Custom\\Local").join("hermes"),
    );
}

#[test]
#[serial]
fn with_test_home_neutralizes_hermes_env_vars() {
    // An ambient HERMES_HOME / LOCALAPPDATA (e.g. set by a Hermes install)
    // must not leak into the test home — otherwise other tests in this
    // module would read/write a real Hermes config via get_hermes_config_path().
    let saved_hh = std::env::var_os("HERMES_HOME");
    let saved_la = std::env::var_os("LOCALAPPDATA");
    std::env::set_var("HERMES_HOME", "/ambient/hermes-home");
    std::env::set_var("LOCALAPPDATA", "/ambient/local-appdata");

    let inside = with_env_test_home(|| {
        (
            std::env::var_os("HERMES_HOME"),
            std::env::var_os("LOCALAPPDATA"),
        )
    });
    // The thread-local helper leaves env alone but path resolution ignores
    // the ambient values, so the Hermes dir stays inside the test home.
    let (thread_dir, thread_home) =
        with_test_home(|| (get_hermes_dir(), crate::config::get_home_dir()));

    match saved_hh {
        Some(v) => std::env::set_var("HERMES_HOME", v),
        None => std::env::remove_var("HERMES_HOME"),
    }
    match saved_la {
        Some(v) => std::env::set_var("LOCALAPPDATA", v),
        None => std::env::remove_var("LOCALAPPDATA"),
    }

    assert_eq!(
        inside.0, None,
        "with_test_home must clear ambient HERMES_HOME"
    );
    assert_eq!(
        inside.1, None,
        "with_test_home must clear ambient LOCALAPPDATA"
    );
    assert!(
        thread_dir.starts_with(&thread_home),
        "{thread_dir:?} escaped the thread test home {thread_home:?}"
    );
}
