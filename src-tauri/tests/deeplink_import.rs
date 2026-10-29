use std::sync::Arc;

use base64::prelude::*;
use cc_switch_lib::{
    import_provider_from_deeplink, parse_deeplink_url, AppState, AppType, Database, ProviderService,
};

#[path = "support.rs"]
mod support;
use support::TestEnv;

#[test]
fn deeplink_import_claude_provider_persists_to_db() {
    let _test_env = TestEnv::new();

    let url = "ccswitch://v1/import?resource=provider&app=claude&name=DeepLink%20Claude&homepage=https%3A%2F%2Fexample.com&endpoint=https%3A%2F%2Fapi.example.com%2Fv1&apiKey=sk-test-claude-key&model=claude-sonnet-4&icon=claude";
    let request = parse_deeplink_url(url).expect("parse deeplink url");

    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db.clone());

    let provider_id = import_provider_from_deeplink(&state, request.clone())
        .expect("import provider from deeplink");

    // Verify DB state
    let providers = db.get_all_providers("claude").expect("get providers");
    let provider = providers
        .get(&provider_id)
        .expect("provider created via deeplink");

    assert_eq!(provider.name, request.name.clone().unwrap());
    assert_eq!(provider.website_url.as_deref(), request.homepage.as_deref());
    assert_eq!(provider.icon.as_deref(), Some("claude"));
    let auth_token = provider
        .settings_config
        .pointer("/env/ANTHROPIC_AUTH_TOKEN")
        .and_then(|v| v.as_str());
    let base_url = provider
        .settings_config
        .pointer("/env/ANTHROPIC_BASE_URL")
        .and_then(|v| v.as_str());
    assert_eq!(auth_token, request.api_key.as_deref());
    assert_eq!(base_url, request.endpoint.as_deref());
}

#[test]
fn deeplink_import_codex_provider_builds_auth_and_config() {
    let _test_env = TestEnv::new();

    let url = "ccswitch://v1/import?resource=provider&app=codex&name=DeepLink%20Codex&homepage=https%3A%2F%2Fopenai.example&endpoint=https%3A%2F%2Fapi.openai.example%2Fv1&apiKey=sk-test-codex-key&model=gpt-4o&icon=openai";
    let request = parse_deeplink_url(url).expect("parse deeplink url");

    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db.clone());

    let provider_id = import_provider_from_deeplink(&state, request.clone())
        .expect("import provider from deeplink");

    let providers = db.get_all_providers("codex").expect("get providers");
    let provider = providers
        .get(&provider_id)
        .expect("provider created via deeplink");

    assert_eq!(provider.name, request.name.clone().unwrap());
    assert_eq!(provider.website_url.as_deref(), request.homepage.as_deref());
    assert_eq!(provider.icon.as_deref(), Some("openai"));
    let auth_value = provider
        .settings_config
        .pointer("/auth/OPENAI_API_KEY")
        .and_then(|v| v.as_str());
    let config_text = provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert_eq!(auth_value, request.api_key.as_deref());
    assert!(
        config_text.contains(request.endpoint.as_deref().unwrap()),
        "config.toml content should contain endpoint"
    );
    assert!(
        config_text.contains("model = \"gpt-4o\""),
        "config.toml content should contain model setting"
    );
}

fn claude_deeplink_with_env(name: &str, env_json: &str) -> String {
    let config = BASE64_STANDARD.encode(format!(r#"{{"env":{env_json}}}"#));
    let config: String = url::form_urlencoded::byte_serialize(config.as_bytes()).collect();
    format!(
        "ccswitch://v1/import?resource=provider&app=claude&name={name}&homepage=https%3A%2F%2Fexample.com&endpoint=https%3A%2F%2Fapi.example.com&apiKey=sk-test&enabled=true&config={config}"
    )
}

#[test]
fn deeplink_import_with_unlisted_env_keys_does_not_auto_switch() {
    let _test_env = TestEnv::new();

    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db.clone());

    // Allow-listed keys only: enabled=true switches to the new provider.
    let first = parse_deeplink_url(&claude_deeplink_with_env(
        "Trusted",
        r#"{"API_TIMEOUT_MS":"600000","NODE_OPTIONS":"--require /tmp/x.js"}"#,
    ))
    .expect("parse deeplink url");
    let first_id = import_provider_from_deeplink(&state, first).expect("import trusted provider");
    assert_eq!(
        ProviderService::current(&state, AppType::Claude).unwrap(),
        first_id
    );

    // An unlisted key keeps the provider imported but not current.
    let second = parse_deeplink_url(&claude_deeplink_with_env(
        "Flagged",
        r#"{"SOME_OTHER_VAR":"1"}"#,
    ))
    .expect("parse deeplink url");
    let second_id = import_provider_from_deeplink(&state, second).expect("import flagged provider");

    let providers = db.get_all_providers("claude").expect("get providers");
    assert!(providers.contains_key(&second_id));
    assert!(providers[&first_id]
        .settings_config
        .pointer("/env/NODE_OPTIONS")
        .is_none());
    assert_eq!(
        ProviderService::current(&state, AppType::Claude).unwrap(),
        first_id
    );
}

#[test]
fn deeplink_import_with_unlisted_env_keys_does_not_become_first_current_provider() {
    let _test_env = TestEnv::new();

    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db.clone());

    // No current provider yet: the import must not take the "first provider
    // becomes current" path and write the unlisted key to the live config.
    let request = parse_deeplink_url(&claude_deeplink_with_env(
        "Flagged",
        r#"{"SOME_OTHER_VAR":"1"}"#,
    ))
    .expect("parse deeplink url");
    let id = import_provider_from_deeplink(&state, request).expect("import flagged provider");

    assert!(db
        .get_all_providers("claude")
        .expect("get providers")
        .contains_key(&id));
    assert_eq!(
        ProviderService::current(&state, AppType::Claude).unwrap(),
        ""
    );
    assert!(
        !cc_switch_lib::get_claude_settings_path().exists(),
        "live Claude settings must not be written"
    );
}
