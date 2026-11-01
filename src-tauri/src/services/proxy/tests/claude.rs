//! Claude takeover fields and live writes.

use super::*;

#[test]
fn managed_account_claude_takeover_uses_api_key_placeholder() {
    let mut provider = Provider::with_id(
        "copilot".to_string(),
        "GitHub Copilot".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.githubcopilot.com",
                "ANTHROPIC_MODEL": "claude-haiku-4.5"
            }
        }),
        None,
    );
    provider.meta = Some(ProviderMeta {
        provider_type: Some("github_copilot".to_string()),
        ..Default::default()
    });

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
    assert_eq!(
        env.get("ANTHROPIC_API_KEY")
            .and_then(|value| value.as_str()),
        Some(PROXY_TOKEN_PLACEHOLDER)
    );
    assert!(
        env.get("ANTHROPIC_AUTH_TOKEN").is_none(),
        "managed OAuth providers should avoid Claude Auth Token login semantics"
    );
}

#[test]
fn managed_account_claude_takeover_sources_copilot_models_from_provider() {
    let mut provider = Provider::with_id(
        "copilot".to_string(),
        "GitHub Copilot".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.githubcopilot.com",
                "ANTHROPIC_MODEL": "claude-sonnet-4.6",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "claude-haiku-4.5",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-4.6",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "claude-sonnet-4.6",
                "CLAUDE_CODE_SUBAGENT_MODEL": "claude-sonnet-4.6[1M]"
            }
        }),
        None,
    );
    provider.meta = Some(ProviderMeta {
        provider_type: Some("github_copilot".to_string()),
        ..Default::default()
    });

    let mut live_config = json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://stale.example.com",
            "ANTHROPIC_API_KEY": "stale-key",
            "ANTHROPIC_MODEL": "stale-model",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "stale-haiku",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME": "Stale Haiku",
            "ANTHROPIC_DEFAULT_SONNET_MODEL": "stale-sonnet",
            "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME": "Stale Sonnet",
            "ANTHROPIC_DEFAULT_OPUS_MODEL": "stale-opus",
            "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME": "Stale Opus",
            "CLAUDE_CODE_SUBAGENT_MODEL": "stale-subagent"
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
        Some("claude-haiku-4.5"),
    );
    assert_env_str(
        env,
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        Some("claude-sonnet-4-6"),
    );
    assert_env_str(
        env,
        "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
        Some("claude-sonnet-4.6"),
    );
    assert_env_str(env, "ANTHROPIC_DEFAULT_OPUS_MODEL", Some("claude-opus-4-8"));
    assert_env_str(
        env,
        "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
        Some("claude-sonnet-4.6"),
    );
    assert_env_str(
        env,
        "CLAUDE_CODE_SUBAGENT_MODEL",
        Some("claude-sonnet-4.6[1M]"),
    );
    assert_env_str(env, "ANTHROPIC_API_KEY", Some(PROXY_TOKEN_PLACEHOLDER));
    assert_env_str(env, "ANTHROPIC_AUTH_TOKEN", None);
}

#[test]
fn managed_account_claude_takeover_removes_stale_subagent_model_when_provider_omits_it() {
    let mut provider = Provider::with_id(
        "codex".to_string(),
        "Codex".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://chatgpt.com/backend-api/codex",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "provider-sonnet"
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
            "ANTHROPIC_API_KEY": "stale-key",
            "CLAUDE_CODE_SUBAGENT_MODEL": "stale-subagent"
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
    assert_env_str(env, "CLAUDE_CODE_SUBAGENT_MODEL", None);
}

#[test]
fn managed_account_claude_takeover_copilot_removes_stale_auth_token() {
    let mut provider = Provider::with_id(
        "copilot".to_string(),
        "GitHub Copilot".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.githubcopilot.com"
            }
        }),
        None,
    );
    provider.meta = Some(ProviderMeta {
        provider_type: Some("github_copilot".to_string()),
        ..Default::default()
    });

    let mut live_config = json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://stale.example.com",
            "ANTHROPIC_AUTH_TOKEN": "stale-token"
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
    assert_env_str(env, "ANTHROPIC_API_KEY", Some(PROXY_TOKEN_PLACEHOLDER));
    assert_env_str(env, "ANTHROPIC_AUTH_TOKEN", None);
}

#[test]
fn normal_claude_takeover_without_token_keeps_auth_token_fallback() {
    let mut live_config = json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://api.example.com",
            "ANTHROPIC_MODEL": "claude-haiku-4.5"
        }
    });

    ProxyService::apply_claude_takeover_fields(&mut live_config, "http://127.0.0.1:15721");

    assert_eq!(
        live_config
            .get("env")
            .and_then(|env| env.get("ANTHROPIC_AUTH_TOKEN"))
            .and_then(|value| value.as_str()),
        Some(PROXY_TOKEN_PLACEHOLDER)
    );
    assert!(
        live_config
            .get("env")
            .and_then(|env| env.get("ANTHROPIC_API_KEY"))
            .is_none(),
        "non-managed providers should retain the legacy fallback behavior"
    );
}

#[tokio::test]
#[serial]
async fn sync_claude_token_does_not_add_anthropic_api_key() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");

    let db = Arc::new(Database::memory().expect("init db"));
    let service = ProxyService::new(db.clone());

    let provider = Provider::with_id(
        "p1".to_string(),
        "P1".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.anthropic.com",
                "ANTHROPIC_AUTH_TOKEN": "stale"
            }
        }),
        None,
    );
    db.save_provider("claude", &provider)
        .expect("save provider");
    db.set_current_provider("claude", "p1")
        .expect("set current provider");

    let live_config = json!({
        "env": {
            "ANTHROPIC_AUTH_TOKEN": "fresh"
        }
    });

    service
        .sync_live_config_to_provider(&AppType::Claude, &live_config)
        .await
        .expect("sync");

    let updated = db
        .get_provider_by_id("p1", "claude")
        .expect("get provider")
        .expect("provider exists");
    let env = updated
        .settings_config
        .get("env")
        .and_then(|v| v.as_object())
        .expect("env object");

    assert_eq!(
        env.get("ANTHROPIC_AUTH_TOKEN").and_then(|v| v.as_str()),
        Some("fresh")
    );
    assert!(
        !env.contains_key("ANTHROPIC_API_KEY"),
        "should not add ANTHROPIC_API_KEY when absent"
    );
}

#[tokio::test]
#[serial]
async fn sync_claude_token_respects_existing_api_key_field() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");

    let db = Arc::new(Database::memory().expect("init db"));
    let service = ProxyService::new(db.clone());

    let provider = Provider::with_id(
        "p1".to_string(),
        "P1".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.anthropic.com",
                "ANTHROPIC_API_KEY": "stale"
            }
        }),
        None,
    );
    db.save_provider("claude", &provider)
        .expect("save provider");
    db.set_current_provider("claude", "p1")
        .expect("set current provider");

    let live_config = json!({
        "env": {
            "ANTHROPIC_AUTH_TOKEN": "fresh"
        }
    });

    service
        .sync_live_config_to_provider(&AppType::Claude, &live_config)
        .await
        .expect("sync");

    let updated = db
        .get_provider_by_id("p1", "claude")
        .expect("get provider")
        .expect("provider exists");
    let env = updated
        .settings_config
        .get("env")
        .and_then(|v| v.as_object())
        .expect("env object");

    assert_eq!(
        env.get("ANTHROPIC_API_KEY").and_then(|v| v.as_str()),
        Some("fresh")
    );
    assert!(
        !env.contains_key("ANTHROPIC_AUTH_TOKEN"),
        "should not add ANTHROPIC_AUTH_TOKEN when absent"
    );
}

#[tokio::test]
#[serial]
async fn hot_switch_provider_updates_claude_live_while_preserving_takeover_fields() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");

    let db = Arc::new(Database::memory().expect("init db"));
    let service = ProxyService::new(db.clone());

    let provider_a = Provider::with_id(
        "a".to_string(),
        "A".to_string(),
        json!({
            "env": {
                "ANTHROPIC_API_KEY": "a-key",
                "ANTHROPIC_BASE_URL": "https://api.a.example",
                "ANTHROPIC_MODEL": "claude-old"
            },
            "permissions": { "allow": ["Bash"] }
        }),
        None,
    );
    let provider_b = Provider::with_id(
        "b".to_string(),
        "B".to_string(),
        json!({
            "env": {
                "ANTHROPIC_API_KEY": "b-key",
                "ANTHROPIC_BASE_URL": "https://api.b.example",
                "ANTHROPIC_MODEL": "claude-new",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "deepseek-v4-flash",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME": "DeepSeek V4 Flash",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "deepseek-v4-pro[1M]",
                "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME": "DeepSeek V4 Pro",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "deepseek-v4-ultra [1m]",
                "CLAUDE_CODE_SUBAGENT_MODEL": "deepseek-v4-pro[1M]"
            },
            "permissions": { "allow": ["Read"] }
        }),
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
        .write_claude_live(&json!({
            "env": {
                "ANTHROPIC_BASE_URL": "http://127.0.0.1:15721",
                "ANTHROPIC_API_KEY": PROXY_TOKEN_PLACEHOLDER,
                "ANTHROPIC_MODEL": "stale-model",
                "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME": "Stale Sonnet",
                "CLAUDE_CODE_SUBAGENT_MODEL": "stale-subagent"
            },
            "permissions": { "allow": ["Bash"] }
        }))
        .expect("seed taken-over live file");

    service
        .hot_switch_provider("claude", "b")
        .await
        .expect("hot switch provider");

    let live = service.read_claude_live().expect("read live config");
    assert_eq!(
        live.get("permissions"),
        provider_b.settings_config.get("permissions"),
        "provider-derived live settings should be refreshed"
    );
    assert_eq!(
        live.get("env")
            .and_then(|env| env.get("ANTHROPIC_API_KEY"))
            .and_then(|v| v.as_str()),
        Some(PROXY_TOKEN_PLACEHOLDER),
        "takeover token placeholder should be preserved"
    );
    assert_eq!(
        live.get("env")
            .and_then(|env| env.get("ANTHROPIC_BASE_URL"))
            .and_then(|v| v.as_str()),
        Some("http://127.0.0.1:15721"),
        "takeover proxy URL should remain active"
    );
    assert!(
        live.get("env")
            .and_then(|env| env.get("ANTHROPIC_MODEL"))
            .is_none(),
        "fallback model override should be removed in takeover mode"
    );
    let live_env = live
        .get("env")
        .and_then(|env| env.as_object())
        .expect("live env");
    assert_eq!(
        live_env
            .get("ANTHROPIC_DEFAULT_HAIKU_MODEL")
            .and_then(|v| v.as_str()),
        Some("claude-haiku-4-5"),
        "takeover mode should expose a stable Haiku role model"
    );
    assert_eq!(
        live_env
            .get("ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME")
            .and_then(|v| v.as_str()),
        Some("DeepSeek V4 Flash"),
        "model menu should show the current provider Haiku display name"
    );
    assert_eq!(
        live_env
            .get("ANTHROPIC_DEFAULT_SONNET_MODEL")
            .and_then(|v| v.as_str()),
        Some("claude-sonnet-4-6[1M]"),
        "Sonnet role should carry the local 1M declaration for Claude Code"
    );
    assert_eq!(
        live_env
            .get("ANTHROPIC_DEFAULT_SONNET_MODEL_NAME")
            .and_then(|v| v.as_str()),
        Some("DeepSeek V4 Pro"),
        "stale model display names should be replaced during hot switch"
    );
    assert_eq!(
        live_env
            .get("ANTHROPIC_DEFAULT_OPUS_MODEL")
            .and_then(|v| v.as_str()),
        Some("claude-opus-4-8[1M]"),
        "Opus role should preserve the current provider 1M capability marker"
    );
    assert_eq!(
        live_env
            .get("ANTHROPIC_DEFAULT_OPUS_MODEL_NAME")
            .and_then(|v| v.as_str()),
        Some("deepseek-v4-ultra"),
        "implicit display names should strip the local 1M marker"
    );
    assert_eq!(
        live_env
            .get("CLAUDE_CODE_SUBAGENT_MODEL")
            .and_then(|v| v.as_str()),
        Some("deepseek-v4-pro[1M]"),
        "subagent model should follow the target provider during hot switch"
    );

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
async fn update_live_backup_from_provider_applies_claude_common_config() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");

    let db = Arc::new(Database::memory().expect("init db"));
    db.set_config_snippet(
        "claude",
        Some(
            serde_json::json!({
                "includeCoAuthoredBy": false
            })
            .to_string(),
        ),
    )
    .expect("set common config snippet");

    let service = ProxyService::new(db.clone());

    let mut provider = Provider::with_id(
        "p1".to_string(),
        "P1".to_string(),
        json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "token",
                "ANTHROPIC_BASE_URL": "https://claude.example"
            }
        }),
        None,
    );
    provider.meta = Some(ProviderMeta {
        common_config_enabled: Some(true),
        ..Default::default()
    });

    service
        .update_live_backup_from_provider("claude", &provider)
        .await
        .expect("update live backup");

    let backup = db
        .get_live_backup("claude")
        .await
        .expect("get live backup")
        .expect("backup exists");
    let stored: Value = serde_json::from_str(&backup.original_config).expect("parse backup json");

    assert_eq!(
        stored.get("includeCoAuthoredBy").and_then(|v| v.as_bool()),
        Some(false),
        "common config should be applied into Claude restore backup"
    );
}
