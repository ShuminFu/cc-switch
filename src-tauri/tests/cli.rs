//! Integration tests for the `ccs` CLI (PRODUCT-11): drive `cli::run` against a
//! per-test temp HOME and check it behaves like the GUI commands.

use serde_json::{json, Value};

use cc_switch_lib::cli::{self, lock};
use cc_switch_lib::{get_claude_settings_path, read_json_file, AppType, MultiAppConfig, Provider};

#[path = "support.rs"]
mod support;
use support::{create_disk_test_state_with_config, TestEnv};

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Run {
    fn json(&self) -> Value {
        serde_json::from_str(&self.stdout)
            .unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {}", self.stdout))
    }
}

fn ccs(args: &[&str]) -> Run {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = cli::run(
        std::iter::once("ccs").chain(args.iter().copied()),
        &mut out,
        &mut err,
    );
    // Each run installs the config-dir override for the process; clear it so
    // the next test starts from its own TestEnv home.
    cli::config_dir::reset();
    Run {
        code,
        stdout: String::from_utf8_lossy(&out).into_owned(),
        stderr: String::from_utf8_lossy(&err).into_owned(),
    }
}

fn claude_provider(id: &str, name: &str, token: &str) -> Provider {
    Provider::with_id(
        id.to_string(),
        name.to_string(),
        json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": token,
                "ANTHROPIC_BASE_URL": format!("https://{id}.example.com")
            }
        }),
        None,
    )
}

/// Seed `<home>/.cc-switch/cc-switch.db` with two Claude providers, "a" current.
fn seed_claude() {
    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Claude)
            .expect("claude manager");
        manager.current = "a".to_string();
        manager
            .providers
            .insert("a".into(), claude_provider("a", "Alpha", "token-a"));
        manager
            .providers
            .insert("b".into(), claude_provider("b", "Packy Code", "token-b"));
    }
    let state = create_disk_test_state_with_config(&config).expect("seed disk db");
    drop(state);
}

#[test]
fn list_and_current_report_seeded_providers() {
    let _env = TestEnv::new();
    seed_claude();

    let run = ccs(&["--json", "list", "claude"]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    let v = run.json();
    assert_eq!(v["app"], "claude");
    assert_eq!(v["current"], "a");
    let ids: Vec<&str> = v["providers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&"a") && ids.contains(&"b"));

    let run = ccs(&["current", "claude"]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    assert_eq!(run.stdout.trim(), "a (Alpha)");

    let run = ccs(&["--json", "current"]);
    assert_eq!(run.code, 0);
    let apps = run.json()["apps"].as_array().unwrap().clone();
    assert!(apps
        .iter()
        .any(|a| a["app"] == "claude" && a["provider"]["id"] == "a"));
    // Additive apps are omitted from the all-apps view.
    assert!(!apps.iter().any(|a| a["app"] == "opencode"));

    let run = ccs(&["ls"]);
    assert!(run.stdout.contains("* a"), "{}", run.stdout);
}

#[test]
fn switch_by_name_writes_live_settings_like_the_gui() {
    let _env = TestEnv::new();
    seed_claude();

    let run = ccs(&["--json", "switch", "claude", "packy code"]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    assert_eq!(run.json()["provider"]["id"], "b");

    let live: Value = read_json_file(&get_claude_settings_path()).expect("read live settings");
    assert_eq!(live["env"]["ANTHROPIC_AUTH_TOKEN"], "token-b");
    assert_eq!(live["env"]["ANTHROPIC_BASE_URL"], "https://b.example.com");

    let run = ccs(&["current", "claude"]);
    assert_eq!(run.stdout.trim(), "b (Packy Code)");

    let run = ccs(&["switch", "claude", "nope"]);
    assert_eq!(run.code, cli::EXIT_FAILED);
    assert!(run.stderr.contains("Available"), "{}", run.stderr);
}

#[test]
fn writes_are_refused_while_the_gui_holds_the_writer_lock() {
    let env = TestEnv::new();
    seed_claude();
    let config_dir = env.home().join(".cc-switch");
    let gui = lock::WriterLock::try_acquire(&config_dir, lock::LockOwner::Gui).expect("gui lock");

    let run = ccs(&["--json", "switch", "claude", "b"]);
    assert_eq!(run.code, cli::EXIT_LOCKED);
    assert!(run.stderr.contains("desktop app"), "{}", run.stderr);
    assert_eq!(run.json()["code"], "locked");
    assert_eq!(run.json()["lockedBy"]["owner"], "gui");

    // Live file untouched, current provider unchanged.
    assert!(!get_claude_settings_path().exists());
    let run = ccs(&["current", "claude"]);
    assert_eq!(run.code, 0, "reads still work: {}", run.stderr);
    assert_eq!(run.stdout.trim(), "a (Alpha)");

    // The GUI owns the proxy: `proxy stop` must not try to stop it.
    let run = ccs(&["proxy", "stop"]);
    assert_eq!(run.code, cli::EXIT_FAILED);
    assert!(run.stderr.contains("desktop app"));

    drop(gui);
    let run = ccs(&["switch", "claude", "b"]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
}

#[test]
fn export_import_roundtrip_and_usage_summary() {
    let env = TestEnv::new();
    seed_claude();
    let file = env.home().join("backup.sql");

    let run = ccs(&["export", file.to_str().unwrap()]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    assert!(file.is_file());

    let run = ccs(&["switch", "claude", "b"]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);

    let run = ccs(&["--json", "import", file.to_str().unwrap()]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    assert_eq!(run.json()["success"], true);
    assert!(run.json()["backupId"]
        .as_str()
        .is_some_and(|s| !s.is_empty()));
    // As in the GUI, the device-level selection in settings.json ("b", made by
    // the switch above) wins over the imported is_current flag, and the
    // post-import sync rewrites the live file from it.
    let run = ccs(&["--json", "list", "claude"]);
    assert_eq!(run.json()["providers"].as_array().unwrap().len(), 2);
    assert_eq!(run.json()["current"], "b");
    let live: Value = read_json_file(&get_claude_settings_path()).expect("read live settings");
    assert_eq!(live["env"]["ANTHROPIC_AUTH_TOKEN"], "token-b");

    let run = ccs(&["--json", "usage", "summary", "--days", "7"]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    let v = run.json();
    assert_eq!(v["totalRequests"], 0);
    assert_eq!(v["days"], 7);
}

#[test]
fn config_dir_flag_and_proxy_status_without_proxy() {
    let env = TestEnv::new();
    let custom = env.home().join("portable-data");

    let dir = custom.to_str().unwrap();
    let run = ccs(&["--config-dir", dir, "--json", "proxy", "status"]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    let v = run.json();
    assert_eq!(v["running"], false);
    assert!(v["managedBy"].is_null());
    assert!(
        custom.join("cc-switch.db").is_file(),
        "db created in custom dir"
    );
    assert!(!env.home().join(".cc-switch").join("cc-switch.db").exists());

    let run = ccs(&["proxy", "stop"]);
    assert_eq!(run.code, 0);
    assert!(run.stdout.contains("No headless proxy"));
}

#[test]
fn legacy_unmigrated_config_is_left_for_the_gui() {
    let env = TestEnv::new();
    let dir = env.home().join(".cc-switch");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("config.json"), "{}").unwrap();

    let run = ccs(&["list"]);
    assert_eq!(run.code, cli::EXIT_FAILED);
    assert!(run.stderr.contains("legacy config.json"), "{}", run.stderr);
    assert!(!dir.join("cc-switch.db").exists());
}
