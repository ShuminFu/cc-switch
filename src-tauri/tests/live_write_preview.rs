// 测试夹具直接写文件即可，无需走 atomic_write（见 clippy.toml）
#![allow(clippy::disallowed_methods)]

//! `ProviderService::preview_live_write`（CCS-205）：预览不落盘，且预览出的内容
//! 与随后保存（`ProviderService::update` / `add`）真正写入的字节一致。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use cc_switch_lib::{
    write_codex_live_atomic, AppSet, AppState, AppType, LivePreviewSkipReason, LiveWritePreview,
    McpServer, MultiAppConfig, Provider, ProviderService,
};

#[path = "support.rs"]
mod support;
use support::{create_test_state_with_config, TestEnv};

/// `dir` 下全部文件的内容快照（相对路径 → 字节）
fn snapshot_tree(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else if let Ok(bytes) = std::fs::read(&path) {
                out.insert(path.strip_prefix(root).unwrap().to_path_buf(), bytes);
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

/// 预览不能改动 HOME 下任何文件（live 配置、备份、数据库以外的一切）
fn preview_without_touching_disk(
    home: &Path,
    state: &AppState,
    app_type: AppType,
    provider: &Provider,
) -> LiveWritePreview {
    let before = snapshot_tree(home);
    let preview =
        ProviderService::preview_live_write(state, app_type, Some(&provider.id), provider.clone())
            .expect("preview live write");
    let after = snapshot_tree(home);
    // 数据库文件不归预览管（SQLite 打开 / WAL 可能更新），只比较其余文件
    let strip_db = |tree: BTreeMap<PathBuf, Vec<u8>>| -> BTreeMap<PathBuf, Vec<u8>> {
        tree.into_iter()
            .filter(|(path, _)| !path.starts_with(".cc-switch"))
            .collect()
    };
    assert_eq!(
        strip_db(before.clone()).keys().collect::<Vec<_>>(),
        strip_db(after.clone()).keys().collect::<Vec<_>>(),
        "preview must not create or delete files"
    );
    assert!(
        strip_db(before) == strip_db(after),
        "preview must not modify live files"
    );
    let backups = home.join(".cc-switch").join("backups");
    assert!(
        !backups.exists() || snapshot_tree(&backups).is_empty(),
        "preview must not create config backups"
    );
    preview
}

/// 每个预览文件的 `before` 等于预览时的磁盘内容，`after` 等于保存后的磁盘字节
fn assert_preview_matches_disk_after_save(preview: &LiveWritePreview) {
    assert!(preview.writes_live, "preview should report a live write");
    assert!(
        !preview.files.is_empty(),
        "preview should list target files"
    );
    for file in &preview.files {
        let written = std::fs::read(&file.path).ok();
        assert_eq!(
            written.as_deref(),
            file.after.as_deref().map(str::as_bytes),
            "saved bytes of {} differ from the preview",
            file.path
        );
    }
}

fn codex_mcp_server() -> McpServer {
    McpServer {
        id: "echo-server".into(),
        name: "Echo Server".into(),
        server: json!({ "type": "stdio", "command": "echo" }),
        apps: AppSet::from_apps(&[AppType::Codex]),
        description: None,
        homepage: None,
        docs: None,
        tags: Vec::new(),
    }
}

fn state_with_providers(app_type: AppType, current: &str, providers: Vec<Provider>) -> AppState {
    let mut config = MultiAppConfig::default();
    {
        let manager = config.get_manager_mut(&app_type).expect("app manager");
        manager.current = current.to_string();
        for provider in providers {
            manager.providers.insert(provider.id.clone(), provider);
        }
    }
    create_test_state_with_config(&config).expect("create test state")
}

#[test]
fn preview_claude_equals_bytes_written_on_save() {
    let env = TestEnv::new();
    let provider = Provider::with_id(
        "relay".to_string(),
        "Relay".to_string(),
        json!({ "env": {
            "ANTHROPIC_AUTH_TOKEN": "sk-old",
            "ANTHROPIC_BASE_URL": "https://old.example.com"
        }}),
        None,
    );
    let state = state_with_providers(AppType::Claude, "relay", vec![provider.clone()]);
    ProviderService::switch(&state, AppType::Claude, "relay").expect("seed live");
    let settings_path = cc_switch_lib::get_claude_settings_path();
    let live_before = std::fs::read_to_string(&settings_path).expect("seeded settings.json");

    let mut edited = provider;
    edited.settings_config = json!({ "env": {
        "ANTHROPIC_AUTH_TOKEN": "sk-new",
        "ANTHROPIC_BASE_URL": "https://new.example.com"
    }});
    let preview = preview_without_touching_disk(env.home(), &state, AppType::Claude, &edited);

    let file = preview
        .files
        .iter()
        .find(|file| Path::new(&file.path) == settings_path)
        .expect("settings.json in preview");
    assert_eq!(file.before.as_deref(), Some(live_before.as_str()));
    let after = file.after.as_deref().expect("settings.json is written");
    assert!(after.contains("https://new.example.com") && !after.contains("sk-old"));

    ProviderService::update(&state, AppType::Claude, None, edited).expect("save provider");
    assert_preview_matches_disk_after_save(&preview);
}

#[test]
fn preview_codex_includes_mcp_reprojection_and_equals_saved_bytes() {
    let env = TestEnv::new();
    write_codex_live_atomic(&json!({ "OPENAI_API_KEY": "seed" }), Some(""))
        .expect("seed codex live");

    let provider = Provider::with_id(
        "relay".to_string(),
        "Relay".to_string(),
        json!({
            "auth": { "OPENAI_API_KEY": "sk-old" },
            "config": "model_provider = \"relay\"\nmodel = \"gpt-5\"\n\n[model_providers.relay]\nname = \"relay\"\nbase_url = \"https://old.example.com/v1\"\nwire_api = \"responses\"\n"
        }),
        None,
    );
    let mut config = MultiAppConfig::default();
    {
        let manager = config.get_manager_mut(&AppType::Codex).expect("codex");
        manager.current = "relay".to_string();
        manager
            .providers
            .insert("relay".to_string(), provider.clone());
    }
    config
        .mcp
        .servers
        .get_or_insert_with(Default::default)
        .insert("echo-server".into(), codex_mcp_server());
    let state = create_test_state_with_config(&config).expect("create test state");
    ProviderService::switch(&state, AppType::Codex, "relay").expect("seed live");

    let mut edited = provider;
    edited.settings_config["config"] = Value::String(
        "model_provider = \"relay\"\nmodel = \"gpt-5\"\n\n[model_providers.relay]\nname = \"relay\"\nbase_url = \"https://new.example.com/v1\"\nwire_api = \"responses\"\n"
            .to_string(),
    );
    let preview = preview_without_touching_disk(env.home(), &state, AppType::Codex, &edited);

    let config_path = cc_switch_lib::get_codex_config_path();
    let config_after = preview
        .files
        .iter()
        .find(|file| Path::new(&file.path) == config_path)
        .and_then(|file| file.after.clone())
        .expect("config.toml in preview");
    assert!(config_after.contains("https://new.example.com/v1"));
    assert!(
        config_after.contains("mcp_servers.echo-server"),
        "preview must re-project enabled MCP servers like the save does, got: {config_after}"
    );

    ProviderService::update(&state, AppType::Codex, None, edited).expect("save provider");
    assert_preview_matches_disk_after_save(&preview);
}

#[test]
fn preview_gemini_sees_its_own_pending_writes_and_equals_saved_bytes() {
    let env = TestEnv::new();
    let provider = Provider::with_id(
        "packy".to_string(),
        "PackyCode".to_string(),
        json!({
            "env": {
                "GEMINI_API_KEY": "pk-old",
                "GOOGLE_GEMINI_BASE_URL": "https://www.packyapi.com"
            },
            "config": { "ui": { "theme": "dark" } }
        }),
        Some("https://www.packyapi.com".to_string()),
    );
    let state = state_with_providers(AppType::Gemini, "packy", vec![provider.clone()]);
    ProviderService::switch(&state, AppType::Gemini, "packy").expect("seed live");
    // 删掉 settings.json：预览中"先写 config 再改 selectedType"的第二步必须读到第一步
    std::fs::remove_file(env.home().join(".gemini").join("settings.json"))
        .expect("remove settings.json");

    let mut edited = provider;
    edited.settings_config["env"]["GEMINI_API_KEY"] = Value::String("pk-new".to_string());
    edited.settings_config["config"] = json!({ "ui": { "theme": "light" } });
    let preview = preview_without_touching_disk(env.home(), &state, AppType::Gemini, &edited);

    let settings_after = preview
        .files
        .iter()
        .find(|file| file.path.ends_with("settings.json"))
        .and_then(|file| file.after.clone())
        .expect("settings.json in preview");
    let settings: Value = serde_json::from_str(&settings_after).expect("settings json");
    assert_eq!(settings.pointer("/ui/theme"), Some(&json!("light")));
    assert_eq!(
        settings.pointer("/security/auth/selectedType"),
        Some(&json!("gemini-api-key"))
    );

    ProviderService::update(&state, AppType::Gemini, None, edited).expect("save provider");
    assert_preview_matches_disk_after_save(&preview);
}

#[test]
fn preview_hermes_skips_backups_and_equals_saved_bytes() {
    let env = TestEnv::new();
    // Windows 上默认目录是 %LOCALAPPDATA%\hermes（由注入的 HOME 派生），不是 ~/.hermes
    let hermes_dir = cc_switch_lib::hermes_config::get_hermes_dir();
    assert!(
        hermes_dir.starts_with(env.home()),
        "hermes dir escapes test HOME"
    );
    std::fs::create_dir_all(&hermes_dir).expect("create hermes dir");
    std::fs::write(
        hermes_dir.join("config.yaml"),
        "# user config\nmodel:\n  default: gpt-5\n  provider: custom\n",
    )
    .expect("seed hermes config");

    let state = create_test_state_with_config(&MultiAppConfig::default()).expect("state");
    let provider = Provider::with_id(
        "relay".to_string(),
        "Relay".to_string(),
        json!({
            "base_url": "https://old.example.com/v1",
            "api_key": "sk-old",
            "api_mode": "chat_completions",
            "models": [{ "id": "gpt-5" }]
        }),
        None,
    );
    ProviderService::add(&state, AppType::Hermes, provider.clone(), true).expect("add to live");
    // add 会写 live 并留下一份备份；预览前清掉，便于确认预览不再产生备份
    let _ = std::fs::remove_dir_all(env.home().join(".cc-switch").join("backups"));

    let mut edited = ProviderService::list(&state, AppType::Hermes)
        .expect("list hermes")
        .shift_remove("relay")
        .expect("stored provider");
    edited.settings_config["base_url"] = Value::String("https://new.example.com/v1".to_string());
    let preview = preview_without_touching_disk(env.home(), &state, AppType::Hermes, &edited);
    let after = preview.files[0]
        .after
        .as_deref()
        .expect("config.yaml written");
    assert!(after.contains("https://new.example.com/v1") && after.contains("# user config"));

    ProviderService::update(&state, AppType::Hermes, None, edited).expect("save provider");
    assert_preview_matches_disk_after_save(&preview);
}

#[test]
fn preview_reports_no_live_write_for_non_current_provider() {
    let env = TestEnv::new();
    let current = Provider::with_id(
        "current".to_string(),
        "Current".to_string(),
        json!({ "env": { "ANTHROPIC_AUTH_TOKEN": "sk-current" } }),
        None,
    );
    let other = Provider::with_id(
        "other".to_string(),
        "Other".to_string(),
        json!({ "env": { "ANTHROPIC_AUTH_TOKEN": "sk-other" } }),
        None,
    );
    let state = state_with_providers(AppType::Claude, "current", vec![current, other.clone()]);
    ProviderService::switch(&state, AppType::Claude, "current").expect("seed live");

    let preview = preview_without_touching_disk(env.home(), &state, AppType::Claude, &other);
    assert!(!preview.writes_live);
    assert_eq!(preview.skip_reason, Some(LivePreviewSkipReason::NotCurrent));
    assert!(preview.files.is_empty());
}
