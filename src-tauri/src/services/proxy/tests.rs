use super::takeover::TakeoverError;
use super::*;
use crate::config::{get_claude_settings_path, write_json_file};
use crate::provider::ProviderMeta;
use serde_json::{json, Map};
use serial_test::serial;
use std::env;
use tempfile::TempDir;

struct TempHome {
    #[allow(dead_code)]
    dir: TempDir,
    original_home: Option<String>,
    original_userprofile: Option<String>,
    original_test_home: Option<String>,
}

impl TempHome {
    fn new() -> Self {
        let dir = TempDir::new().expect("failed to create temp home");
        let original_home = env::var("HOME").ok();
        let original_userprofile = env::var("USERPROFILE").ok();
        let original_test_home = env::var("CC_SWITCH_TEST_HOME").ok();

        env::set_var("HOME", dir.path());
        env::set_var("USERPROFILE", dir.path());
        env::set_var("CC_SWITCH_TEST_HOME", dir.path());

        Self {
            dir,
            original_home,
            original_userprofile,
            original_test_home,
        }
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        match &self.original_home {
            Some(value) => env::set_var("HOME", value),
            None => env::remove_var("HOME"),
        }

        match &self.original_userprofile {
            Some(value) => env::set_var("USERPROFILE", value),
            None => env::remove_var("USERPROFILE"),
        }

        match &self.original_test_home {
            Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
            None => env::remove_var("CC_SWITCH_TEST_HOME"),
        }
    }
}

/// 接管写入 Live 的本安装代理 token（替代旧版 PROXY_MANAGED）
fn inbound_token(db: &Database) -> String {
    crate::proxy::inbound_auth::get_or_create_inbound_token(db).expect("read proxy inbound token")
}

fn assert_env_str(env: &Map<String, Value>, key: &str, expected: Option<&str>) {
    assert_eq!(env.get(key).and_then(|value| value.as_str()), expected);
}

async fn use_ephemeral_proxy_port(db: &Arc<Database>) {
    let mut proxy_config = db.get_proxy_config().await.expect("get test proxy config");
    proxy_config.listen_port = 0;
    db.update_proxy_config(proxy_config)
        .await
        .expect("set test proxy config to an ephemeral port");
}

async fn running_codex_base_url(service: &ProxyService) -> String {
    let status = service.get_status().await.expect("get proxy status");
    format!("http://127.0.0.1:{}/v1", status.port)
}

fn seed_codex_model_template() {
    let codex_dir = crate::codex_config::get_codex_config_dir();
    std::fs::create_dir_all(&codex_dir).expect("create codex dir");
    std::fs::write(
        codex_dir.join("models_cache.json"),
        serde_json::to_string(&serde_json::json!({
            "models": [{
                "slug": "gpt-5.5",
                "display_name": "GPT-5.5",
                "model_messages": { "instructions_template": "t" },
                "additional_speed_tiers": [],
                "context_window": 128000
            }]
        }))
        .expect("serialize models_cache"),
    )
    .expect("write models_cache.json");
}

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
async fn concurrent_start_with_ephemeral_port_binds_a_single_server() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");

    let db = Arc::new(Database::memory().expect("init db"));
    use_ephemeral_proxy_port(&db).await;
    let service = ProxyService::new(db.clone());

    // start() 在"未运行"检查后会 yield_now，两次启动在无锁时都会通过检查并各自绑定端口
    let (first, second) = tokio::join!(service.start(), service.start());
    let first = first.expect("first start");
    let second = second.expect("second start");

    assert_eq!(
        first.port, second.port,
        "both starts must report one server"
    );
    assert_ne!(first.port, 0);
    let persisted = db.get_proxy_config().await.expect("get proxy config");
    assert_eq!(persisted.listen_port, first.port);
    let status = service.get_status().await.expect("get status");
    assert_eq!(status.port, first.port);
    tokio::net::TcpStream::connect(("127.0.0.1", first.port))
        .await
        .expect("reported port must accept connections");

    service.stop().await.expect("stop proxy");
}

#[tokio::test]
#[serial]
async fn is_running_is_not_blocked_by_slow_stop() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");

    let db = Arc::new(Database::memory().expect("init db"));
    use_ephemeral_proxy_port(&db).await;
    let service = ProxyService::new(db.clone());
    service.start().await.expect("start proxy");

    service
        .stop_delay_ms_for_test
        .store(1_000, std::sync::atomic::Ordering::SeqCst);
    let stopping = {
        let service = service.clone();
        tokio::spawn(async move { service.stop().await })
    };
    // 等待 stop() 进入慢速阶段
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(!stopping.is_finished(), "stop should still be in progress");

    let running = tokio::time::timeout(std::time::Duration::from_millis(100), service.is_running())
        .await
        .expect("is_running must not wait for stop() to finish");
    assert!(!running);
    tokio::time::timeout(std::time::Duration::from_millis(100), service.get_status())
        .await
        .expect("get_status must not wait for stop() to finish")
        .expect("get status");

    stopping.await.expect("join stop task").expect("stop proxy");
}

#[tokio::test]
#[serial]
async fn update_config_to_occupied_port_keeps_original_server_and_config() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");

    let db = Arc::new(Database::memory().expect("init db"));
    use_ephemeral_proxy_port(&db).await;
    let service = ProxyService::new(db.clone());
    let info = service.start().await.expect("start proxy");
    let original_port = info.port;

    // 占用另一个端口，使切换到该端口时绑定失败
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").expect("occupy port");
    let occupied_port = occupied.local_addr().expect("occupied addr").port();

    let mut new_config = service.get_config().await.expect("get config");
    new_config.listen_port = occupied_port;
    let result = service.update_config(&new_config).await;
    assert!(result.is_err(), "binding an occupied port must fail");

    let status = service.get_status().await.expect("get status");
    assert!(status.running, "original server must keep running");
    assert_eq!(status.port, original_port);
    let config = service.get_config().await.expect("get config");
    assert_eq!(config.listen_port, original_port, "DB must keep old port");
    tokio::net::TcpStream::connect(("127.0.0.1", original_port))
        .await
        .expect("original port must still accept connections");

    drop(occupied);
    service.stop().await.expect("stop proxy");
}

#[tokio::test]
#[serial]
async fn update_config_to_invalid_address_restores_previous_server() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");

    let db = Arc::new(Database::memory().expect("init db"));
    use_ephemeral_proxy_port(&db).await;
    let service = ProxyService::new(db.clone());
    let info = service.start().await.expect("start proxy");
    let original_port = info.port;
    let previous = service.get_config().await.expect("get config");

    // 同端口仅改地址：须先停旧服务器，新地址无效时应以旧配置恢复
    let mut new_config = previous.clone();
    new_config.listen_address = "not-an-address".to_string();
    let result = service.update_config(&new_config).await;
    assert!(result.is_err(), "an invalid address must fail");

    let status = service.get_status().await.expect("get status");
    assert!(status.running, "previous server must be restored");
    assert_eq!(status.port, original_port);
    let config = service.get_config().await.expect("get config");
    assert_eq!(config.listen_address, previous.listen_address);
    assert_eq!(config.listen_port, original_port);
    tokio::net::TcpStream::connect(("127.0.0.1", original_port))
        .await
        .expect("restored server must accept connections");

    service.stop().await.expect("stop proxy");
}

#[tokio::test]
#[serial]
async fn start_with_takeover_ephemeral_port_writes_actual_live_url() {
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

    let info = service
        .start_with_takeover()
        .await
        .expect("start proxy with takeover");
    assert_ne!(info.port, 0, "OS should assign a concrete port");

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

#[tokio::test]
#[serial]
async fn start_with_takeover_writes_per_install_token_instead_of_placeholder() {
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
                "ANTHROPIC_AUTH_TOKEN": "provider-key",
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
                "ANTHROPIC_AUTH_TOKEN": "live-key",
                "ANTHROPIC_BASE_URL": "https://api.anthropic.com"
            }
        }))
        .expect("seed claude live config");

    service
        .start_with_takeover()
        .await
        .expect("start proxy with takeover");

    let token = inbound_token(&db);
    let live = service.read_claude_live().expect("read taken-over live");
    let env = live["env"].as_object().expect("env");
    assert_env_str(env, "ANTHROPIC_AUTH_TOKEN", Some(token.as_str()));
    assert!(!live.to_string().contains(PROXY_TOKEN_PLACEHOLDER));
    assert!(service.detect_takeover_in_live_config_for_app(&AppType::Claude));

    service
        .stop_with_restore()
        .await
        .expect("stop proxy and restore live config");
    let restored = service.read_claude_live().expect("read restored live");
    assert_env_str(
        restored["env"].as_object().expect("env"),
        "ANTHROPIC_AUTH_TOKEN",
        Some("live-key"),
    );
}

#[tokio::test]
#[serial]
async fn proxy_start_migrates_legacy_placeholder_in_taken_over_live_configs() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");

    let db = Arc::new(Database::memory().expect("init db"));
    use_ephemeral_proxy_port(&db).await;
    let service = ProxyService::new(db.clone());

    // Live files written by an older version: raw writes keep PROXY_MANAGED.
    write_json_file(
        &get_claude_settings_path(),
        &json!({
            "env": {
                "ANTHROPIC_BASE_URL": "http://127.0.0.1:15721",
                "ANTHROPIC_AUTH_TOKEN": "PROXY_MANAGED"
            },
            "permissions": { "allow": ["Bash"] }
        }),
    )
    .expect("seed legacy claude live");
    let codex_config_path = crate::codex_config::get_codex_config_path();
    std::fs::create_dir_all(codex_config_path.parent().expect("codex dir"))
        .expect("create codex dir");
    std::fs::write(
        &codex_config_path,
        r#"model_provider = "relay"
model = "gpt-5"

[model_providers.relay]
name = "Relay"
base_url = "http://127.0.0.1:15721/v1"
wire_api = "responses"
experimental_bearer_token = "PROXY_MANAGED"
"#,
    )
    .expect("seed legacy codex config");

    service.start().await.expect("start proxy");

    let token = inbound_token(&db);
    let claude = std::fs::read_to_string(get_claude_settings_path()).expect("read claude");
    assert!(!claude.contains(PROXY_TOKEN_PLACEHOLDER), "{claude}");
    let claude: Value = serde_json::from_str(&claude).expect("parse claude");
    assert_env_str(
        claude["env"].as_object().expect("env"),
        "ANTHROPIC_AUTH_TOKEN",
        Some(token.as_str()),
    );
    assert_eq!(claude["permissions"], json!({ "allow": ["Bash"] }));

    let codex = std::fs::read_to_string(&codex_config_path).expect("read codex config");
    assert!(!codex.contains(PROXY_TOKEN_PLACEHOLDER), "{codex}");
    assert!(
        codex.contains(&format!("experimental_bearer_token = \"{token}\"")),
        "{codex}"
    );
    assert!(service.detect_takeover_in_live_config_for_app(&AppType::Codex));

    service.stop().await.expect("stop proxy");
}

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
        live_config.contains(&inbound_token(&service.db)),
        "live config should carry the proxy placeholder token"
    );

    crate::settings::update_settings(crate::settings::AppSettings::default())
        .expect("reset settings");
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
        .takeover(&AppType::Codex, TakeoverMode::Strict)
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
        live_config.contains(&inbound_token(&service.db)),
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
        .takeover(&AppType::Codex, TakeoverMode::Strict)
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
        live_config.contains(&inbound_token(&service.db)),
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
    assert!(third_party_live.contains(&inbound_token(&service.db)));
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
        .takeover(&AppType::Codex, TakeoverMode::Strict)
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
        live_config.contains(&inbound_token(&db)),
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
        live_config.contains(&inbound_token(&service.db)),
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
        .takeover(&AppType::Codex, TakeoverMode::Strict)
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
        live_config.contains(&inbound_token(&service.db)),
        "third-party takeover should carry its local placeholder in config.toml"
    );

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
            "OPENAI_API_KEY": inbound_token(&service.db)
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
    // ChatGPT login owns Authorization; the proxy token rides in x-api-key
    // and is bound to the per-install token when the live file is written.
    assert_eq!(
        route["http_headers"]["x-api-key"].as_str(),
        Some(PROXY_TOKEN_PLACEHOLDER)
    );
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
        Some(inbound_token(&service.db).as_str()),
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
        Some(inbound_token(&service.db).as_str())
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
/// corrupted state), the bulk `backup_live_configs` path used by
/// `start_with_takeover` must skip every save — instead of overwriting
/// good backups with the proxy config.
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

    // Call bulk backup: must skip all three apps
    service
        .backup_live_configs()
        .await
        .expect("bulk backup should succeed (no-op when all live are placeholders)");

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

fn grok_provider_config(base_url: &str, api_key: &str) -> Value {
    json!({
        "config": format!(
            "[models]\ndefault = \"grok-4.5\"\n\n[model.\"grok-4.5\"]\nmodel = \"grok-4.5\"\nbase_url = \"{base_url}\"\nname = \"Grok\"\napi_key = \"{api_key}\"\napi_backend = \"responses\"\ncontext_window = 500000\n"
        )
    })
}

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

/// CCS-153：services/proxy 拆分后每个生产文件不超过 1500 行，防止重新膨胀成单文件。
#[test]
fn proxy_service_modules_stay_under_line_budget() {
    let files = [
        ("mod.rs", include_str!("mod.rs")),
        ("lifecycle.rs", include_str!("lifecycle.rs")),
        ("config.rs", include_str!("config.rs")),
        ("backup.rs", include_str!("backup.rs")),
        ("hot_switch.rs", include_str!("hot_switch.rs")),
        ("takeover/mod.rs", include_str!("takeover/mod.rs")),
        ("takeover/claude.rs", include_str!("takeover/claude.rs")),
        ("takeover/codex.rs", include_str!("takeover/codex.rs")),
        ("takeover/engine.rs", include_str!("takeover/engine.rs")),
        ("takeover/gemini.rs", include_str!("takeover/gemini.rs")),
        ("takeover/legacy.rs", include_str!("takeover/legacy.rs")),
        ("takeover/grok.rs", include_str!("takeover/grok.rs")),
    ];
    for (name, src) in files {
        let lines = src.lines().count();
        assert!(
            lines <= 1500,
            "services/proxy/{name} has {lines} lines; split it further"
        );
    }
}

// ============================================================================
// CCS-154: unified takeover engine
// ============================================================================

const ENGINE_TEST_PORT: u16 = 15_999;
const ENGINE_TEST_KEY: &str = "sk-engine-test-key";
const ENGINE_TEST_UPSTREAM: &str = "https://engine-test.example.com";

fn use_takeover_engine(engine: crate::services::takeover_snapshot::TakeoverEngine) {
    crate::settings::update_settings_with(|current| {
        let mut next = current.clone();
        next.takeover_engine = engine;
        next
    })
    .expect("update takeover engine");
}

async fn use_fixed_proxy_port(db: &Arc<Database>) {
    let mut proxy_config = db.get_proxy_config().await.expect("get proxy config");
    proxy_config.listen_address = "127.0.0.1".to_string();
    proxy_config.listen_port = ENGINE_TEST_PORT;
    db.update_proxy_config(proxy_config)
        .await
        .expect("set fixed proxy port");
}

/// 一个可接管应用的 (应用, live 文件, 供应商配置)
type EngineTestCase = (AppType, Vec<(&'static str, String)>, Value);

fn engine_test_cases() -> Vec<EngineTestCase> {
    let codex_config = format!(
        "model_provider = \"custom\"\nmodel = \"gpt-5.4\"\n\n[model_providers.custom]\nname = \"Engine\"\nbase_url = \"{ENGINE_TEST_UPSTREAM}/v1\"\nwire_api = \"responses\"\nrequires_openai_auth = true\n"
    );
    let grok_config = format!(
        "[models]\ndefault = \"grok-4.5\"\n\n[model.\"grok-4.5\"]\nmodel = \"grok-4.5\"\nbase_url = \"{ENGINE_TEST_UPSTREAM}/v1\"\nname = \"Engine\"\napi_key = \"{ENGINE_TEST_KEY}\"\napi_backend = \"responses\"\ncontext_window = 500000\n"
    );
    vec![
        (
            AppType::Claude,
            vec![(
                "settings.json",
                format!(
                    "{{\"permissions\":{{\"allow\":[\"Bash\"]}},\"env\":{{\"ANTHROPIC_BASE_URL\":\"{ENGINE_TEST_UPSTREAM}\",\"ANTHROPIC_AUTH_TOKEN\":\"{ENGINE_TEST_KEY}\"}}}}"
                ),
            )],
            json!({ "env": {
                "ANTHROPIC_AUTH_TOKEN": ENGINE_TEST_KEY,
                "ANTHROPIC_BASE_URL": ENGINE_TEST_UPSTREAM
            }}),
        ),
        (
            AppType::Codex,
            vec![
                (
                    "auth.json",
                    format!("{{\"OPENAI_API_KEY\":\"{ENGINE_TEST_KEY}\"}}"),
                ),
                ("config.toml", codex_config.clone()),
            ],
            json!({ "auth": { "OPENAI_API_KEY": ENGINE_TEST_KEY }, "config": codex_config }),
        ),
        (
            AppType::Gemini,
            vec![(
                ".env",
                format!(
                    "GOOGLE_GEMINI_BASE_URL={ENGINE_TEST_UPSTREAM}\nGEMINI_API_KEY={ENGINE_TEST_KEY}\nGEMINI_MODEL=gemini-2.5-pro\n"
                ),
            )],
            json!({ "env": {
                "GEMINI_API_KEY": ENGINE_TEST_KEY,
                "GOOGLE_GEMINI_BASE_URL": ENGINE_TEST_UPSTREAM,
                "GEMINI_MODEL": "gemini-2.5-pro"
            }}),
        ),
        (
            AppType::GrokBuild,
            vec![("config.toml", grok_config.clone())],
            json!({ "config": grok_config }),
        ),
    ]
}

fn seed_engine_case(db: &Database, app: &AppType, files: &[(&str, String)], settings: &Value) {
    let dir = crate::apps::adapter(app).config_dir().expect("config dir");
    std::fs::create_dir_all(&dir).expect("create config dir");
    for (name, body) in files {
        #[allow(clippy::disallowed_methods)]
        std::fs::write(dir.join(name), body).expect("write live file");
    }
    let provider = Provider::with_id(
        "engine".to_string(),
        "Engine".to_string(),
        settings.clone(),
        None,
    );
    db.save_provider(app.as_str(), &provider)
        .expect("save provider");
    db.set_current_provider(app.as_str(), "engine")
        .expect("set db current provider");
    crate::settings::set_current_provider(app, Some("engine")).expect("set local current provider");
}

fn read_engine_case(app: &AppType, files: &[(&str, String)]) -> Vec<(String, Option<Vec<u8>>)> {
    let dir = crate::apps::adapter(app).config_dir().expect("config dir");
    files
        .iter()
        .map(|(name, _)| (name.to_string(), std::fs::read(dir.join(name)).ok()))
        .collect()
}

#[tokio::test]
#[serial]
async fn takeover_base_url_comes_from_the_registry_and_keeps_existing_live_urls() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");
    let db = Arc::new(Database::memory().expect("init db"));
    use_fixed_proxy_port(&db).await;
    let service = ProxyService::new(db);

    let origin = format!("http://127.0.0.1:{ENGINE_TEST_PORT}");
    let expected = [
        (AppType::Claude, origin.clone()),
        (AppType::Codex, format!("{origin}/v1")),
        (AppType::Gemini, origin.clone()),
        (AppType::GrokBuild, format!("{origin}/grokbuild/v1")),
    ];
    for (app, url) in expected {
        assert_eq!(
            service.takeover_base_url(&app).await.expect("base url"),
            url,
            "{app:?}"
        );
    }

    // 每个可接管应用都在注册表上声明了代理路由与接管适配器
    for d in crate::apps::registry::with_caps(crate::apps::AppCaps::PROXY_TAKEOVER) {
        assert!(d.proxy_route.is_some(), "{} has no proxy_route", d.slug);
        assert!(
            crate::apps::adapter(&d.app).takeover().is_some(),
            "{} has no ProxyTakeover",
            d.slug
        );
    }

    for app in [
        AppType::ClaudeDesktop,
        AppType::OpenCode,
        AppType::OpenClaw,
        AppType::Hermes,
    ] {
        assert_eq!(
            service.takeover_base_url(&app).await,
            Err(TakeoverError::Unsupported(app.clone()))
        );
        let err = service
            .takeover(&app, TakeoverMode::Strict)
            .await
            .expect_err("unsupported app");
        assert_eq!(err.to_string(), "该应用不支持代理功能");
    }
}

#[tokio::test]
#[serial]
async fn unified_takeover_writes_the_same_live_files_as_the_legacy_engine() {
    use crate::services::takeover_snapshot::TakeoverEngine;

    for mode in [TakeoverMode::Strict, TakeoverMode::BestEffort] {
        for (app, files, settings) in engine_test_cases() {
            let _home = TempHome::new();
            crate::settings::reload_settings().expect("reload settings");
            let db = Arc::new(Database::memory().expect("init db"));
            use_fixed_proxy_port(&db).await;
            let service = ProxyService::new(db.clone());

            let mut outputs = Vec::new();
            for engine in [TakeoverEngine::Legacy, TakeoverEngine::Unified] {
                use_takeover_engine(engine);
                seed_engine_case(&db, &app, &files, &settings);
                service
                    .takeover(&app, mode)
                    .await
                    .unwrap_or_else(|e| panic!("{app:?} {mode:?} {engine:?}: {e}"));
                assert!(
                    service
                        .live_takeover_matches_current_proxy(&app)
                        .await
                        .expect("match live"),
                    "{app:?} {mode:?} {engine:?}: live must point at the proxy"
                );
                outputs.push(read_engine_case(&app, &files));
            }
            use_takeover_engine(TakeoverEngine::Unified);
            assert_eq!(
                outputs[0], outputs[1],
                "{app:?} {mode:?}: unified and legacy takeover must write identical live files"
            );
        }
    }
}

#[tokio::test]
#[serial]
async fn strict_takeover_requires_live_config_and_best_effort_skips_it() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");
    let db = Arc::new(Database::memory().expect("init db"));
    use_fixed_proxy_port(&db).await;
    let service = ProxyService::new(db);

    for (app, files, _) in engine_test_cases() {
        let before = read_engine_case(&app, &files);
        match service.takeover(&app, TakeoverMode::Strict).await {
            Err(TakeoverError::LiveUnavailable { app: failed, .. }) => assert_eq!(failed, app),
            other => panic!("{app:?}: strict takeover without live must fail, got {other:?}"),
        }
        service
            .takeover(&app, TakeoverMode::BestEffort)
            .await
            .unwrap_or_else(|e| panic!("{app:?}: best-effort takeover must skip: {e}"));
        assert_eq!(
            read_engine_case(&app, &files),
            before,
            "{app:?}: skipped takeover must not create live files"
        );
    }
}

#[tokio::test]
#[serial]
async fn strict_takeover_reports_a_missing_current_provider() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");
    let db = Arc::new(Database::memory().expect("init db"));
    use_fixed_proxy_port(&db).await;
    let service = ProxyService::new(db);

    service
        .write_claude_live(&json!({ "env": { "ANTHROPIC_AUTH_TOKEN": "live-key" } }))
        .expect("seed claude live");
    match service
        .takeover(&AppType::Claude, TakeoverMode::Strict)
        .await
    {
        Err(TakeoverError::Provider { app, message }) => {
            assert_eq!(app, AppType::Claude);
            assert!(message.contains("当前供应商不存在"), "{message}");
        }
        other => panic!("expected a provider error, got {other:?}"),
    }

    // 尽力模式下 Claude 不依赖供应商投影：保留现有凭据字段并写入占位符
    service
        .takeover(&AppType::Claude, TakeoverMode::BestEffort)
        .await
        .expect("best-effort claude takeover");
    assert!(service
        .live_takeover_matches_current_proxy(&AppType::Claude)
        .await
        .expect("match live"));
}

#[tokio::test]
#[serial]
async fn takeover_status_is_generated_from_the_registry() {
    let _home = TempHome::new();
    crate::settings::reload_settings().expect("reload settings");
    let db = Arc::new(Database::memory().expect("init db"));
    let service = ProxyService::new(db.clone());

    let mut grok = db
        .get_proxy_config_for_app("grokbuild")
        .await
        .expect("grokbuild proxy config");
    grok.enabled = true;
    db.update_proxy_config_for_app(grok)
        .await
        .expect("enable grokbuild takeover");

    let status = service.get_takeover_status().await.expect("status");
    let json = serde_json::to_value(&status).expect("serialize status");
    let expected: Value = crate::apps::registry::APPS
        .iter()
        .map(|d| (d.slug.to_string(), json!(d.slug == "grokbuild")))
        .collect::<Map<_, _>>()
        .into();
    assert_eq!(json, expected);
    assert_eq!(json["hermes"], false, "hermes must be reported");
    assert!(service.is_takeover_active().await.expect("active"));
    assert_eq!(
        status.enabled_apps().collect::<Vec<_>>(),
        vec![AppType::GrokBuild]
    );
}
