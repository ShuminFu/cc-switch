use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use serial_test::serial;
use tempfile::TempDir;

use super::watcher::{watch_dirs, DriftTracker};
use super::*;
use crate::config::{get_claude_settings_path, read_json_file, write_json_file};
use crate::database::Database;
use crate::provider::Provider;

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

fn claude_provider(id: &str, token: &str) -> Provider {
    Provider::with_id(
        id.to_string(),
        id.to_uppercase(),
        json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": token,
                "ANTHROPIC_BASE_URL": format!("https://{id}.example.com")
            }
        }),
        None,
    )
}

/// 建库、保存供应商并把 `current` 设为当前供应商，live 按预期输出写好
fn seed_claude(providers: &[Provider], current: &str) -> AppState {
    let db = Arc::new(Database::memory().expect("init db"));
    for provider in providers {
        db.save_provider("claude", provider).expect("save provider");
    }
    db.set_current_provider("claude", current)
        .expect("set db current provider");
    crate::settings::set_current_provider(&AppType::Claude, Some(current))
        .expect("set local current provider");

    let state = AppState::new(db);
    let provider = providers
        .iter()
        .find(|provider| provider.id == current)
        .expect("current provider");
    crate::services::provider::write_live_with_common_config(
        state.db.as_ref(),
        &AppType::Claude,
        provider,
    )
    .expect("write expected live");
    state
}

fn edit_claude_live(edit: impl FnOnce(&mut Value)) {
    let path = get_claude_settings_path();
    let mut live: Value = read_json_file(&path).expect("read claude live");
    edit(&mut live);
    write_json_file(&path, &live).expect("write claude live");
}

fn claude_live() -> Value {
    read_json_file(&get_claude_settings_path()).expect("read claude live")
}

fn field<'a>(report: &'a DriftReport, path: &[&str]) -> Option<&'a DriftField> {
    report.fields.iter().find(|field| {
        field
            .path
            .iter()
            .map(String::as_str)
            .eq(path.iter().copied())
    })
}

async fn use_ephemeral_proxy_port(db: &Database) {
    let mut config = db.get_proxy_config().await.expect("get proxy config");
    config.listen_port = 0;
    db.update_proxy_config(config)
        .await
        .expect("set ephemeral proxy port");
}

#[tokio::test]
#[serial]
async fn drift_clean_live_reports_no_drift() {
    let _home = TempHome::new();
    let state = seed_claude(&[claude_provider("p1", "key-1")], "p1");

    let report = DriftService::detect(&state, &AppType::Claude).expect("detect");
    assert!(!report.has_drift(), "{report:?}");
    assert_eq!(
        report.files,
        vec![get_claude_settings_path().display().to_string()]
    );
}

/// 模拟崩溃：代理为 Claude 接管后直接停掉服务（未恢复 live），live 留着代理 token。
#[tokio::test]
#[serial]
async fn drift_detects_and_repairs_leftover_proxy_placeholder_after_crash() {
    let _home = TempHome::new();
    let state = seed_claude(&[claude_provider("p1", "key-1")], "p1");
    use_ephemeral_proxy_port(&state.db).await;

    state
        .proxy_service
        .set_takeover_for_app("claude", true)
        .await
        .expect("take over claude");
    let taken_over = DriftService::detect(&state, &AppType::Claude).expect("detect");
    assert!(taken_over.takeover_active);
    assert!(
        !taken_over.leftover_placeholder,
        "placeholders are expected while the proxy is taking over"
    );
    assert!(!taken_over.has_drift(), "{taken_over:?}");

    // "崩溃"：服务停止，但 live、备份与 enabled 标志都留在接管状态
    state.proxy_service.stop().await.expect("stop proxy server");

    let report = DriftService::detect(&state, &AppType::Claude).expect("detect");
    assert!(!report.takeover_active);
    assert!(report.leftover_placeholder, "{report:?}");
    assert_eq!(
        field(&report, &["env", "ANTHROPIC_AUTH_TOKEN"]).map(|f| (f.change, f.foldable)),
        Some((DriftChange::Modified, false)),
        "the token placeholder differs from the provider and is never foldable"
    );

    let repaired = DriftService::resolve(&state, &AppType::Claude, DriftResolution::Reconcile)
        .await
        .expect("reconcile");
    assert!(!repaired.leftover_placeholder);
    assert!(!repaired.has_drift(), "{repaired:?}");
    assert_eq!(claude_live()["env"]["ANTHROPIC_AUTH_TOKEN"], json!("key-1"));
    assert!(state
        .db
        .get_live_backup("claude")
        .await
        .expect("get backup")
        .is_none());
}

/// 没有备份的旧版 `PROXY_MANAGED` 残留：从当前供应商重建 live
#[tokio::test]
#[serial]
async fn drift_reconcile_rebuilds_legacy_placeholder_without_backup() {
    let _home = TempHome::new();
    let state = seed_claude(&[claude_provider("p1", "key-1")], "p1");
    edit_claude_live(|live| {
        live["env"]["ANTHROPIC_AUTH_TOKEN"] = json!("PROXY_MANAGED");
        live["env"]["ANTHROPIC_BASE_URL"] = json!("http://127.0.0.1:15721");
    });

    let report = DriftService::detect(&state, &AppType::Claude).expect("detect");
    assert!(report.leftover_placeholder);

    let repaired = DriftService::resolve(&state, &AppType::Claude, DriftResolution::Reconcile)
        .await
        .expect("reconcile");
    assert!(!repaired.has_drift(), "{repaired:?}");
    assert_eq!(
        claude_live()["env"],
        json!({
            "ANTHROPIC_AUTH_TOKEN": "key-1",
            "ANTHROPIC_BASE_URL": "https://p1.example.com"
        })
    );
}

#[tokio::test]
#[serial]
async fn drift_reconcile_is_refused_while_takeover_is_active() {
    let _home = TempHome::new();
    let state = seed_claude(&[claude_provider("p1", "key-1")], "p1");
    use_ephemeral_proxy_port(&state.db).await;
    state
        .proxy_service
        .set_takeover_for_app("claude", true)
        .await
        .expect("take over claude");

    let err = DriftService::resolve(&state, &AppType::Claude, DriftResolution::Reconcile)
        .await
        .expect_err("reconcile must not undo a live takeover");
    assert!(err.to_string().contains("接管"), "{err}");

    state
        .proxy_service
        .stop_with_restore()
        .await
        .expect("stop proxy and restore");
}

/// 外部新增的 `hooks` 会在下次切换时丢失：报告为可并入，并入后切换供应商仍保留
#[tokio::test]
#[serial]
async fn drift_offers_externally_added_hooks_for_folding() {
    let _home = TempHome::new();
    let state = seed_claude(
        &[
            claude_provider("p1", "key-1"),
            claude_provider("p2", "key-2"),
        ],
        "p1",
    );
    let hooks = json!({
        "PreToolUse": [{ "matcher": "Bash", "hooks": [{ "type": "command", "command": "echo hi" }] }]
    });
    edit_claude_live(|live| {
        live["hooks"] = hooks.clone();
        live["env"]["MY_EXTERNAL_API_KEY"] = json!("secret");
    });

    let report = DriftService::detect(&state, &AppType::Claude).expect("detect");
    assert!(!report.leftover_placeholder);
    assert!(report.needs_attention());
    assert_eq!(
        field(&report, &["hooks"]).map(|f| (f.change, f.foldable)),
        Some((DriftChange::Added, true))
    );
    assert_eq!(
        field(&report, &["env", "MY_EXTERNAL_API_KEY"]).map(|f| (f.change, f.foldable)),
        Some((DriftChange::Added, false)),
        "credentials never join the shared common config"
    );
    assert!(
        !serde_json::to_string(&report).unwrap().contains("secret"),
        "reports carry paths only, never values"
    );

    let err = DriftService::resolve(
        &state,
        &AppType::Claude,
        DriftResolution::Fold {
            paths: vec![vec!["env".into(), "MY_EXTERNAL_API_KEY".into()]],
        },
    )
    .await
    .expect_err("credentials are not foldable");
    assert!(err.to_string().contains("MY_EXTERNAL_API_KEY"), "{err}");

    let folded = DriftService::resolve(
        &state,
        &AppType::Claude,
        DriftResolution::Fold {
            paths: vec![vec!["hooks".into()]],
        },
    )
    .await
    .expect("fold hooks");
    assert!(field(&folded, &["hooks"]).is_none(), "{folded:?}");

    let snippet: Value = serde_json::from_str(
        &state
            .db
            .get_config_snippet("claude")
            .expect("get snippet")
            .expect("snippet saved"),
    )
    .expect("snippet is JSON");
    assert_eq!(snippet, json!({ "hooks": hooks }));
    let current = state
        .db
        .get_provider_by_id("p1", "claude")
        .expect("get provider")
        .expect("provider exists");
    assert_eq!(
        current.meta.and_then(|meta| meta.common_config_enabled),
        Some(true)
    );

    // p2 也启用通用配置后，切换过去 hooks 仍在 live 中
    let mut p2 = state
        .db
        .get_provider_by_id("p2", "claude")
        .expect("get provider")
        .expect("provider exists");
    p2.meta
        .get_or_insert_with(Default::default)
        .common_config_enabled = Some(true);
    state.db.save_provider("claude", &p2).expect("save p2");
    crate::services::ProviderService::switch(&state, AppType::Claude, "p2").expect("switch");
    let live = claude_live();
    assert_eq!(live["hooks"], hooks);
    assert_eq!(live["env"]["ANTHROPIC_AUTH_TOKEN"], json!("key-2"));
}

#[tokio::test]
#[serial]
async fn drift_ignore_suppresses_the_same_drift_until_it_changes() {
    let _home = TempHome::new();
    let state = seed_claude(&[claude_provider("p1", "key-1")], "p1");
    edit_claude_live(|live| live["permissions"] = json!({ "allow": ["Bash(ls)"] }));

    let ignored = DriftService::resolve(&state, &AppType::Claude, DriftResolution::Ignore)
        .await
        .expect("ignore");
    assert!(ignored.has_drift());
    assert!(ignored.ignored);
    assert!(!ignored.needs_attention());

    edit_claude_live(|live| live["permissions"] = json!({ "allow": ["Bash(rm)"] }));
    let changed = DriftService::detect(&state, &AppType::Claude).expect("detect");
    assert!(
        changed.needs_attention(),
        "a different value is a new drift"
    );
    assert_ne!(changed.fingerprint, ignored.fingerprint);
}

#[test]
fn drift_diff_parses_toml_and_env_files() {
    let toml_file = LiveFilePreview {
        path: "/x/config.toml".to_string(),
        before: Some("model = \"a\"\nextra = 1\n[features]\nweb = true\n".to_string()),
        after: Some("model = \"a\"\n[features]\nweb = false\n".to_string()),
    };
    let env_file = LiveFilePreview {
        path: "/x/.env".to_string(),
        before: Some("A=1\n".to_string()),
        after: Some("A=1\nB=2\n".to_string()),
    };
    let mut fields = Vec::new();
    diff_file(&toml_file, &mut fields);
    diff_file(&env_file, &mut fields);

    let summary: Vec<(String, DriftChange)> = fields
        .iter()
        .map(|field| (field.path.join("."), field.change))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("extra".to_string(), DriftChange::Added),
            ("features.web".to_string(), DriftChange::Modified),
            ("B".to_string(), DriftChange::Removed),
        ]
    );
}

#[test]
fn drift_fold_merges_toml_paths_into_existing_snippet() {
    let source = FoldSource::Toml(
        "approval_policy = \"never\"\n[features]\nweb = true\n"
            .parse()
            .expect("parse source"),
    );
    let merged = source
        .merge_into(
            "# keep\nsandbox = \"x\"\n",
            &[
                vec!["approval_policy".to_string()],
                vec!["features".to_string(), "web".to_string()],
            ],
        )
        .expect("merge");
    let doc: toml::Value = toml::from_str(&merged).expect("valid toml");
    assert!(merged.contains("# keep"), "{merged}");
    assert_eq!(doc["sandbox"].as_str(), Some("x"));
    assert_eq!(doc["approval_policy"].as_str(), Some("never"));
    assert_eq!(doc["features"]["web"].as_bool(), Some(true));
}

fn report(app: &str, files: &[&Path], fingerprint: &str, drift: bool) -> DriftReport {
    DriftReport {
        app: app.to_string(),
        takeover_active: false,
        leftover_placeholder: drift,
        fields: Vec::new(),
        files: files.iter().map(|p| p.display().to_string()).collect(),
        fingerprint: fingerprint.to_string(),
        ignored: false,
    }
}

#[test]
fn drift_tracker_notifies_each_drift_once_and_maps_paths_to_apps() {
    let settings = PathBuf::from("/h/.claude/settings.json");
    let mut tracker = DriftTracker::default();

    assert_eq!(
        tracker
            .ingest(vec![report("claude", &[&settings], "f1", true)])
            .len(),
        1
    );
    assert!(
        tracker
            .ingest(vec![report("claude", &[&settings], "f1", true)])
            .is_empty(),
        "the same drift is pushed once"
    );
    assert!(tracker
        .ingest(vec![report("claude", &[&settings], "f0", false)])
        .is_empty());
    assert_eq!(
        tracker
            .ingest(vec![report("claude", &[&settings], "f1", true)])
            .len(),
        1,
        "a drift that comes back after being fixed is pushed again"
    );

    assert_eq!(
        tracker.apps_for_paths(std::slice::from_ref(&settings)),
        vec![AppType::Claude]
    );
    assert!(tracker
        .apps_for_paths(&[PathBuf::from("/h/.claude/history.jsonl")])
        .is_empty());
}

#[test]
fn drift_watcher_reports_external_writes() {
    let temp = TempDir::new().expect("tempdir");
    // macOS 的 FSEvents 报告规范化路径（/private/var/...）
    let dir = temp.path().canonicalize().expect("canonical tempdir");
    let (tx, rx) = std::sync::mpsc::channel();
    let _watcher = watch_dirs(
        std::slice::from_ref(&dir),
        Duration::from_millis(100),
        move |paths| {
            let _ = tx.send(paths);
        },
    )
    .expect("watch dir");

    let file = dir.join("settings.json");
    std::fs::write(&file, "{\"hooks\":{}}").expect("external write");

    let paths = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("watcher reports the write");
    assert!(paths.contains(&file), "{paths:?}");
}

fn seed_app(app: &AppType, provider: Provider) -> AppState {
    let db = Arc::new(Database::memory().expect("init db"));
    db.save_provider(app.as_str(), &provider)
        .expect("save provider");
    db.set_current_provider(app.as_str(), &provider.id)
        .expect("set db current provider");
    crate::settings::set_current_provider(app, Some(&provider.id))
        .expect("set local current provider");
    let state = AppState::new(db);
    crate::services::provider::write_live_with_common_config(state.db.as_ref(), app, &provider)
        .expect("write expected live");
    state
}

/// Gemini 默认端点：接管会给 `.env` 新增 `GOOGLE_GEMINI_BASE_URL`，这不算外部改动；
/// 用户在接管期间新增的键仍要报告
#[tokio::test]
#[serial]
async fn drift_ignores_keys_the_gemini_takeover_adds() {
    let _home = TempHome::new();
    let provider = Provider::with_id(
        "g1".into(),
        "G1".into(),
        json!({ "env": { "GEMINI_API_KEY": "gem-key", "GEMINI_MODEL": "gemini-2.5-pro" }, "config": {} }),
        None,
    );
    let state = seed_app(&AppType::Gemini, provider);
    use_ephemeral_proxy_port(&state.db).await;
    state
        .proxy_service
        .set_takeover_for_app("gemini", true)
        .await
        .expect("take over gemini");

    let report = DriftService::detect(&state, &AppType::Gemini).expect("detect");
    assert!(report.takeover_active);
    assert!(!report.has_drift(), "{report:?}");

    let env_path = crate::gemini_config::get_gemini_env_path();
    let mut env = std::fs::read_to_string(&env_path).expect("read .env");
    env.push_str("\nGEMINI_SANDBOX=true\n");
    std::fs::write(&env_path, env).expect("external edit");
    let edited = DriftService::detect(&state, &AppType::Gemini).expect("detect");
    assert_eq!(
        edited
            .fields
            .iter()
            .map(|field| (field.path.join("."), field.change))
            .collect::<Vec<_>>(),
        vec![("GEMINI_SANDBOX".to_string(), DriftChange::Added)]
    );

    state
        .proxy_service
        .stop_with_restore()
        .await
        .expect("stop proxy and restore");
}

/// Codex 官方供应商：接管新增 `model_provider` / `model_providers`，不算外部改动
#[tokio::test]
#[serial]
async fn drift_ignores_keys_the_codex_official_takeover_adds() {
    let _home = TempHome::new();
    let seed_db = Database::memory().expect("init db");
    seed_db
        .ensure_official_seed_by_id(crate::database::CODEX_OFFICIAL_PROVIDER_ID, AppType::Codex)
        .expect("seed official provider");
    let provider = seed_db
        .get_provider_by_id(crate::database::CODEX_OFFICIAL_PROVIDER_ID, "codex")
        .expect("get provider")
        .expect("official provider exists");
    let state = seed_app(&AppType::Codex, provider);
    // 官方供应商依赖 Codex 自己的 ChatGPT 登录
    std::fs::write(
        crate::codex_config::get_codex_auth_path(),
        r#"{"tokens":{"access_token":"chatgpt-login"}}"#,
    )
    .expect("write chatgpt login");
    use_ephemeral_proxy_port(&state.db).await;
    state
        .proxy_service
        .set_takeover_for_app("codex", true)
        .await
        .expect("take over codex");

    let report = DriftService::detect(&state, &AppType::Codex).expect("detect");
    assert!(report.takeover_active);
    assert!(!report.has_drift(), "{report:?}");

    state
        .proxy_service
        .stop_with_restore()
        .await
        .expect("stop proxy and restore");
}
