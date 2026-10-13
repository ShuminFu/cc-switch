use cc_switch_lib::{AppType, Prompt, PromptService};

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

fn prompt(id: &str, content: &str, enabled: bool) -> Prompt {
    Prompt {
        id: id.to_string(),
        name: id.to_string(),
        content: content.to_string(),
        description: None,
        enabled,
        created_at: Some(1),
        updated_at: Some(1),
    }
}

#[test]
fn disabling_enabled_prompt_backfills_live_edits_before_clearing_file() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");

    PromptService::upsert_prompt(&state, AppType::Claude, "a", prompt("a", "original", false))
        .expect("save prompt");
    PromptService::enable_prompt(&state, AppType::Claude, "a").expect("enable prompt");

    // The user (or Claude Code's `#` memory) edits CLAUDE.md directly.
    let live_path = home.join(".claude").join("CLAUDE.md");
    std::fs::write(&live_path, "original\n- remember this\n").expect("edit live file");

    // Toggle off from the UI: the frontend sends the stale DB content.
    PromptService::upsert_prompt(&state, AppType::Claude, "a", prompt("a", "original", false))
        .expect("disable prompt");

    let prompts = PromptService::get_prompts(&state, AppType::Claude).expect("prompts");
    assert_eq!(prompts["a"].content, "original\n- remember this\n");
    assert!(!prompts["a"].enabled);
    assert_eq!(std::fs::read_to_string(&live_path).expect("read live"), "");
}

#[test]
fn disabling_with_edited_content_keeps_editor_content() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");

    PromptService::upsert_prompt(&state, AppType::Claude, "a", prompt("a", "original", false))
        .expect("save prompt");
    PromptService::enable_prompt(&state, AppType::Claude, "a").expect("enable prompt");
    std::fs::write(home.join(".claude").join("CLAUDE.md"), "live edit").expect("edit live");

    PromptService::upsert_prompt(&state, AppType::Claude, "a", prompt("a", "editor", false))
        .expect("save edited disabled prompt");

    let prompts = PromptService::get_prompts(&state, AppType::Claude).expect("prompts");
    assert_eq!(prompts["a"].content, "editor");
}
