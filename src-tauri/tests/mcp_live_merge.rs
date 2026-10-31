//! MCP projection keeps app-native keys of existing live entries (CFG-42) and
//! reports apps whose config directory is missing instead of silently
//! succeeding (CFG-46).

use std::fs;

use serde_json::json;

use cc_switch_lib::adapters::{adapter_for, mcp_sync_skips};
use cc_switch_lib::AppType;

#[path = "support.rs"]
mod support;
use support::{ensure_test_home, TestEnv};

#[test]
fn mcp_sync_skips_reports_missing_config_dirs_without_creating_them() {
    let _env = TestEnv::new();
    let home = ensure_test_home();

    let skips = mcp_sync_skips([AppType::Codex, AppType::Gemini]);
    assert_eq!(skips.len(), 2, "{skips:?}");
    assert_eq!(skips[0].app, "codex");
    assert_eq!(skips[0].missing_dir, home.join(".codex"));
    assert!(!home.join(".codex").exists());

    fs::create_dir_all(home.join(".codex")).unwrap();
    let skips = mcp_sync_skips([AppType::Codex, AppType::Gemini]);
    assert_eq!(skips.len(), 1);
    assert_eq!(skips[0].app, "gemini");

    // apps without MCP projection are never reported
    assert!(mcp_sync_skips([AppType::ClaudeDesktop]).is_empty());
}

#[test]
fn codex_projection_keeps_native_keys_and_enables_server() {
    let _env = TestEnv::new();
    let home = ensure_test_home();
    let codex_dir = home.join(".codex");
    fs::create_dir_all(&codex_dir).unwrap();
    fs::write(
        codex_dir.join("config.toml"),
        "[mcp_servers.ctx]\ncommand = \"old\"\nstartup_timeout_sec = 45\nenabled_tools = [\"search\"]\nenabled = false\n",
    )
    .unwrap();

    let projector = adapter_for(&AppType::Codex).mcp().expect("codex mcp");
    projector
        .upsert_server(
            "ctx",
            &json!({"type": "stdio", "command": "npx", "args": ["-y", "ctx"]}),
        )
        .expect("upsert");

    let text = fs::read_to_string(codex_dir.join("config.toml")).unwrap();
    let doc: toml::Table = toml::from_str(&text).unwrap();
    let entry = &doc["mcp_servers"]["ctx"];
    assert_eq!(entry["command"].as_str(), Some("npx"));
    assert_eq!(entry["startup_timeout_sec"].as_integer(), Some(45));
    assert_eq!(entry["enabled_tools"][0].as_str(), Some("search"));
    assert_eq!(entry["enabled"].as_bool(), Some(true));

    // disabling removes the entry
    projector.remove_server("ctx").expect("remove");
    let text = fs::read_to_string(codex_dir.join("config.toml")).unwrap();
    assert!(!text.contains("ctx"), "{text}");
}

#[test]
fn opencode_projection_keeps_native_keys() {
    let _env = TestEnv::new();
    let home = ensure_test_home();
    let dir = home.join(".config").join("opencode");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("opencode.json"),
        "{\n  \"mcp\": {\n    \"ctx\": {\n      \"type\": \"local\",\n      \"command\": [\"old\"],\n      \"timeout\": 9000,\n      \"enabled\": false\n    }\n  }\n}\n",
    )
    .unwrap();

    let projector = adapter_for(&AppType::OpenCode).mcp().expect("opencode mcp");
    projector
        .upsert_server("ctx", &json!({"type": "stdio", "command": "npx"}))
        .expect("upsert");

    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.join("opencode.json")).unwrap()).unwrap();
    let entry = &value["mcp"]["ctx"];
    assert_eq!(entry["command"], json!(["npx"]));
    assert_eq!(entry["timeout"], json!(9000));
    assert_eq!(entry["enabled"], json!(true));
}
