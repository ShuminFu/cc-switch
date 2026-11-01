//! Codex takeover and hot switch.

use super::*;

#[test]
fn managed_account_claude_takeover_sources_codex_models_from_provider() {
    let mut provider = Provider::with_id(
        "codex".to_string(),
        "Codex".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://chatgpt.com/backend-api/codex",
                "ANTHROPIC_MODEL": "gpt-5.4",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "gpt-5.4-mini",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "gpt-5.4",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "gpt-5.4"
            }
        }),
        None,
    );
    provider.meta = Some(ProviderMeta {
        provider_type: Some("codex_oauth".to_string()),
        ..Default::default()
    });

    let mut live_config = json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://stale.example.com",
            "ANTHROPIC_AUTH_TOKEN": "stale-token",
            "ANTHROPIC_MODEL": "stale-model",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "stale-haiku",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME": "Stale Haiku",
            "ANTHROPIC_DEFAULT_SONNET_MODEL": "stale-sonnet",
            "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME": "Stale Sonnet",
            "ANTHROPIC_DEFAULT_OPUS_MODEL": "stale-opus",
            "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME": "Stale Opus"
        }
    });
    ProxyService::apply_claude_takeover_fields_for_provider(
        &mut live_config,
        "http://127.0.0.1:15721",
        &provider,
    );

    let env = live_config
        .get("env")
        .and_then(|value| value.as_object())
        .expect("env should exist");
    assert_env_str(env, "ANTHROPIC_MODEL", None);
    assert_env_str(
        env,
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        Some("claude-haiku-4-5"),
    );
    assert_env_str(
        env,
        "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
        Some("gpt-5.4-mini"),
    );
    assert_env_str(
        env,
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        Some("claude-sonnet-4-6"),
    );
    assert_env_str(env, "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME", Some("gpt-5.4"));
    assert_env_str(env, "ANTHROPIC_DEFAULT_OPUS_MODEL", Some("claude-opus-4-8"));
    assert_env_str(env, "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME", Some("gpt-5.4"));
    // Codex 系只保留 AUTH_TOKEN；双键会触发 Claude Code 告警（#4919）
    assert_env_str(env, "ANTHROPIC_API_KEY", None);
    assert_env_str(env, "ANTHROPIC_AUTH_TOKEN", Some(PROXY_TOKEN_PLACEHOLDER));
}

#[test]
fn managed_account_claude_takeover_codex_injects_auth_token_without_preexisting_key() {
    let mut provider = Provider::with_id(
        "codex".to_string(),
        "Codex".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://chatgpt.com/backend-api/codex"
            }
        }),
        None,
    );
    provider.meta = Some(ProviderMeta {
        provider_type: Some("codex_oauth".to_string()),
        ..Default::default()
    });

    // 全新安装/热切换形态：传入的 env 没有任何 token 键。
    let mut live_config = provider.settings_config.clone();
    ProxyService::apply_claude_takeover_fields_for_provider(
        &mut live_config,
        "http://127.0.0.1:15721",
        &provider,
    );

    let env = live_config
        .get("env")
        .and_then(|value| value.as_object())
        .expect("env should exist");
    assert_env_str(env, "ANTHROPIC_API_KEY", None);
    assert_env_str(env, "ANTHROPIC_AUTH_TOKEN", Some(PROXY_TOKEN_PLACEHOLDER));
}

#[test]
fn managed_account_claude_takeover_codex_by_base_url_keeps_auth_token() {
    // 无 provider_type meta、仅凭 base_url 识别为受管 codex 的供应商，
    // 也必须保留 AUTH_TOKEN 占位符（与策略选择共用同一判定族）。
    let provider = Provider::with_id(
        "codex-url-only".to_string(),
        "Codex (URL only)".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://chatgpt.com/backend-api/codex"
            }
        }),
        None,
    );
    assert!(provider.uses_managed_account_auth());
    assert!(!provider.is_codex_oauth());

    let mut live_config = provider.settings_config.clone();
    ProxyService::apply_claude_takeover_fields_for_provider(
        &mut live_config,
        "http://127.0.0.1:15721",
        &provider,
    );

    let env = live_config
        .get("env")
        .and_then(|value| value.as_object())
        .expect("env should exist");
    assert_env_str(env, "ANTHROPIC_API_KEY", None);
    assert_env_str(env, "ANTHROPIC_AUTH_TOKEN", Some(PROXY_TOKEN_PLACEHOLDER));
}

// #4919 复现场景：从第三方 Claude 供应商（live 已有 AUTH_TOKEN）切换到
// Codex 受管供应商时，只应保留 AUTH_TOKEN 占位符，不得同时写入 API_KEY。
#[test]
fn managed_account_claude_takeover_codex_from_third_party_keeps_single_auth_key() {
    let mut provider = Provider::with_id(
        "codex".to_string(),
        "Codex".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://chatgpt.com/backend-api/codex"
            }
        }),
        None,
    );
    provider.meta = Some(ProviderMeta {
        provider_type: Some("codex_oauth".to_string()),
        ..Default::default()
    });

    let mut live_config = json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://api.deepseek.com/anthropic",
            "ANTHROPIC_AUTH_TOKEN": "sk-third-party"
        }
    });
    ProxyService::apply_claude_takeover_fields_for_provider(
        &mut live_config,
        "http://127.0.0.1:15721",
        &provider,
    );

    let env = live_config
        .get("env")
        .and_then(|value| value.as_object())
        .expect("env should exist");
    assert_env_str(env, "ANTHROPIC_AUTH_TOKEN", Some(PROXY_TOKEN_PLACEHOLDER));
    assert_env_str(env, "ANTHROPIC_API_KEY", None);
}

#[tokio::test]
#[serial]
async fn codex_takeover_preserves_oauth_auth_json_when_preserve_enabled() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");
    crate::settings::update_settings(crate::settings::AppSettings {
        preserve_codex_official_auth_on_switch: true,
        ..Default::default()
    })
    .expect("enable Codex official auth preservation");

    let db = Arc::new(Database::memory().expect("init db"));
    let service = ProxyService::new(db.clone());
    let oauth_auth = json!({
        "auth_mode": "chatgpt",
        "tokens": {
            "id_token": "oauth-id",
            "access_token": "oauth-access"
        }
    });
    let deepseek_live_config = r#"model_provider = "deepseek"
model = "deepseek-v4-flash"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/v1"
wire_api = "responses"
experimental_bearer_token = "deepseek-key"
"#;
    crate::codex_config::write_codex_live_atomic(&oauth_auth, Some(deepseek_live_config))
        .expect("seed live OAuth auth with DeepSeek config");

    let mut provider = Provider::with_id(
        "deepseek".to_string(),
        "DeepSeek".to_string(),
        json!({
            "auth": {
                "OPENAI_API_KEY": "deepseek-key"
            },
            "config": r#"model_provider = "deepseek"
model = "deepseek-v4-flash"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/v1"
wire_api = "responses"
"#
        }),
        None,
    );
    provider.category = Some("cn_official".to_string());
    db.save_provider("codex", &provider)
        .expect("save DeepSeek provider");
    db.set_current_provider("codex", "deepseek")
        .expect("set current provider");
    crate::settings::set_current_provider(&AppType::Codex, Some("deepseek"))
        .expect("set local current provider");

    service
        .takeover_live_config_strict(&AppType::Codex)
        .await
        .expect("take over Codex live config");

    let live_auth: Value =
        crate::config::read_json_file(&crate::codex_config::get_codex_auth_path())
            .expect("read live auth");
    assert_eq!(
        live_auth, oauth_auth,
        "Codex takeover should not overwrite ChatGPT OAuth auth when preservation is enabled"
    );

    let live_config = std::fs::read_to_string(crate::codex_config::get_codex_config_path())
        .expect("read live config");
    assert!(
        live_config.contains(PROXY_TOKEN_PLACEHOLDER),
        "takeover placeholder should move into config.toml"
    );
    assert!(
        service.detect_takeover_in_live_config_for_app(&AppType::Codex),
        "Codex takeover detection should recognize config.toml placeholders"
    );

    crate::settings::update_settings(crate::settings::AppSettings::default())
        .expect("reset settings");
}

#[tokio::test]
#[serial]
async fn codex_takeover_preserves_oauth_auth_json_even_when_provider_category_is_official() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");
    crate::settings::update_settings(crate::settings::AppSettings {
        preserve_codex_official_auth_on_switch: true,
        ..Default::default()
    })
    .expect("enable Codex official auth preservation");

    let db = Arc::new(Database::memory().expect("init db"));
    let service = ProxyService::new(db.clone());
    let oauth_auth = json!({
        "auth_mode": "chatgpt",
        "tokens": {
            "id_token": "oauth-id",
            "access_token": "oauth-access"
        }
    });
    let deepseek_live_config = r#"model_provider = "deepseek"
model = "deepseek-v4-flash"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/v1"
wire_api = "responses"
experimental_bearer_token = "deepseek-key"
"#;
    crate::codex_config::write_codex_live_atomic(&oauth_auth, Some(deepseek_live_config))
        .expect("seed live OAuth auth with DeepSeek config");

    let mut provider = Provider::with_id(
        "deepseek".to_string(),
        "DeepSeek".to_string(),
        json!({
            "auth": {
                "OPENAI_API_KEY": "deepseek-key"
            },
            "config": r#"model_provider = "deepseek"
model = "deepseek-v4-flash"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/v1"
wire_api = "responses"
"#
        }),
        None,
    );
    provider.category = Some("official".to_string());
    db.save_provider("codex", &provider)
        .expect("save misclassified DeepSeek provider");
    db.set_current_provider("codex", "deepseek")
        .expect("set current provider");
    crate::settings::set_current_provider(&AppType::Codex, Some("deepseek"))
        .expect("set local current provider");

    service
        .takeover_live_config_strict(&AppType::Codex)
        .await
        .expect("take over Codex live config");

    let live_auth: Value =
        crate::config::read_json_file(&crate::codex_config::get_codex_auth_path())
            .expect("read live auth");
    assert_eq!(
        live_auth, oauth_auth,
        "Codex takeover must not rewrite auth.json when preservation is enabled, even if provider category is stale or misclassified"
    );

    let live_config = std::fs::read_to_string(crate::codex_config::get_codex_config_path())
        .expect("read live config");
    assert!(
        live_config.contains(PROXY_TOKEN_PLACEHOLDER),
        "takeover placeholder should move into config.toml"
    );

    crate::settings::update_settings(crate::settings::AppSettings::default())
        .expect("reset settings");
}

#[tokio::test]
#[serial]
async fn codex_takeover_hot_switches_between_builtin_official_and_third_party() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");
    // Exercise the default setting: takeover itself must now preserve native
    // auth regardless of the legacy compatibility toggle.
    crate::settings::update_settings(crate::settings::AppSettings::default())
        .expect("reset settings");

    let db = Arc::new(Database::memory().expect("init db"));
    use_ephemeral_proxy_port(&db).await;
    let service = ProxyService::new(db.clone());
    let oauth_auth = json!({
        "auth_mode": "chatgpt",
        "tokens": {
            "id_token": "oauth-id",
            "access_token": "oauth-access"
        }
    });
    crate::codex_config::write_codex_live_atomic(&oauth_auth, Some("model = \"gpt-5.4\"\n"))
        .expect("seed official live config");

    let mut official = Provider::with_id(
        "codex-official".to_string(),
        "OpenAI Official".to_string(),
        json!({ "auth": {}, "config": "model = \"gpt-5.4\"\n" }),
        None,
    );
    official.category = Some("official".to_string());
    db.save_provider("codex", &official)
        .expect("save official provider");

    let mut third_party = Provider::with_id(
        "rightcode".to_string(),
        "RightCode".to_string(),
        json!({
            "auth": { "OPENAI_API_KEY": "rightcode-key" },
            "config": r#"model_provider = "rightcode"

[model_providers.rightcode]
name = "RightCode"
base_url = "https://rightcode.example/v1"
wire_api = "responses"
"#
        }),
        None,
    );
    third_party.category = Some("custom".to_string());
    db.save_provider("codex", &third_party)
        .expect("save third-party provider");
    db.set_current_provider("codex", "codex-official")
        .expect("set current provider");
    crate::settings::set_current_provider(&AppType::Codex, Some("codex-official"))
        .expect("set local current provider");

    service
        .set_takeover_for_app("codex", true)
        .await
        .expect("enable official takeover");

    let read_auth = || -> Value {
        crate::config::read_json_file(&crate::codex_config::get_codex_auth_path())
            .expect("read live auth")
    };
    assert_eq!(read_auth(), oauth_auth);
    let official_live = std::fs::read_to_string(crate::codex_config::get_codex_config_path())
        .expect("read official takeover config");
    assert!(crate::codex_config::codex_config_has_official_proxy_route(
        &official_live
    ));
    assert!(official_live.contains("requires_openai_auth = true"));
    assert!(!official_live.contains(PROXY_TOKEN_PLACEHOLDER));

    service
        .hot_switch_provider("codex", "rightcode")
        .await
        .expect("switch to third-party provider");
    assert_eq!(
        read_auth(),
        oauth_auth,
        "third-party route must preserve OAuth"
    );
    let third_party_live = std::fs::read_to_string(crate::codex_config::get_codex_config_path())
        .expect("read third-party takeover config");
    assert!(third_party_live.contains(PROXY_TOKEN_PLACEHOLDER));
    assert!(!crate::codex_config::codex_config_has_official_proxy_route(
        &third_party_live
    ));

    service
        .hot_switch_provider("codex", "codex-official")
        .await
        .expect("switch back to official provider");
    assert_eq!(
        read_auth(),
        oauth_auth,
        "official switch must reuse native OAuth"
    );
    let official_live = std::fs::read_to_string(crate::codex_config::get_codex_config_path())
        .expect("read restored official takeover config");
    assert!(crate::codex_config::codex_config_has_official_proxy_route(
        &official_live
    ));
    assert!(!official_live.contains(PROXY_TOKEN_PLACEHOLDER));

    service
        .set_takeover_for_app("codex", false)
        .await
        .expect("disable takeover");
    assert_eq!(read_auth(), oauth_auth);
}

#[tokio::test]
#[serial]
async fn codex_set_takeover_for_app_preserves_oauth_auth_json_when_preserve_enabled() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");
    crate::settings::update_settings(crate::settings::AppSettings {
        preserve_codex_official_auth_on_switch: true,
        ..Default::default()
    })
    .expect("enable Codex official auth preservation");

    let db = Arc::new(Database::memory().expect("init db"));
    use_ephemeral_proxy_port(&db).await;
    let service = ProxyService::new(db.clone());
    let oauth_auth = json!({
        "auth_mode": "chatgpt",
        "tokens": {
            "id_token": "oauth-id",
            "access_token": "oauth-access"
        }
    });
    let deepseek_live_config = r#"model_provider = "deepseek"
model = "deepseek-v4-flash"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/v1"
wire_api = "responses"
experimental_bearer_token = "deepseek-key"
"#;
    crate::codex_config::write_codex_live_atomic(&oauth_auth, Some(deepseek_live_config))
        .expect("seed live OAuth auth with DeepSeek config");

    let mut provider = Provider::with_id(
        "deepseek".to_string(),
        "DeepSeek".to_string(),
        json!({
            "auth": {
                "OPENAI_API_KEY": "deepseek-key"
            },
            "config": r#"model_provider = "deepseek"
model = "deepseek-v4-flash"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/v1"
wire_api = "responses"
"#
        }),
        None,
    );
    provider.category = Some("official".to_string());
    db.save_provider("codex", &provider)
        .expect("save misclassified DeepSeek provider");
    db.set_current_provider("codex", "deepseek")
        .expect("set current provider");
    crate::settings::set_current_provider(&AppType::Codex, Some("deepseek"))
        .expect("set local current provider");

    service
        .set_takeover_for_app("codex", true)
        .await
        .expect("enable Codex takeover");

    let live_auth: Value =
        crate::config::read_json_file(&crate::codex_config::get_codex_auth_path())
            .expect("read live auth");
    assert_eq!(
        live_auth, oauth_auth,
        "the public takeover command path must not rewrite auth.json when preservation is enabled"
    );

    service
        .set_takeover_for_app("codex", false)
        .await
        .expect("disable Codex takeover");
    crate::settings::update_settings(crate::settings::AppSettings::default())
        .expect("reset settings");
}

#[tokio::test]
#[serial]
async fn codex_takeover_preserves_native_auth_even_when_legacy_toggle_is_disabled() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");
    crate::settings::update_settings(crate::settings::AppSettings {
        preserve_codex_official_auth_on_switch: false,
        ..Default::default()
    })
    .expect("disable Codex official auth preservation");

    let db = Arc::new(Database::memory().expect("init db"));
    let service = ProxyService::new(db.clone());
    let oauth_auth = json!({
        "auth_mode": "chatgpt",
        "tokens": {
            "id_token": "oauth-id",
            "access_token": "oauth-access"
        }
    });
    let deepseek_live_config = r#"model_provider = "deepseek"
model = "deepseek-v4-flash"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/v1"
wire_api = "responses"
"#;
    crate::codex_config::write_codex_live_atomic(&oauth_auth, Some(deepseek_live_config))
        .expect("seed live OAuth auth with DeepSeek config");

    let mut provider = Provider::with_id(
        "deepseek".to_string(),
        "DeepSeek".to_string(),
        json!({
            "auth": {
                "OPENAI_API_KEY": "deepseek-key"
            },
            "config": r#"model_provider = "deepseek"
model = "deepseek-v4-flash"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/v1"
wire_api = "responses"
"#
        }),
        None,
    );
    provider.category = Some("cn_official".to_string());
    db.save_provider("codex", &provider)
        .expect("save DeepSeek provider");
    db.set_current_provider("codex", "deepseek")
        .expect("set current provider");
    crate::settings::set_current_provider(&AppType::Codex, Some("deepseek"))
        .expect("set local current provider");

    service
        .takeover_live_config_strict(&AppType::Codex)
        .await
        .expect("take over Codex live config");

    let live_auth: Value =
        crate::config::read_json_file(&crate::codex_config::get_codex_auth_path())
            .expect("read live auth");
    assert_eq!(
        live_auth, oauth_auth,
        "takeover must preserve native OAuth independently of the legacy toggle"
    );

    let live_config = std::fs::read_to_string(crate::codex_config::get_codex_config_path())
        .expect("read live config");
    assert!(
        live_config.contains(PROXY_TOKEN_PLACEHOLDER),
        "third-party takeover should carry its local placeholder in config.toml"
    );

    crate::settings::update_settings(crate::settings::AppSettings::default())
        .expect("reset settings");
}

#[tokio::test]
#[serial]
async fn hot_switch_codex_chat_provider_updates_live_provider_display() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");

    let db = Arc::new(Database::memory().expect("init db"));
    let service = ProxyService::new(db.clone());

    let provider_a = Provider::with_id(
        "a".to_string(),
        "Responses".to_string(),
        json!({
            "auth": {
                "OPENAI_API_KEY": "responses-key"
            },
            "config": r#"model_provider = "stable"
model = "responses-model"

[model_providers.stable]
name = "Stable"
base_url = "https://responses.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#
        }),
        None,
    );
    let mut provider_b = Provider::with_id(
        "b".to_string(),
        "DeepSeek".to_string(),
        json!({
            "auth": {
                "OPENAI_API_KEY": "deepseek-key"
            },
            "config": r#"model_provider = "deepseek"
model = "deepseek-v4-flash"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/v1"
wire_api = "responses"
requires_openai_auth = true
"#
        }),
        None,
    );
    provider_b.meta = Some(ProviderMeta {
        api_format: Some("openai_chat".to_string()),
        ..Default::default()
    });

    db.save_provider("codex", &provider_a)
        .expect("save provider a");
    db.save_provider("codex", &provider_b)
        .expect("save provider b");
    db.set_current_provider("codex", "a")
        .expect("set current provider");
    crate::settings::set_current_provider(&AppType::Codex, Some("a"))
        .expect("set local current provider");
    db.save_live_backup(
        "codex",
        &serde_json::to_string(&provider_a.settings_config).expect("serialize provider a"),
    )
    .await
    .expect("seed live backup");
    service
        .write_codex_live(&json!({
            "auth": {
                "OPENAI_API_KEY": PROXY_TOKEN_PLACEHOLDER
            },
            "config": r#"model_provider = "stable"
model = "responses-model"

[model_providers.stable]
name = "Stable"
base_url = "http://127.0.0.1:15721/v1"
wire_api = "responses"
requires_openai_auth = true
"#
        }))
        .expect("seed taken-over Codex live config");

    service
        .hot_switch_provider("codex", "b")
        .await
        .expect("hot switch Codex provider");

    let live = service.read_codex_live().expect("read Codex live config");
    let live_config = live
        .get("config")
        .and_then(|v| v.as_str())
        .expect("live config string");
    let parsed_live: toml::Value = toml::from_str(live_config).expect("parse live config");

    assert_eq!(
        parsed_live.get("model_provider").and_then(|v| v.as_str()),
        Some("deepseek")
    );
    assert_eq!(
        parsed_live
            .get("model_providers")
            .and_then(|v| v.get("deepseek"))
            .and_then(|v| v.get("name"))
            .and_then(|v| v.as_str()),
        Some("DeepSeek")
    );
    assert_eq!(
        parsed_live
            .get("model_providers")
            .and_then(|v| v.get("deepseek"))
            .and_then(|v| v.get("base_url"))
            .and_then(|v| v.as_str()),
        Some("http://127.0.0.1:15721/v1")
    );
    assert_eq!(
        parsed_live.get("model").and_then(|v| v.as_str()),
        Some("deepseek-v4-flash")
    );
    assert_eq!(
        live.get("auth")
            .and_then(|auth| auth.get("OPENAI_API_KEY"))
            .and_then(|v| v.as_str()),
        Some(PROXY_TOKEN_PLACEHOLDER)
    );
}
