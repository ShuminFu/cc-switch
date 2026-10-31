//! Codex backup, restore, cleanup and sync during takeover.

use super::*;

#[test]
fn codex_takeover_backup_preserves_api_key_login_material() {
    let mut target = json!({ "auth": {}, "config": "" });
    let existing = json!({
        "auth": { "OPENAI_API_KEY": "sk-real" },
        "config": "model = \"gpt-5.4\"\n"
    });

    ProxyService::preserve_codex_auth_in_backup(&mut target, &existing, true)
        .expect("preserve API-key auth");
    assert_eq!(target["auth"]["OPENAI_API_KEY"], "sk-real");
}

#[tokio::test]
#[serial]
async fn codex_backup_rebuild_uses_live_auth_when_backup_is_missing() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");
    let db = Arc::new(Database::memory().expect("init db"));
    let service = ProxyService::new(db.clone());
    crate::codex_config::write_codex_live_atomic(
        &json!({ "OPENAI_API_KEY": "sk-real" }),
        Some("model = \"gpt-5.4\"\n"),
    )
    .expect("seed live auth");
    let mut official = Provider::with_id(
        crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string(),
        "OpenAI Official".to_string(),
        json!({ "auth": {}, "config": "" }),
        None,
    );
    official.category = Some("official".to_string());

    service
        .update_live_backup_from_provider_inner("codex", &official)
        .await
        .expect("rebuild backup");
    let backup = db
        .get_live_backup("codex")
        .await
        .expect("read backup")
        .expect("backup exists");
    let value: Value = serde_json::from_str(&backup.original_config).expect("parse backup");
    assert_eq!(value["auth"]["OPENAI_API_KEY"], "sk-real");
}

#[test]
#[serial]
fn codex_empty_restore_snapshot_does_not_delete_existing_auth_json() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");
    let db = Arc::new(Database::memory().expect("init db"));
    let service = ProxyService::new(db);
    let auth = json!({ "OPENAI_API_KEY": "sk-real" });
    crate::codex_config::write_codex_live_atomic(&auth, Some("model = \"gpt-5.4\"\n"))
        .expect("seed auth");

    service
        .write_codex_live_verbatim(&json!({
            "auth": {},
            "config": "model = \"gpt-5.4-mini\"\n"
        }))
        .expect("restore empty snapshot");

    let live_auth: Value =
        crate::config::read_json_file(&crate::codex_config::get_codex_auth_path())
            .expect("read auth");
    assert_eq!(live_auth, auth);
}

#[tokio::test]
#[serial]
async fn codex_sync_live_does_not_store_credentials_in_builtin_official_row() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");
    let db = Arc::new(Database::memory().expect("init db"));
    let service = ProxyService::new(db.clone());
    let mut official = Provider::with_id(
        crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string(),
        "OpenAI Official".to_string(),
        json!({ "auth": {}, "config": "" }),
        None,
    );
    official.category = Some("official".to_string());
    db.save_provider("codex", &official).expect("save official");
    db.set_current_provider("codex", crate::database::CODEX_OFFICIAL_PROVIDER_ID)
        .expect("set current");
    crate::settings::set_current_provider(
        &AppType::Codex,
        Some(crate::database::CODEX_OFFICIAL_PROVIDER_ID),
    )
    .expect("set local current");
    crate::codex_config::write_codex_live_atomic(
        &json!({ "OPENAI_API_KEY": "sk-real" }),
        Some("model = \"gpt-5.4\"\n"),
    )
    .expect("seed live auth");

    service
        .sync_live_to_provider(&AppType::Codex)
        .await
        .expect("sync live");

    let stored = db
        .get_provider_by_id(crate::database::CODEX_OFFICIAL_PROVIDER_ID, "codex")
        .expect("read official")
        .expect("official exists");
    assert_eq!(stored.settings_config["auth"], json!({}));
}

#[tokio::test]
#[serial]
async fn codex_sync_current_to_live_during_takeover_preserves_oauth_auth_json() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");
    crate::settings::update_settings(crate::settings::AppSettings {
        preserve_codex_official_auth_on_switch: true,
        ..Default::default()
    })
    .expect("enable Codex official auth preservation");

    let db = Arc::new(Database::memory().expect("init db"));
    use_ephemeral_proxy_port(&db).await;
    let state = crate::store::AppState::new(db.clone());
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

    state
        .proxy_service
        .set_takeover_for_app("codex", true)
        .await
        .expect("enable Codex takeover");

    crate::services::provider::ProviderService::sync_current_to_live(&state)
        .expect("sync current providers while Codex is taken over");

    let live_auth: Value =
        crate::config::read_json_file(&crate::codex_config::get_codex_auth_path())
            .expect("read live auth");
    assert_eq!(
        live_auth, oauth_auth,
        "post-change provider sync must not rewrite Codex auth.json during takeover"
    );

    let backup = db
        .get_live_backup("codex")
        .await
        .expect("get live backup")
        .expect("backup exists");
    let backup_value: Value = serde_json::from_str(&backup.original_config).expect("parse backup");
    assert_eq!(
        backup_value.get("auth"),
        Some(&oauth_auth),
        "provider-derived takeover backup should preserve official OAuth auth"
    );
    assert!(
        backup_value
            .get("config")
            .and_then(|value| value.as_str())
            .is_some_and(|config| config.contains("deepseek-key")),
        "provider token should be carried by config.toml in the restore backup"
    );

    state
        .proxy_service
        .set_takeover_for_app("codex", false)
        .await
        .expect("disable Codex takeover");
    let restored_auth: Value =
        crate::config::read_json_file(&crate::codex_config::get_codex_auth_path())
            .expect("read restored auth");
    assert_eq!(
        restored_auth, oauth_auth,
        "turning takeover off should restore the preserved official OAuth auth"
    );

    crate::settings::update_settings(crate::settings::AppSettings::default())
        .expect("reset settings");
}

#[tokio::test]
#[serial]
async fn codex_sync_current_to_live_during_takeover_activation_keeps_proxy_live_config() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");
    crate::settings::update_settings(crate::settings::AppSettings {
        preserve_codex_official_auth_on_switch: true,
        ..Default::default()
    })
    .expect("enable Codex official auth preservation");

    let db = Arc::new(Database::memory().expect("init db"));
    let state = crate::store::AppState::new(db.clone());
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

    state
        .proxy_service
        .backup_live_config_strict(&AppType::Codex)
        .await
        .expect("backup Codex live config");
    state
        .proxy_service
        .takeover_live_config_strict(&AppType::Codex)
        .await
        .expect("take over Codex live config");
    assert!(
        !db.get_proxy_config_for_app("codex")
            .await
            .expect("get Codex proxy config")
            .enabled,
        "this reproduces the activation window before set_takeover_for_app marks enabled=true"
    );

    crate::services::provider::ProviderService::sync_current_to_live(&state)
        .expect("sync current providers during takeover activation");

    let live_auth: Value =
        crate::config::read_json_file(&crate::codex_config::get_codex_auth_path())
            .expect("read live auth");
    assert_eq!(
        live_auth, oauth_auth,
        "activation-time provider sync must not rewrite Codex OAuth auth.json"
    );

    let live_config = std::fs::read_to_string(crate::codex_config::get_codex_config_path())
        .expect("read live config");
    assert!(
        live_config.contains(PROXY_TOKEN_PLACEHOLDER),
        "activation-time provider sync must keep the proxy bearer placeholder"
    );
    assert!(
        live_config.contains("http://127.0.0.1"),
        "activation-time provider sync must keep the local proxy base_url"
    );
    assert!(
        state
            .proxy_service
            .detect_takeover_in_live_config_for_app(&AppType::Codex),
        "Codex live config should still be detected as taken over"
    );

    crate::settings::update_settings(crate::settings::AppSettings::default())
        .expect("reset settings");
}

#[tokio::test]
#[serial]
async fn codex_set_takeover_rebuilds_stale_enabled_state_without_overwriting_backup() {
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
    let original_deepseek_config = r#"model_provider = "deepseek"
model = "deepseek-v4-flash"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/v1"
wire_api = "responses"
experimental_bearer_token = "deepseek-key"
"#;
    let stale_live_config = r#"model_provider = "deepseek"
model = "deepseek-v4-flash"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/v1"
wire_api = "responses"
experimental_bearer_token = "PROXY_MANAGED"
"#;
    crate::codex_config::write_codex_live_atomic(&oauth_auth, Some(stale_live_config))
        .expect("seed stale Codex live config");

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
    db.save_live_backup(
        "codex",
        &serde_json::to_string(&json!({
            "auth": oauth_auth,
            "config": original_deepseek_config
        }))
        .expect("serialize original backup"),
    )
    .await
    .expect("seed original live backup");
    let mut proxy_config = db
        .get_proxy_config_for_app("codex")
        .await
        .expect("get Codex proxy config");
    proxy_config.enabled = true;
    db.update_proxy_config_for_app(proxy_config)
        .await
        .expect("mark Codex takeover enabled");

    service
        .set_takeover_for_app("codex", true)
        .await
        .expect("rebuild Codex takeover");

    let live_auth: Value =
        crate::config::read_json_file(&crate::codex_config::get_codex_auth_path())
            .expect("read live auth");
    assert_eq!(
        live_auth, oauth_auth,
        "repairing stale takeover must restore the preserved OAuth auth from backup"
    );

    let live_config = std::fs::read_to_string(crate::codex_config::get_codex_config_path())
        .expect("read live config");
    let expected_base_url = running_codex_base_url(&service).await;
    assert!(
        live_config.contains(&expected_base_url),
        "stale enabled takeover must be rebuilt to the current proxy base_url"
    );
    assert!(
        live_config.contains(PROXY_TOKEN_PLACEHOLDER),
        "rebuilt takeover should keep the proxy bearer placeholder"
    );
    assert!(
        service
            .live_takeover_matches_current_proxy(&AppType::Codex)
            .await
            .expect("detect rebuilt Codex takeover"),
        "rebuilt Codex live config should match the active proxy address"
    );

    let backup = db
        .get_live_backup("codex")
        .await
        .expect("get Codex live backup")
        .expect("backup exists");
    let backup_value: Value = serde_json::from_str(&backup.original_config).expect("parse backup");
    assert_eq!(
        backup_value.get("auth"),
        Some(&oauth_auth),
        "rebuilding stale takeover must not overwrite the original OAuth backup"
    );
    assert!(
        backup_value
            .get("config")
            .and_then(|value| value.as_str())
            .is_some_and(
                |config| config.contains("deepseek-key") && !config.contains("http://127.0.0.1")
            ),
        "backup should remain the restorable DeepSeek config, not the proxy config"
    );

    service
        .set_takeover_for_app("codex", false)
        .await
        .expect("disable Codex takeover");
    crate::settings::update_settings(crate::settings::AppSettings::default())
        .expect("reset settings");
}

#[test]
#[serial]
fn codex_takeover_cleanup_removes_config_placeholder_without_touching_oauth_auth() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");

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
            r#"model_provider = "deepseek"
model = "deepseek-v4-flash"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "http://127.0.0.1:15721/v1"
wire_api = "responses"
experimental_bearer_token = "PROXY_MANAGED"
"#,
        ),
    )
    .expect("seed taken-over Codex live config");

    assert!(
        service.detect_takeover_in_live_config_for_app(&AppType::Codex),
        "config.toml placeholder should be detected before cleanup"
    );

    service
        .cleanup_codex_takeover_placeholders_in_live()
        .expect("cleanup Codex takeover placeholders");

    let live_auth: Value =
        crate::config::read_json_file(&crate::codex_config::get_codex_auth_path())
            .expect("read live auth");
    assert_eq!(
        live_auth, oauth_auth,
        "cleanup should preserve ChatGPT OAuth auth"
    );

    let live_config = std::fs::read_to_string(crate::codex_config::get_codex_config_path())
        .expect("read live config");
    assert!(
        !live_config.contains(PROXY_TOKEN_PLACEHOLDER),
        "cleanup should remove config.toml proxy bearer placeholder"
    );
    assert!(
        !live_config.contains("http://127.0.0.1:15721"),
        "cleanup should remove local proxy base_url"
    );
}

#[test]
fn apply_codex_proxy_toml_config_restores_upstream_model_for_responses_provider() {
    let input = r#"
model_provider = "responses"
model = "gpt-5.4"

[model_providers.responses]
name = "Responses"
base_url = "http://127.0.0.1:5000/v1"
wire_api = "responses"
"#;
    let mut provider = Provider::with_id(
        "responses".to_string(),
        "Responses".to_string(),
        json!({
            "config": r#"model_provider = "responses"
model = "upstream-responses-model"

[model_providers.responses]
name = "Responses"
base_url = "https://responses.example/v1"
wire_api = "responses"
"#
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
    .expect("restore responses model");
    let parsed: toml::Value = toml::from_str(&output).expect("updated config should be valid TOML");

    assert_eq!(
        parsed.get("model").and_then(|v| v.as_str()),
        Some("upstream-responses-model")
    );
}

#[tokio::test]
#[serial]
async fn update_live_backup_from_provider_applies_codex_common_config() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");

    let db = Arc::new(Database::memory().expect("init db"));
    db.set_config_snippet(
        "codex",
        Some("disable_response_storage = true\n".to_string()),
    )
    .expect("set common config snippet");

    let service = ProxyService::new(db.clone());

    let mut provider = Provider::with_id(
        "p1".to_string(),
        "P1".to_string(),
        json!({
            "auth": {
                "OPENAI_API_KEY": "token"
            },
            "config": r#"model_provider = "any"
model = "gpt-5"

[model_providers.any]
base_url = "https://codex.example/v1"
"#
        }),
        None,
    );
    provider.meta = Some(ProviderMeta {
        common_config_enabled: Some(true),
        ..Default::default()
    });

    service
        .update_live_backup_from_provider("codex", &provider)
        .await
        .expect("update live backup");

    let backup = db
        .get_live_backup("codex")
        .await
        .expect("get live backup")
        .expect("backup exists");
    let stored: Value = serde_json::from_str(&backup.original_config).expect("parse backup json");
    let config = stored
        .get("config")
        .and_then(|v| v.as_str())
        .expect("config string");

    assert!(
        config.contains("disable_response_storage = true"),
        "common config should be applied into Codex restore backup"
    );
}

#[tokio::test]
#[serial]
async fn update_live_backup_from_provider_preserves_codex_mcp_servers() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");

    let db = Arc::new(Database::memory().expect("init db"));
    let service = ProxyService::new(db.clone());

    db.save_live_backup(
        "codex",
        &serde_json::to_string(&json!({
            "auth": {
                "OPENAI_API_KEY": "old-token"
            },
            "config": r#"model_provider = "any"
model = "gpt-4"

[model_providers.any]
base_url = "https://old.example/v1"

[mcp_servers.echo]
command = "npx"
args = ["echo-server"]
"#
        }))
        .expect("serialize seed backup"),
    )
    .await
    .expect("seed live backup");

    let provider = Provider::with_id(
        "p2".to_string(),
        "P2".to_string(),
        json!({
            "auth": {
                "OPENAI_API_KEY": "new-token"
            },
            "config": r#"model_provider = "any"
model = "gpt-5"

[model_providers.any]
base_url = "https://new.example/v1"
"#
        }),
        None,
    );

    service
        .update_live_backup_from_provider("codex", &provider)
        .await
        .expect("update live backup");

    let backup = db
        .get_live_backup("codex")
        .await
        .expect("get live backup")
        .expect("backup exists");
    let stored: Value = serde_json::from_str(&backup.original_config).expect("parse backup json");
    let config = stored
        .get("config")
        .and_then(|v| v.as_str())
        .expect("config string");

    assert!(
        config.contains("[mcp_servers.echo]"),
        "existing Codex MCP section should survive proxy hot-switch backup update"
    );
    assert!(
        config.contains("https://new.example/v1"),
        "provider-specific base_url should still update to the new provider"
    );
}

#[tokio::test]
#[serial]
async fn hot_switch_codex_provider_preserves_provider_model_provider_in_backup_and_restore() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");

    let db = Arc::new(Database::memory().expect("init db"));
    let service = ProxyService::new(db.clone());

    let provider_a = Provider::with_id(
        "a".to_string(),
        "RightCode".to_string(),
        json!({
            "auth": {
                "OPENAI_API_KEY": "rightcode-key"
            },
            "config": r#"model_provider = "rightcode"
model = "gpt-5.4"

[model_providers.rightcode]
name = "RightCode"
base_url = "https://rightcode.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#
        }),
        None,
    );
    let provider_b = Provider::with_id(
        "b".to_string(),
        "AiHubMix".to_string(),
        json!({
            "auth": {
                "OPENAI_API_KEY": "aihubmix-key"
            },
            "config": r#"model_provider = "aihubmix"
model = "gpt-5.4"

[model_providers.aihubmix]
name = "AiHubMix"
base_url = "https://aihubmix.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#
        }),
        None,
    );

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
            "config": r#"model_provider = "rightcode"
model = "gpt-5.4"

[model_providers.rightcode]
name = "RightCode"
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

    let backup = db
        .get_live_backup("codex")
        .await
        .expect("get live backup")
        .expect("backup exists");
    let stored: Value = serde_json::from_str(&backup.original_config).expect("parse backup json");
    let backup_config = stored
        .get("config")
        .and_then(|v| v.as_str())
        .expect("backup config string");
    let parsed_backup: toml::Value = toml::from_str(backup_config).expect("parse backup config");
    assert_eq!(
        parsed_backup.get("model_provider").and_then(|v| v.as_str()),
        Some("aihubmix"),
        "provider-derived restore backup should preserve the provider's model_provider"
    );
    let backup_model_providers = parsed_backup
        .get("model_providers")
        .and_then(|v| v.as_table())
        .expect("backup model_providers");
    assert!(backup_model_providers.get("custom").is_none());
    assert_eq!(
        backup_model_providers
            .get("aihubmix")
            .and_then(|v| v.get("base_url"))
            .and_then(|v| v.as_str()),
        Some("https://aihubmix.example/v1"),
        "provider id should point at the hot-switched provider endpoint"
    );

    let live = service.read_codex_live().expect("read Codex live config");
    let live_config = live
        .get("config")
        .and_then(|v| v.as_str())
        .expect("live config string");
    let parsed_live: toml::Value = toml::from_str(live_config).expect("parse live config");
    assert_eq!(
        parsed_live.get("model_provider").and_then(|v| v.as_str()),
        Some("aihubmix"),
        "hot-switched Codex live config should expose the selected provider"
    );
    assert_eq!(
        parsed_live
            .get("model_providers")
            .and_then(|v| v.get("aihubmix"))
            .and_then(|v| v.get("name"))
            .and_then(|v| v.as_str()),
        Some("AiHubMix"),
        "Codex app provider label should follow the selected provider"
    );
    assert_eq!(
        parsed_live
            .get("model_providers")
            .and_then(|v| v.get("aihubmix"))
            .and_then(|v| v.get("base_url"))
            .and_then(|v| v.as_str()),
        Some("http://127.0.0.1:15721/v1"),
        "taken-over live config should stay pointed at the local proxy"
    );

    service
        .restore_live_config_for_app_with_fallback(&AppType::Codex)
        .await
        .expect("restore Codex live config");

    let live = service.read_codex_live().expect("read Codex live config");
    let live_config = live
        .get("config")
        .and_then(|v| v.as_str())
        .expect("live config string");
    let parsed_live: toml::Value = toml::from_str(live_config).expect("parse live config");
    assert_eq!(
        parsed_live.get("model_provider").and_then(|v| v.as_str()),
        Some("aihubmix"),
        "restored Codex live config should preserve the provider's model_provider"
    );
    assert_eq!(
        live.get("auth")
            .and_then(|auth| auth.get("OPENAI_API_KEY"))
            .and_then(|v| v.as_str()),
        Some("aihubmix-key"),
        "restore should still use the hot-switched provider auth"
    );
}

#[tokio::test]
#[serial]
async fn update_live_backup_from_provider_keeps_new_codex_mcp_entries_on_conflict() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");

    let db = Arc::new(Database::memory().expect("init db"));
    let service = ProxyService::new(db.clone());

    db.save_live_backup(
        "codex",
        &serde_json::to_string(&json!({
            "auth": {
                "OPENAI_API_KEY": "old-token"
            },
            "config": r#"[mcp_servers.shared]
command = "old-command"

[mcp_servers.legacy]
command = "legacy-command"
"#
        }))
        .expect("serialize seed backup"),
    )
    .await
    .expect("seed live backup");

    let provider = Provider::with_id(
        "p2".to_string(),
        "P2".to_string(),
        json!({
            "auth": {
                "OPENAI_API_KEY": "new-token"
            },
            "config": r#"[mcp_servers.shared]
command = "new-command"

[mcp_servers.latest]
command = "latest-command"
"#
        }),
        None,
    );

    service
        .update_live_backup_from_provider("codex", &provider)
        .await
        .expect("update live backup");

    let backup = db
        .get_live_backup("codex")
        .await
        .expect("get live backup")
        .expect("backup exists");
    let stored: Value = serde_json::from_str(&backup.original_config).expect("parse backup json");
    let config = stored
        .get("config")
        .and_then(|v| v.as_str())
        .expect("config string");
    let parsed: toml::Value = toml::from_str(config).expect("parse merged codex config");

    let mcp_servers = parsed
        .get("mcp_servers")
        .expect("mcp_servers should be present");
    assert_eq!(
        mcp_servers
            .get("shared")
            .and_then(|v| v.get("command"))
            .and_then(|v| v.as_str()),
        Some("new-command"),
        "new provider/common-config MCP definition should win on conflict"
    );
    assert_eq!(
        mcp_servers
            .get("legacy")
            .and_then(|v| v.get("command"))
            .and_then(|v| v.as_str()),
        Some("legacy-command"),
        "backup-only MCP entries should still be preserved"
    );
    assert_eq!(
        mcp_servers
            .get("latest")
            .and_then(|v| v.get("command"))
            .and_then(|v| v.as_str()),
        Some("latest-command"),
        "new MCP entries should remain in the restore backup"
    );
}

#[tokio::test]
#[serial]
async fn provider_switch_with_restored_codex_backup_refreshes_catalog_and_common_config() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");
    seed_codex_model_template();

    let db = Arc::new(Database::memory().expect("init db"));
    let state = crate::store::AppState::new(db.clone());

    db.set_config_snippet(
        "codex",
        Some(
            r#"[mcp_servers.shared]
command = "shared-command"
"#
            .to_string(),
        ),
    )
    .expect("set common config snippet");

    let proxy_config = ProxyConfig {
        listen_port: 0,
        ..Default::default()
    };
    db.update_proxy_config(proxy_config)
        .await
        .expect("set test proxy config");
    state
        .proxy_service
        .start()
        .await
        .expect("start proxy server");

    let config_a = r#"model_provider = "provider-a"
model = "model-a"

[model_providers.provider-a]
name = "ProviderA"
base_url = "https://provider-a.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#;
    let config_b = r#"model_provider = "provider-b"
model = "model-b"

[model_providers.provider-b]
name = "ProviderB"
base_url = "https://provider-b.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#;

    let provider_a = Provider::with_id(
        "a".to_string(),
        "ProviderA".to_string(),
        serde_json::json!({
            "auth": { "OPENAI_API_KEY": "key-a" },
            "config": config_a,
            "modelCatalog": { "models": [{ "model": "model-a" }] }
        }),
        None,
    );
    let mut provider_b = Provider::with_id(
        "b".to_string(),
        "ProviderB".to_string(),
        serde_json::json!({
            "auth": { "OPENAI_API_KEY": "key-b" },
            "config": config_b,
            "modelCatalog": { "models": [{ "model": "model-b" }] }
        }),
        None,
    );
    provider_b.meta = Some(ProviderMeta {
        common_config_enabled: Some(true),
        ..Default::default()
    });

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
        .expect("seed live codex config");
    assert!(
        !state
            .proxy_service
            .detect_takeover_in_live_config_for_app(&AppType::Codex),
        "seeded live config should not be proxy-taken-over"
    );

    db.save_live_backup(
        "codex",
        &serde_json::to_string(&provider_a.settings_config).expect("serialize backup"),
    )
    .await
    .expect("seed restored backup");

    crate::services::provider::ProviderService::switch(&state, AppType::Codex, "b")
        .expect("provider switch to provider b");
    state.proxy_service.stop().await.expect("stop proxy server");

    let catalog_path = crate::codex_config::get_codex_model_catalog_path();
    assert!(
        catalog_path.exists(),
        "cc-switch-model-catalog.json must be created on provider switch"
    );
    let catalog_text = std::fs::read_to_string(&catalog_path).expect("read catalog json");
    let catalog: serde_json::Value =
        serde_json::from_str(&catalog_text).expect("parse catalog json");
    let slugs: Vec<&str> = catalog
        .get("models")
        .and_then(|m| m.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|e| e.get("slug").and_then(|s| s.as_str()))
                .collect()
        })
        .unwrap_or_default();
    assert!(
        slugs.contains(&"model-b"),
        "catalog must contain provider B's model after switch; got: {slugs:?}"
    );
    assert!(
        !slugs.contains(&"model-a"),
        "catalog must not contain stale provider A model after switch; got: {slugs:?}"
    );

    let config_path = crate::codex_config::get_codex_config_path();
    let config_text = std::fs::read_to_string(&config_path).expect("read config.toml");
    assert!(
        config_text.contains("model_catalog_json"),
        "config.toml must reference model_catalog_json after switch"
    );
    assert!(
        config_text.contains("[mcp_servers.shared]"),
        "config.toml must keep common config after switch"
    );
    assert!(
        config_text.contains(r#"command = "shared-command""#),
        "config.toml must include common config content after switch"
    );
}

#[tokio::test]
#[serial]
async fn provider_switch_with_restored_codex_backup_propagates_catalog_write_errors() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");
    seed_codex_model_template();

    let db = Arc::new(Database::memory().expect("init db"));
    let state = crate::store::AppState::new(db.clone());

    let proxy_config = ProxyConfig {
        listen_port: 0,
        ..Default::default()
    };
    db.update_proxy_config(proxy_config)
        .await
        .expect("set test proxy config");
    state
        .proxy_service
        .start()
        .await
        .expect("start proxy server");

    let config_a = r#"model_provider = "provider-a"
model = "model-a"

[model_providers.provider-a]
name = "ProviderA"
base_url = "https://provider-a.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#;
    let config_b = r#"model_provider = "provider-b"
model = "model-b"

[model_providers.provider-b]
name = "ProviderB"
base_url = "https://provider-b.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#;

    let provider_a = Provider::with_id(
        "a".to_string(),
        "ProviderA".to_string(),
        serde_json::json!({
            "auth": { "OPENAI_API_KEY": "key-a" },
            "config": config_a,
            "modelCatalog": { "models": [{ "model": "model-a" }] }
        }),
        None,
    );
    let provider_b = Provider::with_id(
        "b".to_string(),
        "ProviderB".to_string(),
        serde_json::json!({
            "auth": { "OPENAI_API_KEY": "key-b" },
            "config": config_b,
            "modelCatalog": { "models": [{ "model": "model-b" }] }
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
        .expect("seed live codex config");
    assert!(
        !state
            .proxy_service
            .detect_takeover_in_live_config_for_app(&AppType::Codex),
        "seeded live config should not be proxy-taken-over"
    );

    db.save_live_backup(
        "codex",
        &serde_json::to_string(&provider_a.settings_config).expect("serialize backup"),
    )
    .await
    .expect("seed restored backup");

    let catalog_path = crate::codex_config::get_codex_model_catalog_path();
    if catalog_path.exists() {
        std::fs::remove_file(&catalog_path).expect("remove catalog file");
    }
    std::fs::create_dir_all(&catalog_path).expect("turn catalog path into directory");

    let err = crate::services::provider::ProviderService::switch(&state, AppType::Codex, "b")
        .expect_err("provider switch should fail when catalog cannot be written");
    state.proxy_service.stop().await.expect("stop proxy server");

    let message = err.to_string();
    assert!(
        message.contains("写入 Codex 配置失败") || message.contains("原子替换失败"),
        "switch should surface catalog write failure, got: {message}"
    );
}

/// Regression: turning proxy takeover off restores Live from the backup. The
/// backup snapshot is `read_codex_live_settings()` output (`{auth, config}`,
/// never an inline `modelCatalog`). The restore must NOT route the config
/// through catalog projection, which would see no specs and strip the
/// `model_catalog_json` pointer — silently dropping the user's Codex model
/// mapping from Live even though the DB SSOT still holds it.
#[tokio::test]
#[serial]
async fn codex_restore_from_backup_preserves_model_catalog_pointer() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");

    let db = Arc::new(Database::memory().expect("init db"));
    let service = ProxyService::new(db.clone());

    // Pre-takeover Live state: config.toml points at the cc-switch generated
    // catalog file, and that file exists on disk (takeover never touches it).
    let catalog_path = crate::codex_config::get_codex_model_catalog_path();
    if let Some(parent) = catalog_path.parent() {
        std::fs::create_dir_all(parent).expect("create codex dir");
    }
    std::fs::write(
        &catalog_path,
        r#"{"models":[{"slug":"deepseek-v4-flash"}]}"#,
    )
    .expect("seed generated catalog file");

    let pointer = catalog_path.to_string_lossy().replace('\\', "/");
    let backup_config = format!(
        "model_provider = \"custom\"\n\
         model = \"deepseek-v4-flash\"\n\
         model_catalog_json = \"{pointer}\"\n\n\
         [model_providers.custom]\n\
         name = \"DeepSeek\"\n\
         base_url = \"https://api.deepseek.example/v1\"\n\
         wire_api = \"responses\"\n"
    );
    let backup_json = serde_json::to_string(&json!({
        "auth": { "OPENAI_API_KEY": "deepseek-key" },
        "config": backup_config,
    }))
    .expect("serialize backup");
    db.save_live_backup("codex", &backup_json)
        .await
        .expect("seed live backup");

    // Turning takeover off restores Live from this backup.
    service
        .restore_live_config_for_app_with_fallback(&AppType::Codex)
        .await
        .expect("restore codex live from backup");

    let restored = std::fs::read_to_string(crate::codex_config::get_codex_config_path())
        .expect("read restored config.toml");
    assert!(
        restored.contains("model_catalog_json"),
        "restore must preserve the model_catalog_json pointer, got:\n{restored}"
    );
    assert!(
        restored.contains(pointer.as_str()),
        "restored pointer must still reference the cc-switch generated catalog file"
    );
}

/// Regression: a hot-switch during takeover rebuilds the backup from the DB
/// provider (`update_live_backup_from_provider`), so the backup carries an
/// inline `modelCatalog` (DB SSOT) but a `config.toml` text WITHOUT a
/// `model_catalog_json` pointer. Restoring that backup must project the
/// inline catalog — (re)generating both the catalog file and the pointer —
/// or the Codex model mapping vanishes from Live after takeover-off.
#[tokio::test]
#[serial]
async fn codex_restore_from_backup_projects_inline_model_catalog() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");

    let db = Arc::new(Database::memory().expect("init db"));
    let service = ProxyService::new(db.clone());

    // Catalog projection needs a model template; seed `models_cache.json`
    // with the template slug so we don't depend on the `codex` CLI.
    let codex_dir = crate::codex_config::get_codex_config_dir();
    std::fs::create_dir_all(&codex_dir).expect("create codex dir");
    std::fs::write(
        codex_dir.join("models_cache.json"),
        r#"{"models":[{"slug":"gpt-5.5"}]}"#,
    )
    .expect("seed models_cache template");

    // Provider-rebuilt backup shape: inline modelCatalog, pointer-less config.
    let backup_json = serde_json::to_string(&json!({
        "auth": { "OPENAI_API_KEY": "deepseek-key" },
        "config": "model_provider = \"custom\"\nmodel = \"deepseek-v4-flash\"\n\n[model_providers.custom]\nname = \"DeepSeek\"\nbase_url = \"https://api.deepseek.example/v1\"\nwire_api = \"responses\"\n",
        "modelCatalog": {
            "models": [
                { "model": "deepseek-v4-flash", "displayName": "DeepSeek V4 Flash", "contextWindow": 1_000_000 }
            ]
        }
    }))
    .expect("serialize backup");
    db.save_live_backup("codex", &backup_json)
        .await
        .expect("seed live backup");

    service
        .restore_live_config_for_app_with_fallback(&AppType::Codex)
        .await
        .expect("restore codex live from backup");

    let restored = std::fs::read_to_string(crate::codex_config::get_codex_config_path())
        .expect("read restored config.toml");
    let catalog_path = crate::codex_config::get_codex_model_catalog_path();
    assert!(
        restored.contains("model_catalog_json"),
        "restore must (re)generate the model_catalog_json pointer from inline catalog, got:\n{restored}"
    );
    assert!(
        catalog_path.exists(),
        "restore must generate the cc-switch catalog file on disk"
    );
    let catalog: Value = serde_json::from_str(
        &std::fs::read_to_string(&catalog_path).expect("read generated catalog"),
    )
    .expect("parse generated catalog");
    let slugs: Vec<&str> = catalog
        .get("models")
        .and_then(|m| m.as_array())
        .expect("catalog models")
        .iter()
        .filter_map(|m| m.get("slug").and_then(|s| s.as_str()))
        .collect();
    assert!(
        slugs.contains(&"deepseek-v4-flash"),
        "generated catalog must contain the inline model, got slugs: {slugs:?}"
    );
}

/// Regression: a provider-rebuilt backup can pair an inline `modelCatalog`
/// with EMPTY `auth.json` (`{}`) — the bearer-token / Mobile-compat shape
/// where the API key lives in the config's `experimental_bearer_token`. The
/// empty-auth restore branch deletes `auth.json` and writes config raw; it
/// must still project the inline catalog (decision is orthogonal to auth), or
/// the model mapping vanishes on takeover-off for this provider shape.
#[tokio::test]
#[serial]
async fn codex_restore_empty_auth_backup_still_projects_inline_catalog() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");

    let db = Arc::new(Database::memory().expect("init db"));
    let service = ProxyService::new(db.clone());

    let codex_dir = crate::codex_config::get_codex_config_dir();
    std::fs::create_dir_all(&codex_dir).expect("create codex dir");
    std::fs::write(
        codex_dir.join("models_cache.json"),
        r#"{"models":[{"slug":"gpt-5.5"}]}"#,
    )
    .expect("seed models_cache template");

    // Empty auth.json + key carried in config.toml's experimental_bearer_token,
    // plus the inline modelCatalog (DB SSOT).
    let backup_json = serde_json::to_string(&json!({
        "auth": {},
        "config": "model_provider = \"custom\"\nmodel = \"deepseek-v4-flash\"\n\n[model_providers.custom]\nname = \"DeepSeek\"\nbase_url = \"https://api.deepseek.example/v1\"\nwire_api = \"responses\"\nexperimental_bearer_token = \"sk-deepseek\"\n",
        "modelCatalog": {
            "models": [ { "model": "deepseek-v4-flash", "displayName": "DeepSeek V4 Flash" } ]
        }
    }))
    .expect("serialize backup");
    db.save_live_backup("codex", &backup_json)
        .await
        .expect("seed live backup");

    service
        .restore_live_config_for_app_with_fallback(&AppType::Codex)
        .await
        .expect("restore codex live from backup");

    let restored = std::fs::read_to_string(crate::codex_config::get_codex_config_path())
        .expect("read restored config.toml");
    assert!(
        restored.contains("model_catalog_json"),
        "empty-auth restore must still project the inline catalog pointer, got:\n{restored}"
    );
    assert!(
        crate::codex_config::get_codex_model_catalog_path().exists(),
        "empty-auth restore must generate the cc-switch catalog file"
    );
    assert!(
        !crate::codex_config::get_codex_auth_path().exists(),
        "empty-auth restore must delete auth.json rather than write an empty one"
    );
}
