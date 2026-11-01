//! Gemini / Grok Build takeover and hot switch.

use super::*;

#[tokio::test]
#[serial]
async fn hot_switch_grokbuild_updates_backup_and_current_provider() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");

    let db = Arc::new(Database::memory().expect("init db"));
    let service = ProxyService::new(db.clone());
    let provider_a = Provider::with_id(
        "grok-a".to_string(),
        "Grok A".to_string(),
        grok_provider_config("https://a.example.com/v1", "a-key"),
        None,
    );
    let provider_b = Provider::with_id(
        "grok-b".to_string(),
        "Grok B".to_string(),
        grok_provider_config("https://b.example.com/v1", "b-key"),
        None,
    );
    db.save_provider("grokbuild", &provider_a)
        .expect("save provider a");
    db.save_provider("grokbuild", &provider_b)
        .expect("save provider b");
    db.set_current_provider("grokbuild", "grok-a")
        .expect("set db current");
    crate::settings::set_current_provider(&AppType::GrokBuild, Some("grok-a"))
        .expect("set local current");
    let mut original_settings = provider_a.settings_config.clone();
    original_settings["config"] = json!(format!(
        "{}\n[mcp_servers.demo]\ncommand = \"demo\"\n",
        original_settings["config"]
            .as_str()
            .expect("provider config")
    ));
    db.save_live_backup(
        "grokbuild",
        &serde_json::to_string(&original_settings).expect("serialize backup"),
    )
    .await
    .expect("seed backup");

    service
        .hot_switch_provider("grokbuild", "grok-b")
        .await
        .expect("hot switch Grok Build");

    assert_eq!(
        crate::settings::get_effective_current_provider(&db, &AppType::GrokBuild)
            .expect("read current")
            .as_deref(),
        Some("grok-b")
    );
    let backup = db
        .get_live_backup("grokbuild")
        .await
        .expect("read backup")
        .expect("backup exists");
    let backup: Value = serde_json::from_str(&backup.original_config).expect("parse backup");
    assert!(backup["config"]
        .as_str()
        .is_some_and(|config| config.contains("https://b.example.com/v1")));
    assert!(backup["config"]
        .as_str()
        .is_some_and(|config| config.contains("[mcp_servers.demo]")));
}

#[tokio::test]
#[serial]
async fn failed_hot_switch_grokbuild_keeps_previous_current_and_backup() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");

    let db = Arc::new(Database::memory().expect("init db"));
    let service = ProxyService::new(db.clone());
    let provider_a = Provider::with_id(
        "grok-a".to_string(),
        "Grok A".to_string(),
        grok_provider_config("https://a.example.com/v1", "a-key"),
        None,
    );
    let provider_b = Provider::with_id(
        "grok-b".to_string(),
        "Broken Grok".to_string(),
        json!({ "config": "not valid toml = [" }),
        None,
    );
    db.save_provider("grokbuild", &provider_a)
        .expect("save provider a");
    db.save_provider("grokbuild", &provider_b)
        .expect("save provider b");
    db.set_current_provider("grokbuild", "grok-a")
        .expect("set db current");
    crate::settings::set_current_provider(&AppType::GrokBuild, Some("grok-a"))
        .expect("set local current");

    let original_backup =
        serde_json::to_string(&provider_a.settings_config).expect("serialize backup");
    db.save_live_backup("grokbuild", &original_backup)
        .await
        .expect("seed backup");
    let takeover = crate::grok_config::apply_proxy_takeover(
        provider_a.settings_config["config"]
            .as_str()
            .expect("provider config"),
        "http://127.0.0.1:15721/grokbuild/v1",
        PROXY_TOKEN_PLACEHOLDER,
    )
    .expect("build takeover config");
    service
        .write_grok_live(&json!({ "config": takeover }))
        .expect("seed taken-over live");

    service
        .hot_switch_provider("grokbuild", "grok-b")
        .await
        .expect_err("invalid Grok config must fail");

    assert_eq!(
        crate::settings::get_effective_current_provider(&db, &AppType::GrokBuild)
            .expect("read current")
            .as_deref(),
        Some("grok-a")
    );
    assert_eq!(
        crate::settings::get_current_provider(&AppType::GrokBuild).as_deref(),
        Some("grok-a")
    );
    let backup = db
        .get_live_backup("grokbuild")
        .await
        .expect("read backup")
        .expect("backup exists");
    assert_eq!(backup.original_config, original_backup);
}
