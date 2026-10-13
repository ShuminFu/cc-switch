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
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
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
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
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

#[test]
fn importing_twice_in_the_same_second_keeps_both_prompts() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");

    let live_path = home.join(".claude").join("CLAUDE.md");
    std::fs::create_dir_all(live_path.parent().expect("parent")).expect("create claude dir");
    std::fs::write(&live_path, "first").expect("write live");
    let first = PromptService::import_from_file(&state, AppType::Claude).expect("first import");
    std::fs::write(&live_path, "second").expect("rewrite live");
    let second = PromptService::import_from_file(&state, AppType::Claude).expect("second import");

    assert_ne!(first, second, "import IDs must not collide");
    let prompts = PromptService::get_prompts(&state, AppType::Claude).expect("prompts");
    assert_eq!(prompts[&first].content, "first");
    assert_eq!(prompts[&second].content, "second");
}

#[test]
fn legacy_second_resolution_prompt_ids_still_load_and_enable() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let state = create_test_state().expect("create test state");

    for id in [
        "imported-1700000000",
        "backup-1700000001",
        "auto-imported-1700000002",
    ] {
        PromptService::upsert_prompt(&state, AppType::Claude, id, prompt(id, id, false))
            .expect("save legacy prompt");
    }
    let prompts = PromptService::get_prompts(&state, AppType::Claude).expect("prompts");
    assert_eq!(prompts.len(), 3);

    PromptService::enable_prompt(&state, AppType::Claude, "imported-1700000000")
        .expect("enable legacy prompt");
    assert_eq!(
        std::fs::read_to_string(home.join(".claude").join("CLAUDE.md")).expect("read live"),
        "imported-1700000000"
    );
}
