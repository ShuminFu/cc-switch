//! Unit tests for `codex_config` (moved out of codex_config.rs, TEST-29).

use super::*;
use serde_json::json;

#[test]
fn catalog_tool_profile_from_api_format() {
    assert_eq!(
        CodexCatalogToolProfile::from_api_format(Some("anthropic")),
        CodexCatalogToolProfile::Anthropic
    );
    assert_eq!(
        CodexCatalogToolProfile::from_api_format(Some("openai_responses")),
        CodexCatalogToolProfile::NativeResponses
    );
    assert_eq!(
        CodexCatalogToolProfile::from_api_format(Some("openai_chat")),
        CodexCatalogToolProfile::ProxyChat
    );
    assert_eq!(
        CodexCatalogToolProfile::from_api_format(None),
        CodexCatalogToolProfile::ProxyChat
    );
}

#[test]
fn unified_session_bucket_injects_for_empty_official_config() {
    let injected = inject_codex_unified_session_bucket("").expect("inject");
    let doc: toml::Table = toml::from_str(&injected).expect("parse injected config");

    assert_eq!(
        doc.get("model_provider").and_then(|v| v.as_str()),
        Some(CC_SWITCH_CODEX_MODEL_PROVIDER_ID)
    );
    let custom = doc["model_providers"][CC_SWITCH_CODEX_MODEL_PROVIDER_ID]
        .as_table()
        .expect("custom provider table");
    assert_eq!(custom.get("name").and_then(|v| v.as_str()), Some("OpenAI"));
    assert_eq!(
        custom.get("requires_openai_auth").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        custom.get("supports_websockets").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        custom.get("wire_api").and_then(|v| v.as_str()),
        Some("responses")
    );
}

#[test]
fn official_proxy_route_uses_native_auth_and_local_responses_provider() {
    let input = r#"model = "gpt-5.4"
experimental_bearer_token = "PROXY_MANAGED"

[mcp_servers.example]
command = "example"
"#;
    let output = apply_codex_official_proxy_route(input, "http://127.0.0.1:15721/v1")
        .expect("apply official proxy route");
    let doc: toml::Value = toml::from_str(&output).expect("parse output");

    assert_eq!(
        doc.get("model_provider").and_then(toml::Value::as_str),
        Some(CC_SWITCH_CODEX_OFFICIAL_PROXY_PROVIDER_ID)
    );
    assert!(doc.get("experimental_bearer_token").is_none());
    assert!(
        doc.get("mcp_servers").is_some(),
        "unrelated config survives"
    );

    let provider = &doc["model_providers"][CC_SWITCH_CODEX_OFFICIAL_PROXY_PROVIDER_ID];
    assert_eq!(
        provider.get("base_url").and_then(toml::Value::as_str),
        Some("http://127.0.0.1:15721/v1")
    );
    assert_eq!(
        provider
            .get("requires_openai_auth")
            .and_then(toml::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        provider
            .get("supports_websockets")
            .and_then(toml::Value::as_bool),
        Some(false)
    );
    assert!(codex_config_has_official_proxy_route(&output));
}

#[test]
fn official_proxy_route_cleanup_only_removes_owned_provider() {
    let projected =
        apply_codex_official_proxy_route("model = \"gpt-5.4\"\n", "http://127.0.0.1:15721/v1")
            .expect("project");
    let cleaned = remove_codex_official_proxy_route(&projected).expect("clean");
    let doc: toml::Value = toml::from_str(&cleaned).expect("parse cleaned");
    assert!(doc.get("model_provider").is_none());
    assert!(doc.get("model_providers").is_none());
    assert_eq!(
        doc.get("model").and_then(toml::Value::as_str),
        Some("gpt-5.4")
    );
}

#[test]
fn official_proxy_route_rejects_non_table_model_providers_without_panicking() {
    for input in [
        "model_providers = 3\n",
        "[[model_providers]]\nname = \"broken\"\n",
    ] {
        let result = apply_codex_official_proxy_route(input, "http://127.0.0.1:15721/v1");
        assert!(result.is_err());
    }
}

#[test]
fn official_proxy_route_normalizes_inline_tables_and_cleans_stale_placeholder() {
    let input = r#"model_provider = "rightcode"
model_providers = { rightcode = { name = "RightCode", experimental_bearer_token = "PROXY_MANAGED" } }
"#;
    let projected = apply_codex_official_proxy_route(input, "http://127.0.0.1:15721/v1")
        .expect("project inline provider table");
    let projected_doc: toml::Value = toml::from_str(&projected).expect("parse projected");
    assert!(projected_doc["model_providers"]["rightcode"]
        .get("experimental_bearer_token")
        .is_none());
    assert!(projected_doc["model_providers"]
        .get(CC_SWITCH_CODEX_OFFICIAL_PROXY_PROVIDER_ID)
        .is_some());

    let cleaned = remove_codex_official_proxy_route(&projected).expect("clean projected");
    let cleaned_doc: toml::Value = toml::from_str(&cleaned).expect("parse cleaned");
    assert!(cleaned_doc.get("model_provider").is_none());
    assert!(cleaned_doc["model_providers"].get("rightcode").is_some());
    assert!(cleaned_doc["model_providers"]
        .get(CC_SWITCH_CODEX_OFFICIAL_PROXY_PROVIDER_ID)
        .is_none());
}

#[test]
fn unified_session_bucket_preserves_other_keys_and_explicit_routing() {
    let with_catalog = "model_catalog_json = \"cc-switch-model-catalog.json\"\n";
    let injected = inject_codex_unified_session_bucket(with_catalog).expect("inject");
    assert!(injected.contains("model_catalog_json"));
    assert!(injected.contains("model_provider = \"custom\""));

    // 用户显式指定过 model_provider 的官方配置不被覆盖
    let explicit = "model_provider = \"openai_https\"\n";
    let unchanged = inject_codex_unified_session_bucket(explicit).expect("inject");
    assert_eq!(unchanged, explicit);
}

#[test]
fn unified_session_bucket_skips_conflicting_custom_table() {
    // 残留的非注入形态 custom 表：设置 model_provider 会把官方流量
    // 路由到表里的第三方端点，必须整体拒绝注入。
    let stale = r#"[model_providers.custom]
name = "Relay"
base_url = "https://relay.example/v1"
"#;
    let unchanged = inject_codex_unified_session_bucket(stale).expect("inject");
    assert_eq!(unchanged, stale);

    // 已是注入形态的 custom 表（如重复注入）则照常补上 model_provider
    let injected_once = inject_codex_unified_session_bucket("").expect("inject");
    let reinjected = inject_codex_unified_session_bucket(&injected_once).expect("re-inject");
    assert_eq!(reinjected, injected_once);
}

#[test]
fn unified_session_bucket_strip_round_trips_injection() {
    let injected = inject_codex_unified_session_bucket("").expect("inject");
    let stripped = strip_codex_unified_session_bucket(&injected).expect("strip");
    assert_eq!(stripped.trim(), "");

    let with_catalog = "model_catalog_json = \"cc-switch-model-catalog.json\"\n";
    let injected = inject_codex_unified_session_bucket(with_catalog).expect("inject");
    let stripped = strip_codex_unified_session_bucket(&injected).expect("strip");
    assert_eq!(stripped, with_catalog);
}

#[test]
fn unified_session_bucket_strip_keeps_third_party_custom_entry() {
    // 第三方模板同样用 custom 路由，但条目带 base_url 等差异字段，
    // 形态不等于注入产物，必须原样保留。
    let third_party = r#"model_provider = "custom"

[model_providers.custom]
name = "Relay"
base_url = "https://relay.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#;
    let untouched = strip_codex_unified_session_bucket(third_party).expect("strip");
    assert_eq!(untouched, third_party);
}

#[test]
fn unified_session_bucket_strip_from_settings_only_touches_config() {
    let injected = inject_codex_unified_session_bucket("").expect("inject");
    let mut settings = json!({
        "auth": { "tokens": { "access_token": "secret" } },
        "config": injected,
    });
    strip_codex_unified_session_bucket_from_settings(&mut settings).expect("strip settings");
    assert_eq!(
        settings
            .get("config")
            .and_then(|v| v.as_str())
            .map(str::trim),
        Some("")
    );
    assert!(settings.pointer("/auth/tokens/access_token").is_some());
}

#[test]
fn strip_mcp_servers_from_settings_removes_table_and_legacy_form() {
    let mut settings = json!({
        "auth": { "OPENAI_API_KEY": "sk-test" },
        "config": "# user comment\nmodel = \"gpt-5.5\"\n\n[mcp_servers.echo]\ntype = \"stdio\"\ncommand = \"echo\"\n\n[mcp.servers.legacy]\ncommand = \"noop\"\n",
    });
    strip_codex_mcp_servers_from_settings(&mut settings).expect("strip mcp");
    let config = settings
        .get("config")
        .and_then(|v| v.as_str())
        .expect("config text");
    assert!(!config.contains("mcp_servers"), "got: {config}");
    assert!(
        !config.contains("[mcp"),
        "legacy [mcp.servers] gone: {config}"
    );
    assert!(config.contains("# user comment"), "comments preserved");
    assert!(config.contains("model = \"gpt-5.5\""));
}

#[test]
fn strip_mcp_servers_from_settings_is_noop_without_mcp() {
    let original = "# comment\nmodel = \"gpt-5.5\"\n";
    let mut settings = json!({
        "auth": {},
        "config": original,
    });
    strip_codex_mcp_servers_from_settings(&mut settings).expect("strip mcp");
    assert_eq!(
        settings.get("config").and_then(|v| v.as_str()),
        Some(original),
        "config text must be byte-identical when nothing is stripped"
    );
}

#[test]
fn extract_base_url_prefers_active_provider_section() {
    let input = r#"model_provider = "azure"

[model_providers.azure]
base_url = "https://azure.example.com/v1"

[model_providers.other]
base_url = "https://other.example.com/v1"
"#;

    assert_eq!(
        extract_codex_base_url(input).as_deref(),
        Some("https://azure.example.com/v1")
    );
}

#[test]
fn extract_base_url_falls_back_to_top_level_only() {
    let top_level = r#"base_url = "https://top-level.example.com/v1""#;
    assert_eq!(
        extract_codex_base_url(top_level).as_deref(),
        Some("https://top-level.example.com/v1")
    );
}

// Mirrors the frontend extractCodexBaseUrl: a non-active provider section
// is never a credential source, whether the active provider points
// elsewhere (e.g. the built-in "openai") or none is selected at all.
#[test]
fn extract_base_url_ignores_non_active_provider_sections() {
    let mismatched = r#"model_provider = "openai"

[model_providers.custom]
base_url = "https://leftover.example.com/v1"
"#;
    assert_eq!(extract_codex_base_url(mismatched), None);

    let no_active = r#"[model_providers.any]
base_url = "https://single.example.com/v1"
"#;
    assert_eq!(extract_codex_base_url(no_active), None);
}

#[test]
fn prepare_provider_live_config_rejects_key_without_config() {
    let err = prepare_codex_provider_live_config(&json!({"OPENAI_API_KEY": "sk-test"}), "")
        .expect_err("empty config with API key should not truncate live config");

    assert!(
        err.to_string().contains("config.toml"),
        "error should explain missing config.toml, got: {err}"
    );
}

#[test]
fn prepare_provider_live_config_uses_top_level_token_for_reserved_provider() {
    let input = r#"model_provider = "openai"
model = "gpt-5"
"#;

    let output = prepare_codex_provider_live_config(&json!({"OPENAI_API_KEY": "sk-test"}), input)
        .expect("prepare live config");
    let parsed: toml::Value = toml::from_str(&output).expect("parse output");

    assert_eq!(
        parsed
            .get("experimental_bearer_token")
            .and_then(|v| v.as_str()),
        Some("sk-test")
    );
    assert!(
        parsed.get("model_providers").is_none(),
        "reserved provider tables should not be synthesized"
    );
}

#[test]
fn extract_bearer_uses_top_level_token_for_reserved_provider() {
    let input = r#"model_provider = "openai"
experimental_bearer_token = "top-level-key"

[model_providers.openai]
experimental_bearer_token = "stale-table-key"
"#;

    assert_eq!(
        extract_codex_experimental_bearer_token(input).as_deref(),
        Some("top-level-key")
    );
}

#[test]
fn should_not_restore_provider_token_for_oauth_only_template() {
    let oauth_template = json!({
        "auth": {
            "auth_mode": "chatgpt",
            "tokens": {
                "access_token": "oauth-access"
            }
        }
    });
    let api_key_template = json!({
        "auth": {
            "OPENAI_API_KEY": "sk-test"
        }
    });

    assert!(
        !should_restore_codex_provider_token_for_backfill(Some("custom"), &oauth_template),
        "OAuth-only templates should not backfill bearer tokens into OPENAI_API_KEY"
    );
    assert!(
        should_restore_codex_provider_token_for_backfill(Some("custom"), &api_key_template),
        "custom API-key providers should still restore provider bearer tokens"
    );
    assert!(
        !should_restore_codex_provider_token_for_backfill(Some("official"), &api_key_template),
        "official providers should never restore third-party bearer tokens"
    );
}

#[test]
fn prepare_provider_live_config_does_not_create_incomplete_provider_table() {
    let input = r#"model_provider = "vendor_x"
model = "gpt-5"
"#;

    let output = prepare_codex_provider_live_config(&json!({"OPENAI_API_KEY": "sk-test"}), input)
        .expect("prepare live config");
    let parsed: toml::Value = toml::from_str(&output).expect("parse output");

    assert_eq!(
        parsed
            .get("experimental_bearer_token")
            .and_then(|v| v.as_str()),
        Some("sk-test")
    );
    assert!(
        parsed.get("model_providers").is_none(),
        "missing provider tables should not be synthesized without endpoint fields"
    );
}

#[test]
fn prepare_provider_live_config_preserves_custom_provider_id() {
    let input = r#"model_provider = "vendor_alpha"
model = "gpt-5.4"
profile = "work"

[model_providers.vendor_alpha]
name = "Vendor Alpha"
base_url = "https://alpha.example/v1"
wire_api = "responses"

[profiles.work]
model_provider = "vendor_alpha"
model = "gpt-5.4"
"#;

    let result = prepare_codex_provider_live_config(&json!({"OPENAI_API_KEY": "sk-test"}), input)
        .expect("prepare live config");
    let parsed: toml::Value = toml::from_str(&result).unwrap();

    assert_eq!(
        parsed.get("model_provider").and_then(|v| v.as_str()),
        Some("vendor_alpha")
    );
    assert!(
        parsed
            .get("model_providers")
            .and_then(|v| v.get("custom"))
            .is_none(),
        "provider writes should not force custom provider ids"
    );
    assert_eq!(
        parsed
            .get("model_providers")
            .and_then(|v| v.get("vendor_alpha"))
            .and_then(|v| v.get("experimental_bearer_token"))
            .and_then(|v| v.as_str()),
        Some("sk-test")
    );
    assert_eq!(
        parsed
            .get("profiles")
            .and_then(|v| v.get("work"))
            .and_then(|v| v.get("model_provider"))
            .and_then(|v| v.as_str()),
        Some("vendor_alpha"),
        "profile provider references should be preserved"
    );
}

#[test]
fn backfill_preserves_live_model_provider_id() {
    let mut live_settings = json!({
        "auth": {},
        "config": r#"model_provider = "vendor_beta"

[model_providers.vendor_beta]
name = "Vendor Beta"
base_url = "https://beta.example/v1"
wire_api = "responses"
"#,
    });
    let template_settings = json!({
        "auth": {},
        "config": r#"model_provider = "custom"

[model_providers.custom]
name = "Custom"
base_url = "https://custom.example/v1"
wire_api = "responses"
"#,
    });

    restore_codex_settings_for_backfill(&mut live_settings, &template_settings, false).unwrap();
    let config = live_settings.get("config").and_then(Value::as_str).unwrap();
    let parsed: toml::Value = toml::from_str(config).unwrap();

    assert_eq!(
        parsed.get("model_provider").and_then(|v| v.as_str()),
        Some("vendor_beta")
    );
    assert!(
        parsed
            .get("model_providers")
            .and_then(|v| v.get("vendor_beta"))
            .is_some(),
        "backfill should not rewrite user-selected provider tables"
    );
}

#[test]
fn base_url_writes_into_correct_model_provider_section() {
    let input = r#"model_provider = "any"
model = "gpt-5.1-codex"

[model_providers.any]
name = "any"
wire_api = "responses"
"#;

    let result = update_codex_toml_field(input, "base_url", "https://example.com/v1").unwrap();
    let parsed: toml::Value = toml::from_str(&result).unwrap();

    let base_url = parsed
        .get("model_providers")
        .and_then(|v| v.get("any"))
        .and_then(|v| v.get("base_url"))
        .and_then(|v| v.as_str())
        .expect("base_url should be in model_providers.any");
    assert_eq!(base_url, "https://example.com/v1");

    // Should NOT have top-level base_url
    assert!(parsed.get("base_url").is_none());

    // wire_api preserved
    let wire_api = parsed
        .get("model_providers")
        .and_then(|v| v.get("any"))
        .and_then(|v| v.get("wire_api"))
        .and_then(|v| v.as_str());
    assert_eq!(wire_api, Some("responses"));
}

#[test]
fn wire_api_writes_into_correct_model_provider_section() {
    let input = r#"model_provider = "chat_only"
model = "gpt-5.1-codex"

[model_providers.chat_only]
name = "Chat Only"
base_url = "https://example.com/v1"
wire_api = "chat"
"#;

    let result = update_codex_toml_field(input, "wire_api", "responses").unwrap();
    let parsed: toml::Value = toml::from_str(&result).unwrap();

    let provider = parsed
        .get("model_providers")
        .and_then(|v| v.get("chat_only"))
        .expect("model_providers.chat_only should exist");

    assert_eq!(
        provider.get("wire_api").and_then(|v| v.as_str()),
        Some("responses")
    );
    assert_eq!(
        provider.get("base_url").and_then(|v| v.as_str()),
        Some("https://example.com/v1")
    );
    assert!(parsed.get("wire_api").is_none());
}

#[test]
fn base_url_creates_section_when_missing() {
    let input = r#"model_provider = "custom"
model = "gpt-4"
"#;

    let result = update_codex_toml_field(input, "base_url", "https://custom.api/v1").unwrap();
    let parsed: toml::Value = toml::from_str(&result).unwrap();

    let base_url = parsed
        .get("model_providers")
        .and_then(|v| v.get("custom"))
        .and_then(|v| v.get("base_url"))
        .and_then(|v| v.as_str())
        .expect("should create section and set base_url");
    assert_eq!(base_url, "https://custom.api/v1");
}

#[test]
fn base_url_falls_back_to_top_level_without_model_provider() {
    let input = r#"model = "gpt-4"
"#;

    let result = update_codex_toml_field(input, "base_url", "https://fallback.api/v1").unwrap();
    let parsed: toml::Value = toml::from_str(&result).unwrap();

    let base_url = parsed
        .get("base_url")
        .and_then(|v| v.as_str())
        .expect("should set top-level base_url");
    assert_eq!(base_url, "https://fallback.api/v1");
}

#[test]
fn clearing_base_url_removes_only_from_correct_section() {
    let input = r#"model_provider = "any"

[model_providers.any]
name = "any"
base_url = "https://old.api/v1"
wire_api = "responses"

[mcp_servers.context7]
command = "npx"
"#;

    let result = update_codex_toml_field(input, "base_url", "").unwrap();
    let parsed: toml::Value = toml::from_str(&result).unwrap();

    // base_url removed from model_providers.any
    let any_section = parsed
        .get("model_providers")
        .and_then(|v| v.get("any"))
        .expect("model_providers.any should exist");
    assert!(any_section.get("base_url").is_none());

    // wire_api preserved
    assert_eq!(
        any_section.get("wire_api").and_then(|v| v.as_str()),
        Some("responses")
    );

    // mcp_servers untouched
    assert!(parsed.get("mcp_servers").is_some());
}

#[test]
fn model_field_operates_on_top_level() {
    let input = r#"model_provider = "any"
model = "gpt-4"

[model_providers.any]
name = "any"
"#;

    let result = update_codex_toml_field(input, "model", "gpt-5").unwrap();
    let parsed: toml::Value = toml::from_str(&result).unwrap();
    assert_eq!(parsed.get("model").and_then(|v| v.as_str()), Some("gpt-5"));

    // Clear model
    let result2 = update_codex_toml_field(&result, "model", "").unwrap();
    let parsed2: toml::Value = toml::from_str(&result2).unwrap();
    assert!(parsed2.get("model").is_none());
}

#[test]
fn preserves_comments_and_whitespace() {
    let input = r#"# My Codex config
model_provider = "any"
model = "gpt-4"

# Provider section
[model_providers.any]
name = "any"
base_url = "https://old.api/v1"
"#;

    let result = update_codex_toml_field(input, "base_url", "https://new.api/v1").unwrap();

    // Comments should be preserved
    assert!(result.contains("# My Codex config"));
    assert!(result.contains("# Provider section"));
}

#[test]
fn does_not_misplace_when_profiles_section_follows() {
    let input = r#"model_provider = "any"

[model_providers.any]
name = "any"
base_url = "https://old.api/v1"

[profiles.default]
model = "gpt-4"
"#;

    let result = update_codex_toml_field(input, "base_url", "https://new.api/v1").unwrap();
    let parsed: toml::Value = toml::from_str(&result).unwrap();

    // base_url in correct section
    let base_url = parsed
        .get("model_providers")
        .and_then(|v| v.get("any"))
        .and_then(|v| v.get("base_url"))
        .and_then(|v| v.as_str());
    assert_eq!(base_url, Some("https://new.api/v1"));

    // profiles section untouched
    let profile_model = parsed
        .get("profiles")
        .and_then(|v| v.get("default"))
        .and_then(|v| v.get("model"))
        .and_then(|v| v.as_str());
    assert_eq!(profile_model, Some("gpt-4"));
}

#[test]
fn remove_base_url_if_predicate() {
    let input = r#"model_provider = "any"

[model_providers.any]
name = "any"
base_url = "http://127.0.0.1:5000/v1"
wire_api = "responses"
"#;

    let result = remove_codex_toml_base_url_if(input, |url| url.starts_with("http://127.0.0.1"));
    let parsed: toml::Value = toml::from_str(&result).unwrap();

    let any_section = parsed
        .get("model_providers")
        .and_then(|v| v.get("any"))
        .unwrap();
    assert!(any_section.get("base_url").is_none());
    assert_eq!(
        any_section.get("wire_api").and_then(|v| v.as_str()),
        Some("responses")
    );
}

#[test]
fn remove_base_url_if_keeps_non_matching() {
    let input = r#"model_provider = "any"

[model_providers.any]
base_url = "https://production.api/v1"
"#;

    let result = remove_codex_toml_base_url_if(input, |url| url.starts_with("http://127.0.0.1"));
    let parsed: toml::Value = toml::from_str(&result).unwrap();

    let base_url = parsed
        .get("model_providers")
        .and_then(|v| v.get("any"))
        .and_then(|v| v.get("base_url"))
        .and_then(|v| v.as_str());
    assert_eq!(base_url, Some("https://production.api/v1"));
}

#[test]
fn codex_model_catalog_uses_provider_models_and_context() {
    let template = json!({
        "slug": "gpt-5.5",
        "display_name": "GPT-5.5",
        "description": "Frontier model",
        "base_instructions": "gpt-5.5 base instructions",
        "model_messages": {
            "instructions_template": "gpt-5.5 instructions template",
            "instructions_variables": {
                "personality_default": "",
                "personality_friendly": "",
                "personality_pragmatic": ""
            }
        },
        "additional_speed_tiers": ["fast"],
        "service_tiers": [
            {
                "id": "priority",
                "name": "Fast",
                "description": "1.5x speed, increased usage"
            }
        ],
        "availability_nux": {
            "message": "GPT-5.5 is now available."
        },
        "upgrade": {
            "target": "gpt-5.5"
        },
        "context_window": 272000,
        "max_context_window": 272000
    });
    let settings = json!({
        "modelCatalog": {
            "models": [
                {
                    "model": "deepseek-v4-flash",
                    "displayName": "DeepSeek V4 Flash",
                    "contextWindow": "64000"
                },
                {
                    "model": "kimi-k2",
                    "display_name": "Kimi K2"
                }
            ]
        }
    });
    let specs = codex_catalog_model_specs(&settings, r#"model_context_window = 128000"#);
    let catalog =
        codex_model_catalog_from_specs(&specs, &template, CodexCatalogToolProfile::ProxyChat);
    let models = catalog
        .get("models")
        .and_then(|value| value.as_array())
        .expect("models should be an array");

    assert_eq!(models.len(), 2);
    assert_eq!(
        models[0].get("slug").and_then(|value| value.as_str()),
        Some("deepseek-v4-flash")
    );
    assert_eq!(
        models[0]
            .get("context_window")
            .and_then(|value| value.as_u64()),
        Some(64_000)
    );
    assert_eq!(
        models[1]
            .get("context_window")
            .and_then(|value| value.as_u64()),
        Some(128_000)
    );
    assert!(
        models[0].get("model_messages").is_some(),
        "Codex requires model_messages in custom catalogs"
    );
    assert_eq!(
        models[0]
            .get("base_instructions")
            .and_then(|value| value.as_str()),
        Some("gpt-5.5 base instructions")
    );
    assert_eq!(
        models[0].get("model_messages"),
        template.get("model_messages"),
        "custom catalog entries should keep the gpt-5.5 agent template"
    );
    assert_eq!(
        models[0].get("additional_speed_tiers"),
        Some(&json!([])),
        "generated third-party entries should not inherit OpenAI speed tiers"
    );
    assert!(
        models[0]
            .get("availability_nux")
            .is_some_and(|value| value.is_null()),
        "generated third-party entries should not inherit GPT-5.5 launch messaging"
    );
}

#[test]
fn native_responses_profile_suppresses_apply_patch_and_keeps_shell() {
    // Native (direct) /responses providers must NOT emit a freeform
    // apply_patch (type=="custom") tool — gateways like MiMo reject it.
    // The native profile uses the bundled clean template and relies on
    // shell_type="shell_command" for edits, plus per-row overrides.
    let settings = json!({
        "modelCatalog": {
            "models": [
                {
                    "model": "MiniMax-M3",
                    "displayName": "MiniMax-M3",
                    "contextWindow": 1_000_000,
                    "supportsParallelToolCalls": true,
                    "inputModalities": ["text", "image"],
                    "baseInstructions": "You are Codex, a coding agent based on MiniMax-M3."
                }
            ]
        }
    });

    let catalog =
        codex_model_catalog_from_settings(&settings, "", CodexCatalogToolProfile::NativeResponses)
            .expect("native catalog generation should not error")
            .expect("non-empty modelCatalog must yield a catalog");

    let entry = &catalog["models"][0];
    assert_eq!(
        entry.get("slug").and_then(|v| v.as_str()),
        Some("MiniMax-M3")
    );
    assert_eq!(
        entry.get("shell_type").and_then(|v| v.as_str()),
        Some("shell_command"),
        "native entries edit via shell, not the custom apply_patch tool"
    );
    assert!(
        entry.get("apply_patch_tool_type").is_none(),
        "native entries must NOT declare a freeform apply_patch tool"
    );
    // `base_instructions` is REQUIRED by Codex's catalog parser, so it must
    // be present — and the per-row official override must win over the
    // template default.
    assert_eq!(
        entry.get("base_instructions").and_then(|v| v.as_str()),
        Some("You are Codex, a coding agent based on MiniMax-M3."),
        "per-row baseInstructions override must apply (and field must exist)"
    );
    assert!(
        entry.get("model_messages").is_none(),
        "native entries must not carry the gpt-5.5 model_messages persona text"
    );
    assert_eq!(
        entry.get("supports_parallel_tool_calls"),
        Some(&json!(true)),
        "per-row supportsParallelToolCalls override must apply"
    );
    assert_eq!(
        entry.get("input_modalities"),
        Some(&json!(["text", "image"])),
        "per-row inputModalities override must apply"
    );
    assert_eq!(
        entry.get("context_window").and_then(|v| v.as_u64()),
        Some(1_000_000)
    );
}

#[test]
fn catalog_infers_image_input_independently_of_tool_profile() {
    // Start from a deliberately text-only template to prove that every
    // profile overwrites template defaults with shared capability logic.
    let template = json!({
        "input_modalities": ["text"],
        "apply_patch_tool_type": "freeform"
    });
    let specs = vec![
        CodexCatalogModelSpec {
            model: "gpt-5.4".to_string(),
            display_name: "GPT 5.4".to_string(),
            context_window: 128_000,
            supports_parallel_tool_calls: None,
            input_modalities: None,
            base_instructions: None,
        },
        CodexCatalogModelSpec {
            model: "deepseek/deepseek-v4-pro".to_string(),
            display_name: "DeepSeek V4 Pro".to_string(),
            context_window: 128_000,
            supports_parallel_tool_calls: None,
            input_modalities: None,
            base_instructions: None,
        },
        CodexCatalogModelSpec {
            model: "glm-5.2v".to_string(),
            display_name: "GLM 5.2V".to_string(),
            context_window: 128_000,
            supports_parallel_tool_calls: None,
            input_modalities: None,
            base_instructions: None,
        },
        CodexCatalogModelSpec {
            model: "deepseek-v4-flash".to_string(),
            display_name: "Explicit Visual Override".to_string(),
            context_window: 128_000,
            supports_parallel_tool_calls: None,
            input_modalities: Some(vec!["text".to_string(), "image".to_string()]),
            base_instructions: None,
        },
        CodexCatalogModelSpec {
            model: "custom-text-alias".to_string(),
            display_name: "Explicit Text Override".to_string(),
            context_window: 128_000,
            supports_parallel_tool_calls: None,
            input_modalities: Some(vec!["text".to_string()]),
            base_instructions: None,
        },
    ];

    for profile in [
        CodexCatalogToolProfile::ProxyChat,
        CodexCatalogToolProfile::NativeResponses,
        CodexCatalogToolProfile::Anthropic,
    ] {
        let catalog = codex_model_catalog_from_specs(&specs, &template, profile);
        let models = catalog["models"].as_array().expect("models array");
        let modalities = |slug: &str| {
            models
                .iter()
                .find(|entry| entry["slug"] == slug)
                .and_then(|entry| entry.get("input_modalities"))
                .cloned()
                .unwrap_or(Value::Null)
        };

        assert_eq!(modalities("gpt-5.4"), json!(["text", "image"]));
        assert_eq!(modalities("deepseek/deepseek-v4-pro"), json!(["text"]));
        assert_eq!(modalities("glm-5.2v"), json!(["text", "image"]));
        assert_eq!(
            modalities("deepseek-v4-flash"),
            json!(["text", "image"]),
            "explicit provider metadata must override the text-only registry"
        );
        assert_eq!(modalities("custom-text-alias"), json!(["text"]));
    }
}

#[test]
fn native_responses_catalog_always_carries_base_instructions() {
    // Regression guard for the "missing field `base_instructions`" parse
    // error: Codex refuses to load a model catalog whose entries lack
    // base_instructions. Synthesized presets carry no per-row override, so
    // the entry MUST inherit the template's neutral default rather than
    // dropping the field entirely.
    let settings = json!({
        "modelCatalog": { "models": [{ "model": "qwen3-coder-plus" }] }
    });

    let catalog =
        codex_model_catalog_from_settings(&settings, "", CodexCatalogToolProfile::NativeResponses)
            .expect("native catalog generation should not error")
            .expect("non-empty modelCatalog must yield a catalog");

    let base = catalog["models"][0]
        .get("base_instructions")
        .and_then(|v| v.as_str());
    assert!(
        base.is_some_and(|s| !s.trim().is_empty()),
        "every native entry must carry a non-empty base_instructions (Codex requires it)"
    );
}

#[test]
fn proxy_chat_profile_still_keeps_apply_patch() {
    // Regression guard for Mode A: the proxy-chat profile must keep the
    // freeform apply_patch tool (the proxy rewrites custom<->function).
    let template = load_codex_native_responses_template();
    let specs = vec![CodexCatalogModelSpec {
        model: "x".to_string(),
        display_name: "x".to_string(),
        context_window: 128_000,
        supports_parallel_tool_calls: None,
        input_modalities: None,
        base_instructions: None,
    }];
    // Using a gpt-5.5-shaped template under ProxyChat must NOT strip
    // apply_patch_tool_type. (The native template lacks it, so synthesize
    // one with the field present to prove ProxyChat leaves it intact.)
    let mut proxy_template = template.clone();
    proxy_template["apply_patch_tool_type"] = json!("freeform");
    let catalog =
        codex_model_catalog_from_specs(&specs, &proxy_template, CodexCatalogToolProfile::ProxyChat);
    assert_eq!(
        catalog["models"][0]
            .get("apply_patch_tool_type")
            .and_then(|v| v.as_str()),
        Some("freeform"),
        "ProxyChat must preserve apply_patch_tool_type (no native stripping)"
    );
}

#[test]
fn model_catalog_json_field_writes_relative_filename() {
    let input = r#"model_provider = "any"

[model_providers.any]
name = "any"
"#;
    let catalog_path = Path::new("/tmp/cc-switch-model-catalog.json");

    let result = set_codex_model_catalog_json_field(input, Some(catalog_path)).unwrap();
    let parsed: toml::Value = toml::from_str(&result).unwrap();
    assert_eq!(
        parsed
            .get("model_catalog_json")
            .and_then(|value| value.as_str()),
        Some(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME)
    );
    assert!(
        parsed
            .get("model_providers")
            .and_then(|value| value.get("any"))
            .and_then(|value| value.get("model_catalog_json"))
            .is_none(),
        "model_catalog_json should stay top-level"
    );
}

#[test]
fn native_web_search_field_disables_at_top_level() {
    // Native `/responses` gateways reject the web_search tool, so the
    // NativeResponses profile must write the top-level disable line even
    // when sections are present (it must NOT land inside a section).
    let input = r#"model_provider = "custom"

[model_providers.custom]
name = "xiaomi_mimo"
"#;
    let result = set_codex_native_web_search_field(input, true).unwrap();
    let parsed: toml::Value = toml::from_str(&result).unwrap();
    assert_eq!(
        parsed.get("web_search").and_then(|value| value.as_str()),
        Some("disabled")
    );
    assert!(
        parsed
            .get("model_providers")
            .and_then(|value| value.get("custom"))
            .and_then(|value| value.get("web_search"))
            .is_none(),
        "web_search should stay top-level"
    );
}

#[test]
fn native_web_search_field_removes_own_sentinel_when_not_disabled() {
    // Switching away from a native provider must re-enable web search by
    // removing cc-switch's own "disabled" sentinel.
    let input = r#"model = "gpt-5.5"
web_search = "disabled"
"#;
    let result = set_codex_native_web_search_field(input, false).unwrap();
    let parsed: toml::Value = toml::from_str(&result).unwrap();
    assert!(
        parsed.get("web_search").is_none(),
        "cc-switch's disabled sentinel should be removed when not native"
    );
}

#[test]
fn native_web_search_field_preserves_user_value() {
    // A user's own web_search value must never be clobbered by cleanup,
    // only cc-switch's "disabled" sentinel is owned/removable.
    let input = r#"web_search = "enabled"
"#;
    let result = set_codex_native_web_search_field(input, false).unwrap();
    let parsed: toml::Value = toml::from_str(&result).unwrap();
    assert_eq!(
        parsed.get("web_search").and_then(|value| value.as_str()),
        Some("enabled"),
        "a user-set web_search value must be preserved"
    );
}

#[test]
fn anthropic_profile_disables_web_search_without_catalog() {
    // Regression: even when no model catalog is generated (empty/absent
    // modelCatalog), an Anthropic provider must still disable web_search — the
    // Responses→Anthropic transform drops the hosted tool, so leaving it on
    // exposes a dead tool. The None-catalog branch previously always left it on.
    let config = "model = \"claude-sonnet-4-6\"\n";
    let settings = serde_json::json!({});

    let anthropic = prepare_codex_config_text_with_model_catalog(
        &settings,
        config,
        CodexCatalogToolProfile::Anthropic,
    )
    .unwrap();
    let parsed: toml::Value = toml::from_str(&anthropic).unwrap();
    assert_eq!(
        parsed.get("web_search").and_then(|v| v.as_str()),
        Some("disabled"),
        "Anthropic profile must disable web_search even with no catalog"
    );

    // ProxyChat on the same no-catalog path must NOT add a disable line.
    let proxy = prepare_codex_config_text_with_model_catalog(
        &settings,
        config,
        CodexCatalogToolProfile::ProxyChat,
    )
    .unwrap();
    let parsed: toml::Value = toml::from_str(&proxy).unwrap();
    assert!(
        parsed.get("web_search").is_none(),
        "ProxyChat profile must not disable web_search on the no-catalog path"
    );
}

#[test]
fn web_search_blacklist_disables_only_known_reject_gateways() {
    let cfg = |model: &str, base_url: &str| {
        format!(
                "model_provider = \"custom\"\nmodel = \"{model}\"\n\n[model_providers.custom]\nname = \"x\"\nbase_url = \"{base_url}\"\nwire_api = \"responses\"\n"
            )
    };

    // Blacklisted by host (first-party reject gateways) → disable.
    for (model, host) in [
        ("mimo-v2.5-pro", "https://api.xiaomimimo.com/v1"),
        ("mimo-v2.5", "https://token-plan-cn.xiaomimimo.com/v1"),
        ("LongCat-2.0", "https://api.longcat.chat/openai/v1"),
        ("MiniMax-M3", "https://api.minimax.io/v1"),
        ("MiniMax-M3", "https://api.minimaxi.com/v1"),
    ] {
        assert!(
            codex_native_gateway_rejects_web_search(&cfg(model, host)),
            "{host} should be blacklisted"
        );
    }

    // Blacklisted by MODEL brand even on an aggregator host (SiliconFlow
    // fronting a reject vendor's model) → disable.
    for (model, host) in [
        ("MiniMax-M3", "https://api.siliconflow.cn/v1"),
        ("MiniMaxAI/MiniMax-M3", "https://api.siliconflow.cn/v1"),
        ("mimo-v2.5-pro", "https://some-aggregator.example/v1"),
        (
            "qwen/qwen3-coder-plus",
            "https://some-aggregator.example/v1",
        ),
    ] {
        assert!(
            codex_native_gateway_rejects_web_search(&cfg(model, host)),
            "{model} @ {host} should be blacklisted by model brand"
        );
    }

    // Qwen3-Coder is blacklisted by model, not by DashScope host. This keeps
    // general Qwen models that support built-in web_search on the same host
    // enabled while protecting the native qwen3-coder-plus preset.
    assert!(codex_native_gateway_rejects_web_search(&cfg(
        "qwen3-coder-plus",
        "https://dashscope.aliyuncs.com/compatible-mode/v1",
    )));
    assert!(!codex_native_gateway_rejects_web_search(&cfg(
        "qwen3.7-plus",
        "https://dashscope.aliyuncs.com/compatible-mode/v1",
    )));

    // NOT blacklisted → keep Codex default (relays/GPT, DouBao, general Qwen,
    // and any unknown provider incl. an aggregator serving a non-reject model).
    for (model, host) in [
        ("gpt-5.5", "https://www.packyapi.com/v1"),
        ("gpt-5-codex", "https://aihubmix.com/v1"),
        (
            "doubao-seed-2-1-pro-260628",
            "https://ark.cn-beijing.volces.com/api/v3",
        ),
        ("Pro/moonshotai/Kimi-K2.6", "https://api.siliconflow.cn/v1"),
    ] {
        assert!(
            !codex_native_gateway_rejects_web_search(&cfg(model, host)),
            "{model} @ {host} should NOT be blacklisted"
        );
    }
}

#[test]
fn resolve_catalog_path_returns_none_when_config_missing_field() {
    let generated = PathBuf::from("/tmp/.codex/cc-switch-model-catalog.json");
    assert!(resolve_cc_switch_catalog_path("", &generated).is_none());
    assert!(
        resolve_cc_switch_catalog_path("model = \"gpt-5\"", &generated).is_none(),
        "no model_catalog_json field should yield None"
    );
}

#[test]
fn resolve_catalog_path_accepts_cc_switch_owned_file() {
    let generated = PathBuf::from("/tmp/.codex/cc-switch-model-catalog.json");
    let config = r#"model_catalog_json = "/tmp/.codex/cc-switch-model-catalog.json"
"#;
    let resolved = resolve_cc_switch_catalog_path(config, &generated).expect("path resolves");
    assert_eq!(resolved, generated);
}

#[test]
fn resolve_catalog_path_rejects_user_owned_external_file() {
    let generated = PathBuf::from("/tmp/.codex/cc-switch-model-catalog.json");
    let config = r#"model_catalog_json = "/Users/me/.codex/my-handwritten-catalog.json"
"#;
    assert!(
        resolve_cc_switch_catalog_path(config, &generated).is_none(),
        "external catalog files should be left alone"
    );
}

#[test]
fn build_simplified_catalog_round_trips_user_input() {
    let config = "";
    let catalog = r#"{
            "models": [
                { "slug": "deepseek-v4-pro", "display_name": "deepseek-v4-pro", "context_window": 1000000 },
                { "slug": "deepseek-v4-flash", "display_name": "DeepSeek Flash", "context_window": 1000000 }
            ]
        }"#;
    let result = build_simplified_catalog_from_texts(config, catalog).expect("entries found");
    let models = result
        .get("models")
        .and_then(|m| m.as_array())
        .expect("models array");
    assert_eq!(models.len(), 2);

    // First entry: display_name == slug → displayName squashed; explicit
    // context_window != default 128_000 → preserved.
    assert_eq!(
        models[0].get("model").and_then(|v| v.as_str()),
        Some("deepseek-v4-pro")
    );
    assert!(models[0].get("displayName").is_none());
    assert_eq!(
        models[0].get("contextWindow").and_then(|v| v.as_u64()),
        Some(1_000_000)
    );

    // Second entry: display_name distinct from slug → preserved.
    assert_eq!(
        models[1].get("displayName").and_then(|v| v.as_str()),
        Some("DeepSeek Flash")
    );
}

#[test]
fn build_simplified_catalog_squashes_default_context_window() {
    // Default fallback is 128_000 when config.toml has no model_context_window.
    let catalog = r#"{
            "models": [{ "slug": "kimi", "display_name": "kimi", "context_window": 128000 }]
        }"#;
    let result = build_simplified_catalog_from_texts("", catalog).expect("entry");
    let entry = &result.get("models").unwrap().as_array().unwrap()[0];
    assert!(
            entry.get("contextWindow").is_none(),
            "default 128_000 should be squashed so the form shows blank, matching the user's blank input"
        );
}

#[test]
fn build_simplified_catalog_respects_explicit_model_context_window() {
    // When config.toml sets model_context_window, that becomes the default fallback.
    let config = r#"model_context_window = 200000
"#;
    let catalog = r#"{
            "models": [
                { "slug": "a", "display_name": "a", "context_window": 200000 },
                { "slug": "b", "display_name": "b", "context_window": 500000 }
            ]
        }"#;
    let result = build_simplified_catalog_from_texts(config, catalog).expect("entries");
    let models = result.get("models").unwrap().as_array().unwrap();
    // Matches default → squashed.
    assert!(models[0].get("contextWindow").is_none());
    // Different from default → preserved.
    assert_eq!(
        models[1].get("contextWindow").and_then(|v| v.as_u64()),
        Some(500_000)
    );
}

#[test]
fn build_simplified_catalog_squashes_inferred_modalities_and_keeps_overrides() {
    let catalog = r#"{
            "models": [
                { "slug": "gpt-5.4", "input_modalities": ["text", "image"] },
                { "slug": "deepseek-v4-pro", "input_modalities": ["text"] },
                { "slug": "gpt-text-override", "input_modalities": ["text"] },
                { "slug": "deepseek-v4-flash", "input_modalities": ["text", "image"] }
            ]
        }"#;

    let result = build_simplified_catalog_from_texts("", catalog).expect("entries");
    let models = result.get("models").unwrap().as_array().unwrap();

    assert!(
        models[0].get("inputModalities").is_none(),
        "GPT text+image is inferred and must not become a sticky hidden override"
    );
    assert!(
        models[1].get("inputModalities").is_none(),
        "confirmed text-only capability is inferred and must remain registry-driven"
    );
    assert_eq!(
        models[2].get("inputModalities"),
        Some(&json!(["text"])),
        "an unknown model explicitly forced to text-only must round-trip"
    );
    assert_eq!(
        models[3].get("inputModalities"),
        Some(&json!(["text", "image"])),
        "an explicit image override for a registered text-only model must round-trip"
    );
}

#[test]
fn build_simplified_catalog_returns_none_when_unparseable() {
    assert!(build_simplified_catalog_from_texts("", "not json").is_none());
    assert!(build_simplified_catalog_from_texts("", "{}").is_none());
    assert!(
        build_simplified_catalog_from_texts("", r#"{"models": []}"#).is_none(),
        "empty models array should yield None so the field is not inserted at all"
    );
    assert!(
        build_simplified_catalog_from_texts("", r#"{"models": [{"display_name": "no slug"}]}"#,)
            .is_none(),
        "entries lacking slug are skipped; a fully-skipped catalog yields None"
    );
}

#[test]
fn codex_cli_candidates_are_non_empty() {
    let candidates = codex_cli_candidates();
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate == Path::new("codex")),
        "codex CLI candidates must include the PATH entry"
    );
}

#[test]
fn codex_cli_candidates_include_user_node_manager_bins() {
    let temp_home = tempfile::tempdir().expect("create temp home");
    let home = temp_home.path();
    let expected = [
        home.join(".nvm/versions/node/v22.14.0/bin/codex"),
        home.join(".volta/bin/codex"),
        home.join(".asdf/shims/codex"),
        home.join(".local/share/mise/shims/codex"),
        home.join(".local/share/fnm/node-versions/v22.14.0/installation/bin/codex"),
    ];

    for candidate in &expected {
        std::fs::create_dir_all(candidate.parent().expect("candidate parent"))
            .expect("create candidate parent");
        std::fs::write(candidate, "").expect("create candidate");
    }

    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    push_home_codex_cli_candidates(&mut candidates, &mut seen, home);

    for candidate in expected {
        assert!(
            candidates.contains(&candidate),
            "user-level Codex CLI candidate should be discovered: {}",
            candidate.display()
        );
    }
}

#[test]
fn codex_cli_candidates_deduplicate_entries() {
    let temp_home = tempfile::tempdir().expect("create temp home");
    let home = temp_home.path();
    let candidate = home.join(".volta/bin/codex");
    std::fs::create_dir_all(candidate.parent().expect("candidate parent"))
        .expect("create candidate parent");
    std::fs::write(&candidate, "").expect("create candidate");

    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    push_existing_codex_cli_candidate(&mut candidates, &mut seen, candidate.clone());
    push_home_codex_cli_candidates(&mut candidates, &mut seen, home);

    assert_eq!(
        candidates.iter().filter(|path| **path == candidate).count(),
        1,
        "duplicate candidates should be removed"
    );
}

#[test]
fn static_template_is_valid_json_with_slug() {
    let template =
        load_codex_model_template_static().expect("static template must parse as valid JSON");
    assert_eq!(
        template.get("slug").and_then(|v| v.as_str()),
        Some("gpt-5.5"),
        "static template slug must be gpt-5.5"
    );
}

#[test]
fn static_template_has_required_keys() {
    let template =
        load_codex_model_template_static().expect("static template must parse as valid JSON");
    for key in &[
        "model_messages",
        "base_instructions",
        "context_window",
        "display_name",
    ] {
        assert!(
            template.get(key).is_some(),
            "static template must contain key '{key}'"
        );
    }
}

#[test]
#[cfg(target_os = "windows")]
fn set_catalog_json_field_writes_filename_ignoring_unc_path() {
    let input = r#"model_provider = "custom"
model = "glm-5"
"#;
    // Simulate a WSL UNC path as cc-switch would see it on Windows;
    // the function now writes just the relative filename.
    let unc_path =
        Path::new(r"\\wsl.localhost\Ubuntu\home\user\.codex\cc-switch-model-catalog.json");

    let result = set_codex_model_catalog_json_field(input, Some(unc_path)).unwrap();
    let parsed: toml::Value = toml::from_str(&result).unwrap();

    let written_path = parsed
        .get("model_catalog_json")
        .and_then(|v| v.as_str())
        .expect("model_catalog_json should be set");
    assert_eq!(
        written_path, CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME,
        "should write only the relative filename, not the UNC path"
    );
}

#[test]
fn set_catalog_json_field_writes_filename_for_any_path() {
    let input = r#"model_provider = "custom"
model = "glm-5"
"#;
    let regular_path = Path::new("/home/user/.codex/cc-switch-model-catalog.json");

    let result = set_codex_model_catalog_json_field(input, Some(regular_path)).unwrap();
    let parsed: toml::Value = toml::from_str(&result).unwrap();

    assert_eq!(
        parsed.get("model_catalog_json").and_then(|v| v.as_str()),
        Some(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME),
        "should write only the relative filename, not the full path"
    );
}

#[test]
fn set_catalog_json_none_removes_cc_switch_owned_by_filename() {
    // After the WSL fix, TOML may contain a Linux-style path.
    // The None arm must still remove it (file_name match catches any format).
    let input = r#"model_catalog_json = "/home/user/.codex/cc-switch-model-catalog.json"
"#;
    let result = set_codex_model_catalog_json_field(input, None).unwrap();
    let parsed: toml::Value = toml::from_str(&result).unwrap();
    assert!(
        parsed.get("model_catalog_json").is_none(),
        "None arm should remove cc-switch-owned field regardless of path format"
    );
}

#[test]
fn set_catalog_json_none_preserves_user_owned_catalog() {
    let input = r#"model_catalog_json = "/Users/me/.codex/my-custom-catalog.json"
"#;
    let result = set_codex_model_catalog_json_field(input, None).unwrap();
    let parsed: toml::Value = toml::from_str(&result).unwrap();
    assert_eq!(
        parsed.get("model_catalog_json").and_then(|v| v.as_str()),
        Some("/Users/me/.codex/my-custom-catalog.json"),
        "None arm should NOT remove user-owned catalog"
    );
}

#[test]
fn resolve_catalog_finds_relative_filename() {
    let config_text = r#"model_provider = "custom"
model_catalog_json = "cc-switch-model-catalog.json"
"#;
    let generated_path = PathBuf::from("/home/user/.codex/cc-switch-model-catalog.json");
    let result = resolve_cc_switch_catalog_path(config_text, &generated_path);
    assert_eq!(
        result,
        Some(generated_path),
        "relative filename should resolve to generated_path for file I/O"
    );
}

#[test]
fn resolve_catalog_ignores_user_owned_relative() {
    let config_text = r#"model_catalog_json = "my-custom-catalog.json"
"#;
    let generated_path = PathBuf::from("/home/user/.codex/cc-switch-model-catalog.json");
    let result = resolve_cc_switch_catalog_path(config_text, &generated_path);
    assert_eq!(
        result, None,
        "user-owned catalog should not be claimed by cc-switch"
    );
}

#[test]
fn set_catalog_json_none_removes_relative_path() {
    let input = r#"model_catalog_json = "cc-switch-model-catalog.json"
"#;
    let result = set_codex_model_catalog_json_field(input, None).unwrap();
    let parsed: toml::Value = toml::from_str(&result).unwrap();
    assert!(
        parsed.get("model_catalog_json").is_none(),
        "None arm should remove relative cc-switch-owned field"
    );
}
