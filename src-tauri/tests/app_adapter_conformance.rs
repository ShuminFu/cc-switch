//! Adapter conformance suite: one contract, looped over every entry in `APPS`.
//!
//! Every managed app gets the same checks, so a new app is held to them the
//! moment it is added to the registry:
//!
//! - credentials round-trip: provider settings -> `credentials()` /
//!   `usage_credentials()`, and through the live writer and reader back again;
//! - MCP upsert then remove leaves no trace of the server;
//! - prompt and skills paths live under the app's (isolated) config home, and
//!   `PromptService` writes the file the adapter names;
//! - proxy takeover then cleanup restores the original live bytes;
//! - registry caps match the optional traits the adapter implements;
//! - the Codex `model_provider` id stays the one migrated history points at.
//!
//! A test-only ninth app (`DummyAdapter`) runs through the adapter-level
//! contract too. It is not an `AppType` variant: what adding a real ninth app
//! costs in backend files is measured by
//! `new_app_backend_touch_points_stay_within_budget`, and that it needs no
//! schema migration by `dummy_app_needs_no_schema_migration`.

// 测试夹具直接写文件即可，无需走 atomic_write（见 clippy.toml）
#![allow(clippy::disallowed_methods)]

mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use cc_switch_lib::live_config_testkit::{
    self as kit, adapter, AppAdapter, AppCaps, Credentials, SessionSource, SyncMode, APPS,
};
use cc_switch_lib::{
    AppError, AppType, Database, Prompt, PromptService, Provider, ProviderService,
};
use serde_json::{json, Value};
use support::{create_test_state, TestEnv};

const API_KEY: &str = "sk-conformance-key";
const BASE_URL: &str = "https://conformance.example.com/v1";
const PROVIDER_ID: &str = "conformance-provider";
const MCP_SERVER_ID: &str = "ccs-conformance-mcp";

// ============================================================================
// Cases
// ============================================================================

/// 一个待校验的应用：注册表里的真实应用或测试专用的第九个应用
struct Case<'a> {
    slug: &'a str,
    caps: AppCaps,
    sync_mode: SyncMode,
    adapter: &'a dyn AppAdapter,
    fixture: Fixture,
}

/// 每个应用的供应商样例：凭据为 [`API_KEY`] / [`BASE_URL`]
struct Fixture {
    settings: Value,
    /// 从 `read_live()` 的结果里取出该供应商的 `settings_config`；
    /// `None` 表示该应用没有通用 live 读取（Claude Desktop 走专用写入路径）
    live_entry: Option<fn(&Value, &str) -> Value>,
}

fn whole_live(live: &Value, _id: &str) -> Value {
    live.clone()
}

/// 每个注册应用都必须在这里有样例；新增应用时缺失会直接让套件失败
fn fixture_for(slug: &str) -> Option<Fixture> {
    let anthropic_env = json!({
        "env": {
            "ANTHROPIC_AUTH_TOKEN": API_KEY,
            "ANTHROPIC_BASE_URL": BASE_URL
        }
    });
    let fixture = match slug {
        "claude" => Fixture {
            settings: anthropic_env,
            live_entry: Some(whole_live),
        },
        "claude-desktop" => Fixture {
            settings: anthropic_env,
            live_entry: None,
        },
        "codex" => Fixture {
            settings: json!({
                "auth": { "OPENAI_API_KEY": API_KEY },
                "config": format!(
                    "model_provider = \"{id}\"\nmodel = \"gpt-5.4\"\n\n[model_providers.{id}]\nname = \"Conformance\"\nbase_url = \"{BASE_URL}\"\nwire_api = \"responses\"\nrequires_openai_auth = true\n",
                    id = kit::CC_SWITCH_CODEX_MODEL_PROVIDER_ID
                )
            }),
            live_entry: Some(whole_live),
        },
        "gemini" => Fixture {
            settings: json!({
                "env": {
                    "GEMINI_API_KEY": API_KEY,
                    "GOOGLE_GEMINI_BASE_URL": BASE_URL,
                    "GEMINI_MODEL": "gemini-2.5-pro"
                }
            }),
            live_entry: Some(whole_live),
        },
        "grokbuild" => Fixture {
            settings: json!({
                "config": format!(
                    "[models]\ndefault = \"grok-4.5\"\n\n[model.\"grok-4.5\"]\nmodel = \"grok-4.5\"\nbase_url = \"{BASE_URL}\"\nname = \"Conformance\"\napi_key = \"{API_KEY}\"\napi_backend = \"responses\"\ncontext_window = 500000\n"
                )
            }),
            live_entry: Some(whole_live),
        },
        "opencode" => Fixture {
            settings: json!({
                "npm": "@ai-sdk/openai-compatible",
                "name": "Conformance",
                "options": { "baseURL": BASE_URL, "apiKey": API_KEY },
                "models": { "conformance-model": { "name": "Conformance Model" } }
            }),
            live_entry: Some(|live, id| live["provider"][id].clone()),
        },
        "openclaw" => Fixture {
            settings: json!({
                "baseUrl": BASE_URL,
                "apiKey": API_KEY,
                "api": "openai-completions",
                "models": [{ "id": "conformance-model", "name": "Conformance Model" }]
            }),
            live_entry: Some(|live, id| live["models"]["providers"][id].clone()),
        },
        "hermes" => Fixture {
            settings: json!({
                "name": PROVIDER_ID,
                "base_url": BASE_URL,
                "api_key": API_KEY,
                "model": "conformance-model"
            }),
            live_entry: Some(|live, id| {
                live["custom_providers"]
                    .as_array()
                    .and_then(|providers| {
                        providers
                            .iter()
                            .find(|provider| provider["name"] == id)
                            .cloned()
                    })
                    .unwrap_or(Value::Null)
            }),
        },
        _ => return None,
    };
    Some(fixture)
}

/// `APPS` 中的全部应用
fn registry_cases() -> Vec<Case<'static>> {
    APPS.iter()
        .map(|descriptor| Case {
            slug: descriptor.slug,
            caps: descriptor.caps,
            sync_mode: descriptor.sync_mode,
            adapter: adapter(&descriptor.app),
            fixture: fixture_for(descriptor.slug).unwrap_or_else(|| {
                panic!(
                    "{} has no conformance fixture; add one to fixture_for()",
                    descriptor.slug
                )
            }),
        })
        .collect()
}

fn app_type(case: &Case<'_>) -> AppType {
    AppType::from_str(case.slug).expect("registry slug parses")
}

fn provider(settings: Value) -> Provider {
    Provider::with_id(
        PROVIDER_ID.to_string(),
        "Conformance".to_string(),
        settings,
        None,
    )
}

fn expected_credentials() -> Credentials {
    Credentials {
        api_key: API_KEY.to_string(),
        base_url: BASE_URL.to_string(),
    }
}

// ============================================================================
// Test-only ninth app
// ============================================================================

/// 测试专用的第九个应用：切换模式，`~/.dummy-cli/config.json` 承载当前供应商，
/// 只有提示词能力（与 OpenClaw 相同的能力面，不需要按应用的数据库列）。
#[derive(Debug)]
struct DummyAdapter {
    home: PathBuf,
}

impl DummyAdapter {
    fn dir(&self) -> PathBuf {
        self.home.join(".dummy-cli")
    }

    fn config_path(&self) -> PathBuf {
        self.dir().join("config.json")
    }
}

fn dummy_error(message: &str) -> AppError {
    AppError::Message(format!("dummy app: {message}"))
}

impl AppAdapter for DummyAdapter {
    fn config_dir(&self) -> Result<PathBuf, AppError> {
        Ok(self.dir())
    }

    fn prompt_file(&self) -> Option<PathBuf> {
        Some(self.dir().join("AGENTS.md"))
    }

    fn skills_dir(&self) -> Option<PathBuf> {
        Some(self.dir().join("skills"))
    }

    fn read_live(&self) -> Result<Value, AppError> {
        let text = fs::read_to_string(self.config_path())
            .map_err(|e| dummy_error(&format!("read config: {e}")))?;
        serde_json::from_str(&text).map_err(|e| dummy_error(&format!("parse config: {e}")))
    }

    fn import_live(&self) -> Result<Value, AppError> {
        self.read_live()
    }

    fn write_provider(&self, provider: &Provider) -> Result<(), AppError> {
        fs::create_dir_all(self.dir()).map_err(|e| dummy_error(&e.to_string()))?;
        let text = serde_json::to_string_pretty(&provider.settings_config)
            .map_err(|e| dummy_error(&e.to_string()))?;
        fs::write(self.config_path(), text).map_err(|e| dummy_error(&e.to_string()))
    }

    fn validate_provider(&self, provider: &Provider) -> Result<(), AppError> {
        if provider
            .settings_config
            .get("api")
            .is_some_and(Value::is_object)
        {
            Ok(())
        } else {
            Err(dummy_error("settings must have an `api` object"))
        }
    }

    fn credentials(&self, provider: &Provider) -> Result<Credentials, AppError> {
        let (base_url, api_key) = self.usage_credentials(&provider.settings_config);
        if api_key.is_empty() || base_url.is_empty() {
            return Err(dummy_error("missing api.key or api.url"));
        }
        Ok(Credentials { api_key, base_url })
    }

    fn usage_credentials(&self, settings: &Value) -> (String, String) {
        let field = |name: &str| {
            settings["api"][name]
                .as_str()
                .unwrap_or_default()
                .to_string()
        };
        (field("url"), field("key"))
    }

    fn session_source(&self) -> Option<&dyn SessionSource> {
        Some(self)
    }
}

impl SessionSource for DummyAdapter {
    fn session_roots(&self) -> Vec<PathBuf> {
        vec![self.dir().join("sessions")]
    }
}

fn dummy_case(dummy: &DummyAdapter) -> Case<'_> {
    Case {
        slug: "dummy",
        caps: AppCaps::PROMPTS,
        sync_mode: SyncMode::Switch,
        adapter: dummy,
        fixture: Fixture {
            settings: json!({ "api": { "url": BASE_URL, "key": API_KEY } }),
            live_entry: Some(whole_live),
        },
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// HOME 下所有文件的字节快照（不含 CC Switch 自己的数据目录）
fn snapshot(home: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(dir: &Path, home: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
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

/// 快照中包含 `needle` 的文件
fn files_containing(snapshot: &BTreeMap<PathBuf, Vec<u8>>, needle: &str) -> Vec<PathBuf> {
    snapshot
        .iter()
        .filter(|(_, bytes)| String::from_utf8_lossy(bytes).contains(needle))
        .map(|(path, _)| path.clone())
        .collect()
}

fn describe_diff(
    before: &BTreeMap<PathBuf, Vec<u8>>,
    after: &BTreeMap<PathBuf, Vec<u8>>,
) -> String {
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

// ============================================================================
// Contract
// ============================================================================

fn assert_credentials_round_trip(case: &Case<'_>) {
    let slug = case.slug;
    let adapter = case.adapter;
    let provider = provider(case.fixture.settings.clone());

    adapter
        .validate_provider(&provider)
        .unwrap_or_else(|e| panic!("{slug}: fixture fails validation: {e}"));
    assert_eq!(
        adapter
            .credentials(&provider)
            .unwrap_or_else(|e| panic!("{slug}: credentials(): {e}")),
        expected_credentials(),
        "{slug}: credentials()"
    );
    assert_eq!(
        adapter.usage_credentials(&provider.settings_config),
        (BASE_URL.to_string(), API_KEY.to_string()),
        "{slug}: usage_credentials()"
    );
    assert_eq!(
        adapter.usage_credentials(&json!({})),
        (String::new(), String::new()),
        "{slug}: usage_credentials() on empty settings"
    );
    let empty = Provider::with_id("empty".to_string(), "E".to_string(), json!({}), None);
    assert!(
        adapter.credentials(&empty).is_err(),
        "{slug}: credentials() must reject settings without a key"
    );

    let Some(live_entry) = case.fixture.live_entry else {
        return;
    };
    adapter
        .write_provider(&provider)
        .unwrap_or_else(|e| panic!("{slug}: write_provider: {e}"));
    let live = adapter
        .read_live()
        .unwrap_or_else(|e| panic!("{slug}: read_live after write: {e}"));
    let back = Provider::with_id(
        PROVIDER_ID.to_string(),
        "Conformance".to_string(),
        live_entry(&live, PROVIDER_ID),
        None,
    );
    assert_eq!(
        adapter
            .credentials(&back)
            .unwrap_or_else(|e| panic!("{slug}: credentials from live: {e}\n{live:#}")),
        expected_credentials(),
        "{slug}: credentials after write_provider -> read_live"
    );

    match case.sync_mode {
        SyncMode::Additive => {
            assert!(
                adapter
                    .live_has_provider(PROVIDER_ID)
                    .expect("live_has_provider"),
                "{slug}: written provider is in live"
            );
            adapter
                .remove_provider(PROVIDER_ID)
                .unwrap_or_else(|e| panic!("{slug}: remove_provider: {e}"));
            assert!(
                !adapter
                    .live_has_provider(PROVIDER_ID)
                    .expect("live_has_provider"),
                "{slug}: removed provider is gone from live"
            );
        }
        SyncMode::Switch => {
            // 切换模式应用的 live 只承载当前供应商：remove 是 no-op
            adapter.remove_provider(PROVIDER_ID).expect("remove no-op");
            assert!(!adapter.live_has_provider(PROVIDER_ID).expect("no-op"));
            assert_eq!(adapter.read_live().expect("live kept"), live);
        }
    }
}

fn assert_mcp_upsert_and_remove(case: &Case<'_>, home: &Path) {
    let slug = case.slug;
    let Some(mcp) = case.adapter.mcp() else {
        return;
    };
    if let Ok(dir) = case.adapter.config_dir() {
        fs::create_dir_all(dir).expect("create config dir");
    }
    let before = snapshot(home);
    assert!(files_containing(&before, MCP_SERVER_ID).is_empty());

    let spec = json!({ "type": "stdio", "command": "conformance-mcp", "args": ["--v1"] });
    mcp.upsert(MCP_SERVER_ID, &spec)
        .unwrap_or_else(|e| panic!("{slug}: mcp upsert: {e}"));
    let inserted = snapshot(home);
    let holders = files_containing(&inserted, MCP_SERVER_ID);
    assert!(
        !holders.is_empty(),
        "{slug}: mcp upsert wrote nothing:\n{}",
        describe_diff(&before, &inserted)
    );

    let updated_spec = json!({ "type": "stdio", "command": "conformance-mcp", "args": ["--v2"] });
    mcp.upsert(MCP_SERVER_ID, &updated_spec)
        .unwrap_or_else(|e| panic!("{slug}: mcp re-upsert: {e}"));
    let updated = snapshot(home);
    assert_eq!(
        files_containing(&updated, MCP_SERVER_ID),
        holders,
        "{slug}: re-upsert writes the same files"
    );
    assert!(
        files_containing(&updated, "--v2").len() == holders.len()
            && files_containing(&updated, "--v1").is_empty(),
        "{slug}: re-upsert replaces the server spec:\n{}",
        describe_diff(&inserted, &updated)
    );

    mcp.remove(MCP_SERVER_ID)
        .unwrap_or_else(|e| panic!("{slug}: mcp remove: {e}"));
    let removed = snapshot(home);
    assert!(
        files_containing(&removed, MCP_SERVER_ID).is_empty(),
        "{slug}: mcp remove left the server behind:\n{}",
        describe_diff(&before, &removed)
    );
    mcp.remove(MCP_SERVER_ID)
        .unwrap_or_else(|e| panic!("{slug}: removing a missing server must be a no-op: {e}"));
}

fn assert_paths(case: &Case<'_>, home: &Path) {
    let slug = case.slug;
    let adapter = case.adapter;
    let under_home = |what: &str, path: &Path| {
        assert!(
            path.starts_with(home),
            "{slug}: {what} {} escapes the test HOME {}",
            path.display(),
            home.display()
        );
    };

    // Claude Desktop 的 3P 配置库只在 macOS / Windows 上有路径
    let config_dir = adapter.config_dir().ok();
    if let Some(dir) = &config_dir {
        under_home("config_dir", dir);
    }

    match adapter.prompt_file() {
        Some(prompt) => {
            under_home("prompt_file", &prompt);
            let name = prompt.file_name().unwrap().to_string_lossy().into_owned();
            assert!(
                ["CLAUDE.md", "AGENTS.md", "GEMINI.md"].contains(&name.as_str()),
                "{slug}: unexpected prompt file name {name}"
            );
            assert_eq!(
                prompt.parent(),
                config_dir.as_deref(),
                "{slug}: prompt file sits in the config dir"
            );
        }
        None => assert!(!case.caps.contains(AppCaps::PROMPTS)),
    }

    if let Some(skills) = adapter.skills_dir() {
        under_home("skills_dir", &skills);
        assert_eq!(skills.file_name().unwrap(), "skills", "{slug}: skills dir");
    }
    if case.caps.contains(AppCaps::SKILLS) {
        assert!(adapter.skills_dir().is_some(), "{slug}: SKILLS needs a dir");
    }

    if let Some(source) = adapter.session_source() {
        let roots = source.session_roots();
        assert!(!roots.is_empty(), "{slug}: session roots");
        // 用户显式设置的 XDG_DATA_HOME 是合法的会话根（OpenCode 优先遵循它）
        let xdg_data_home = std::env::var_os("XDG_DATA_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        for root in roots {
            if xdg_data_home
                .as_deref()
                .is_some_and(|xdg| root.starts_with(xdg))
            {
                continue;
            }
            under_home("session root", &root);
        }
    }
}

fn assert_caps_match_traits(case: &Case<'_>) {
    let slug = case.slug;
    let adapter = case.adapter;
    assert_eq!(
        adapter.prompt_file().is_some(),
        case.caps.contains(AppCaps::PROMPTS),
        "{slug}: prompt_file() vs PROMPTS"
    );
    assert_eq!(
        adapter.mcp().is_some(),
        case.caps.contains(AppCaps::MCP),
        "{slug}: mcp() vs MCP"
    );
    assert_eq!(
        adapter.takeover().is_some(),
        case.caps.contains(AppCaps::PROXY_TAKEOVER),
        "{slug}: takeover() vs PROXY_TAKEOVER"
    );
    if case.sync_mode == SyncMode::Additive {
        assert!(
            adapter.import_live().is_err(),
            "{slug}: additive apps import per provider, not as `default`"
        );
    }
    if let Some(takeover) = adapter.takeover() {
        let settings = case.fixture.settings.clone();
        assert!(
            !takeover.has_placeholder(&settings),
            "{slug}: a real key is not a takeover placeholder"
        );
        assert!(
            !takeover.official_provider_supported(&provider(settings)),
            "{slug}: a custom provider is not an official one"
        );
    }
}

// ============================================================================
// Tests over APPS
// ============================================================================

#[test]
fn every_registered_app_has_a_fixture() {
    assert_eq!(registry_cases().len(), APPS.len());
}

#[test]
fn credentials_round_trip_for_every_app() {
    for case in registry_cases() {
        let _env = TestEnv::new();
        assert_credentials_round_trip(&case);
    }
}

#[test]
fn claude_desktop_profile_carries_the_credentials() {
    let _env = TestEnv::new();
    let dirs = tempfile::tempdir().expect("tempdir");
    let normal = dirs.path().join("normal");
    let threep = dirs.path().join("threep");
    let db = Database::init().expect("init db");
    let settings = fixture_for("claude-desktop").unwrap().settings;
    kit::apply_claude_desktop_provider_at(&db, &provider(settings), &normal, &threep)
        .expect("apply desktop profile");

    let written = snapshot(dirs.path());
    assert!(
        !files_containing(&written, BASE_URL).is_empty()
            && !files_containing(&written, API_KEY).is_empty(),
        "desktop 3P profile must carry the direct gateway credentials: {:?}",
        written.keys().collect::<Vec<_>>()
    );
}

#[test]
fn mcp_upsert_and_remove_for_every_app() {
    for case in registry_cases() {
        let env = TestEnv::new();
        assert_mcp_upsert_and_remove(&case, env.home());
    }
}

#[test]
fn prompt_and_skills_paths_for_every_app() {
    for case in registry_cases() {
        let env = TestEnv::new();
        assert_paths(&case, env.home());

        if !case.caps.contains(AppCaps::PROMPTS) {
            continue;
        }
        let state = create_test_state().expect("state");
        let content = format!("# conformance prompt for {}\n", case.slug);
        PromptService::upsert_prompt(
            &state,
            app_type(&case),
            "conformance",
            Prompt {
                id: "conformance".to_string(),
                name: "Conformance".to_string(),
                content: content.clone(),
                description: None,
                enabled: true,
                created_at: None,
                updated_at: None,
            },
        )
        .unwrap_or_else(|e| panic!("{}: upsert prompt: {e}", case.slug));
        let prompt_file = case.adapter.prompt_file().unwrap();
        assert_eq!(
            fs::read_to_string(&prompt_file).expect("read prompt file"),
            content,
            "{}: PromptService writes adapter.prompt_file()",
            case.slug
        );
    }
}

#[test]
fn caps_match_implemented_traits_for_every_app() {
    for case in registry_cases() {
        assert_caps_match_traits(&case);
    }
}

/// 切到该供应商，开启接管再关闭，返回 (接管前, 接管中, 关闭后) 的 HOME 快照
async fn takeover_cycle(case: &Case<'_>, home: &Path) -> [BTreeMap<PathBuf, Vec<u8>>; 3] {
    let slug = case.slug;
    let app = app_type(case);
    let state = create_test_state().expect("state");
    let mut proxy_config = state.db.get_proxy_config().await.expect("proxy config");
    proxy_config.listen_port = 0;
    state
        .db
        .update_proxy_config(proxy_config)
        .await
        .expect("use an ephemeral proxy port");

    fs::create_dir_all(case.adapter.config_dir().unwrap()).expect("config dir");
    state
        .db
        .save_provider(slug, &provider(case.fixture.settings.clone()))
        .expect("save provider");
    ProviderService::switch(&state, app, PROVIDER_ID)
        .unwrap_or_else(|e| panic!("{slug}: switch: {e}"));
    let before = snapshot(home);

    state
        .proxy_service
        .set_takeover_for_app(slug, true)
        .await
        .unwrap_or_else(|e| panic!("{slug}: enable takeover: {e}"));
    let during = snapshot(home);
    let takeover = case.adapter.takeover().unwrap();
    assert!(
        takeover.has_placeholder(&case.adapter.read_live().expect("read live")),
        "{slug}: live carries the takeover placeholder:\n{}",
        describe_diff(&before, &during)
    );

    state
        .proxy_service
        .set_takeover_for_app(slug, false)
        .await
        .unwrap_or_else(|e| panic!("{slug}: disable takeover: {e}"));
    let after = snapshot(home);
    [before, during, after]
}

#[tokio::test]
async fn takeover_apply_then_cleanup_restores_original_bytes() {
    let cases = registry_cases();
    let takeover_cases: Vec<_> = cases
        .iter()
        .filter(|case| case.caps.contains(AppCaps::PROXY_TAKEOVER))
        .collect();
    assert!(!takeover_cases.is_empty());

    for case in takeover_cases {
        let env = TestEnv::new();
        let [before, during, after] = takeover_cycle(case, env.home()).await;
        assert_ne!(before, during, "{}: takeover rewrote nothing", case.slug);
        // 客户端流量必须改走本地代理。凭据不一定被替换：Codex 有意保留
        // auth.json（登录材料），由 config.toml 里的代理 bearer token 生效。
        assert!(
            files_containing(&during, BASE_URL).is_empty(),
            "{}: live must not point at the upstream during takeover: {:?}\n{}",
            case.slug,
            files_containing(&during, BASE_URL),
            describe_diff(&before, &during)
        );
        assert!(
            before == after,
            "{}: takeover cleanup must restore the original bytes:\n{}",
            case.slug,
            describe_diff(&before, &after)
        );
    }
}

/// 迁移过的 Codex 会话历史把 `threads.model_provider` 写成了这个 id；
/// 写 live、接管、恢复都不能换桶。
#[tokio::test]
async fn codex_model_provider_id_is_stable() {
    assert_eq!(kit::CC_SWITCH_CODEX_MODEL_PROVIDER_ID, "custom");

    let env = TestEnv::new();
    let case = registry_cases()
        .into_iter()
        .find(|case| case.slug == "codex")
        .expect("codex is registered");
    let config_path = PathBuf::from(".codex").join("config.toml");
    let model_provider = |snapshot: &BTreeMap<PathBuf, Vec<u8>>| -> Option<String> {
        let text = String::from_utf8(snapshot.get(&config_path)?.clone()).ok()?;
        let table: toml::Table = toml::from_str(&text).ok()?;
        table.get("model_provider")?.as_str().map(str::to_string)
    };

    let phases = takeover_cycle(&case, env.home()).await;
    for (phase, snapshot) in ["switch", "takeover", "restore"].iter().zip(&phases) {
        assert_eq!(
            model_provider(snapshot).as_deref(),
            Some(kit::CC_SWITCH_CODEX_MODEL_PROVIDER_ID),
            "codex model_provider after {phase}"
        );
    }
}

// ============================================================================
// Test-only ninth app
// ============================================================================

#[test]
fn dummy_ninth_app_passes_the_adapter_contract() {
    let env = TestEnv::new();
    let dummy = DummyAdapter {
        home: env.home().to_path_buf(),
    };
    let case = dummy_case(&dummy);
    assert!(
        APPS.iter().all(|descriptor| descriptor.slug != case.slug),
        "the dummy slug must not collide with a registered app"
    );

    assert_caps_match_traits(&case);
    assert_paths(&case, env.home());
    assert_credentials_round_trip(&case);
    assert_mcp_upsert_and_remove(&case, env.home());
    assert!(dummy.mcp().is_none() && dummy.takeover().is_none());
}

/// 切换模式、无接管、无 MCP / Skills 开关的新应用只用到以 `app_type` 文本为键的表，
/// 不需要新增迁移。接管（proxy_config 的 CHECK 约束）与 MCP / Skills
///（按应用的 enabled_* 列，见 CCS-156）目前仍需要迁移。
#[test]
fn dummy_app_needs_no_schema_migration() {
    let _env = TestEnv::new();
    let db = Database::init().expect("fresh db");
    let slug = "dummy";
    let provider = provider(json!({ "api": { "url": BASE_URL, "key": API_KEY } }));

    db.save_provider(slug, &provider).expect("save provider");
    db.set_current_provider(slug, PROVIDER_ID)
        .expect("set current provider");
    assert_eq!(
        db.get_current_provider(slug).expect("current provider"),
        Some(PROVIDER_ID.to_string())
    );
    let stored = db.get_all_providers(slug).expect("providers");
    assert_eq!(
        stored[PROVIDER_ID].settings_config,
        provider.settings_config
    );

    let prompt = Prompt {
        id: "p".to_string(),
        name: "P".to_string(),
        content: "hello".to_string(),
        description: None,
        enabled: true,
        created_at: None,
        updated_at: None,
    };
    db.save_prompt(slug, &prompt).expect("save prompt");
    assert_eq!(db.get_prompts(slug).expect("prompts")["p"].content, "hello");

    // 其它应用不受影响
    for descriptor in APPS {
        assert!(db
            .get_all_providers(descriptor.slug)
            .expect("providers")
            .is_empty());
    }
}

// ============================================================================
// Cost of a ninth app
// ============================================================================

/// 新增应用时必然要改的后端文件（不含新建的 `apps/<app>/`）
const NEW_APP_FIXED_FILES: [&str; 3] = ["app_config.rs", "apps/registry.rs", "apps/mod.rs"];

/// 新增一个应用需要改动的后端文件数上限（含新建的 `apps/<app>/mod.rs`），只允许下降。
///
/// P4 目标是 6 个以内。当前的 9 个与一次实测一致：给 `AppType` 加一个变体并在
/// 注册表 / `apps::adapter()` 接入后，编译器只在 [`exhaustive_app_match_files`]
/// 列出的文件里报不穷尽 match。CCS-155 之后 settings.rs 已不在其中；剩余文件随
/// app_config.rs 以及 deeplink / paths / services/config.rs / proxy/providers 的
/// 分派收敛下降。
const NEW_APP_BACKEND_FILE_BUDGET: usize = 9;

/// 去掉顶层的 `#[cfg(test)]` 条目：带花括号的块跳到列 0 的 `}`，单行声明
///（`mod foo;`）只跳过该行
fn production_source(src: &str) -> String {
    let mut out = String::new();
    let mut lines = src.lines();
    while let Some(line) = lines.next() {
        if line != "#[cfg(test)]" {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let Some(item) = lines.next() else {
            break;
        };
        if item.trim_end().ends_with('{') {
            for rest in lines.by_ref() {
                if rest == "}" {
                    break;
                }
            }
        }
    }
    out
}

/// `#[cfg(test)] mod foo;` 声明的子模块文件（相对 src）
fn test_only_module_files(rel: &str, src: &str) -> Vec<String> {
    let dir = match rel.rsplit_once('/') {
        Some((parent, "mod.rs")) => format!("{parent}/"),
        Some((parent, file)) => format!("{parent}/{}/", file.trim_end_matches(".rs")),
        None if rel == "lib.rs" || rel == "main.rs" => String::new(),
        None => format!("{}/", rel.trim_end_matches(".rs")),
    };
    let lines: Vec<&str> = src.lines().map(str::trim).collect();
    lines
        .windows(2)
        .filter(|pair| pair[0] == "#[cfg(test)]")
        .filter_map(|pair| {
            let name = pair[1]
                .trim_start_matches("pub ")
                .strip_prefix("mod ")?
                .strip_suffix(';')?;
            Some(name.to_string())
        })
        .flat_map(|name| [format!("{dir}{name}.rs"), format!("{dir}{name}/")])
        .collect()
}

/// 是否为通配分支（`_ =>`、`_ if ..`、`other =>`）
fn is_catch_all_arm(line: &str) -> bool {
    let Some((pattern, _)) = line.split_once("=>") else {
        return false;
    };
    let pattern = pattern.split(" if ").next().unwrap_or("").trim();
    pattern == "_"
        || (!pattern.is_empty()
            && pattern
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
}

/// 以 `AppType::` 变体为分支、没有通配分支的 match 个数
fn count_exhaustive_app_matches(production: &str) -> usize {
    let lines: Vec<&str> = production
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("//"))
        .collect();
    let mut count = 0;
    for (i, line) in lines.iter().enumerate() {
        let is_match = line.ends_with('{')
            && line
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .any(|word| word == "match");
        if !is_match {
            continue;
        }
        let Some(first_arm) = lines[i + 1..].iter().find(|l| !l.starts_with('#')) else {
            continue;
        };
        if !(first_arm.starts_with("AppType::")
            || first_arm.starts_with("crate::app_config::AppType::"))
        {
            continue;
        }
        // 扫描到 match 结束，只看深度 1 上的分支
        let mut depth = 1i32;
        let mut catch_all = false;
        for arm in &lines[i + 1..] {
            if depth == 1 && is_catch_all_arm(arm) {
                catch_all = true;
            }
            depth += arm.matches('{').count() as i32;
            depth -= arm.matches('}').count() as i32;
            if depth <= 0 {
                break;
            }
        }
        if !catch_all {
            count += 1;
        }
    }
    count
}

/// 生产代码中含有不穷尽即编译失败的 `AppType` match 的文件及其个数，
/// 即新增 `AppType` 变体后编译器会要求补分支的文件。
fn exhaustive_app_match_files() -> BTreeMap<String, usize> {
    fn collect(dir: &Path, root: &Path, out: &mut Vec<(String, String)>) {
        let mut entries: Vec<_> = fs::read_dir(dir)
            .expect("read src dir")
            .map(|entry| entry.expect("dir entry").path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                collect(&path, root, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((rel, fs::read_to_string(&path).expect("read source")));
            }
        }
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut sources = Vec::new();
    collect(&root, &root, &mut sources);
    let test_only: Vec<String> = sources
        .iter()
        .flat_map(|(rel, src)| test_only_module_files(rel, src))
        .collect();

    sources
        .iter()
        .filter(|(rel, _)| {
            !test_only.iter().any(|excluded| {
                rel == excluded || rel.starts_with(excluded.as_str()) && excluded.ends_with('/')
            })
        })
        .map(|(rel, src)| {
            (
                rel.clone(),
                count_exhaustive_app_matches(&production_source(src)),
            )
        })
        .filter(|(_, count)| *count > 0)
        .collect()
}

#[test]
fn new_app_backend_touch_points_stay_within_budget() {
    let exhaustive = exhaustive_app_match_files();
    assert!(
        exhaustive.contains_key("apps/mod.rs"),
        "the counter must see apps::adapter()'s exhaustive match: {exhaustive:?}"
    );

    let mut touched: BTreeSet<String> = exhaustive.keys().cloned().collect();
    touched.extend(NEW_APP_FIXED_FILES.iter().map(|file| file.to_string()));
    touched.insert("apps/<new app>/mod.rs".to_string());

    let listing = touched
        .iter()
        .map(|file| match exhaustive.get(file) {
            Some(count) => format!("{file} ({count} exhaustive match)"),
            None => file.clone(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        touched.len() <= NEW_APP_BACKEND_FILE_BUDGET,
        "a new app now touches {} backend files (budget {NEW_APP_BACKEND_FILE_BUDGET}); \
         dispatch through apps::adapter() or the registry instead:\n{listing}",
        touched.len()
    );
    println!(
        "backend files a new app touches ({}):\n{listing}",
        touched.len()
    );
}

/// 计数器自检：通配分支不计入，变体分支计入
#[test]
fn catch_all_arm_detection() {
    assert!(is_catch_all_arm("_ => None,"));
    assert!(is_catch_all_arm("_ if x => None,"));
    assert!(is_catch_all_arm("other => other.as_str(),"));
    assert!(!is_catch_all_arm("AppType::Claude => 1,"));
    assert!(!is_catch_all_arm("AppType::Claude | AppType::Codex => {"));
    assert!(!is_catch_all_arm("let x = 1;"));

    assert_eq!(
        test_only_module_files("proxy/usage/mod.rs", "#[cfg(test)]\nmod cost_golden;\n"),
        ["proxy/usage/cost_golden.rs", "proxy/usage/cost_golden/"]
    );
    assert_eq!(
        test_only_module_files("lib.rs", "#[cfg(test)]\nmod bindings;\n"),
        ["bindings.rs", "bindings/"]
    );
    let src = "#[cfg(test)]\nmod helpers;\nfn f(app: &AppType) {\n    match app {\n        AppType::Claude => 1,\n        AppType::Codex => {\n            2\n        }\n    };\n    match app {\n        AppType::Claude => 1,\n        _ => 0,\n    };\n}\n#[cfg(test)]\nmod tests {\n    fn g(app: &AppType) {\n        match app {\n            AppType::Claude => 1,\n        };\n    }\n}\n";
    assert_eq!(count_exhaustive_app_matches(&production_source(src)), 1);
}

#[test]
fn dummy_adapter_rejects_foreign_shapes() {
    let env = TestEnv::new();
    let dummy = DummyAdapter {
        home: env.home().to_path_buf(),
    };
    assert!(dummy.read_live().is_err(), "no live before the first write");
    let foreign = provider(json!({ "env": { "ANTHROPIC_AUTH_TOKEN": API_KEY } }));
    assert!(dummy.validate_provider(&foreign).is_err());
}
