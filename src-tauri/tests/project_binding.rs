//! 目录绑定 / 项目级应用 / 新分组 / 基线恢复的集成测试
//! （PRODUCT-12 / PRODUCT-17 / PRODUCT-63 / PRODUCT-68 / PRODUCT-69）
//!
//! support.rs 把 HOME 指向每个测试独立的临时目录；绑定的"仓库"也建在该临时目录内。

use std::fs;
use std::path::Path;

use serde_json::{json, Value};

use cc_switch_lib::{
    project_binding, AppType, McpServer, ProfilePayload, ProfileScope, ProfileService, Prompt,
    Provider, ProviderService,
};

#[path = "support.rs"]
mod support;
use support::{create_test_state, TestEnv};

fn claude_provider(id: &str, token: &str, base_url: &str) -> Provider {
    Provider::with_id(
        id.to_string(),
        id.to_uppercase(),
        json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": token,
                "ANTHROPIC_BASE_URL": base_url
            }
        }),
        None,
    )
}

fn mcp_server(id: &str, apps: Value) -> McpServer {
    serde_json::from_value(json!({
        "id": id,
        "name": id,
        "server": { "command": "echo", "args": [] },
        "apps": apps
    }))
    .expect("construct mcp server")
}

fn prompt(id: &str, enabled: bool) -> Prompt {
    Prompt {
        id: id.to_string(),
        name: id.to_uppercase(),
        content: format!("# prompt {id}\n"),
        description: None,
        enabled,
        created_at: Some(1_000),
        updated_at: Some(1_000),
    }
}

fn enabled_mcp(state: &cc_switch_lib::AppState, app: &AppType) -> Vec<String> {
    let mut ids: Vec<String> = state
        .db
        .get_all_mcp_servers()
        .expect("list mcp")
        .values()
        .filter(|s| s.apps.is_enabled_for(app))
        .map(|s| s.id.clone())
        .collect();
    ids.sort();
    ids
}

fn seed_claude_live(home: &Path, provider: &Provider) {
    let dir = home.join(".claude");
    fs::create_dir_all(&dir).expect("create .claude");
    fs::write(
        dir.join("settings.json"),
        serde_json::to_string_pretty(&provider.settings_config).unwrap(),
    )
    .expect("seed live settings.json");
}

fn set_payload(state: &cc_switch_lib::AppState, id: &str, edit: impl FnOnce(&mut ProfilePayload)) {
    let mut profile = state.db.get_profile(id).unwrap().expect("profile exists");
    let mut payload: ProfilePayload = serde_json::from_str(&profile.payload).unwrap();
    edit(&mut payload);
    profile.payload = serde_json::to_string(&payload).unwrap();
    state.db.save_profile(&profile).unwrap();
}

/// PRODUCT-68：从"不使用项目"应用项目 B，再选"不使用项目"，回到应用前的状态
#[test]
fn clearing_after_first_apply_restores_the_pre_project_configuration() {
    let env = TestEnv::new();
    let state = create_test_state().expect("state");

    let p1 = claude_provider("p1", "k1", "https://one.test");
    let p2 = claude_provider("p2", "k2", "https://two.test");
    state.db.save_provider("claude", &p1).unwrap();
    state.db.save_provider("claude", &p2).unwrap();
    state.db.set_current_provider("claude", "p1").unwrap();
    seed_claude_live(env.home(), &p1);
    for (id, on) in [("x", true), ("y", true), ("z", false)] {
        state
            .db
            .save_mcp_server(&mcp_server(id, json!({ "claude": on })))
            .unwrap();
    }
    state
        .db
        .save_prompt("claude", &prompt("pr1", true))
        .unwrap();
    state
        .db
        .save_prompt("claude", &prompt("pr2", false))
        .unwrap();

    // 项目 B：p2 + {z} + pr2
    let b = ProfileService::create(&state, "B", ProfileScope::Claude).unwrap();
    set_payload(&state, &b.id, |p| {
        p.providers.claude = Some("p2".into());
        p.mcp.claude = Some(vec!["z".into()]);
        p.prompts.claude = Some("pr2".into());
    });

    let (warnings, _) = ProfileService::apply(&state, &b.id, ProfileScope::Claude).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(enabled_mcp(&state, &AppType::Claude), vec!["z".to_string()]);
    assert!(state.db.get_profile_baseline("claude").unwrap().is_some());

    let (warnings, _) = ProfileService::clear_current(&state, ProfileScope::Claude).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        enabled_mcp(&state, &AppType::Claude),
        vec!["x".to_string(), "y".to_string()]
    );
    assert_eq!(
        state.db.get_current_provider("claude").unwrap().as_deref(),
        Some("p1")
    );
    let prompts = state.db.get_prompts("claude").unwrap();
    assert!(prompts["pr1"].enabled && !prompts["pr2"].enabled);
    assert_eq!(state.db.get_current_profile_id("claude").unwrap(), None);
    assert!(
        state.db.get_profile_baseline("claude").unwrap().is_none(),
        "baseline is consumed by the restore"
    );

    // 离开 B 时自动保存了 B 的状态
    let b_payload: ProfilePayload =
        serde_json::from_str(&state.db.get_profile(&b.id).unwrap().unwrap().payload).unwrap();
    assert_eq!(b_payload.providers.claude.as_deref(), Some("p2"));

    // 没有基线时"不使用项目"只清标记
    state
        .db
        .set_current_profile_id("claude", Some(&b.id))
        .unwrap();
    let (warnings, stop) = ProfileService::clear_current(&state, ProfileScope::Claude).unwrap();
    assert!(warnings.is_empty() && !stop);
    assert_eq!(
        state.db.get_current_provider("claude").unwrap().as_deref(),
        Some("p1"),
        "no baseline: configuration untouched"
    );
}

/// PRODUCT-17：Gemini 分组的项目切换 MCP + Prompt，其他分组不受影响
#[test]
fn gemini_scope_profile_switches_mcp_and_prompt_only_for_gemini() {
    let _env = TestEnv::new();
    let state = create_test_state().expect("state");

    state
        .db
        .save_mcp_server(&mcp_server("g1", json!({ "gemini": true, "claude": true })))
        .unwrap();
    state
        .db
        .save_mcp_server(&mcp_server("g2", json!({ "gemini": false })))
        .unwrap();
    state
        .db
        .save_prompt("gemini", &prompt("gp1", false))
        .unwrap();

    let project = ProfileService::create(&state, "Gem", ProfileScope::Gemini).unwrap();
    let payload: ProfilePayload = serde_json::from_str(&project.payload).unwrap();
    assert_eq!(payload.mcp.gemini, Some(vec!["g1".to_string()]));
    assert_eq!(payload.mcp.claude, None, "other scopes not captured");

    set_payload(&state, &project.id, |p| {
        p.mcp.gemini = Some(vec!["g2".into()]);
        p.prompts.gemini = Some("gp1".into());
    });
    let (warnings, _) = ProfileService::apply(&state, &project.id, ProfileScope::Gemini).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        enabled_mcp(&state, &AppType::Gemini),
        vec!["g2".to_string()]
    );
    assert_eq!(
        enabled_mcp(&state, &AppType::Claude),
        vec!["g1".to_string()],
        "claude enablement untouched"
    );
    assert!(state.db.get_prompts("gemini").unwrap()["gp1"].enabled);
    assert_eq!(
        state
            .db
            .get_current_profile_id("gemini")
            .unwrap()
            .as_deref(),
        Some(project.id.as_str())
    );
    assert_eq!(state.db.get_current_profile_id("claude").unwrap(), None);
}

/// 叠加模式应用（OpenCode）不记录"当前供应商"
#[test]
fn additive_app_scope_never_captures_a_provider() {
    let _env = TestEnv::new();
    let state = create_test_state().expect("state");
    let provider = Provider::with_id("oc1".into(), "OC".into(), json!({}), None);
    state.db.save_provider("opencode", &provider).unwrap();
    state.db.set_current_provider("opencode", "oc1").unwrap();

    let project = ProfileService::create(&state, "OC", ProfileScope::OpenCode).unwrap();
    let payload: ProfilePayload = serde_json::from_str(&project.payload).unwrap();
    assert_eq!(payload.providers.opencode, None);
    assert_eq!(payload.mcp.opencode, Some(vec![]));

    let openclaw = ProfileService::create(&state, "Claw", ProfileScope::OpenClaw).unwrap();
    let payload: ProfilePayload = serde_json::from_str(&openclaw.payload).unwrap();
    assert_eq!(payload.mcp.openclaw, None, "OpenClaw has no MCP");
    assert_eq!(payload.skills.openclaw, None, "OpenClaw has no Skills");
}

/// PRODUCT-69：Claude Desktop 走本地路由时，apply 后不建议停代理
#[test]
fn apply_keeps_proxy_running_for_claude_desktop_local_routing() {
    let _env = TestEnv::new();
    let state = create_test_state().expect("state");

    let mut desktop = claude_provider("d1", "dk", "https://desktop.test");
    desktop.meta = Some(serde_json::from_value(json!({ "claudeDesktopMode": "proxy" })).unwrap());
    state.db.save_provider("claude-desktop", &desktop).unwrap();
    state
        .db
        .set_current_provider("claude-desktop", "d1")
        .unwrap();

    let project = ProfileService::create(&state, "Codex", ProfileScope::Codex).unwrap();
    let (_, should_stop) = ProfileService::apply(&state, &project.id, ProfileScope::Codex).unwrap();
    assert!(!should_stop, "Claude Desktop still needs the local gateway");

    // Direct 模式：没有消费者，建议停代理
    desktop.meta = None;
    state.db.save_provider("claude-desktop", &desktop).unwrap();
    let (_, should_stop) = ProfileService::apply(&state, &project.id, ProfileScope::Codex).unwrap();
    assert!(should_stop);
}

/// PRODUCT-12：项目级应用只写绑定目录内的 `.claude/settings.local.json`，
/// 全局 settings.json 不变；已有文件先备份，用户自己的键原样保留。
#[test]
fn project_level_binding_writes_only_inside_the_bound_directory() {
    let env = TestEnv::new();
    let state = create_test_state().expect("state");

    let global = claude_provider("global", "gk", "https://global.test");
    let work = claude_provider("work", "wk", "https://work.test");
    state.db.save_provider("claude", &global).unwrap();
    state.db.save_provider("claude", &work).unwrap();
    ProviderService::switch(&state, AppType::Claude, "global").unwrap();
    let global_settings = env.home().join(".claude/settings.json");
    let global_before = fs::read_to_string(&global_settings).unwrap();

    let project = ProfileService::create(&state, "Work", ProfileScope::Claude).unwrap();
    set_payload(&state, &project.id, |p| {
        p.providers.claude = Some("work".into())
    });

    let repo = env.home().join("code/api");
    fs::create_dir_all(repo.join(".git")).unwrap();
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::create_dir_all(repo.join(".claude")).unwrap();
    let local = repo.join(".claude/settings.local.json");
    fs::write(
        &local,
        r#"{"permissions":{"allow":["Bash(ls)"]},"env":{"MY_VAR":"1"}}"#,
    )
    .unwrap();

    project_binding::bind(
        &state.db,
        &repo.join("src"),
        &project.id,
        vec![ProfileScope::Claude],
        true,
    )
    .unwrap();

    // 从子目录检测并应用
    let report = project_binding::apply_for_directory(&state, &repo.join("src"))
        .unwrap()
        .expect("binding found");
    assert_eq!(report.project_level_scopes, vec![ProfileScope::Claude]);
    assert!(report.global_scopes.is_empty());
    assert_eq!(report.written_files.len(), 1);
    assert_eq!(report.backups.len(), 1, "existing file is backed up first");
    let backup = Path::new(&report.backups[0]);
    assert!(backup.starts_with(fs::canonicalize(&repo).unwrap()));
    assert!(fs::read_to_string(backup).unwrap().contains("MY_VAR"));

    let written: Value = serde_json::from_str(&fs::read_to_string(&local).unwrap()).unwrap();
    assert_eq!(written["env"]["ANTHROPIC_BASE_URL"], "https://work.test");
    assert_eq!(written["env"]["ANTHROPIC_AUTH_TOKEN"], "wk");
    assert_eq!(written["env"]["MY_VAR"], "1");
    assert_eq!(written["permissions"]["allow"][0], "Bash(ls)");

    // 全局配置与当前供应商保持不变
    assert_eq!(fs::read_to_string(&global_settings).unwrap(), global_before);
    assert_eq!(
        state.db.get_current_provider("claude").unwrap().as_deref(),
        Some("global")
    );

    // 再次应用：内容未变，不写也不备份
    let again = project_binding::apply_for_directory(&state, &repo)
        .unwrap()
        .unwrap();
    assert!(again.written_files.is_empty() && again.backups.is_empty());

    // 绑定目录之外：没有绑定
    assert!(project_binding::apply_for_directory(&state, env.home())
        .unwrap()
        .is_none());
}

/// 非项目级绑定（以及不支持项目级的分组）走全局切换
#[test]
fn global_binding_applies_the_profile_to_the_scope() {
    let env = TestEnv::new();
    let state = create_test_state().expect("state");

    let a = claude_provider("a", "ka", "https://a.test");
    let b = claude_provider("b", "kb", "https://b.test");
    state.db.save_provider("claude", &a).unwrap();
    state.db.save_provider("claude", &b).unwrap();
    state.db.set_current_provider("claude", "a").unwrap();
    seed_claude_live(env.home(), &a);

    let project = ProfileService::create(&state, "B", ProfileScope::Claude).unwrap();
    set_payload(&state, &project.id, |p| {
        p.providers.claude = Some("b".into())
    });

    let dir = env.home().join("blog");
    fs::create_dir_all(&dir).unwrap();
    project_binding::bind(&state.db, &dir, &project.id, vec![], false).unwrap();

    let report = project_binding::apply_for_directory(&state, &dir)
        .unwrap()
        .unwrap();
    assert_eq!(report.global_scopes, vec![ProfileScope::Claude]);
    assert!(report.written_files.is_empty());
    assert!(
        !dir.join(".claude").exists(),
        "nothing written into the folder"
    );
    assert_eq!(
        state.db.get_current_provider("claude").unwrap().as_deref(),
        Some("b")
    );
    assert_eq!(
        state
            .db
            .get_current_profile_id("claude")
            .unwrap()
            .as_deref(),
        Some(project.id.as_str())
    );

    // 删除项目一并删除绑定
    ProfileService::delete(&state, &project.id).unwrap();
    assert!(project_binding::load_bindings(&state.db)
        .unwrap()
        .is_empty());
}
