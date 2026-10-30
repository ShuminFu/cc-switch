use std::env;
use std::fs;
use std::sync::Arc;

use serde_json::json;
use serial_test::serial;
use tempfile::TempDir;

use super::*;
use crate::config::get_claude_settings_path;
use crate::database::Database;
use crate::provider::{Provider, ProviderMeta};

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
        crate::settings::reload_settings().expect("reload settings");

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
        for (key, value) in [
            ("HOME", &self.original_home),
            ("USERPROFILE", &self.original_userprofile),
            ("CC_SWITCH_TEST_HOME", &self.original_test_home),
        ] {
            match value {
                Some(value) => env::set_var(key, value),
                None => env::remove_var(key),
            }
        }
    }
}

fn claude_provider(id: &str) -> Provider {
    Provider::with_id(
        id.to_string(),
        id.to_uppercase(),
        json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": format!("key-{id}"),
                "ANTHROPIC_BASE_URL": format!("https://{id}.example.com")
            }
        }),
        None,
    )
}

fn codex_provider(id: &str) -> Provider {
    Provider::with_id(
        id.to_string(),
        id.to_uppercase(),
        json!({
            "auth": { "OPENAI_API_KEY": format!("token-{id}") },
            "config": format!(
                "model_provider = \"custom\"\nmodel = \"gpt-5\"\n\n[model_providers.custom]\nname = \"{id}\"\nbase_url = \"https://{id}.example/v1\"\nwire_api = \"responses\"\nrequires_openai_auth = true\n"
            )
        }),
        None,
    )
}

/// 建库、保存供应商并把 `current` 设为当前供应商（live 不写，由用例自行布置）
fn seed(app: &AppType, providers: &[Provider], current: &str) -> AppState {
    let db = Arc::new(Database::memory().expect("init db"));
    for provider in providers {
        db.save_provider(app.as_str(), provider)
            .expect("save provider");
    }
    db.set_current_provider(app.as_str(), current)
        .expect("set db current provider");
    crate::settings::set_current_provider(app, Some(current)).expect("set local current provider");
    AppState::new(db)
}

fn journal_len(app: &AppType) -> usize {
    list_entries(&journal_dir(app)).expect("list journal").len()
}

/// 用户手写的 live（非 CC Switch 的输出格式），重新写一遍 A 不可能得到同样的字节
const CLAUDE_LIVE_A: &str = "{ \"env\": {\"ANTHROPIC_AUTH_TOKEN\":\"key-a\",   \"ANTHROPIC_BASE_URL\": \"https://a.example.com\"},\n  \"theme\":\"dark\" }\n";

#[test]
#[serial]
fn journal_switch_then_revert_restores_claude_live_byte_for_byte() {
    let _home = TempHome::new();
    let app = AppType::Claude;
    let state = seed(&app, &[claude_provider("a"), claude_provider("b")], "a");
    let live_path = get_claude_settings_path();
    fs::create_dir_all(live_path.parent().unwrap()).unwrap();
    fs::write(&live_path, CLAUDE_LIVE_A).unwrap();

    let result = ProviderService::switch(&state, app.clone(), "b").expect("switch to b");
    assert!(result.undoable);
    let switched = fs::read_to_string(&live_path).unwrap();
    assert!(switched.contains("key-b"), "{switched}");
    assert_eq!(journal_len(&app), 1);

    let reverted = ProviderService::revert_last_switch(&state, app.clone()).expect("revert");
    assert_eq!(reverted.provider_id.as_deref(), Some("a"));
    assert!(reverted
        .restored_files
        .contains(&live_path.display().to_string()));

    assert_eq!(fs::read(&live_path).unwrap(), CLAUDE_LIVE_A.as_bytes());
    assert_eq!(
        crate::settings::get_current_provider(&app).as_deref(),
        Some("a")
    );
    assert_eq!(
        state
            .db
            .get_current_provider(app.as_str())
            .unwrap()
            .as_deref(),
        Some("a")
    );
    assert_eq!(journal_len(&app), 0, "the reverted entry is consumed");

    let err = ProviderService::revert_last_switch(&state, app).expect_err("nothing left");
    assert!(matches!(
        err,
        AppError::Localized {
            key: "switch.nothingToRevert",
            ..
        }
    ));
}

#[test]
#[serial]
fn journal_revert_restores_every_codex_file_and_removes_created_ones() {
    let _home = TempHome::new();
    let app = AppType::Codex;
    let state = seed(&app, &[codex_provider("a"), codex_provider("b")], "a");
    let config_path = crate::codex_config::get_codex_config_path();
    let auth_path = crate::codex_config::get_codex_auth_path();
    fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    // A 只有 config.toml（带用户注释），auth.json 尚不存在
    let config_a = "# my codex\nmodel_provider = \"custom\"\nmodel = \"gpt-5\"\n\n[model_providers.custom]\nname = \"a\"\nbase_url = \"https://a.example/v1\"\nwire_api = \"responses\"\nrequires_openai_auth = true\n";
    fs::write(&config_path, config_a).unwrap();
    assert!(!auth_path.exists());

    ProviderService::switch(&state, app.clone(), "b").expect("switch to b");
    assert!(auth_path.exists(), "switch writes auth.json");

    ProviderService::revert_last_switch(&state, app.clone()).expect("revert");
    assert_eq!(fs::read(&config_path).unwrap(), config_a.as_bytes());
    assert!(
        !auth_path.exists(),
        "a file created by the switch is removed"
    );
    assert_eq!(
        crate::settings::get_current_provider(&app).as_deref(),
        Some("a")
    );
}

#[test]
#[serial]
fn journal_is_bounded_and_reverts_newest_first() {
    let _home = TempHome::new();
    let app = AppType::Claude;
    let state = seed(&app, &[claude_provider("a"), claude_provider("b")], "a");
    let live_path = get_claude_settings_path();
    fs::create_dir_all(live_path.parent().unwrap()).unwrap();
    fs::write(&live_path, CLAUDE_LIVE_A).unwrap();

    let mut lives = Vec::new();
    for i in 0..JOURNAL_LIMIT + 5 {
        lives.push(fs::read(&live_path).unwrap());
        let target = if i % 2 == 0 { "b" } else { "a" };
        ProviderService::switch(&state, app.clone(), target).expect("switch");
    }
    assert_eq!(journal_len(&app), JOURNAL_LIMIT);

    // 依次撤销：每次还原到对应切换之前的字节，直到日志耗尽
    for _ in 0..JOURNAL_LIMIT {
        ProviderService::revert_last_switch(&state, app.clone()).expect("revert");
        assert_eq!(fs::read(&live_path).unwrap(), lives.pop().unwrap());
    }
    assert_eq!(journal_len(&app), 0);
    assert!(ProviderService::revert_last_switch(&state, app).is_err());
}

#[cfg(unix)]
#[test]
#[serial]
fn journal_entries_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let _home = TempHome::new();
    let app = AppType::Claude;
    let state = seed(&app, &[claude_provider("a"), claude_provider("b")], "a");
    ProviderService::switch(&state, app.clone(), "b").expect("switch to b");

    let dir = journal_dir(&app);
    assert_eq!(
        fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let entries = list_entries(&dir).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        fs::metadata(&entries[0]).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
#[serial]
fn journal_revert_refuses_when_current_provider_changed_since() {
    let _home = TempHome::new();
    let app = AppType::Claude;
    let state = seed(
        &app,
        &[
            claude_provider("a"),
            claude_provider("b"),
            claude_provider("c"),
        ],
        "a",
    );
    ProviderService::switch(&state, app.clone(), "b").expect("switch to b");
    // 绕过切换直接改当前供应商：最近一条日志已过期
    crate::settings::set_current_provider(&app, Some("c")).unwrap();

    let err = ProviderService::revert_last_switch(&state, app.clone()).expect_err("stale");
    assert!(matches!(
        err,
        AppError::Localized {
            key: "switch.revertStale",
            ..
        }
    ));
    assert_eq!(journal_len(&app), 1, "a refused revert keeps the entry");
}

#[test]
#[serial]
fn journal_revert_refuses_when_previous_provider_was_deleted() {
    let _home = TempHome::new();
    let app = AppType::Claude;
    let state = seed(&app, &[claude_provider("a"), claude_provider("b")], "a");
    let live_path = get_claude_settings_path();
    fs::create_dir_all(live_path.parent().unwrap()).unwrap();
    fs::write(&live_path, CLAUDE_LIVE_A).unwrap();

    ProviderService::switch(&state, app.clone(), "b").expect("switch to b");
    state.db.delete_provider(app.as_str(), "a").unwrap();
    let switched = fs::read(&live_path).unwrap();

    let err = ProviderService::revert_last_switch(&state, app.clone()).expect_err("missing");
    assert!(matches!(
        err,
        AppError::Localized {
            key: "switch.revertPreviousMissing",
            ..
        }
    ));
    assert_eq!(
        fs::read(&live_path).unwrap(),
        switched,
        "live left untouched"
    );
    assert_eq!(
        crate::settings::get_effective_current_provider(&state.db, &app)
            .unwrap()
            .as_deref(),
        Some("b")
    );
    assert_eq!(journal_len(&app), 1, "a refused revert keeps the entry");
}

#[test]
#[serial]
fn journal_skips_switch_without_previous_current_provider() {
    let _home = TempHome::new();
    let app = AppType::Claude;
    let db = Arc::new(Database::memory().expect("init db"));
    db.save_provider(app.as_str(), &claude_provider("b"))
        .expect("save provider");
    crate::settings::set_current_provider(&app, None).unwrap();
    let state = AppState::new(db);

    let result = ProviderService::switch(&state, app.clone(), "b").expect("switch to b");
    assert!(
        !result.undoable,
        "nothing to restore the current provider to"
    );
    assert_eq!(journal_len(&app), 0);
}

#[test]
#[serial]
fn journal_revert_restores_additive_live_and_managed_flag() {
    let _home = TempHome::new();
    let app = AppType::OpenCode;
    let mut plain = Provider::with_id(
        "plain".into(),
        "Plain".into(),
        json!({
            "npm": "@ai-sdk/openai-compatible",
            "options": { "baseURL": "https://api.plain.example/v1", "apiKey": "sk-plain" },
            "models": {}
        }),
        None,
    );
    plain.meta = Some(ProviderMeta {
        live_config_managed: Some(false),
        ..Default::default()
    });
    let db = Arc::new(Database::memory().expect("init db"));
    db.save_provider(app.as_str(), &plain).unwrap();
    let state = AppState::new(db);

    let live_path = crate::opencode_config::get_opencode_config_path();
    fs::create_dir_all(live_path.parent().unwrap()).unwrap();
    let live_before =
        "{\n  \"$schema\": \"https://opencode.ai/config.json\", \"provider\": {}\n}\n";
    fs::write(&live_path, live_before).unwrap();

    ProviderService::switch(&state, app.clone(), "plain").expect("add plain to live");
    assert!(fs::read_to_string(&live_path).unwrap().contains("sk-plain"));

    let reverted = ProviderService::revert_last_switch(&state, app.clone()).expect("revert");
    assert_eq!(reverted.provider_id, None);
    assert_eq!(fs::read(&live_path).unwrap(), live_before.as_bytes());
    let stored = state
        .db
        .get_provider_by_id("plain", app.as_str())
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.meta.and_then(|meta| meta.live_config_managed),
        Some(false)
    );
}
