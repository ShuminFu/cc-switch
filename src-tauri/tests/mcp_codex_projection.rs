//! Residual MCP -> `~/.codex/config.toml` projection gaps (TEST-07):
//! idempotent writes (byte-identical), the inline-table import form, and a
//! direct `remove_server_from_codex` that keeps unrelated keys and comments.

use std::fs;

use cc_switch_lib::{
    get_codex_config_path, import_from_codex, remove_server_from_codex, sync_enabled_to_codex,
    sync_single_server_to_codex, MultiAppConfig,
};
use serde_json::json;

#[path = "support.rs"]
mod support;
use support::TestEnv;

fn seed(text: &str) -> std::path::PathBuf {
    let path = get_codex_config_path();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, text).unwrap();
    path
}

fn config_with_enabled_server(id: &str) -> MultiAppConfig {
    let mut config = MultiAppConfig::default();
    config.mcp.codex.servers.insert(
        id.into(),
        json!({
            "id": id,
            "enabled": true,
            "server": { "type": "stdio", "command": "echo", "args": ["ok"] }
        }),
    );
    config
}

#[test]
fn sync_enabled_to_codex_twice_is_byte_identical() {
    let _env = TestEnv::new();
    let path = seed("# top comment\nmodel = \"gpt-5\"\n");
    let config = config_with_enabled_server("echo");

    sync_enabled_to_codex(&config).unwrap();
    let first = fs::read(&path).unwrap();
    sync_enabled_to_codex(&config).unwrap();
    let second = fs::read(&path).unwrap();
    assert_eq!(
        String::from_utf8_lossy(&first),
        String::from_utf8_lossy(&second),
        "re-projecting the same state must not change the file"
    );
    let text = String::from_utf8(second).unwrap();
    assert!(text.starts_with("# top comment"), "{text}");
    assert!(text.contains("[mcp_servers.echo]"), "{text}");
}

#[test]
fn sync_single_server_twice_is_byte_identical() {
    let _env = TestEnv::new();
    let path = seed("model = \"gpt-5\"\n");
    let config = MultiAppConfig::default();
    let spec = json!({ "type": "stdio", "command": "npx", "args": ["-y", "srv"] });

    sync_single_server_to_codex(&config, "srv", &spec).unwrap();
    let first = fs::read_to_string(&path).unwrap();
    sync_single_server_to_codex(&config, "srv", &spec).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), first);
}

#[test]
fn import_from_codex_accepts_inline_table_servers() {
    let _env = TestEnv::new();
    seed(
        r#"model = "gpt-5"

[mcp_servers]
inline_one = { command = "echo", args = ["hi"] }
inline_http = { type = "http", url = "https://example.com/mcp" }
"#,
    );
    let mut config = MultiAppConfig::default();
    let changed = import_from_codex(&mut config).expect("import inline tables");
    assert!(changed >= 2, "imported {changed}");
    let servers = config.mcp.servers.as_ref().expect("unified servers");
    let one = servers.get("inline_one").expect("inline_one imported");
    assert!(one.apps.codex);
    assert_eq!(one.server["command"], "echo");
    assert_eq!(one.server["args"], json!(["hi"]));
    let http = servers.get("inline_http").expect("inline_http imported");
    assert_eq!(http.server["url"], "https://example.com/mcp");
}

#[test]
fn remove_server_from_codex_keeps_other_servers_keys_and_comments() {
    let _env = TestEnv::new();
    let path = seed(
        r#"# keep this comment
model = "gpt-5"

[mcp_servers.gone]
command = "echo"

# about kept
[mcp_servers.kept]
command = "npx"

[profiles.work]
model = "o3"
"#,
    );
    remove_server_from_codex("gone").unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(!text.contains("[mcp_servers.gone]"), "{text}");
    assert!(text.contains("[mcp_servers.kept]"), "{text}");
    assert!(text.contains("# keep this comment"), "{text}");
    assert!(text.contains("[profiles.work]"), "{text}");
    let doc: toml::Value = toml::from_str(&text).unwrap();
    assert_eq!(doc["model"].as_str(), Some("gpt-5"));

    // Removing an unknown id leaves the file byte-identical.
    remove_server_from_codex("never-existed").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), text);
}

#[test]
fn remove_server_from_codex_without_config_is_a_no_op() {
    let env = TestEnv::new();
    remove_server_from_codex("anything").unwrap();
    assert!(!get_codex_config_path().exists());
    let _ = env.home();
}

#[test]
fn remove_server_from_codex_fails_closed_on_invalid_toml() {
    let _env = TestEnv::new();
    let path = seed("this is = = not toml");
    assert!(remove_server_from_codex("x").is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "this is = = not toml");
}
