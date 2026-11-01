//! Proxy lifecycle, config updates, backups and recovery.

use super::*;

#[tokio::test]
#[serial]
async fn takeover_with_ephemeral_port_writes_actual_live_url() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");

    let db = Arc::new(Database::memory().expect("init db"));
    use_ephemeral_proxy_port(&db).await;
    let service = ProxyService::new(db.clone());

    let provider = Provider::with_id(
        "p1".to_string(),
        "P1".to_string(),
        json!({
            "env": {
                "ANTHROPIC_API_KEY": "provider-key",
                "ANTHROPIC_BASE_URL": "https://api.anthropic.com"
            }
        }),
        None,
    );
    db.save_provider("claude", &provider)
        .expect("save provider");
    db.set_current_provider("claude", "p1")
        .expect("set db current provider");
    crate::settings::set_current_provider(&AppType::Claude, Some("p1"))
        .expect("set local current provider");
    service
        .write_claude_live(&json!({
            "env": {
                "ANTHROPIC_API_KEY": "live-key",
                "ANTHROPIC_BASE_URL": "https://api.anthropic.com"
            }
        }))
        .expect("seed claude live config");

    service
        .set_takeover_for_app("claude", true)
        .await
        .expect("take over claude");
    let info = service.get_status().await.expect("proxy status");
    assert_ne!(info.port, 0, "OS should assign a concrete port");
    assert!(
        service
            .get_takeover_status()
            .await
            .expect("takeover status")
            .claude,
        "per-app takeover status reports the app (SVC-51)"
    );

    let stored_config = db.get_proxy_config().await.expect("read proxy config");
    assert_eq!(
        stored_config.listen_port, info.port,
        "resolved dynamic port should be persisted for DB-only proxy URL paths"
    );

    let live = service.read_claude_live().expect("read taken-over live");
    let base_url = live
        .get("env")
        .and_then(|env| env.get("ANTHROPIC_BASE_URL"))
        .and_then(|value| value.as_str())
        .expect("taken-over base url");
    assert_eq!(base_url, format!("http://127.0.0.1:{}", info.port));
    assert!(
        !base_url.contains(":0"),
        "takeover must never write an unresolved :0 port"
    );

    service
        .stop_with_restore()
        .await
        .expect("stop proxy and restore live config");
}

/// SVC-06: when the proxy restarts on a new port, a failed live rewrite must be
/// reported instead of swallowed, and must not stop the other apps from being
/// rewritten.
#[tokio::test]
#[serial]
async fn update_config_port_change_reports_failed_live_rewrite() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");

    let db = Arc::new(Database::memory().expect("init db"));
    use_ephemeral_proxy_port(&db).await;
    let service = ProxyService::new(db.clone());

    let provider = Provider::with_id(
        "p1".to_string(),
        "P1".to_string(),
        json!({
            "env": {
                "ANTHROPIC_API_KEY": "provider-key",
                "ANTHROPIC_BASE_URL": "https://api.example.com"
            }
        }),
        None,
    );
    db.save_provider("claude", &provider)
        .expect("save provider");
    db.set_current_provider("claude", "p1")
        .expect("set db current provider");
    crate::settings::set_current_provider(&AppType::Claude, Some("p1"))
        .expect("set local current provider");
    service
        .write_claude_live(&provider.settings_config)
        .expect("seed claude live config");

    let gemini_provider = Provider::with_id(
        "g1".to_string(),
        "G1".to_string(),
        json!({
            "env": {
                "GEMINI_API_KEY": "gemini-key",
                "GOOGLE_GEMINI_BASE_URL": "https://gemini.example.com"
            }
        }),
        None,
    );
    db.save_provider("gemini", &gemini_provider)
        .expect("save gemini provider");
    db.set_current_provider("gemini", "g1")
        .expect("set db current gemini provider");
    crate::settings::set_current_provider(&AppType::Gemini, Some("g1"))
        .expect("set local current gemini provider");
    service
        .write_gemini_live(&gemini_provider.settings_config)
        .expect("seed gemini live config");

    service
        .set_takeover_for_app("claude", true)
        .await
        .expect("take over claude");
    service
        .set_takeover_for_app("gemini", true)
        .await
        .expect("take over gemini");
    let old_port = service.get_status().await.expect("status").port;

    // Restart on a fresh ephemeral port while the Claude rewrite fails.
    let mut new_config = db.get_proxy_config().await.expect("read proxy config");
    new_config.listen_port = 0;
    FAIL_CLAUDE_LIVE_WRITE.with(|fail| fail.set(true));
    let result = service.update_config(&new_config).await;
    FAIL_CLAUDE_LIVE_WRITE.with(|fail| fail.set(false));

    let err = result.expect_err("failed Claude live rewrite must be reported");
    assert!(
        err.message().contains("claude"),
        "error should name the app: {err}"
    );
    // SVC-22: the failure keeps a typed kind next to the unchanged text.
    assert_eq!(err.kind(), TakeoverErrorKind::LiveWrite);

    let new_port = service.get_status().await.expect("status").port;
    assert_ne!(new_port, old_port, "proxy should have restarted");

    let gemini_live = service.read_gemini_live().expect("read gemini live");
    assert_eq!(
        gemini_live
            .get("env")
            .and_then(|env| env.get("GOOGLE_GEMINI_BASE_URL"))
            .and_then(|value| value.as_str()),
        Some(format!("http://127.0.0.1:{new_port}").as_str()),
        "Gemini must still be rewritten to the new port"
    );

    service
        .stop_with_restore()
        .await
        .expect("stop proxy and restore live config");
}

#[tokio::test]
#[serial]
async fn update_config_to_busy_port_keeps_running_server_and_config() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");

    let db = Arc::new(Database::memory().expect("init db"));
    use_ephemeral_proxy_port(&db).await;
    let service = ProxyService::new(db.clone());

    let provider = Provider::with_id(
        "p1".to_string(),
        "P1".to_string(),
        json!({
            "env": {
                "ANTHROPIC_API_KEY": "provider-key",
                "ANTHROPIC_BASE_URL": "https://api.example.com"
            }
        }),
        None,
    );
    db.save_provider("claude", &provider)
        .expect("save provider");
    db.set_current_provider("claude", "p1")
        .expect("set current");
    crate::settings::set_current_provider(&AppType::Claude, Some("p1")).expect("set local current");
    service
        .write_claude_live(&provider.settings_config)
        .expect("seed claude live");
    service
        .set_takeover_for_app("claude", true)
        .await
        .expect("take over claude");

    let old_port = service.get_status().await.expect("status").port;
    let previous = db.get_proxy_config().await.expect("read proxy config");

    let blocker = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind blocker");
    let busy_port = blocker.local_addr().unwrap().port();
    let mut new_config = previous.clone();
    new_config.listen_address = "127.0.0.1".to_string();
    new_config.listen_port = busy_port;

    let err = service
        .update_config(&new_config)
        .await
        .expect_err("a busy port must be rejected");
    assert!(
        err.message().contains(&busy_port.to_string()),
        "names the port: {err}"
    );
    assert_eq!(err.code(), "takeover.port_unavailable");

    assert!(service.is_running().await, "old server keeps running");
    assert_eq!(service.get_status().await.expect("status").port, old_port);
    let after = db.get_proxy_config().await.expect("read proxy config");
    assert_eq!(after.listen_port, previous.listen_port);
    assert_eq!(after.listen_address, previous.listen_address);

    let live = service.read_claude_live().expect("read claude live");
    let base_url = live
        .pointer("/env/ANTHROPIC_BASE_URL")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    assert!(
        base_url.contains(&format!(":{old_port}")),
        "live still points at the running proxy: {base_url}"
    );

    drop(blocker);
    service
        .stop_with_restore()
        .await
        .expect("stop proxy and restore live config");
}

#[test]
fn update_toml_base_url_updates_active_model_provider_base_url() {
    let input = r#"
model_provider = "any"
model = "gpt-5.1-codex"
disable_response_storage = true

[model_providers.any]
name = "any"
base_url = "https://anyrouter.top/v1"
wire_api = "responses"
requires_openai_auth = true
"#;

    let new_url = "http://127.0.0.1:5000/v1";
    let output = crate::codex_config::update_codex_toml_field(input, "base_url", new_url)
        .expect("update base_url");

    let parsed: toml::Value = toml::from_str(&output).expect("updated config should be valid TOML");

    let base_url = parsed
        .get("model_providers")
        .and_then(|v| v.get("any"))
        .and_then(|v| v.get("base_url"))
        .and_then(|v| v.as_str())
        .expect("model_providers.any.base_url should exist");

    assert_eq!(base_url, new_url);
    assert!(
        parsed.get("base_url").is_none(),
        "should not write top-level base_url"
    );

    let wire_api = parsed
        .get("model_providers")
        .and_then(|v| v.get("any"))
        .and_then(|v| v.get("wire_api"))
        .and_then(|v| v.as_str())
        .expect("model_providers.any.wire_api should exist");
    assert_eq!(wire_api, "responses");
}

#[test]
fn update_toml_base_url_falls_back_to_top_level_base_url() {
    let input = r#"
model = "gpt-5.1-codex"
"#;

    let new_url = "http://127.0.0.1:5000/v1";
    let output = crate::codex_config::update_codex_toml_field(input, "base_url", new_url)
        .expect("update top-level base_url");

    let parsed: toml::Value = toml::from_str(&output).expect("updated config should be valid TOML");

    let base_url = parsed
        .get("base_url")
        .and_then(|v| v.as_str())
        .expect("base_url should exist");

    assert_eq!(base_url, new_url);
}

#[tokio::test]
#[serial]
async fn switch_proxy_target_updates_live_backup_when_taken_over() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");

    let db = Arc::new(Database::memory().expect("init db"));
    let service = ProxyService::new(db.clone());

    let provider_a = Provider::with_id(
        "a".to_string(),
        "A".to_string(),
        json!({
            "env": {
                "ANTHROPIC_API_KEY": "a-key"
            }
        }),
        None,
    );
    let provider_b = Provider::with_id(
        "b".to_string(),
        "B".to_string(),
        json!({
            "env": {
                "ANTHROPIC_API_KEY": "b-key"
            }
        }),
        None,
    );
    db.save_provider("claude", &provider_a)
        .expect("save provider a");
    db.save_provider("claude", &provider_b)
        .expect("save provider b");
    db.set_current_provider("claude", "a")
        .expect("set current provider");

    // 模拟"已接管"状态：存在 Live 备份（内容不重要，会被热切换更新）
    db.save_live_backup("claude", "{\"env\":{}}")
        .await
        .expect("seed live backup");

    service
        .switch_proxy_target("claude", "b")
        .await
        .expect("switch proxy target");

    // 断言：本地 settings 的 current provider 已同步
    assert_eq!(
        crate::settings::get_current_provider(&AppType::Claude).as_deref(),
        Some("b")
    );

    // 断言：Live 备份已更新为目标供应商配置（用于 stop_with_restore 恢复）
    let backup = db
        .get_live_backup("claude")
        .await
        .expect("get live backup")
        .expect("backup exists");
    let expected = serde_json::to_string(&provider_b.settings_config).expect("serialize");
    assert_eq!(backup.original_config, expected);
}

#[tokio::test]
#[serial]
async fn hot_switch_provider_serializes_same_app_switches() {
    use tokio::time::{sleep, Duration};

    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");

    let db = Arc::new(Database::memory().expect("init db"));
    let service = ProxyService::new(db.clone());

    let provider_a = Provider::with_id(
        "a".to_string(),
        "A".to_string(),
        json!({ "env": { "ANTHROPIC_API_KEY": "a-key" } }),
        None,
    );
    let provider_b = Provider::with_id(
        "b".to_string(),
        "B".to_string(),
        json!({ "env": { "ANTHROPIC_API_KEY": "b-key" } }),
        None,
    );
    let provider_c = Provider::with_id(
        "c".to_string(),
        "C".to_string(),
        json!({ "env": { "ANTHROPIC_API_KEY": "c-key" } }),
        None,
    );

    db.save_provider("claude", &provider_a)
        .expect("save provider a");
    db.save_provider("claude", &provider_b)
        .expect("save provider b");
    db.save_provider("claude", &provider_c)
        .expect("save provider c");
    db.set_current_provider("claude", "a")
        .expect("set current provider");
    crate::settings::set_current_provider(&AppType::Claude, Some("a"))
        .expect("set local current provider");
    db.save_live_backup("claude", "{\"env\":{}}")
        .await
        .expect("seed live backup");

    let guard = service.lock_switch_for_test("claude").await;
    let service_for_b = service.clone();
    let service_for_c = service.clone();

    let switch_b = tokio::spawn(async move {
        service_for_b
            .hot_switch_provider("claude", "b")
            .await
            .expect("switch to b")
    });
    sleep(Duration::from_millis(20)).await;
    let switch_c = tokio::spawn(async move {
        service_for_c
            .hot_switch_provider("claude", "c")
            .await
            .expect("switch to c")
    });

    sleep(Duration::from_millis(20)).await;
    drop(guard);

    let outcome_b = switch_b.await.expect("join switch b");
    let outcome_c = switch_c.await.expect("join switch c");
    assert!(outcome_b.logical_target_changed);
    assert!(outcome_c.logical_target_changed);

    assert_eq!(
        crate::settings::get_effective_current_provider(&db, &AppType::Claude)
            .expect("effective current"),
        Some("c".to_string())
    );
    assert_eq!(
        crate::settings::get_current_provider(&AppType::Claude).as_deref(),
        Some("c")
    );
    assert_eq!(
        db.get_current_provider("claude").expect("db current"),
        Some("c".to_string())
    );

    let backup = db
        .get_live_backup("claude")
        .await
        .expect("get live backup")
        .expect("backup exists");
    let expected = serde_json::to_string(&provider_c.settings_config).expect("serialize");
    assert_eq!(backup.original_config, expected);
}

#[tokio::test]
#[serial]
async fn restore_waits_for_hot_switch_and_restores_latest_backup() {
    use tokio::time::{sleep, Duration};

    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");

    let db = Arc::new(Database::memory().expect("init db"));
    let service = ProxyService::new(db.clone());

    let provider_a = Provider::with_id(
        "a".to_string(),
        "A".to_string(),
        json!({ "env": { "ANTHROPIC_API_KEY": "a-key" } }),
        None,
    );
    let provider_b = Provider::with_id(
        "b".to_string(),
        "B".to_string(),
        json!({ "env": { "ANTHROPIC_API_KEY": "b-key" } }),
        None,
    );

    db.save_provider("claude", &provider_a)
        .expect("save provider a");
    db.save_provider("claude", &provider_b)
        .expect("save provider b");
    db.set_current_provider("claude", "a")
        .expect("set current provider");
    crate::settings::set_current_provider(&AppType::Claude, Some("a"))
        .expect("set local current provider");
    db.save_live_backup(
        "claude",
        &serde_json::to_string(&provider_a.settings_config).expect("serialize provider a"),
    )
    .await
    .expect("seed live backup");
    service
        .write_claude_live(&json!({ "env": { "ANTHROPIC_API_KEY": "stale" } }))
        .expect("seed live file");

    let guard = service.lock_switch_for_test("claude").await;
    let service_for_switch = service.clone();
    let service_for_restore = service.clone();

    let switch_to_b = tokio::spawn(async move {
        service_for_switch
            .hot_switch_provider("claude", "b")
            .await
            .expect("switch to b")
    });
    sleep(Duration::from_millis(20)).await;
    let restore = tokio::spawn(async move {
        service_for_restore
            .restore_live_config_for_app_with_fallback(&AppType::Claude)
            .await
            .expect("restore claude live")
    });

    sleep(Duration::from_millis(20)).await;
    drop(guard);

    let outcome = switch_to_b.await.expect("join switch");
    restore.await.expect("join restore");
    assert!(outcome.logical_target_changed);

    assert_eq!(
        crate::settings::get_effective_current_provider(&db, &AppType::Claude)
            .expect("effective current"),
        Some("b".to_string())
    );

    let backup = db
        .get_live_backup("claude")
        .await
        .expect("get live backup")
        .expect("backup exists");
    let expected = serde_json::to_string(&provider_b.settings_config).expect("serialize");
    assert_eq!(backup.original_config, expected);
    assert_eq!(
        service.read_claude_live().expect("read live"),
        provider_b.settings_config
    );
}

/// Regression: when the backup row itself contains the proxy placeholder
/// (a corrupted state where previous start/stop cycles saved the proxy
/// config as the "original Live"), restore must NOT write it back to Live.
/// It should fall through to the SSOT (current provider) path and rebuild
/// Live from the provider DB instead.
#[tokio::test]
#[serial]
async fn restore_falls_through_to_ssot_when_backup_is_proxy_placeholder() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");

    let db = Arc::new(Database::memory().expect("init db"));
    let service = ProxyService::new(db.clone());

    // Seed DB with a current provider that has a real API key
    let provider = Provider::with_id(
        "p1".to_string(),
        "P1".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.minimaxi.com/anthropic",
                "ANTHROPIC_API_KEY": "real-key-from-db"
            }
        }),
        None,
    );
    db.save_provider("claude", &provider)
        .expect("save provider");
    db.set_current_provider("claude", "p1")
        .expect("set current provider");

    // Seed backup with proxy placeholder (the corrupted state)
    let corrupted_backup = serde_json::to_string(&json!({
        "env": {
            "ANTHROPIC_AUTH_TOKEN": PROXY_TOKEN_PLACEHOLDER,
            "ANTHROPIC_BASE_URL": "http://127.0.0.1:15721"
        }
    }))
    .expect("serialize corrupted backup");
    db.save_live_backup("claude", &corrupted_backup)
        .await
        .expect("seed corrupted backup");

    // Seed Live with the same proxy placeholder (matches the corrupted state)
    service
        .write_claude_live(&json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": PROXY_TOKEN_PLACEHOLDER,
                "ANTHROPIC_BASE_URL": "http://127.0.0.1:15721"
            }
        }))
        .expect("seed taken-over live file");

    // Restore: must NOT use the corrupted backup
    service
        .restore_live_config_for_app_with_fallback(&AppType::Claude)
        .await
        .expect("restore should succeed via SSOT");

    // The backup should still be the corrupted one (we didn't touch it on this path)
    let backup_after = db
        .get_live_backup("claude")
        .await
        .expect("get backup")
        .expect("backup still exists");
    assert_eq!(
        backup_after.original_config, corrupted_backup,
        "restore must NOT overwrite the corrupted backup"
    );

    // Live should now reflect the SSOT (provider DB), NOT the proxy URL
    let restored_live = service.read_claude_live().expect("read live");
    let restored_url = restored_live
        .get("env")
        .and_then(|env| env.get("ANTHROPIC_BASE_URL"))
        .and_then(|v| v.as_str());
    assert_eq!(
        restored_url,
        Some("https://api.minimaxi.com/anthropic"),
        "Live must be rebuilt from SSOT, not from the corrupted backup"
    );
    let restored_key = restored_live
        .get("env")
        .and_then(|env| env.get("ANTHROPIC_API_KEY"))
        .and_then(|v| v.as_str());
    assert_eq!(
        restored_key,
        Some("real-key-from-db"),
        "Live must carry the real API key from the provider DB"
    );
    assert_ne!(
        restored_live
            .get("env")
            .and_then(|env| env.get("ANTHROPIC_AUTH_TOKEN"))
            .and_then(|v| v.as_str()),
        Some(PROXY_TOKEN_PLACEHOLDER),
        "Live must not still carry the proxy placeholder"
    );
}

/// Regression: when Live is already a proxy placeholder (a corrupted state
/// where previous stop failed to restore), backup must NOT overwrite a
/// previously-good backup with the proxy config. This prevents the bug
/// where stop-then-start cycles permanently corrupt the backup.
#[tokio::test]
#[serial]
async fn backup_skips_when_live_is_already_proxy_placeholder() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");

    let db = Arc::new(Database::memory().expect("init db"));
    let service = ProxyService::new(db.clone());

    // Seed a GOOD backup (the "real" original Live)
    let good_backup = serde_json::to_string(&json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://api.minimaxi.com/anthropic",
            "ANTHROPIC_AUTH_TOKEN": "real-token"
        }
    }))
    .expect("serialize good backup");
    db.save_live_backup("claude", &good_backup)
        .await
        .expect("seed good backup");

    // Seed Live with proxy placeholder (the corrupted state)
    service
        .write_claude_live(&json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": PROXY_TOKEN_PLACEHOLDER,
                "ANTHROPIC_BASE_URL": "http://127.0.0.1:15721"
            }
        }))
        .expect("seed taken-over live file");

    // Call backup_live_config_strict: must skip
    service
        .backup_live_config_strict(&AppType::Claude)
        .await
        .expect("backup should succeed (no-op when live is placeholder)");

    // The good backup must still be intact
    let backup_after = db
        .get_live_backup("claude")
        .await
        .expect("get backup")
        .expect("backup still exists");
    assert_eq!(
        backup_after.original_config, good_backup,
        "must not overwrite a good backup with a proxy placeholder"
    );
}

/// Regression: when ALL apps have Live=proxy-placeholder (worst-case
/// corrupted state), backing up each app must skip every save — instead
/// of overwriting good backups with the proxy config.
#[tokio::test]
#[serial]
async fn bulk_backup_skips_all_when_live_is_proxy_placeholder() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");

    let db = Arc::new(Database::memory().expect("init db"));
    let service = ProxyService::new(db.clone());

    // Seed good backups for all three apps
    let good_backup = serde_json::to_string(&json!({
        "env": {
            "ANTHROPIC_AUTH_TOKEN": "real-token"
        }
    }))
    .expect("serialize good backup");
    db.save_live_backup("claude", &good_backup)
        .await
        .expect("seed claude backup");

    let codex_good_backup = serde_json::to_string(&json!({
        "auth": { "OPENAI_API_KEY": "real-codex-token" }
    }))
    .expect("serialize codex good backup");
    db.save_live_backup("codex", &codex_good_backup)
        .await
        .expect("seed codex backup");

    let gemini_good_backup = serde_json::to_string(&json!({
        "env": { "GEMINI_API_KEY": "real-gemini-key" }
    }))
    .expect("serialize gemini good backup");
    db.save_live_backup("gemini", &gemini_good_backup)
        .await
        .expect("seed gemini backup");

    // Seed all three Live files with proxy placeholders
    service
        .write_claude_live(&json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": PROXY_TOKEN_PLACEHOLDER,
                "ANTHROPIC_BASE_URL": "http://127.0.0.1:15721"
            }
        }))
        .expect("seed claude live");
    let codex_dir = crate::codex_config::get_codex_config_dir();
    std::fs::create_dir_all(&codex_dir).expect("create codex dir");
    std::fs::write(
        crate::codex_config::get_codex_config_path(),
        r#"model_provider = "custom"

[model_providers.custom]
name = "Custom"
base_url = "http://127.0.0.1:15721/v1"
wire_api = "chat"
experimental_bearer_token = "PROXY_MANAGED"
"#,
    )
    .expect("seed codex config.toml");
    std::fs::write(
        crate::codex_config::get_codex_auth_path(),
        r#"{"OPENAI_API_KEY":"PROXY_MANAGED"}"#,
    )
    .expect("seed codex auth.json");
    let gemini_env_path = crate::gemini_config::get_gemini_env_path();
    if let Some(parent) = gemini_env_path.parent() {
        std::fs::create_dir_all(parent).expect("create gemini dir");
    }
    std::fs::write(&gemini_env_path, "GEMINI_API_KEY=PROXY_MANAGED\n").expect("seed gemini env");

    // Back up every app: must skip all three
    for app in [AppType::Claude, AppType::Codex, AppType::Gemini] {
        service
            .backup_live_config_strict(&app)
            .await
            .expect("backup should succeed (no-op when live is a placeholder)");
    }

    // All three good backups must still be intact
    for (app_type, original) in [
        ("claude", good_backup.as_str()),
        ("codex", codex_good_backup.as_str()),
        ("gemini", gemini_good_backup.as_str()),
    ] {
        let backup_after = db
            .get_live_backup(app_type)
            .await
            .expect("get backup")
            .expect("backup still exists");
        assert_eq!(
            backup_after.original_config, original,
            "must not overwrite good backup for {app_type} with proxy placeholder"
        );
    }
}
