//! Command-layer tests (TEST-03 / CMD-31) and guards for the shared test
//! harness (TEST-08).
//!
//! The commands below are called exactly as `generate_handler!` exposes them
//! (or through the same code path when the command needs `tauri::State`), and
//! the IPC payloads are deserialised from the JSON the frontend sends, so a
//! renamed argument field or a changed response shape fails here instead of
//! only in the running app.
//!
//! These tests live in one binary on purpose: every integration-test binary
//! links the whole crate (~0.5 GB in debug).

use cc_switch_lib::{
    get_claude_settings_path, get_settings, list_db_backups, merge_deeplink_config, parse_deeplink,
    read_json_file, switch_provider_test_hook, AppSettings, AppType, DeepLinkImportRequest,
    MultiAppConfig, Prompt, PromptService, Provider, ProviderService,
};
use serde_json::json;

#[path = "support.rs"]
mod support;
use support::{
    create_disk_test_state, create_test_state, create_test_state_with_config, ensure_test_home,
    test_mutex, TestEnv,
};

// ---------------------------------------------------------------------------
// Harness (TEST-08)
// ---------------------------------------------------------------------------

#[test]
fn harness_panicking_test_does_not_poison_later_tests() {
    // Simulate a test that panics while holding the environment lock.
    let crashed = std::thread::spawn(|| {
        let env = TestEnv::new();
        std::fs::write(env.home().join("leftover.txt"), "x").unwrap();
        panic!("deliberate failure inside a TestEnv");
    })
    .join();
    assert!(crashed.is_err(), "the helper thread must have panicked");

    // Both entry points still work and hand out a clean home.
    let env = TestEnv::new();
    assert!(!env.home().join("leftover.txt").exists());
    drop(env);
    let legacy = test_mutex().lock().expect("legacy lock never poisons");
    assert!(!legacy.home().join("leftover.txt").exists());
}

#[test]
fn harness_each_test_env_gets_its_own_empty_home() {
    let first_home = {
        let env = TestEnv::new();
        assert_eq!(ensure_test_home(), env.home());
        assert_eq!(
            std::env::var_os("CC_SWITCH_TEST_HOME").as_deref(),
            Some(env.home().as_os_str())
        );
        std::fs::create_dir_all(env.home().join(".claude")).unwrap();
        env.home().to_path_buf()
    };
    assert!(
        !first_home.exists(),
        "the per-test home must be removed when the TestEnv is dropped"
    );

    let env = TestEnv::new();
    assert_ne!(env.home(), first_home.as_path());
    // Only the settings file written by the settings reset may exist.
    let entries: Vec<String> = std::fs::read_dir(env.home())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name != ".cc-switch")
        .collect();
    assert!(entries.is_empty(), "fresh home is not empty: {entries:?}");
}

#[test]
fn harness_test_state_uses_an_in_memory_database() {
    let env = TestEnv::new();
    let db_file = env.home().join(".cc-switch").join("cc-switch.db");
    let _state = create_test_state().expect("create state");
    assert!(
        !db_file.exists(),
        "create_test_state must not create a database file"
    );
    let _disk = create_disk_test_state().expect("create disk state");
    assert!(db_file.exists());
}

// ---------------------------------------------------------------------------
// switch_provider
// ---------------------------------------------------------------------------

fn claude_provider(id: &str, token: &str) -> Provider {
    Provider::with_id(
        id.to_string(),
        id.to_string(),
        json!({ "env": { "ANTHROPIC_AUTH_TOKEN": token, "ANTHROPIC_BASE_URL": "https://api.test" } }),
        None,
    )
}

#[test]
fn switch_provider_writes_live_config_and_returns_frontend_shape() {
    let _env = TestEnv::new();
    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Claude)
            .expect("claude manager");
        manager.current = "a".to_string();
        manager
            .providers
            .insert("a".to_string(), claude_provider("a", "token-a"));
        manager
            .providers
            .insert("b".to_string(), claude_provider("b", "token-b"));
    }
    let state = create_test_state_with_config(&config).expect("create state");

    let result = switch_provider_test_hook(&state, AppType::Claude, "b").expect("switch to b");

    // src/lib/api/providers.ts: `interface SwitchResult { warnings: string[] }`
    let wire = serde_json::to_value(&result).expect("serialize SwitchResult");
    assert!(wire.get("warnings").and_then(|w| w.as_array()).is_some());
    assert_eq!(wire.as_object().map(|o| o.len()), Some(1), "{wire}");

    let live: serde_json::Value =
        read_json_file(&get_claude_settings_path()).expect("read live settings");
    assert_eq!(
        live.pointer("/env/ANTHROPIC_AUTH_TOKEN")
            .and_then(|v| v.as_str()),
        Some("token-b")
    );
    assert_eq!(
        state
            .db
            .get_current_provider(AppType::Claude.as_str())
            .expect("read current")
            .as_deref(),
        Some("b")
    );
}

#[test]
fn switch_provider_rejects_unknown_app_and_missing_provider() {
    let _env = TestEnv::new();
    let state = create_test_state().expect("create state");

    // The command parses `app` with AppType::from_str before doing anything.
    assert!("not-an-app".parse::<AppType>().is_err());

    let err = switch_provider_test_hook(&state, AppType::Claude, "missing")
        .expect_err("switching to a missing provider must fail");
    assert!(!err.to_string().is_empty());
    assert!(
        !get_claude_settings_path().exists(),
        "a failed switch must not create a live config"
    );
}

// ---------------------------------------------------------------------------
// settings
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_settings_command_returns_camel_case_payload() {
    let _env = TestEnv::new();
    let settings = get_settings().await.expect("get_settings");
    let wire = serde_json::to_value(&settings).expect("serialize settings");
    let obj = wire.as_object().expect("settings object");
    assert!(
        obj.keys().all(|k| !k.contains('_')),
        "settings keys must be camelCase for the frontend: {:?}",
        obj.keys().filter(|k| k.contains('_')).collect::<Vec<_>>()
    );
}

#[test]
fn save_settings_payload_from_frontend_deserialises() {
    let _env = TestEnv::new();
    // Shape sent by src/lib/api/settings.ts `invoke("save_settings", { settings })`.
    let payload = json!({
        "showInTray": true,
        "minimizeToTrayOnClose": false,
        "language": "en",
        "preserveCodexOfficialAuthOnSwitch": true,
    });
    let parsed: AppSettings = serde_json::from_value(payload).expect("deserialize AppSettings");
    assert!(parsed.preserve_codex_official_auth_on_switch);
    assert_eq!(parsed.language.as_deref(), Some("en"));

    // And it round-trips through its own serialisation unchanged.
    let wire = serde_json::to_value(&parsed).expect("serialize");
    let again: AppSettings = serde_json::from_value(wire.clone()).expect("re-deserialize");
    assert_eq!(serde_json::to_value(&again).unwrap(), wire);
}

// ---------------------------------------------------------------------------
// deep-link import
// ---------------------------------------------------------------------------

#[test]
fn parse_deeplink_command_maps_errors_to_strings() {
    let _env = TestEnv::new();
    let ok = parse_deeplink(
        "ccswitch://v1/import?resource=provider&app=claude&name=Demo&endpoint=https%3A%2F%2Fapi.example.com&apiKey=sk-demo"
            .to_string(),
    )
    .expect("valid deep link parses");
    assert_eq!(ok.resource, "provider");
    assert_eq!(ok.app.as_deref(), Some("claude"));

    // The frontend (DeepLinkImportDialog) receives this request as camelCase JSON.
    let wire = serde_json::to_value(&ok).expect("serialize request");
    assert_eq!(wire["apiKey"], "sk-demo");
    assert!(wire.get("api_key").is_none());

    let err = parse_deeplink("https://not-a-deeplink.example".to_string())
        .expect_err("non ccswitch URL must be rejected");
    assert!(!err.is_empty());
}

#[test]
fn merge_deeplink_config_accepts_frontend_request_shape() {
    let _env = TestEnv::new();
    let request: DeepLinkImportRequest = serde_json::from_value(json!({
        "version": "v1",
        "resource": "provider",
        "app": "claude",
        "name": "Demo",
        "endpoint": "https://api.example.com",
        "apiKey": "sk-demo",
    }))
    .expect("deserialize request");
    let merged = merge_deeplink_config(request).expect("merge without config is a no-op");
    assert_eq!(merged.api_key.as_deref(), Some("sk-demo"));
    assert_eq!(merged.endpoint.as_deref(), Some("https://api.example.com"));
}

// ---------------------------------------------------------------------------
// import / export / backup restore
// ---------------------------------------------------------------------------

fn provider_ids(state: &cc_switch_lib::AppState) -> Vec<String> {
    let mut ids: Vec<String> = state
        .db
        .get_all_providers(AppType::Claude.as_str())
        .expect("list providers")
        .keys()
        .cloned()
        .collect();
    ids.sort();
    ids
}

#[test]
fn import_creates_backup_that_list_and_restore_commands_can_use() {
    let env = TestEnv::new();
    let state = create_disk_test_state().expect("create disk state");

    assert!(tauri::async_runtime::block_on(list_db_backups())
        .expect("list backups")
        .is_empty());

    state
        .db
        .save_provider(AppType::Claude.as_str(), &claude_provider("before", "t1"))
        .expect("save before");
    let export_path = env.home().join("export.sql");
    state.db.export_sql(&export_path).expect("export");

    state
        .db
        .save_provider(AppType::Claude.as_str(), &claude_provider("after", "t2"))
        .expect("save after");
    assert_eq!(provider_ids(&state), vec!["after", "before"]);

    // import_config_from_file: import replaces the DB and returns the id of the
    // safety backup it took first.
    let backup_id = state.db.import_sql(&export_path).expect("import");
    assert_eq!(provider_ids(&state), vec!["before"]);

    // list_db_backups command sees that backup.
    let backups = tauri::async_runtime::block_on(list_db_backups()).expect("list backups");
    let entry = backups
        .iter()
        .find(|b| b.filename.contains(&backup_id))
        .unwrap_or_else(|| panic!("backup {backup_id} not listed in {backups:?}"));
    assert!(entry.size_bytes > 0);
    // src/lib/api/settings.ts `BackupEntry { filename, sizeBytes, createdAt }`
    let wire = serde_json::to_value(entry).expect("serialize BackupEntry");
    for key in ["filename", "sizeBytes", "createdAt"] {
        assert!(wire.get(key).is_some(), "missing {key} in {wire}");
    }

    // restore_db_backup: restoring brings the pre-import state back.
    state
        .db
        .restore_from_backup(&entry.filename)
        .expect("restore backup");
    assert_eq!(provider_ids(&state), vec!["after", "before"]);
}

#[test]
fn restore_rejects_unknown_and_path_traversal_filenames() {
    let _env = TestEnv::new();
    let state = create_disk_test_state().expect("create disk state");
    state
        .db
        .save_provider(AppType::Claude.as_str(), &claude_provider("keep", "t"))
        .expect("save");

    for name in ["does-not-exist.db", "../cc-switch.db", "../../etc/passwd"] {
        assert!(
            state.db.restore_from_backup(name).is_err(),
            "restore of {name:?} must fail"
        );
    }
    assert_eq!(
        provider_ids(&state),
        vec!["keep"],
        "live DB must be untouched"
    );
}

// ---------------------------------------------------------------------------
// proxy start / stop
// ---------------------------------------------------------------------------

#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn proxy_start_and_stop_on_an_ephemeral_port() {
    let _env = TestEnv::new();
    let state = create_test_state().expect("create state");

    // Port 0: never collide with a real cc-switch or a parallel test run.
    let mut config = state.db.get_proxy_config().await.expect("proxy config");
    config.listen_port = 0;
    state
        .db
        .update_proxy_config(config)
        .await
        .expect("set ephemeral port");

    // start_proxy_server
    let info = state.proxy_service.start().await.expect("start proxy");
    assert_ne!(info.port, 0, "the bound port must be reported");
    assert!(state.proxy_service.is_running().await);
    let status = state.proxy_service.get_status().await.expect("status");
    assert!(status.running);
    assert_eq!(status.port, info.port);

    // stop_proxy_server refuses while an app is taken over; nothing is here.
    let takeover = state
        .proxy_service
        .get_takeover_status()
        .await
        .expect("takeover status");
    assert!(!takeover.claude && !takeover.codex && !takeover.gemini);

    state.proxy_service.stop().await.expect("stop proxy");
    assert!(!state.proxy_service.is_running().await);
    // Stopping twice is harmless.
    let _ = state.proxy_service.stop().await;
}

// ---------------------------------------------------------------------------
// prompts: the service behind enable_prompt / upsert_prompt / delete_prompt
// (SVC-25: PromptService had no dedicated tests)
// ---------------------------------------------------------------------------

fn prompt(id: &str, content: &str, enabled: bool) -> Prompt {
    Prompt {
        id: id.to_string(),
        name: id.to_string(),
        content: content.to_string(),
        description: None,
        enabled,
        created_at: None,
        updated_at: None,
    }
}

fn claude_prompt_file(env: &TestEnv) -> std::path::PathBuf {
    env.home().join(".claude").join("CLAUDE.md")
}

fn enabled_prompts(state: &cc_switch_lib::AppState) -> Vec<String> {
    PromptService::get_prompts(state, AppType::Claude)
        .expect("list prompts")
        .into_iter()
        .filter(|(_, p)| p.enabled)
        .map(|(id, _)| id)
        .collect()
}

fn seed_prompts(state: &cc_switch_lib::AppState) {
    for (id, content) in [("a", "alpha"), ("b", "beta")] {
        PromptService::upsert_prompt(state, AppType::Claude, id, prompt(id, content, false))
            .expect("upsert prompt");
    }
}

#[test]
fn enable_prompt_writes_live_file_and_is_exclusive() {
    let env = TestEnv::new();
    let state = create_test_state().expect("create state");
    seed_prompts(&state);

    PromptService::enable_prompt(&state, AppType::Claude, "a").expect("enable a");
    assert_eq!(
        std::fs::read_to_string(claude_prompt_file(&env)).unwrap(),
        "alpha"
    );
    PromptService::enable_prompt(&state, AppType::Claude, "b").expect("enable b");
    assert_eq!(
        std::fs::read_to_string(claude_prompt_file(&env)).unwrap(),
        "beta"
    );
    assert_eq!(enabled_prompts(&state), vec!["b"]);
    assert_eq!(
        PromptService::get_current_file_content(AppType::Claude)
            .unwrap()
            .as_deref(),
        Some("beta")
    );
}

#[test]
fn enabling_another_prompt_backfills_live_edits_into_the_enabled_one() {
    let env = TestEnv::new();
    let state = create_test_state().expect("create state");
    seed_prompts(&state);
    PromptService::enable_prompt(&state, AppType::Claude, "a").expect("enable a");

    // The user edits CLAUDE.md by hand, then switches prompts.
    std::fs::write(claude_prompt_file(&env), "alpha, edited by hand").unwrap();
    PromptService::enable_prompt(&state, AppType::Claude, "b").expect("enable b");

    let prompts = PromptService::get_prompts(&state, AppType::Claude).unwrap();
    assert_eq!(prompts["a"].content, "alpha, edited by hand");
    assert!(!prompts["a"].enabled);
}

#[test]
fn enabling_with_unmanaged_live_content_creates_a_backup_prompt() {
    let env = TestEnv::new();
    let state = create_test_state().expect("create state");
    PromptService::upsert_prompt(&state, AppType::Claude, "a", prompt("a", "alpha", false))
        .expect("upsert");
    std::fs::create_dir_all(env.home().join(".claude")).unwrap();
    std::fs::write(claude_prompt_file(&env), "pre-existing user prompt").unwrap();

    PromptService::enable_prompt(&state, AppType::Claude, "a").expect("enable a");

    let prompts = PromptService::get_prompts(&state, AppType::Claude).unwrap();
    assert!(
        prompts
            .values()
            .any(|p| p.id.starts_with("backup-") && p.content == "pre-existing user prompt"),
        "the user's original prompt must be preserved: {prompts:?}"
    );
}

#[test]
fn saving_a_disabled_prompt_never_wipes_an_unmanaged_live_file() {
    let env = TestEnv::new();
    let state = create_test_state().expect("create state");
    std::fs::create_dir_all(env.home().join(".claude")).unwrap();
    std::fs::write(claude_prompt_file(&env), "user's own CLAUDE.md").unwrap();

    // First prompt added as disabled, then edited again while still disabled.
    PromptService::upsert_prompt(&state, AppType::Claude, "a", prompt("a", "alpha", false))
        .expect("upsert");
    PromptService::upsert_prompt(&state, AppType::Claude, "a", prompt("a", "alpha2", false))
        .expect("upsert again");
    // Importing the live file (saved as a disabled prompt) must not wipe it either.
    PromptService::import_from_file(&state, AppType::Claude).expect("import");

    assert_eq!(
        std::fs::read_to_string(claude_prompt_file(&env)).unwrap(),
        "user's own CLAUDE.md"
    );
}

#[test]
fn enable_prompt_failures_leave_the_previous_selection_intact() {
    let env = TestEnv::new();
    let state = create_test_state().expect("create state");
    seed_prompts(&state);
    PromptService::enable_prompt(&state, AppType::Claude, "a").expect("enable a");

    // Unknown id.
    PromptService::enable_prompt(&state, AppType::Claude, "missing")
        .expect_err("unknown prompt must be rejected");
    assert_eq!(enabled_prompts(&state), vec!["a"]);

    // Live write fails: CLAUDE.md is replaced by a directory.
    let file = claude_prompt_file(&env);
    std::fs::remove_file(&file).unwrap();
    std::fs::create_dir_all(&file).unwrap();
    PromptService::enable_prompt(&state, AppType::Claude, "b")
        .expect_err("live write failure must surface");
    assert_eq!(
        enabled_prompts(&state),
        vec!["a"],
        "a failed live write must not move the enabled flag"
    );
}

#[test]
fn enabled_prompt_cannot_be_deleted_and_disabling_the_last_one_clears_the_file() {
    let env = TestEnv::new();
    let state = create_test_state().expect("create state");
    PromptService::upsert_prompt(&state, AppType::Claude, "a", prompt("a", "alpha", false))
        .expect("upsert");
    PromptService::enable_prompt(&state, AppType::Claude, "a").expect("enable a");

    PromptService::delete_prompt(&state, AppType::Claude, "a")
        .expect_err("enabled prompt must not be deletable");

    PromptService::upsert_prompt(&state, AppType::Claude, "a", prompt("a", "alpha", false))
        .expect("disable a");
    assert_eq!(
        std::fs::read_to_string(claude_prompt_file(&env)).unwrap(),
        ""
    );
    PromptService::delete_prompt(&state, AppType::Claude, "a").expect("delete disabled prompt");
    assert!(PromptService::get_prompts(&state, AppType::Claude)
        .unwrap()
        .is_empty());
}

// ---------------------------------------------------------------------------
// custom endpoints (SVC-25)
// ---------------------------------------------------------------------------

#[test]
fn custom_endpoints_are_normalised_added_used_and_removed() {
    let _env = TestEnv::new();
    let state = create_test_state().expect("create state");
    state
        .db
        .save_provider(AppType::Claude.as_str(), &claude_provider("p", "t"))
        .expect("save provider");

    ProviderService::add_custom_endpoint(&state, AppType::Claude, "p", "   ".to_string())
        .expect_err("blank URL is rejected");
    ProviderService::add_custom_endpoint(
        &state,
        AppType::Claude,
        "p",
        " https://relay.example.com/v1/ ".to_string(),
    )
    .expect("add endpoint");

    let endpoints =
        ProviderService::get_custom_endpoints(&state, AppType::Claude, "p").expect("list");
    assert_eq!(endpoints.len(), 1);
    assert_eq!(endpoints[0].url, "https://relay.example.com/v1");
    assert!(endpoints[0].last_used.is_none());

    // Marking an endpoint as used (by either spelling) must not fail or add a
    // duplicate. Note: `last_used` is not persisted today (provider_endpoints
    // has no such column), so it is not asserted here.
    ProviderService::update_endpoint_last_used(
        &state,
        AppType::Claude,
        "p",
        "https://relay.example.com/v1/".to_string(),
    )
    .expect("mark used");
    assert_eq!(
        ProviderService::get_custom_endpoints(&state, AppType::Claude, "p")
            .expect("list")
            .len(),
        1
    );

    ProviderService::remove_custom_endpoint(
        &state,
        AppType::Claude,
        "p",
        "https://relay.example.com/v1/".to_string(),
    )
    .expect("remove");
    assert!(
        ProviderService::get_custom_endpoints(&state, AppType::Claude, "p")
            .expect("list")
            .is_empty()
    );
}
