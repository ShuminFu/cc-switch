//! PRODUCT-24: a managed `policy.json` blocks denied provider endpoints.

use serde_json::json;

use cc_switch_lib::{AppType, Provider, ProviderService};

#[path = "support.rs"]
mod support;
use support::{create_test_state, TestEnv};

fn write_user_policy(env: &TestEnv, body: &str) {
    let dir = env.home().join(".cc-switch");
    std::fs::create_dir_all(&dir).expect("create config dir");
    std::fs::write(dir.join("policy.json"), body).expect("write policy");
}

fn claude_provider(id: &str, base_url: &str) -> Provider {
    Provider::with_id(
        id.to_string(),
        id.to_string(),
        json!({ "env": { "ANTHROPIC_BASE_URL": base_url, "ANTHROPIC_AUTH_TOKEN": "sk-test" } }),
        None,
    )
}

#[test]
fn denied_endpoint_cannot_be_added_or_switched_to_by_update() {
    let env = TestEnv::new();
    write_user_policy(
        &env,
        r#"{"version":1,"name":"Acme IT","endpoints":{"deny":["*.example-relay.com"]}}"#,
    );
    let state = create_test_state().expect("state");

    let err = ProviderService::add(
        &state,
        AppType::Claude,
        claude_provider("relay", "https://api.example-relay.com"),
        false,
    )
    .expect_err("denied endpoint must be rejected");
    let msg = err.to_string();
    assert!(msg.contains("example-relay.com"), "{msg}");
    assert!(msg.contains("Acme IT"), "{msg}");
    assert!(state
        .db
        .get_provider_by_id("relay", AppType::Claude.as_str())
        .expect("query")
        .is_none());

    // An allowed provider saves, but editing it to a denied endpoint is refused.
    ProviderService::add(
        &state,
        AppType::Claude,
        claude_provider("ok", "https://api.anthropic.com"),
        false,
    )
    .expect("allowed endpoint");
    ProviderService::update(
        &state,
        AppType::Claude,
        None,
        claude_provider("ok", "https://x.example-relay.com/v1"),
    )
    .expect_err("update to a denied endpoint must be rejected");
}

#[test]
fn malformed_policy_is_ignored() {
    let env = TestEnv::new();
    write_user_policy(&env, "{ broken");
    let state = create_test_state().expect("state");
    ProviderService::add(
        &state,
        AppType::Claude,
        claude_provider("relay", "https://api.example-relay.com"),
        false,
    )
    .expect("an unreadable policy is not enforced");
}

#[test]
fn ccs_policy_reports_the_active_policy() {
    let env = TestEnv::new();
    let run = |args: &[&str]| {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = cc_switch_lib::cli::run(
            std::iter::once("ccs").chain(args.iter().copied()),
            &mut out,
            &mut err,
        );
        cc_switch_lib::cli::config_dir::reset();
        (code, String::from_utf8_lossy(&out).into_owned())
    };

    let (code, text) = run(&["policy"]);
    assert_eq!(code, 0);
    assert!(text.contains("No managed policy"), "{text}");

    write_user_policy(
        &env,
        r#"{"name":"Acme IT","endpoints":{"deny":["*.example-relay.com"]}}"#,
    );
    let (code, json) = run(&["--json", "policy"]);
    assert_eq!(code, 0);
    let value: serde_json::Value = serde_json::from_str(&json).expect("json output");
    assert_eq!(value["state"], "active");
    assert_eq!(value["policy"]["name"], "Acme IT");
}
