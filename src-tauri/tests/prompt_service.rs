//! PromptService live-file ownership (SVC-39).

use std::fs;

use cc_switch_lib::{AppType, Prompt, PromptService};

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, TestEnv};

fn prompt(id: &str, content: &str, enabled: bool) -> Prompt {
    Prompt {
        id: id.to_string(),
        name: id.to_uppercase(),
        content: content.to_string(),
        description: None,
        enabled,
        created_at: Some(1_000),
        updated_at: Some(1_000),
    }
}

#[test]
fn enabling_another_prompt_keeps_ui_edit_of_the_enabled_one() {
    let _env = TestEnv::new();
    let home = ensure_test_home();
    fs::create_dir_all(home.join(".claude")).expect("create ~/.claude");
    let state = create_test_state().expect("create test state");

    PromptService::upsert_prompt(&state, AppType::Claude, "a", prompt("a", "A v1", true))
        .expect("save A");
    PromptService::upsert_prompt(&state, AppType::Claude, "b", prompt("b", "B", false))
        .expect("save B");
    // Edit A in the UI (still enabled), then enable B.
    PromptService::upsert_prompt(&state, AppType::Claude, "a", prompt("a", "A v2", true))
        .expect("edit A");
    PromptService::enable_prompt(&state, AppType::Claude, "b").expect("enable B");

    let prompts = PromptService::get_prompts(&state, AppType::Claude).expect("list");
    assert_eq!(prompts["a"].content, "A v2", "UI edit survives");
    assert!(!prompts["a"].enabled);
    assert!(prompts["b"].enabled);
    assert_eq!(prompts.values().filter(|p| p.enabled).count(), 1);
}

#[test]
fn toggling_prompts_does_not_accumulate_backups() {
    let _env = TestEnv::new();
    let home = ensure_test_home();
    fs::create_dir_all(home.join(".claude")).expect("create ~/.claude");
    let state = create_test_state().expect("create test state");

    PromptService::upsert_prompt(&state, AppType::Claude, "a", prompt("a", "A", false))
        .expect("save A");
    PromptService::upsert_prompt(&state, AppType::Claude, "b", prompt("b", "B", false))
        .expect("save B");
    for _ in 0..10 {
        PromptService::enable_prompt(&state, AppType::Claude, "a").expect("enable A");
        PromptService::enable_prompt(&state, AppType::Claude, "b").expect("enable B");
    }

    let prompts = PromptService::get_prompts(&state, AppType::Claude).expect("list");
    let backups = prompts
        .keys()
        .filter(|id| id.starts_with("backup-"))
        .count();
    assert!(backups <= 1, "at most one backup prompt, got {backups}");
}

#[test]
fn external_edit_of_the_live_file_is_still_backfilled() {
    let _env = TestEnv::new();
    let home = ensure_test_home();
    fs::create_dir_all(home.join(".claude")).expect("create ~/.claude");
    let state = create_test_state().expect("create test state");

    PromptService::upsert_prompt(&state, AppType::Claude, "a", prompt("a", "A", true))
        .expect("save A");
    PromptService::upsert_prompt(&state, AppType::Claude, "b", prompt("b", "B", false))
        .expect("save B");
    fs::write(home.join(".claude").join("CLAUDE.md"), "edited by hand").expect("edit live");
    PromptService::enable_prompt(&state, AppType::Claude, "b").expect("enable B");

    let prompts = PromptService::get_prompts(&state, AppType::Claude).expect("list");
    assert_eq!(prompts["a"].content, "edited by hand");
}
