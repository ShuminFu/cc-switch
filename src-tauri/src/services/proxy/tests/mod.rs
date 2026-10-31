use super::*;
use crate::provider::ProviderMeta;
use serial_test::serial;
use std::env;
use tempfile::TempDir;

thread_local! {
    /// Failure injection for `write_claude_live` (root ignores file modes, so a
    /// read-only settings file cannot be used to make only the write fail).
    pub(super) static FAIL_CLAUDE_LIVE_WRITE: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

struct TempHome {
    _dir: TempDir,
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
            _dir: dir,
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

fn grok_provider_config(base_url: &str, api_key: &str) -> Value {
    json!({
        "config": format!(
            "[models]\ndefault = \"grok-4.5\"\n\n[model.\"grok-4.5\"]\nmodel = \"grok-4.5\"\nbase_url = \"{base_url}\"\nname = \"Grok\"\napi_key = \"{api_key}\"\napi_backend = \"responses\"\ncontext_window = 500000\n"
        )
    })
}

mod claude;
mod codex_live;
mod codex_restore;
mod codex_takeover;
mod lifecycle;
mod other_apps;
