//! Scripted end-to-end scenario (PRODUCT-45): switch -> takeover -> upstream
//! failure -> failover -> takeover off, on a temp HOME, asserting the live
//! Claude Code file byte for byte at each step.
//!
//! Covers the "residue" / "blanking" bug classes (v3.16.2 Takeover-Residue
//! Recovery, v3.16.3 Proxy Placeholder Backup / Restore Loop): after takeover
//! ends, `~/.claude/settings.json` must carry the real provider again, keep
//! the provider's non-routing keys (permissions, hooks), and contain no proxy
//! placeholder.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Router;
use cc_switch_lib::{get_claude_settings_path, AppType, Provider, ProviderService};
use serde_json::{json, Value};

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, TestEnv};

/// A mock upstream that always answers with `status` and counts its hits.
struct Upstream {
    base_url: String,
    hits: Arc<AtomicUsize>,
}

async fn spawn_upstream(status: u16) -> Upstream {
    let hits = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .fallback(
            |State((hits, status)): State<(Arc<AtomicUsize>, u16)>| async move {
                hits.fetch_add(1, Ordering::SeqCst);
                let body = if status == 200 {
                    json!({
                        "id": "msg_scenario",
                        "type": "message",
                        "role": "assistant",
                        "model": "claude-sonnet-4-5",
                        "content": [{"type": "text", "text": "ok"}],
                        "stop_reason": "end_turn",
                        "stop_sequence": null,
                        "usage": {"input_tokens": 3, "output_tokens": 1}
                    })
                } else {
                    json!({"type": "error", "error": {"type": "api_error", "message": "down"}})
                };
                (
                    StatusCode::from_u16(status).unwrap(),
                    [("content-type", "application/json")],
                    body.to_string(),
                )
                    .into_response()
            },
        )
        .with_state((hits.clone(), status));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock upstream");
    let addr = listener.local_addr().expect("upstream addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Upstream {
        base_url: format!("http://{addr}"),
        hits,
    }
}

fn provider(id: &str, base_url: &str) -> Provider {
    Provider::with_id(
        id.to_string(),
        format!("Scenario {id}"),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": base_url,
                "ANTHROPIC_AUTH_TOKEN": format!("sk-scenario-{id}"),
            },
            "permissions": {"allow": ["Bash(ls:*)"]},
            "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "echo done"}]}]}
        }),
        None,
    )
}

fn read_live() -> String {
    std::fs::read_to_string(get_claude_settings_path()).expect("read live settings.json")
}

fn live_json() -> Value {
    serde_json::from_str(&read_live()).expect("live settings.json is JSON")
}

#[tokio::test(flavor = "current_thread")]
#[allow(
    clippy::await_holding_lock,
    reason = "the scenario must hold the test HOME for its whole async lifetime"
)]
async fn switch_takeover_failover_and_restore_keep_the_live_file_exact() {
    let _env = TestEnv::new();
    ensure_test_home();

    // Whatever Claude Code had before cc-switch managed the file.
    let settings_path = get_claude_settings_path();
    std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
    std::fs::write(
        &settings_path,
        r#"{"env": {"ANTHROPIC_BASE_URL": "https://previous.example.invalid"}}"#,
    )
    .unwrap();

    let failing = spawn_upstream(500).await;
    let healthy = spawn_upstream(200).await;

    let state = create_test_state().expect("test state");
    state
        .db
        .save_provider("claude", &provider("a-primary", &failing.base_url))
        .expect("save primary");
    state
        .db
        .save_provider("claude", &provider("b-backup", &healthy.base_url))
        .expect("save backup");

    // 1. Switch: the live file is the primary's configuration.
    ProviderService::switch(&state, AppType::Claude, "a-primary").expect("switch to primary");
    let after_switch = read_live();
    let live = live_json();
    assert_eq!(live["env"]["ANTHROPIC_BASE_URL"], failing.base_url);
    assert_eq!(live["permissions"]["allow"][0], "Bash(ls:*)");
    assert_eq!(live["hooks"]["Stop"][0]["hooks"][0]["command"], "echo done");

    // 2. Takeover with failover over [a-primary, b-backup] on an ephemeral
    //    port (the queue is ordered by sort index, then id).
    let mut proxy_config = state.db.get_proxy_config().await.expect("proxy config");
    proxy_config.listen_address = "127.0.0.1".to_string();
    proxy_config.listen_port = 0;
    state
        .db
        .update_proxy_config(proxy_config)
        .await
        .expect("ephemeral proxy port");
    let mut app_config = state
        .db
        .get_proxy_config_for_app("claude")
        .await
        .expect("claude proxy config");
    app_config.auto_failover_enabled = true;
    app_config.circuit_timeout_seconds = 600;
    state
        .db
        .update_proxy_config_for_app(app_config)
        .await
        .expect("enable failover");
    for id in ["a-primary", "b-backup"] {
        state
            .db
            .add_to_failover_queue("claude", id)
            .expect("queue provider");
    }

    state
        .proxy_service
        .set_takeover_for_app("claude", true)
        .await
        .expect("enable Claude takeover");
    let port = state
        .proxy_service
        .get_status()
        .await
        .expect("proxy status")
        .port;
    let live = live_json();
    let base_url = live["env"]["ANTHROPIC_BASE_URL"]
        .as_str()
        .expect("takeover base url");
    assert!(
        base_url.starts_with("http://127.0.0.1:") && base_url.contains(&port.to_string()),
        "takeover points Claude Code at the local route, got {base_url}"
    );
    assert_eq!(
        live["permissions"]["allow"][0], "Bash(ls:*)",
        "takeover rewrites routing only, not the provider's other keys"
    );

    // 3. The primary upstream fails; the request fails over to the backup.
    let response = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap()
        .post(format!("http://127.0.0.1:{port}/v1/messages"))
        .header("content-type", "application/json")
        .header("anthropic-version", "2023-06-01")
        .header("x-api-key", "client-placeholder-key")
        .json(&json!({
            "model": "claude-sonnet-4-5",
            "max_tokens": 16,
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .send()
        .await
        .expect("request through the local route");
    assert_eq!(response.status(), 200, "failover served the request");
    assert!(
        failing.hits.load(Ordering::SeqCst) >= 1,
        "primary was tried"
    );
    assert_eq!(healthy.hits.load(Ordering::SeqCst), 1, "backup served it");

    // 4. Takeover off: the live file is the pre-takeover projection of the
    //    provider that is current now, byte for byte, with no placeholder.
    let current = state
        .db
        .get_current_provider("claude")
        .expect("current provider")
        .expect("a current provider");
    state
        .proxy_service
        .set_takeover_for_app("claude", false)
        .await
        .expect("disable Claude takeover");
    let restored = read_live();
    // The mock upstreams also listen on 127.0.0.1: look for the proxy's port.
    assert!(
        !restored.contains(&format!("127.0.0.1:{port}")),
        "no proxy residue after takeover ends: {restored}"
    );
    let live = live_json();
    assert_eq!(live["permissions"]["allow"][0], "Bash(ls:*)");
    assert_eq!(live["hooks"]["Stop"][0]["hooks"][0]["command"], "echo done");
    if current == "a-primary" {
        assert_eq!(restored, after_switch, "restore is byte-exact");
    } else {
        // Failover promoted the backup: the restored file must equal what a
        // plain switch to the backup writes.
        assert_eq!(current, "b-backup");
        assert_eq!(live["env"]["ANTHROPIC_BASE_URL"], healthy.base_url);
        ProviderService::switch(&state, AppType::Claude, "b-backup").expect("switch to backup");
        assert_eq!(read_live(), restored, "restore equals a plain switch");
    }

    let _ = state.proxy_service.stop().await;
}
