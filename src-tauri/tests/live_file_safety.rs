//! Live config files must survive provider switches: unparsable Gemini
//! `settings.json` is never replaced, and Claude/Codex/Gemini live files are
//! backed up under `~/.cc-switch/backups/<app>/` before being overwritten.

use std::fs;
use std::path::Path;

use serde_json::json;

use cc_switch_lib::{AppType, MultiAppConfig, Provider, ProviderService};

#[path = "support.rs"]
mod support;
use support::{create_test_state_with_config, ensure_test_home, TestEnv};

fn codex_config_with(providers: Vec<Provider>, current: &str) -> MultiAppConfig {
    let mut config = MultiAppConfig::default();
    let manager = config
        .get_manager_mut(&AppType::Codex)
        .expect("codex manager");
    manager.current = current.to_string();
    for p in providers {
        manager.providers.insert(p.id.clone(), p);
    }
    config
}

fn codex_provider(id: &str, category: &str, auth: serde_json::Value, config: &str) -> Provider {
    let mut p = Provider::with_id(
        id.to_string(),
        id.to_string(),
        json!({ "auth": auth, "config": config }),
        None,
    );
    p.category = Some(category.to_string());
    p
}

/// CFG-38: with default settings a ChatGPT login in auth.json survives a
/// round trip official -> third-party -> official.
#[test]
fn codex_default_switch_keeps_chatgpt_login_in_auth_json() {
    let _env = TestEnv::new();
    let home = ensure_test_home();
    let codex_dir = home.join(".codex");
    fs::create_dir_all(&codex_dir).unwrap();
    let oauth = r#"{"auth_mode":"chatgpt","tokens":{"access_token":"at","refresh_token":"rt"},"last_refresh":"2026-01-01T00:00:00Z"}"#;
    fs::write(codex_dir.join("auth.json"), oauth).unwrap();
    fs::write(codex_dir.join("config.toml"), "model = \"gpt-5\"\n").unwrap();

    let official = codex_provider(
        "official",
        "official",
        serde_json::from_str(oauth).unwrap(),
        "model = \"gpt-5\"\n",
    );
    let third = codex_provider(
        "third",
        "custom",
        json!({ "OPENAI_API_KEY": "sk-third" }),
        "model_provider = \"custom\"\nmodel = \"m\"\n\n[model_providers.custom]\nname = \"custom\"\nbase_url = \"https://third.example.com/v1\"\nwire_api = \"responses\"\n",
    );
    let config = codex_config_with(vec![official, third], "official");
    let state = create_test_state_with_config(&config).expect("state");

    ProviderService::switch(&state, AppType::Codex, "third").expect("switch to third");
    assert_eq!(
        fs::read_to_string(codex_dir.join("auth.json")).unwrap(),
        oauth,
        "third-party switch must not touch the ChatGPT login"
    );
    let live_config = fs::read_to_string(codex_dir.join("config.toml")).unwrap();
    assert!(live_config.contains("sk-third"), "{live_config}");

    ProviderService::switch(&state, AppType::Codex, "official").expect("switch back");
    let auth: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(codex_dir.join("auth.json")).unwrap()).unwrap();
    assert_eq!(auth["tokens"]["refresh_token"], "rt");
    assert!(auth.get("OPENAI_API_KEY").is_none());
}

/// CFG-38: switching to an official provider without stored login material
/// never leaves a third-party key in auth.json.
#[test]
fn codex_official_switch_strips_leftover_third_party_key() {
    let _env = TestEnv::new();
    let home = ensure_test_home();
    let codex_dir = home.join(".codex");
    fs::create_dir_all(&codex_dir).unwrap();
    fs::write(
        codex_dir.join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-third"}"#,
    )
    .unwrap();
    fs::write(codex_dir.join("config.toml"), "model = \"m\"\n").unwrap();

    let official = codex_provider("official", "official", json!({}), "model = \"gpt-5\"\n");
    let third = codex_provider(
        "third",
        "custom",
        json!({ "OPENAI_API_KEY": "sk-third" }),
        "model = \"m\"\n",
    );
    let config = codex_config_with(vec![official, third], "third");
    let state = create_test_state_with_config(&config).expect("state");

    ProviderService::switch(&state, AppType::Codex, "official").expect("switch");
    let raw = fs::read_to_string(codex_dir.join("auth.json")).unwrap();
    assert!(!raw.contains("sk-third"), "{raw}");
}

fn backups_in(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = match fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| {
                (
                    e.file_name().to_string_lossy().to_string(),
                    fs::read(e.path()).expect("read backup"),
                )
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    out.sort();
    out
}

fn gemini_config(providers: Vec<Provider>, current: &str) -> MultiAppConfig {
    let mut config = MultiAppConfig::default();
    let manager = config
        .get_manager_mut(&AppType::Gemini)
        .expect("gemini manager");
    manager.current = current.to_string();
    for p in providers {
        manager.providers.insert(p.id.clone(), p);
    }
    config
}

fn gemini_provider(id: &str, with_config: bool) -> Provider {
    let mut settings = json!({
        "env": {
            "GEMINI_API_KEY": format!("{id}-key"),
            "GOOGLE_GEMINI_BASE_URL": "https://example.com"
        }
    });
    if with_config {
        settings["config"] = json!({});
    }
    Provider::with_id(id.to_string(), id.to_string(), settings, None)
}

#[test]
fn gemini_switch_fails_and_keeps_unparsable_settings_json() {
    let _env = TestEnv::new();
    let home = ensure_test_home();

    let gemini_dir = home.join(".gemini");
    fs::create_dir_all(&gemini_dir).unwrap();
    let settings_path = gemini_dir.join("settings.json");
    fs::write(&settings_path, "{").unwrap();

    for with_config in [true, false] {
        let config = gemini_config(vec![gemini_provider("target", with_config)], "target");
        let state = create_test_state_with_config(&config).expect("create test state");

        let err = ProviderService::switch(&state, AppType::Gemini, "target")
            .expect_err("switch must fail when settings.json cannot be parsed");
        assert!(
            err.to_string().contains("settings.json"),
            "error should name the file: {err}"
        );
        assert_eq!(
            fs::read_to_string(&settings_path).unwrap(),
            "{",
            "unparsable settings.json must be left untouched (with_config={with_config})"
        );
    }
}

#[test]
fn gemini_switch_preserves_commented_settings_and_backs_up_original() {
    let _env = TestEnv::new();
    let home = ensure_test_home();

    let gemini_dir = home.join(".gemini");
    fs::create_dir_all(&gemini_dir).unwrap();
    let settings_path = gemini_dir.join("settings.json");
    let original = r#"// user comment
{
  "mcpServers": { "foo": { "command": "foo-bin" } },
  "ui": { "theme": "GitHub" }
}
"#;
    fs::write(&settings_path, original).unwrap();

    let config = gemini_config(vec![gemini_provider("target", true)], "target");
    let state = create_test_state_with_config(&config).expect("create test state");

    ProviderService::switch(&state, AppType::Gemini, "target").expect("switch should succeed");

    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
    assert_eq!(
        value.pointer("/ui/theme").and_then(|v| v.as_str()),
        Some("GitHub"),
        "user settings must survive: {value}"
    );
    assert_eq!(
        value
            .pointer("/security/auth/selectedType")
            .and_then(|v| v.as_str()),
        Some("gemini-api-key")
    );

    let backups = backups_in(&home.join(".cc-switch").join("backups").join("gemini"));
    assert!(
        backups
            .iter()
            .any(|(name, bytes)| name.starts_with("settings_") && bytes == original.as_bytes()),
        "original settings.json (with comments) must be backed up: {:?}",
        backups.iter().map(|(n, _)| n).collect::<Vec<_>>()
    );
}

#[test]
fn claude_switch_backs_up_live_settings_and_warns_when_unreadable() {
    let _env = TestEnv::new();
    let home = ensure_test_home();

    let claude_dir = home.join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    let settings_path = claude_dir.join("settings.json");
    // Hand-edited file with a trailing comma: unreadable for backfill.
    let original = "{ \"env\": { \"ANTHROPIC_API_KEY\": \"a-key\" }, \"hooks\": {}, }";
    fs::write(&settings_path, original).unwrap();

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Claude)
            .expect("claude manager");
        manager.current = "a".to_string();
        for id in ["a", "b"] {
            manager.providers.insert(
                id.to_string(),
                Provider::with_id(
                    id.to_string(),
                    id.to_uppercase(),
                    json!({ "env": { "ANTHROPIC_API_KEY": format!("{id}-key") } }),
                    None,
                ),
            );
        }
    }
    let state = create_test_state_with_config(&config).expect("create test state");

    let result =
        ProviderService::switch(&state, AppType::Claude, "b").expect("switch should succeed");
    assert!(
        result.warnings.iter().any(|w| w == "backfill_skipped:a"),
        "unreadable live file must surface a backfill warning: {:?}",
        result.warnings
    );

    let backups = backups_in(&home.join(".cc-switch").join("backups").join("claude"));
    assert!(
        backups
            .iter()
            .any(|(name, bytes)| name.starts_with("settings_") && bytes == original.as_bytes()),
        "previous Claude settings.json must be backed up"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for (name, _) in &backups {
            let path = home
                .join(".cc-switch")
                .join("backups")
                .join("claude")
                .join(name);
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600,
                "backups hold secrets and must be 0600"
            );
        }
    }
}

#[test]
fn codex_switch_backs_up_auth_and_config() {
    let _env = TestEnv::new();
    let home = ensure_test_home();
    // compat mode: third-party switches replace auth.json
    cc_switch_lib::update_settings(cc_switch_lib::AppSettings {
        preserve_codex_official_auth_on_switch: false,
        ..Default::default()
    })
    .unwrap();

    let codex_dir = home.join(".codex");
    fs::create_dir_all(&codex_dir).unwrap();
    let auth_original = r#"{"OPENAI_API_KEY":"old-key"}"#;
    let config_original = "model = \"old-model\"\n";
    fs::write(codex_dir.join("auth.json"), auth_original).unwrap();
    fs::write(codex_dir.join("config.toml"), config_original).unwrap();

    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Codex)
            .expect("codex manager");
        manager.current = "target".to_string();
        manager.providers.insert(
            "target".to_string(),
            Provider::with_id(
                "target".to_string(),
                "Target".to_string(),
                json!({
                    "auth": { "OPENAI_API_KEY": "new-key" },
                    "config": "model = \"new-model\"\n"
                }),
                None,
            ),
        );
    }
    let state = create_test_state_with_config(&config).expect("create test state");

    ProviderService::switch(&state, AppType::Codex, "target").expect("switch should succeed");

    let backups = backups_in(&home.join(".cc-switch").join("backups").join("codex"));
    assert!(
        backups
            .iter()
            .any(|(name, bytes)| name.starts_with("config_") && bytes == config_original.as_bytes()),
        "previous config.toml must be backed up: {:?}",
        backups.iter().map(|(n, _)| n).collect::<Vec<_>>()
    );
    assert!(
        backups
            .iter()
            .any(|(name, bytes)| name.starts_with("auth_") && bytes == auth_original.as_bytes()),
        "previous auth.json must be backed up"
    );
}
