//! Conformance suite for the per-app live-config adapters (CFG-06, CFG-21).
//!
//! Every adapter in the registry is exercised against the same safety
//! properties:
//! * a missing live file is reported, never invented;
//! * an unparsable live file is never overwritten;
//! * writes keep comments, key order, unknown keys, BOM and CRLF where the
//!   format allows it;
//! * the previous file is backed up before it is replaced;
//! * MCP projection round-trips through each app's native format and only
//!   touches the servers it owns.

use std::fs;
use std::path::Path;

use serde_json::{json, Value};

use cc_switch_lib::adapters::{adapter_for, all_adapters, mcp_adapters, WriteMode};
use cc_switch_lib::{AppType, MultiAppConfig, Provider};

#[path = "support.rs"]
mod support;
use support::TestEnv;

fn provider(id: &str, settings: Value) -> Provider {
    Provider::with_id(id.to_string(), id.to_string(), settings, None)
}

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn backups(home: &Path, app: &str) -> Vec<String> {
    let dir = home.join(".cc-switch").join("backups").join(app);
    let mut out: Vec<String> = fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| fs::read_to_string(e.path()).unwrap_or_default())
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// Create the config directories so every app counts as "installed".
fn install_all_apps(home: &Path) {
    for dir in [
        ".claude",
        ".codex",
        ".gemini",
        ".grok",
        ".config/opencode",
        ".openclaw",
        ".hermes",
    ] {
        fs::create_dir_all(home.join(dir)).unwrap();
    }
}

// ---------------------------------------------------------------------------
// Registry-wide properties
// ---------------------------------------------------------------------------

#[test]
fn every_adapter_reports_a_missing_live_config_without_creating_it() {
    let env = TestEnv::new();
    for adapter in all_adapters() {
        let caps = adapter.capabilities();
        if !caps.generic_live_read {
            assert!(adapter.read_live().is_err(), "{:?}", adapter.app());
            continue;
        }
        let err = adapter.read_live().expect_err(&format!(
            "{:?}: missing live config must be an error",
            adapter.app()
        ));
        assert!(
            !err.to_string().is_empty(),
            "{:?} returned an empty error",
            adapter.app()
        );
        for path in adapter.live_paths() {
            assert!(
                !path.exists(),
                "{:?} created {} while reading",
                adapter.app(),
                path.display()
            );
        }
    }
    drop(env);
}

#[test]
fn additive_adapters_never_overwrite_an_unparsable_live_file() {
    let env = TestEnv::new();
    install_all_apps(env.home());
    for adapter in all_adapters() {
        if adapter.capabilities().mode != WriteMode::Additive {
            continue;
        }
        let path = adapter.live_paths()[0].clone();
        let garbage = "{ this is: [not valid\n";
        write(&path, garbage);

        let settings = match adapter.app() {
            AppType::OpenCode => {
                json!({"npm": "@ai-sdk/openai", "options": {"baseURL": "https://x"}})
            }
            AppType::OpenClaw => {
                json!({"baseUrl": "https://x", "api": "openai-completions", "models": []})
            }
            AppType::Hermes => json!({"base_url": "https://x", "api_key": "k"}),
            other => panic!("unexpected additive app {other:?}"),
        };
        let result = adapter.write_live(&provider("p1", settings));
        assert!(
            result.is_err(),
            "{:?} accepted an unparsable file",
            adapter.app()
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            garbage,
            "{:?} modified an unparsable file",
            adapter.app()
        );
        if let Some(mcp) = adapter.mcp() {
            let spec = json!({"type": "stdio", "command": "npx"});
            assert!(
                mcp.upsert_server("srv", &spec).is_err(),
                "{:?}",
                adapter.app()
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), garbage);
        }
    }
}

#[test]
fn mcp_projectors_fail_loudly_on_unparsable_files_and_leave_them_untouched() {
    let env = TestEnv::new();
    install_all_apps(env.home());
    for adapter in mcp_adapters() {
        let mcp = adapter.mcp().unwrap();
        let path = match adapter.app() {
            AppType::Claude => env.home().join(".claude.json"),
            AppType::Gemini => env.home().join(".gemini/settings.json"),
            AppType::Codex => env.home().join(".codex/config.toml"),
            AppType::GrokBuild => env.home().join(".grok/config.toml"),
            _ => adapter.live_paths()[0].clone(),
        };
        let garbage = "[[[ not valid = \n{";
        write(&path, garbage);
        assert!(
            mcp.remove_server("srv").is_err(),
            "{:?}: remove must report an unparsable file (CFG-45)",
            adapter.app()
        );
        assert!(
            mcp.upsert_server("srv", &json!({"type": "stdio", "command": "npx"}))
                .is_err(),
            "{:?}",
            adapter.app()
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            garbage,
            "{:?}",
            adapter.app()
        );
    }
}

#[test]
fn mcp_specs_round_trip_through_every_native_format() {
    let specs = [
        (
            "stdio",
            json!({"type": "stdio", "command": "npx", "args": ["-y", "@mcp/fs"], "env": {"ROOT": "/tmp"}}),
        ),
        (
            "remote",
            json!({"type": "http", "url": "https://mcp.example.com/mcp", "headers": {"X-Key": "v"}}),
        ),
    ];
    for adapter in mcp_adapters() {
        let env = TestEnv::new();
        install_all_apps(env.home());
        let mcp = adapter.mcp().unwrap();
        for (id, spec) in &specs {
            mcp.upsert_server(id, spec).unwrap();
        }
        let mut imported = MultiAppConfig::default();
        mcp.import_servers(&mut imported).unwrap();
        let servers = imported.mcp.servers.expect("servers imported");
        for (id, spec) in &specs {
            let got = &servers
                .get(*id)
                .unwrap_or_else(|| panic!("{:?} lost server {id}", adapter.app()))
                .server;
            for key in ["command", "url"] {
                assert_eq!(
                    got.get(key),
                    spec.get(key),
                    "{:?} {id} {key}",
                    adapter.app()
                );
            }
            assert_eq!(
                got.get("type"),
                spec.get("type"),
                "{:?} {id}: transport flipped (CFG-16)",
                adapter.app()
            );
            if let Some(args) = spec.get("args") {
                assert_eq!(got.get("args"), Some(args), "{:?} {id} args", adapter.app());
            }
        }

        // removing one server keeps the other
        mcp.remove_server("stdio").unwrap();
        let mut imported = MultiAppConfig::default();
        mcp.import_servers(&mut imported).unwrap();
        let servers = imported.mcp.servers.unwrap_or_default();
        assert!(!servers.contains_key("stdio"), "{:?}", adapter.app());
        assert!(servers.contains_key("remote"), "{:?}", adapter.app());
    }
}

// ---------------------------------------------------------------------------
// Per-format preservation fixtures
// ---------------------------------------------------------------------------

const OPENCODE_FIXTURE: &str = r#"{
  // personal OpenCode setup
  "$schema": "https://opencode.ai/config.json",
  "theme": "tokyonight",
  "provider": {
    /* the gateway I actually use */
    "zeta": {
      "npm": "@ai-sdk/openai-compatible",
      "options": { "baseURL": "https://zeta.example/v1" }
    }
  },
  "agent": { "build": { "model": "zeta/m1" } }
}
"#;

#[test]
fn opencode_provider_write_only_touches_its_entry() {
    let env = TestEnv::new();
    install_all_apps(env.home());
    let adapter = adapter_for(&AppType::OpenCode);
    let path = adapter.live_paths()[0].clone();
    write(&path, OPENCODE_FIXTURE);

    adapter
        .write_live(&provider(
            "alpha",
            json!({"npm": "@ai-sdk/openai", "options": {"baseURL": "https://alpha.example/v1"}}),
        ))
        .unwrap();
    let written = fs::read_to_string(&path).unwrap();
    assert!(written.contains("// personal OpenCode setup"), "{written}");
    assert!(
        written.contains("/* the gateway I actually use */"),
        "{written}"
    );
    assert!(written.contains("\"theme\": \"tokyonight\""));
    // original order is kept (theme before provider before agent)
    let theme = written.find("\"theme\"").unwrap();
    let prov = written.find("\"provider\"").unwrap();
    let agent = written.find("\"agent\"").unwrap();
    assert!(theme < prov && prov < agent, "{written}");
    assert!(adapter.live_has_provider("alpha").unwrap());
    assert!(adapter.live_has_provider("zeta").unwrap());
    assert_eq!(
        backups(env.home(), "opencode"),
        vec![OPENCODE_FIXTURE.to_string()]
    );

    // removing the provider restores the original document byte-for-byte
    adapter.remove_provider_from_live("alpha").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), OPENCODE_FIXTURE);

    // MCP sync keeps comments too (CFG-07)
    let mcp = adapter.mcp().unwrap();
    mcp.upsert_server(
        "fs",
        &json!({"type": "stdio", "command": "npx", "args": ["-y", "fs"]}),
    )
    .unwrap();
    let written = fs::read_to_string(&path).unwrap();
    assert!(written.contains("/* the gateway I actually use */"));
    mcp.remove_server("fs").unwrap();
    let after_remove = fs::read_to_string(&path).unwrap();
    assert!(after_remove.contains("// personal OpenCode setup"));
    assert!(!after_remove.contains("\"fs\""));
}

#[test]
fn opencode_missing_file_is_created_with_schema() {
    let env = TestEnv::new();
    install_all_apps(env.home());
    let adapter = adapter_for(&AppType::OpenCode);
    adapter
        .write_live(&provider("p", json!({"npm": "x", "options": {}})))
        .unwrap();
    let value = adapter.read_live().unwrap();
    assert_eq!(value["$schema"], json!("https://opencode.ai/config.json"));
    assert!(value["provider"]["p"].is_object());
}

const OPENCLAW_FIXTURE: &str = "{\n  // OpenClaw config\n  models: {\n    mode: 'merge',\n    providers: {\n      // keep this one\n      existing: { baseUrl: 'https://e', api: 'openai-completions', models: [] },\n    },\n  },\n  agents: { defaults: { timeoutSeconds: 30 } },\n}\n";

#[test]
fn openclaw_provider_write_keeps_in_section_comments() {
    let env = TestEnv::new();
    install_all_apps(env.home());
    let adapter = adapter_for(&AppType::OpenClaw);
    let path = adapter.live_paths()[0].clone();
    write(&path, OPENCLAW_FIXTURE);

    adapter
        .write_live(&provider(
            "fresh",
            json!({"baseUrl": "https://f", "api": "openai-completions", "models": []}),
        ))
        .unwrap();
    let written = fs::read_to_string(&path).unwrap();
    assert!(written.contains("// keep this one"), "{written}");
    assert!(written.contains("// OpenClaw config"));
    assert!(written.contains("mode: 'merge'"));
    assert!(
        written.contains("existing: { baseUrl: 'https://e'"),
        "{written}"
    );
    assert!(adapter.live_has_provider("fresh").unwrap());
    assert!(adapter.live_has_provider("existing").unwrap());
}

const HERMES_FIXTURE: &str = "# Hermes config\r\nmodel:\r\n  default: m1\r\n  provider: existing\r\n\r\n# my providers\r\ncustom_providers:\r\n  # the old one\r\n  - name: existing\r\n    base_url: https://e.example/v1\r\n    api_key: k1\r\n\r\n# agent settings below\r\nagent:\r\n  max_turns: 7\r\n";

#[test]
fn hermes_provider_write_keeps_comments_and_crlf() {
    let env = TestEnv::new();
    install_all_apps(env.home());
    let adapter = adapter_for(&AppType::Hermes);
    let path = adapter.live_paths()[0].clone();
    write(&path, HERMES_FIXTURE);

    adapter
        .write_live(&provider(
            "fresh",
            json!({"base_url": "https://f.example/v1", "api_key": "k2"}),
        ))
        .unwrap();
    let written = fs::read_to_string(&path).unwrap();
    assert!(
        !written.replace("\r\n", "").contains('\n'),
        "mixed line endings: {written:?}"
    );
    for comment in [
        "# Hermes config",
        "# my providers",
        "# the old one",
        "# agent settings below",
    ] {
        assert!(written.contains(comment), "lost {comment:?}: {written:?}");
    }
    assert!(written.contains("  - name: existing\r\n    base_url: https://e.example/v1\r\n"));
    assert!(
        written.ends_with("agent:\r\n  max_turns: 7\r\n"),
        "{written:?}"
    );
    assert!(adapter.live_has_provider("fresh").unwrap());
    assert!(!backups(env.home(), "hermes").is_empty());
}

#[test]
fn claude_settings_keep_key_order_indent_bom_and_get_backed_up() {
    let env = TestEnv::new();
    install_all_apps(env.home());
    let adapter = adapter_for(&AppType::Claude);
    let path = adapter.live_paths()[0].clone();
    let original = "\u{feff}{\r\n    \"permissions\": {\r\n        \"allow\": []\r\n    },\r\n    \"env\": {\r\n        \"ANTHROPIC_BASE_URL\": \"https://a\"\r\n    }\r\n}\r\n";
    write(&path, original);

    let live = adapter.read_live().unwrap();
    // round trip of an unchanged snapshot is byte-identical (CFG-14, CFG-28)
    adapter.write_live(&provider("p", live.clone())).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), original);

    let mut changed = live;
    changed["env"]["ANTHROPIC_BASE_URL"] = json!("https://b");
    adapter.write_live(&provider("p", changed)).unwrap();
    let written = fs::read_to_string(&path).unwrap();
    assert_eq!(written, original.replace("https://a", "https://b"));
    assert!(backups(env.home(), "claude").contains(&original.to_string()));
}

#[test]
fn claude_mcp_projection_preserves_the_rest_of_claude_json() {
    let env = TestEnv::new();
    install_all_apps(env.home());
    let path = env.home().join(".claude.json");
    let original = "{\n  \"numStartups\": 3,\n  \"mcpServers\": {\n    \"mine\": {\n      \"command\": \"uvx\",\n      \"custom\": true\n    }\n  },\n  \"projects\": {}\n}";
    write(&path, original);
    let mcp = adapter_for(&AppType::Claude).mcp().unwrap();

    let a = json!({"type": "stdio", "command": "npx", "args": ["a"]});
    let b = json!({"type": "http", "url": "https://b/mcp"});
    mcp.project(&[("a", &a), ("b", &b)], &["gone"]).unwrap();
    let value: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
    assert_eq!(keys, vec!["numStartups", "mcpServers", "projects"]);
    let servers: Vec<_> = value["mcpServers"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(servers, vec!["mine", "a", "b"]);
    assert_eq!(value["mcpServers"]["mine"]["custom"], json!(true));

    // projecting the same state again writes nothing (CFG-41)
    let before = fs::read_to_string(&path).unwrap();
    let mtime = fs::metadata(&path).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    mcp.project(&[("a", &a), ("b", &b)], &["gone"]).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), mtime);
}

#[test]
fn gemini_mcp_projection_leaves_user_servers_alone() {
    let env = TestEnv::new();
    install_all_apps(env.home());
    let path = env.home().join(".gemini/settings.json");
    let original = "{\n  \"theme\": \"dark\",\n  \"mcpServers\": {\n    \"hand\": {\n      \"command\": \"x\"\n    }\n  }\n}\n";
    write(&path, original);
    let mcp = adapter_for(&AppType::Gemini).mcp().unwrap();
    mcp.upsert_server("srv", &json!({"type": "stdio", "command": "npx"}))
        .unwrap();
    let value: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    // hand-written entry not rewritten (no injected timeout, CFG-30)
    assert_eq!(value["mcpServers"]["hand"], json!({"command": "x"}));
    assert!(value["mcpServers"]["srv"].get("timeout").is_none());
    assert_eq!(value["theme"], json!("dark"));
    mcp.remove_server("srv").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn codex_mcp_projection_keeps_comments_and_stringifies_scalar_args() {
    let env = TestEnv::new();
    install_all_apps(env.home());
    let path = env.home().join(".codex/config.toml");
    let original = "# my codex config\nmodel = \"gpt-5\"  # pinned\n\n[profiles.fast]\nmodel = \"gpt-5-mini\"\n";
    write(&path, original);
    let mcp = adapter_for(&AppType::Codex).mcp().unwrap();
    mcp.upsert_server(
        "srv",
        &json!({"type": "stdio", "command": "npx", "args": ["--port", 8080, true]}),
    )
    .unwrap();
    let written = fs::read_to_string(&path).unwrap();
    assert!(
        written.starts_with("# my codex config\nmodel = \"gpt-5\"  # pinned\n"),
        "{written}"
    );
    assert!(
        written.contains("args = [\"--port\", \"8080\", \"true\"]"),
        "{written}"
    );
    mcp.remove_server("srv").unwrap();
    let written = fs::read_to_string(&path).unwrap();
    assert!(written.contains("# my codex config"));
    assert!(!written.contains("[mcp_servers.srv]"));
}
