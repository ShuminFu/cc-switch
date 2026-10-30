//! Proxy takeover crash/restore E2E (CCS-152).
//!
//! For every proxy-capable app (claude, codex, gemini, grokbuild, and the
//! additive opencode, openclaw and hermes, CCS-163) and both ways
//! of turning takeover on (`start_with_takeover` and `set_takeover_for_app`):
//!
//! 1. seed the app's live files with hand-written bytes (non-canonical
//!    indentation, comments, key order) and a matching current provider;
//! 2. apply takeover, then "kill the process": the tokio runtime that runs the
//!    proxy server is dropped and the app state goes away without `stop`,
//!    `stop_with_restore` or `cleanup_before_exit`;
//! 3. restart: reopen the same on-disk database and run the startup recovery
//!    the app runs (`has_any_live_backup` / `detect_takeover_in_live_configs`
//!    -> `recover_from_crash`);
//! 4. assert the live files under HOME are restored byte-for-byte and that a
//!    second restart finds nothing left to recover.
//!
//! The `takeoverEngine = "legacy"` fallback is covered too: it restores the
//! same settings but rewrites them through the JSON backup, so it is
//! byte-exact only for files CC Switch wrote itself.
//!
//! This suite is the gate for the takeover consolidation (CCS-153 / CCS-154):
//! it must stay green on Linux, macOS and Windows before any takeover code moves.

// 测试夹具直接写文件即可，无需走 atomic_write（见 clippy.toml）
#![allow(clippy::disallowed_methods)]

mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cc_switch_lib::live_config_testkit::{self as kit, adapter, EntryEndpoint, EntryTakeover};
use cc_switch_lib::{
    update_settings, AppSettings, AppState, AppType, Database, Provider, ProviderService,
};
use serde_json::{json, Value};
use support::TestEnv;

const PROVIDER_ID: &str = "crash-provider";
const API_KEY: &str = "sk-crash-e2e-key";
const UPSTREAM: &str = "https://crash-e2e.example.com";

// ============================================================================
// Cases
// ============================================================================

/// 一个可接管应用的 live 样例
struct Case {
    slug: &'static str,
    app: AppType,
    /// (相对 `config_dir()` 的文件名, 手写原文)
    files: Vec<(&'static str, String)>,
    /// 与 live 一致的供应商配置
    settings: Value,
}

/// 用户手写风格的 live 文件：缩进、注释、键顺序都不是 CC Switch 写入器的输出
fn cases() -> Vec<Case> {
    let codex_config = format!(
        "# hand-written Codex config\nmodel_provider = \"custom\"\nmodel = \"gpt-5.4\"\n\n[model_providers.custom]\nname = \"Crash E2E\"\nbase_url = \"{UPSTREAM}/v1\"\nwire_api = \"responses\"\nrequires_openai_auth = true\n"
    );
    let grok_config = format!(
        "# hand-written Grok Build config\n[models]\ndefault = \"grok-4.5\"\n\n[model.\"grok-4.5\"]\nmodel = \"grok-4.5\"\nbase_url = \"{UPSTREAM}/v1\"\nname = \"Crash E2E\"\napi_key = \"{API_KEY}\"\napi_backend = \"responses\"\ncontext_window = 500000\n"
    );
    vec![
        Case {
            slug: "claude",
            app: AppType::Claude,
            files: vec![(
                "settings.json",
                format!(
                    "{{\n    \"permissions\": {{ \"allow\": [\"Bash(ls:*)\"] }},\n    \"env\": {{\"ANTHROPIC_BASE_URL\": \"{UPSTREAM}\", \"ANTHROPIC_AUTH_TOKEN\": \"{API_KEY}\"}}\n}}\n"
                ),
            )],
            settings: json!({
                "env": { "ANTHROPIC_AUTH_TOKEN": API_KEY, "ANTHROPIC_BASE_URL": UPSTREAM }
            }),
        },
        Case {
            slug: "codex",
            app: AppType::Codex,
            files: vec![
                ("auth.json", format!("{{\"OPENAI_API_KEY\":\"{API_KEY}\"}}")),
                ("config.toml", codex_config.clone()),
            ],
            settings: json!({ "auth": { "OPENAI_API_KEY": API_KEY }, "config": codex_config }),
        },
        Case {
            slug: "gemini",
            app: AppType::Gemini,
            files: vec![(
                ".env",
                format!(
                    "# hand-written Gemini env\nGOOGLE_GEMINI_BASE_URL={UPSTREAM}\nGEMINI_API_KEY={API_KEY}\nGEMINI_MODEL=gemini-2.5-pro"
                ),
            )],
            settings: json!({
                "env": {
                    "GEMINI_API_KEY": API_KEY,
                    "GOOGLE_GEMINI_BASE_URL": UPSTREAM,
                    "GEMINI_MODEL": "gemini-2.5-pro"
                }
            }),
        },
        Case {
            slug: "grokbuild",
            app: AppType::GrokBuild,
            files: vec![("config.toml", grok_config.clone())],
            settings: json!({ "config": grok_config }),
        },
        // 累加模式应用：live 里还有一个不归 CC Switch 管的条目，接管只改写选中条目
        Case {
            slug: "opencode",
            app: AppType::OpenCode,
            files: vec![(
                "opencode.json",
                format!(
                    "{{\n  // hand-written OpenCode config\n  \"$schema\": \"https://opencode.ai/config.json\",\n  \"model\": \"{PROVIDER_ID}/crash-model\",\n  \"provider\": {{\n    \"{PROVIDER_ID}\": {{\n      \"npm\": \"@ai-sdk/openai-compatible\",\n      \"name\": \"Crash E2E\",\n      \"options\": {{ \"baseURL\": \"{UPSTREAM}/v1\", \"apiKey\": \"{API_KEY}\" }},\n      \"models\": {{ \"crash-model\": {{ \"name\": \"Crash Model\" }} }}\n    }},\n    \"hand-only\": {{\n      \"npm\": \"@ai-sdk/openai-compatible\",\n      \"options\": {{ \"baseURL\": \"https://other.example.com/v1\", \"apiKey\": \"sk-other\" }}\n    }}\n  }}\n}}\n"
                ),
            )],
            settings: json!({
                "npm": "@ai-sdk/openai-compatible",
                "name": "Crash E2E",
                "options": { "baseURL": format!("{UPSTREAM}/v1"), "apiKey": API_KEY },
                "models": { "crash-model": { "name": "Crash Model" } }
            }),
        },
        Case {
            slug: "openclaw",
            app: AppType::OpenClaw,
            files: vec![(
                "openclaw.json",
                format!(
                    "// hand-written OpenClaw config\n{{\n  models: {{\n    mode: 'merge',\n    providers: {{\n      '{PROVIDER_ID}': {{\n        baseUrl: '{UPSTREAM}/v1',\n        apiKey: '{API_KEY}',\n        api: 'openai-completions',\n        models: [{{ id: 'crash-model', name: 'Crash Model' }}],\n      }},\n      'hand-only': {{ baseUrl: 'https://other.example.com/v1', apiKey: 'sk-other', api: 'openai-completions' }},\n    }},\n  }},\n  agents: {{ defaults: {{ model: {{ primary: '{PROVIDER_ID}/crash-model' }} }} }},\n}}\n"
                ),
            )],
            settings: json!({
                "baseUrl": format!("{UPSTREAM}/v1"),
                "apiKey": API_KEY,
                "api": "openai-completions",
                "models": [{ "id": "crash-model", "name": "Crash Model" }]
            }),
        },
        Case {
            slug: "hermes",
            app: AppType::Hermes,
            files: vec![(
                "config.yaml",
                format!(
                    "# hand-written Hermes config\nmodel:\n  default: crash-model\n  provider: {PROVIDER_ID}\n\nagent:\n  max_turns: 50   # keep it short\n\ncustom_providers:\n  - name: {PROVIDER_ID}\n    base_url: {UPSTREAM}/v1\n    api_key: {API_KEY}\n    model: crash-model\n  - name: hand-only\n    base_url: https://other.example.com/v1\n    api_key: sk-other\n"
                ),
            )],
            settings: json!({
                "name": PROVIDER_ID,
                "base_url": format!("{UPSTREAM}/v1"),
                "api_key": API_KEY,
                "model": "crash-model"
            }),
        },
    ]
}

/// 开启接管的入口
#[derive(Debug, Clone, Copy)]
enum Entry {
    /// 启动代理并接管所有已存在的 live（`start_with_takeover`）
    StartWithTakeover,
    /// 只接管单个应用（`set_takeover_for_app`，界面上的接管开关）
    PerApp,
}

const ENTRIES: [Entry; 2] = [Entry::StartWithTakeover, Entry::PerApp];

/// live 文件的写法
#[derive(Debug, Clone, Copy)]
enum Seed {
    /// 用户手写的原文（[`cases`]）
    HandWritten,
    /// CC Switch 切换供应商时写入器的输出
    WrittenByCcSwitch,
}

// ============================================================================
// Helpers
// ============================================================================

type Snapshot = BTreeMap<PathBuf, Vec<u8>>;

/// HOME 下所有文件的字节快照（不含 CC Switch 自己的数据目录）
fn snapshot(home: &Path) -> Snapshot {
    fn walk(dir: &Path, home: &Path, out: &mut Snapshot) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.expect("dir entry").path();
            let rel = path.strip_prefix(home).unwrap().to_path_buf();
            if rel == Path::new(".cc-switch") {
                continue;
            }
            if path.is_dir() {
                walk(&path, home, out);
            } else {
                out.insert(rel, fs::read(&path).expect("read snapshot file"));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(home, home, &mut out);
    out
}

fn describe_diff(before: &Snapshot, after: &Snapshot) -> String {
    let paths: BTreeSet<_> = before.keys().chain(after.keys()).collect();
    let mut out = Vec::new();
    for path in paths {
        match (before.get(path), after.get(path)) {
            (Some(a), Some(b)) if a == b => {}
            (Some(a), Some(b)) => out.push(format!(
                "changed {}:\n--- before\n{}\n--- after\n{}",
                path.display(),
                String::from_utf8_lossy(a),
                String::from_utf8_lossy(b)
            )),
            (Some(_), None) => out.push(format!("removed {}", path.display())),
            (None, Some(_)) => out.push(format!("added {}", path.display())),
            (None, None) => unreachable!(),
        }
    }
    out.join("\n")
}

/// 一次"进程生命周期"的运行时：drop 即终止其上的代理服务器任务，不做任何清理
fn process_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build runtime")
}

fn use_takeover_engine(engine: &str) {
    let mut settings = serde_json::to_value(AppSettings::default()).expect("settings to json");
    settings["takeoverEngine"] = json!(engine);
    let settings: AppSettings = serde_json::from_value(settings).expect("settings from json");
    update_settings(settings).expect("update settings");
}

fn seed_live(case: &Case, seed: Seed, db: &Database) {
    let dir = adapter(&case.app).config_dir().expect("config dir");
    fs::create_dir_all(&dir).expect("create config dir");
    let provider = Provider::with_id(
        PROVIDER_ID.to_string(),
        "Crash E2E".to_string(),
        case.settings.clone(),
        None,
    );
    db.save_provider(case.slug, &provider)
        .expect("save provider");
    db.set_current_provider(case.slug, PROVIDER_ID)
        .expect("set current provider");
    match seed {
        Seed::HandWritten => {
            for (name, body) in &case.files {
                fs::write(dir.join(name), body).expect("write live file");
            }
        }
        Seed::WrittenByCcSwitch => {
            kit::write_provider_live(db, &case.app, &provider).expect("write provider live");
        }
    }
}

/// 第一个"进程"：接管后被直接杀掉。返回接管前与接管期间的 HOME 快照，
/// 以及接管前 live 的设置内容。
fn run_until_killed(
    case: &Case,
    seed: Seed,
    entry: Entry,
    home: &Path,
) -> (Snapshot, Snapshot, Value) {
    let db = Arc::new(Database::init().expect("open database"));
    let state = AppState::new(db.clone());
    let rt = process_runtime();

    rt.block_on(async {
        let mut proxy_config = db.get_proxy_config().await.expect("proxy config");
        proxy_config.listen_port = 0;
        db.update_proxy_config(proxy_config)
            .await
            .expect("use an ephemeral proxy port");
    });
    seed_live(case, seed, &db);
    let before = snapshot(home);
    let live_before = adapter(&case.app).read_live().expect("read original live");

    rt.block_on(async {
        match entry {
            Entry::StartWithTakeover => state.proxy_service.start_with_takeover().await.map(|_| ()),
            Entry::PerApp => state
                .proxy_service
                .set_takeover_for_app(case.slug, true)
                .await
                .map(|_| ()),
        }
        .unwrap_or_else(|e| panic!("{} {entry:?}: takeover: {e}", case.slug));
    });

    let during = snapshot(home);
    let live = adapter(&case.app).read_live().expect("read live");
    assert!(
        adapter(&case.app)
            .takeover()
            .expect("proxy-capable app")
            .has_placeholder(&live),
        "{} {entry:?}: live must be taken over before the crash:\n{}",
        case.slug,
        describe_diff(&before, &during)
    );
    assert!(
        rt.block_on(db.has_any_live_backup()).expect("backup query"),
        "{} {entry:?}: takeover must leave a backup to recover from",
        case.slug
    );

    // kill -9：代理任务随运行时一起终止，不调用 stop / stop_with_restore / cleanup_before_exit
    drop(rt);
    drop(state);
    drop(db);
    (before, during, live_before)
}

/// 重启后的启动恢复，与 `lib.rs` setup 中的流程一致。返回是否执行了恢复。
fn restart_and_recover() -> bool {
    let db = Arc::new(Database::init().expect("reopen database"));
    let state = AppState::new(db.clone());
    let rt = process_runtime();
    rt.block_on(async {
        let has_backups = db.has_any_live_backup().await.expect("backup query");
        let live_taken_over = state.proxy_service.detect_takeover_in_live_configs();
        if has_backups || live_taken_over {
            state
                .proxy_service
                .recover_from_crash()
                .await
                .expect("recover from crash");
            assert!(
                !db.has_any_live_backup().await.expect("backup query"),
                "recovery must delete the backups"
            );
            true
        } else {
            false
        }
    })
}

/// 接管 → 杀进程 → 重启恢复，返回 (接管前, 接管中, 恢复后) 的 HOME 快照
fn crash_cycle(case: &Case, seed: Seed, entry: Entry) -> [Snapshot; 3] {
    let env = TestEnv::new();
    let (before, during, _) = run_until_killed(case, seed, entry, env.home());
    assert_ne!(
        before, during,
        "{} {entry:?}: takeover rewrote nothing",
        case.slug
    );

    assert!(
        restart_and_recover(),
        "{} {entry:?}: startup must detect the takeover leftover",
        case.slug
    );
    let after = snapshot(env.home());

    // 再次重启：没有残留，不再恢复，也不改动任何文件
    assert!(
        !restart_and_recover(),
        "{} {entry:?}: a second restart must find nothing to recover",
        case.slug
    );
    assert_eq!(
        snapshot(env.home()),
        after,
        "{} {entry:?}: a second restart must not touch live files",
        case.slug
    );
    [before, during, after]
}

// ============================================================================
// Tests
// ============================================================================

#[test]
fn crash_after_takeover_restores_hand_written_live_files_byte_for_byte() {
    for case in cases() {
        for entry in ENTRIES {
            let [before, _, after] = crash_cycle(&case, Seed::HandWritten, entry);
            assert!(
                before == after,
                "{} {entry:?}: crash recovery must restore the original bytes:\n{}",
                case.slug,
                describe_diff(&before, &after)
            );
        }
    }
}

#[test]
fn crash_after_takeover_restores_cc_switch_written_live_files_byte_for_byte() {
    for case in cases() {
        for entry in ENTRIES {
            let [before, _, after] = crash_cycle(&case, Seed::WrittenByCcSwitch, entry);
            assert!(
                before == after,
                "{} {entry:?}: crash recovery must restore the original bytes:\n{}",
                case.slug,
                describe_diff(&before, &after)
            );
        }
    }
}

#[test]
fn legacy_engine_restores_cc_switch_written_live_files_byte_for_byte() {
    for case in cases() {
        for entry in ENTRIES {
            let env = TestEnv::new();
            use_takeover_engine("legacy");
            let (before, _, _) =
                run_until_killed(&case, Seed::WrittenByCcSwitch, entry, env.home());
            assert!(restart_and_recover(), "{} {entry:?}: recovered", case.slug);
            let after = snapshot(env.home());
            assert!(
                before == after,
                "{} {entry:?}: legacy recovery must restore the original bytes:\n{}",
                case.slug,
                describe_diff(&before, &after)
            );
        }
    }
}

#[test]
fn legacy_engine_restores_hand_written_settings_but_not_their_formatting() {
    for case in cases() {
        let env = TestEnv::new();
        use_takeover_engine("legacy");
        let (before, _, live_before) =
            run_until_killed(&case, Seed::HandWritten, Entry::PerApp, env.home());

        assert!(restart_and_recover(), "{}: recovered", case.slug);
        let live_after = adapter(&case.app).read_live().expect("read restored live");
        assert_eq!(
            live_after, live_before,
            "{}: legacy recovery must restore the same settings",
            case.slug
        );
        if case.slug == "claude" {
            // 旧路径经 JSON 备份重新序列化：手写缩进不保留（unified 引擎的改进点）
            assert_ne!(
                before,
                snapshot(env.home()),
                "claude: legacy recovery rewrites settings.json through the JSON backup"
            );
        }
    }
}

#[test]
fn takeover_snapshot_is_dropped_when_the_backup_is_rewritten() {
    // 接管期间备份被重写（热切换等），快照不再对应备份：恢复按新备份写入，
    // 不能把接管前的原文覆盖回去。
    let case = cases().into_iter().next().expect("claude case");
    let env = TestEnv::new();
    let _ = run_until_killed(&case, Seed::HandWritten, Entry::PerApp, env.home());

    let rewritten = json!({
        "env": { "ANTHROPIC_AUTH_TOKEN": "sk-rewritten", "ANTHROPIC_BASE_URL": UPSTREAM }
    });
    {
        let db = Database::init().expect("reopen database");
        let rt = process_runtime();
        rt.block_on(db.save_live_backup("claude", &rewritten.to_string()))
            .expect("rewrite backup");
    }

    assert!(restart_and_recover(), "claude: recovered");
    let live = adapter(&AppType::Claude).read_live().expect("read live");
    assert_eq!(
        live["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-rewritten",
        "recovery must follow the rewritten backup, not the stale snapshot"
    );
}

// ============================================================================
// Failed hot switch (CCS-212)
// ============================================================================

const SECOND_PROVIDER_ID: &str = "crash-provider-b";

/// 接管后热切换到另一个供应商，并让最后一步（数据库提交当前供应商）失败：
/// 此时备份已被热切换重写，回滚必须把接管前的备份行（含快照）原样写回。
/// 返回 (接管前的 HOME 快照, 回滚后备份行上的接管快照)。
fn takeover_then_fail_hot_switch(case: &Case, home: &Path) -> (Snapshot, Option<String>) {
    let db = Arc::new(Database::init().expect("open database"));
    let state = AppState::new(db.clone());
    let rt = process_runtime();

    rt.block_on(async {
        let mut proxy_config = db.get_proxy_config().await.expect("proxy config");
        proxy_config.listen_port = 0;
        db.update_proxy_config(proxy_config)
            .await
            .expect("use an ephemeral proxy port");
    });
    seed_live(case, Seed::HandWritten, &db);
    let second = Provider::with_id(
        SECOND_PROVIDER_ID.to_string(),
        "Crash E2E B".to_string(),
        case.settings.clone(),
        None,
    );
    db.save_provider(case.slug, &second)
        .expect("save second provider");
    let before = snapshot(home);

    let snapshot_after_rollback = rt.block_on(async {
        state
            .proxy_service
            .set_takeover_for_app(case.slug, true)
            .await
            .unwrap_or_else(|e| panic!("{}: takeover: {e}", case.slug));

        let conn = rusqlite::Connection::open(home.join(".cc-switch").join("cc-switch.db"))
            .expect("open database file");
        conn.execute_batch(
            "CREATE TRIGGER reject_current_update
                 BEFORE UPDATE OF is_current ON providers
                 BEGIN
                   SELECT RAISE(ABORT, 'forced current-provider commit failure');
                 END;",
        )
        .expect("install failure trigger");
        let error = state
            .proxy_service
            .hot_switch_provider(case.slug, SECOND_PROVIDER_ID)
            .await
            .expect_err("hot switch must fail at the provider commit");
        assert!(
            error.contains("forced current-provider commit failure"),
            "{}: unexpected hot switch error: {error}",
            case.slug
        );
        conn.execute_batch("DROP TRIGGER reject_current_update;")
            .expect("drop failure trigger");

        db.get_live_backup_raw_snapshot(case.slug)
            .await
            .expect("read takeover snapshot")
    });

    drop(rt);
    drop(state);
    drop(db);
    (before, snapshot_after_rollback)
}

/// 会在热切换时重写备份的应用（累加模式应用的备份在热切换时保持不变）
fn whole_file_cases() -> Vec<Case> {
    cases()
        .into_iter()
        .filter(|case| {
            adapter(&case.app)
                .takeover()
                .is_some_and(|takeover| takeover.entry_takeover().is_none())
        })
        .collect()
}

#[test]
fn failed_hot_switch_keeps_the_takeover_snapshot() {
    let cases = whole_file_cases();
    assert_eq!(
        cases.iter().map(|case| case.slug).collect::<Vec<_>>(),
        ["claude", "codex", "gemini", "grokbuild"]
    );
    for case in cases {
        let env = TestEnv::new();
        let (before, snapshot_after_rollback) = takeover_then_fail_hot_switch(&case, env.home());
        assert!(
            snapshot_after_rollback.is_some(),
            "{}: rollback must write the takeover snapshot back",
            case.slug
        );

        assert!(restart_and_recover(), "{}: recovered", case.slug);
        let after = snapshot(env.home());
        assert!(
            before == after,
            "{}: restore after a failed hot switch must give back the original bytes:\n{}",
            case.slug,
            describe_diff(&before, &after)
        );
    }
}

#[test]
fn legacy_engine_failed_hot_switch_records_no_snapshot() {
    for case in whole_file_cases() {
        let env = TestEnv::new();
        use_takeover_engine("legacy");
        let (_, snapshot_after_rollback) = takeover_then_fail_hot_switch(&case, env.home());
        assert_eq!(
            snapshot_after_rollback, None,
            "{}: legacy engine never stores a takeover snapshot",
            case.slug
        );
        assert!(restart_and_recover(), "{}: recovered", case.slug);
    }
}

// ============================================================================
// Additive apps (CCS-163)
// ============================================================================

/// 条目级接管的累加模式应用
fn entry_cases() -> Vec<(Case, &'static dyn EntryTakeover)> {
    cases()
        .into_iter()
        .filter_map(|case| {
            let entries = adapter(&case.app).takeover()?.entry_takeover()?;
            Some((case, entries))
        })
        .collect()
}

fn entry_endpoint(entries: &dyn EntryTakeover, live: &Value, id: &str) -> EntryEndpoint {
    entries
        .entries(live)
        .into_iter()
        .find(|(entry_id, _)| entry_id == id)
        .map(|(_, entry)| entries.endpoint(&entry))
        .unwrap_or_else(|| panic!("live has no entry {id}: {live}"))
}

#[test]
fn additive_takeover_rewrites_only_the_selected_entry() {
    let cases = entry_cases();
    assert_eq!(
        cases.iter().map(|(case, _)| case.slug).collect::<Vec<_>>(),
        ["opencode", "openclaw", "hermes"]
    );
    for (case, entries) in cases {
        for entry in ENTRIES {
            let env = TestEnv::new();
            let (_, _, live_before) = run_until_killed(&case, Seed::HandWritten, entry, env.home());
            let live = adapter(&case.app).read_live().expect("read live");

            let taken = entry_endpoint(entries, &live, PROVIDER_ID);
            let base_url = taken.base_url.expect("proxy base url");
            assert!(
                base_url.starts_with("http://127.0.0.1:")
                    && base_url.ends_with(&format!("/{}/v1", case.slug)),
                "{} {entry:?}: selected entry points at the app route: {base_url}",
                case.slug
            );
            assert!(
                taken
                    .api_key
                    .as_deref()
                    .is_some_and(|key| key.starts_with("ccs-proxy-")),
                "{} {entry:?}: selected entry carries the install token",
                case.slug
            );
            assert_eq!(
                entry_endpoint(entries, &live, "hand-only"),
                entry_endpoint(entries, &live_before, "hand-only"),
                "{} {entry:?}: other entries are left alone",
                case.slug
            );
            // 代理（故障转移关闭时）按当前供应商转发：必须是被接管的条目
            let db = Database::init().expect("reopen database");
            assert_eq!(
                db.get_current_provider(case.slug)
                    .expect("current provider"),
                Some(PROVIDER_ID.to_string()),
                "{} {entry:?}",
                case.slug
            );
            drop(db);
            assert!(restart_and_recover(), "{} {entry:?}: recovered", case.slug);
        }
    }
}

#[test]
fn additive_crash_recovery_keeps_edits_to_other_entries() {
    for (case, entries) in entry_cases() {
        let env = TestEnv::new();
        let (_, _, live_before) =
            run_until_killed(&case, Seed::HandWritten, Entry::PerApp, env.home());

        // 接管期间用户在应用里改了另一个条目的 Key
        let edited = EntryEndpoint {
            base_url: Some("https://other.example.com/v1".to_string()),
            api_key: Some("sk-edited".to_string()),
        };
        entries
            .write_endpoint("hand-only", &edited)
            .expect("edit another entry");

        assert!(restart_and_recover(), "{}: recovered", case.slug);
        let live = adapter(&case.app).read_live().expect("read restored live");
        assert_eq!(
            entry_endpoint(entries, &live, PROVIDER_ID),
            entry_endpoint(entries, &live_before, PROVIDER_ID),
            "{}: the taken-over entry is restored",
            case.slug
        );
        assert_eq!(
            entry_endpoint(entries, &live, "hand-only"),
            edited,
            "{}: recovery must not roll back edits to other entries",
            case.slug
        );
    }
}

#[test]
fn additive_provider_save_during_takeover_keeps_the_proxy_endpoint() {
    for (case, entries) in entry_cases() {
        let env = TestEnv::new();
        let db = Arc::new(Database::init().expect("open database"));
        let state = AppState::new(db.clone());
        let rt = process_runtime();
        rt.block_on(async {
            let mut proxy_config = db.get_proxy_config().await.expect("proxy config");
            proxy_config.listen_port = 0;
            db.update_proxy_config(proxy_config)
                .await
                .expect("use an ephemeral proxy port");
        });
        seed_live(&case, Seed::HandWritten, &db);
        let _ = env.home();

        rt.block_on(state.proxy_service.set_takeover_for_app(case.slug, true))
            .unwrap_or_else(|e| panic!("{}: takeover: {e}", case.slug));
        let proxied = entry_endpoint(
            entries,
            &adapter(&case.app).read_live().expect("read live"),
            PROVIDER_ID,
        );

        // 接管期间在 CC Switch 里轮换该供应商的 Key
        let mut provider = db
            .get_provider_by_id(PROVIDER_ID, case.slug)
            .expect("read provider")
            .expect("provider");
        let mut rotated = serde_json::to_string(&provider.settings_config).unwrap();
        rotated = rotated.replace(API_KEY, "sk-rotated");
        provider.settings_config = serde_json::from_str(&rotated).unwrap();
        ProviderService::update(&state, case.app.clone(), None, provider)
            .unwrap_or_else(|e| panic!("{}: update provider: {e}", case.slug));

        let live = adapter(&case.app).read_live().expect("read live");
        assert_eq!(
            entry_endpoint(entries, &live, PROVIDER_ID),
            proxied,
            "{}: saving the provider keeps the entry on the proxy",
            case.slug
        );

        rt.block_on(state.proxy_service.set_takeover_for_app(case.slug, false))
            .unwrap_or_else(|e| panic!("{}: disable takeover: {e}", case.slug));
        let live = adapter(&case.app).read_live().expect("read restored live");
        assert_eq!(
            entry_endpoint(entries, &live, PROVIDER_ID),
            EntryEndpoint {
                base_url: Some(format!("{UPSTREAM}/v1")),
                api_key: Some("sk-rotated".to_string()),
            },
            "{}: disabling takeover restores the rotated key",
            case.slug
        );
    }
}
