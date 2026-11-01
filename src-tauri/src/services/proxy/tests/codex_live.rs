//! Codex live-config writes outside takeover (auth preservation, TOML handling).

use super::*;

#[test]
#[serial]
fn codex_custom_provider_live_write_preserves_oauth_auth_json() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");
    crate::settings::update_settings(crate::settings::AppSettings {
        preserve_codex_official_auth_on_switch: true,
        ..Default::default()
    })
    .expect("enable Codex official auth preservation");

    let db = Arc::new(Database::memory().expect("init db"));
    let service = ProxyService::new(db);
    let oauth_auth = json!({
        "auth_mode": "chatgpt",
        "tokens": {
            "id_token": "oauth-id",
            "access_token": "oauth-access"
        }
    });
    crate::codex_config::write_codex_live_atomic(
        &oauth_auth,
        Some(
            r#"model_provider = "openai"
model = "gpt-5-codex"
"#,
        ),
    )
    .expect("seed live OAuth auth");

    let mut provider = Provider::with_id(
        "rightcode".to_string(),
        "RightCode".to_string(),
        json!({
            "auth": {
                "OPENAI_API_KEY": "rightcode-key"
            },
            "config": r#"model_provider = "rightcode"
model = "gpt-5-codex"

[model_providers.rightcode]
name = "RightCode"
base_url = "https://rightcode.example/v1"
wire_api = "responses"
"#
        }),
        None,
    );
    provider.category = Some("custom".to_string());
    let takeover_settings = json!({
        "auth": {
            "OPENAI_API_KEY": PROXY_TOKEN_PLACEHOLDER
        },
        "config": r#"model_provider = "rightcode"
model = "gpt-5-codex"

[model_providers.rightcode]
name = "RightCode"
base_url = "http://127.0.0.1:15721/v1"
wire_api = "responses"
"#
    });

    service
        .write_codex_live_for_provider(&takeover_settings, Some(&provider))
        .expect("write provider-driven Codex live config");

    let live_auth: Value =
        crate::config::read_json_file(&crate::codex_config::get_codex_auth_path())
            .expect("read live auth");
    assert_eq!(
        live_auth, oauth_auth,
        "third-party Codex proxy writes must not overwrite ChatGPT OAuth login state"
    );

    let live_config = std::fs::read_to_string(crate::codex_config::get_codex_config_path())
        .expect("read live config");
    assert!(
        live_config.contains("experimental_bearer_token"),
        "proxy placeholder should move into config.toml instead of auth.json"
    );
    assert!(
        live_config.contains(PROXY_TOKEN_PLACEHOLDER),
        "live config should carry the proxy placeholder token"
    );

    crate::settings::update_settings(crate::settings::AppSettings::default())
        .expect("reset settings");
}

#[test]
#[serial]
fn codex_custom_provider_live_write_can_overwrite_auth_when_preserve_disabled() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");
    crate::settings::update_settings(crate::settings::AppSettings {
        preserve_codex_official_auth_on_switch: false,
        ..Default::default()
    })
    .expect("disable Codex official auth preservation");

    let db = Arc::new(Database::memory().expect("init db"));
    let service = ProxyService::new(db);
    let oauth_auth = json!({
        "auth_mode": "chatgpt",
        "tokens": {
            "id_token": "oauth-id",
            "access_token": "oauth-access"
        }
    });
    crate::codex_config::write_codex_live_atomic(
        &oauth_auth,
        Some(
            r#"model_provider = "openai"
model = "gpt-5-codex"
"#,
        ),
    )
    .expect("seed live OAuth auth");

    let mut provider = Provider::with_id(
        "rightcode".to_string(),
        "RightCode".to_string(),
        json!({
            "auth": {
                "OPENAI_API_KEY": "rightcode-key"
            },
            "config": r#"model_provider = "rightcode"
model = "gpt-5-codex"

[model_providers.rightcode]
name = "RightCode"
base_url = "https://rightcode.example/v1"
wire_api = "responses"
"#
        }),
        None,
    );
    provider.category = Some("custom".to_string());
    let takeover_auth = json!({
        "OPENAI_API_KEY": PROXY_TOKEN_PLACEHOLDER
    });
    let takeover_settings = json!({
        "auth": takeover_auth,
        "config": r#"model_provider = "rightcode"
model = "gpt-5-codex"

[model_providers.rightcode]
name = "RightCode"
base_url = "http://127.0.0.1:15721/v1"
wire_api = "responses"
"#
    });

    service
        .write_codex_live_for_provider(&takeover_settings, Some(&provider))
        .expect("write provider-driven Codex live config");

    let live_auth: Value =
        crate::config::read_json_file(&crate::codex_config::get_codex_auth_path())
            .expect("read live auth");
    assert_eq!(
        live_auth,
        json!({
            "OPENAI_API_KEY": PROXY_TOKEN_PLACEHOLDER
        }),
        "disabled preservation should let third-party switches overwrite auth.json"
    );

    let live_config = std::fs::read_to_string(crate::codex_config::get_codex_config_path())
        .expect("read live config");
    assert!(
        !live_config.contains("experimental_bearer_token"),
        "provider token should stay in auth.json when preservation is disabled"
    );

    crate::settings::update_settings(crate::settings::AppSettings::default())
        .expect("reset settings");
}

#[test]
fn apply_codex_proxy_toml_config_forces_local_responses_wire_api() {
    let input = r#"
model_provider = "chat_only"
model = "gpt-5.1-codex"

[model_providers.chat_only]
name = "Chat Only"
base_url = "https://chat-only.example/v1"
wire_api = "chat"
"#;

    let proxy_url = "http://127.0.0.1:5000/v1";
    let output = ProxyService::apply_codex_proxy_toml_config_for_provider(input, proxy_url, None)
        .expect("apply proxy config");
    let parsed: toml::Value = toml::from_str(&output).expect("updated config should be valid TOML");

    let provider = parsed
        .get("model_providers")
        .and_then(|v| v.get("chat_only"))
        .expect("model_providers.chat_only should exist");

    assert_eq!(
        provider.get("base_url").and_then(|v| v.as_str()),
        Some(proxy_url)
    );
    assert_eq!(
        provider.get("wire_api").and_then(|v| v.as_str()),
        Some("responses")
    );
}

#[test]
fn apply_codex_proxy_toml_config_routes_builtin_official_with_native_auth() {
    let mut provider = Provider::with_id(
        "codex-official".to_string(),
        "OpenAI Official".to_string(),
        json!({ "auth": {}, "config": "" }),
        None,
    );
    provider.category = Some("official".to_string());
    let proxy_url = "http://127.0.0.1:5000/v1";

    let output = ProxyService::apply_codex_proxy_toml_config_for_provider(
        "experimental_bearer_token = \"PROXY_MANAGED\"\n",
        proxy_url,
        Some(&provider),
    )
    .expect("apply official proxy config");
    let parsed: toml::Value = toml::from_str(&output).expect("valid official route");
    let route_id = crate::codex_config::CC_SWITCH_CODEX_OFFICIAL_PROXY_PROVIDER_ID;
    let route = &parsed["model_providers"][route_id];

    assert_eq!(parsed["model_provider"].as_str(), Some(route_id));
    assert_eq!(route["base_url"].as_str(), Some(proxy_url));
    assert_eq!(route["requires_openai_auth"].as_bool(), Some(true));
    assert!(parsed.get("experimental_bearer_token").is_none());
}

#[test]
fn apply_codex_proxy_toml_config_fails_closed_for_invalid_official_config() {
    let mut provider = Provider::with_id(
        crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string(),
        "OpenAI Official".to_string(),
        json!({ "auth": {}, "config": "" }),
        None,
    );
    provider.category = Some("official".to_string());

    let result = ProxyService::apply_codex_proxy_toml_config_for_provider(
        "model_providers = 3\n",
        "http://127.0.0.1:5000/v1",
        Some(&provider),
    );
    assert!(result.is_err());
}

#[test]
fn apply_codex_proxy_toml_config_keeps_upstream_model_for_chat_provider() {
    let input = r#"
model_provider = "deepseek"
model = "deepseek-v4-flash"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/v1"
wire_api = "responses"
"#;
    let mut provider = Provider::with_id(
        "deepseek".to_string(),
        "DeepSeek".to_string(),
        json!({
            "config": input
        }),
        None,
    );
    provider.meta = Some(ProviderMeta {
        api_format: Some("openai_chat".to_string()),
        ..Default::default()
    });

    let proxy_url = "http://127.0.0.1:5000/v1";
    let output =
        ProxyService::apply_codex_proxy_toml_config_for_provider(input, proxy_url, Some(&provider))
            .expect("apply chat proxy config");
    let parsed: toml::Value = toml::from_str(&output).expect("updated config should be valid TOML");

    assert_eq!(
        parsed.get("model").and_then(|v| v.as_str()),
        Some("deepseek-v4-flash")
    );
    assert_eq!(
        parsed
            .get("model_providers")
            .and_then(|v| v.get("deepseek"))
            .and_then(|v| v.get("base_url"))
            .and_then(|v| v.as_str()),
        Some(proxy_url)
    );
}

#[test]
fn apply_codex_proxy_toml_config_preserves_model_for_responses_provider() {
    let input = r#"
model_provider = "responses"
model = "upstream-responses-model"

[model_providers.responses]
name = "Responses"
base_url = "https://responses.example/v1"
wire_api = "responses"
"#;
    let mut provider = Provider::with_id(
        "responses".to_string(),
        "Responses".to_string(),
        json!({
            "config": input
        }),
        None,
    );
    provider.meta = Some(ProviderMeta {
        api_format: Some("openai_responses".to_string()),
        ..Default::default()
    });

    let output = ProxyService::apply_codex_proxy_toml_config_for_provider(
        input,
        "http://127.0.0.1:5000/v1",
        Some(&provider),
    )
    .expect("apply responses proxy config");
    let parsed: toml::Value = toml::from_str(&output).expect("updated config should be valid TOML");

    assert_eq!(
        parsed.get("model").and_then(|v| v.as_str()),
        Some("upstream-responses-model")
    );
}

#[tokio::test]
#[serial]
async fn codex_direct_live_write_rolls_back_when_provider_commit_fails() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");

    let db = Arc::new(Database::memory().expect("init db"));
    let state = crate::store::AppState::new(db.clone());
    db.update_proxy_config(ProxyConfig {
        listen_port: 0,
        ..Default::default()
    })
    .await
    .expect("set test proxy config");
    state
        .proxy_service
        .start()
        .await
        .expect("start proxy server");

    let provider_a = Provider::with_id(
        "a".to_string(),
        "Provider A".to_string(),
        json!({
            "auth": { "OPENAI_API_KEY": "key-a" },
            "config": "model_provider = \"provider-a\"\nmodel = \"model-a\"\n\n[model_providers.provider-a]\nname = \"Provider A\"\nbase_url = \"https://a.example/v1\"\nwire_api = \"responses\"\nrequires_openai_auth = true\n"
        }),
        None,
    );
    let provider_b = Provider::with_id(
        "b".to_string(),
        "Provider B".to_string(),
        json!({
            "auth": { "OPENAI_API_KEY": "key-b" },
            "config": "model_provider = \"provider-b\"\nmodel = \"model-b\"\n\n[model_providers.provider-b]\nname = \"Provider B\"\nbase_url = \"https://b.example/v1\"\nwire_api = \"responses\"\nrequires_openai_auth = true\n"
        }),
        None,
    );
    db.save_provider("codex", &provider_a)
        .expect("save provider a");
    db.save_provider("codex", &provider_b)
        .expect("save provider b");
    db.set_current_provider("codex", "a")
        .expect("set current provider a");
    crate::settings::set_current_provider(&AppType::Codex, Some("a"))
        .expect("set local current provider a");
    state
        .proxy_service
        .write_codex_live_for_provider(&provider_a.settings_config, Some(&provider_a))
        .expect("seed direct live config");
    db.save_live_backup(
        "codex",
        &serde_json::to_string(&provider_a.settings_config).expect("serialize backup"),
    )
    .await
    .expect("seed restored backup");
    let original_live = state
        .proxy_service
        .read_codex_live()
        .expect("read original live config");

    {
        let conn = db.conn.lock().expect("lock database");
        conn.execute_batch(
            "CREATE TRIGGER reject_codex_current_update
             BEFORE UPDATE OF is_current ON providers
             WHEN NEW.app_type = 'codex'
             BEGIN
               SELECT RAISE(ABORT, 'forced current-provider commit failure');
             END;",
        )
        .expect("install failure trigger");
    }

    let error = crate::services::provider::ProviderService::switch(&state, AppType::Codex, "b")
        .expect_err("database commit should fail");
    state.proxy_service.stop().await.expect("stop proxy server");

    assert!(error
        .to_string()
        .contains("forced current-provider commit failure"));
    assert_eq!(
        state
            .proxy_service
            .read_codex_live()
            .expect("read rolled-back live config"),
        original_live,
        "commit failure must restore the exact direct Live snapshot"
    );
    assert_eq!(
        db.get_current_provider("codex")
            .expect("read current provider")
            .as_deref(),
        Some("a")
    );
    assert_eq!(
        crate::settings::get_current_provider(&AppType::Codex).as_deref(),
        Some("a")
    );
}
