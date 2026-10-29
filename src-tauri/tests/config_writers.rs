//! Round-trip snapshots for every live-config writer, the OAuth token stores
//! and the Codex state DB migration.
//!
//! Each writer reads a fixture from `tests/fixtures/live/<writer>/` (comments,
//! unknown keys) seeded into an isolated HOME, runs the production write path
//! and snapshots every file it touches. Every case runs three times: with the
//! fixture as checked in (LF), converted to CRLF, and prefixed with a UTF-8
//! BOM. The variants are derived at runtime so `.gitattributes` line-ending
//! normalisation can never change the inputs.
//!
//! Snapshots live next to the fixtures as `<variant>.snap`. In the rendered
//! text a BOM is shown as `<BOM>`, a carriage return as `<CR>` and the HOME
//! directory as `<HOME>`. Sections a writer must not touch are additionally
//! asserted to be byte-identical with the input.
//!
//! Update snapshots after an intended behaviour change with
//! `INSTA_UPDATE=always cargo test --test config_writers` (or
//! `cargo insta review`) and review the diff.

mod support;

use std::collections::HashMap;
use std::fmt::Display;
use std::fs;
use std::path::{Path, PathBuf};

use cc_switch_lib::live_config_testkit as kit;
use cc_switch_lib::{update_settings, AppSettings, AppType, Database, Provider};
use serde_json::Value;
use support::TestEnv;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Variant {
    Lf,
    Crlf,
    Bom,
}

const VARIANTS: [Variant; 3] = [Variant::Lf, Variant::Crlf, Variant::Bom];

impl Variant {
    fn name(self) -> &'static str {
        match self {
            Variant::Lf => "lf",
            Variant::Crlf => "crlf",
            Variant::Bom => "bom",
        }
    }

    fn apply(self, text: &str) -> String {
        match self {
            Variant::Lf => text.to_string(),
            Variant::Crlf => text.replace('\n', "\r\n"),
            Variant::Bom => format!("\u{feff}{text}"),
        }
    }
}

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/live")
}

/// Read a fixture file; checked-in fixtures are always LF.
fn fixture(writer: &str, name: &str) -> String {
    let path = fixtures_root().join(writer).join(name);
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()));
    assert!(
        !text.contains('\r') && !text.starts_with('\u{feff}'),
        "fixture {} must be plain LF without BOM; variants are derived at runtime",
        path.display()
    );
    text
}

fn fixture_json(writer: &str, name: &str) -> Value {
    serde_json::from_str(&fixture(writer, name))
        .unwrap_or_else(|e| panic!("parse fixture {writer}/{name}: {e}"))
}

/// Isolated HOME with every app config dir pinned below it, so the writers
/// resolve the same layout on every platform.
struct LiveEnv {
    home: PathBuf,
    _env: TestEnv,
}

impl LiveEnv {
    fn new() -> Self {
        Self::with_settings(AppSettings::default())
    }

    fn with_settings(base: AppSettings) -> Self {
        let env = TestEnv::new();
        let home = env.home().to_path_buf();
        let dir = |rel: &str| Some(home.join(rel).to_string_lossy().into_owned());
        update_settings(AppSettings {
            claude_config_dir: dir(".claude"),
            codex_config_dir: dir(".codex"),
            gemini_config_dir: dir(".gemini"),
            grok_config_dir: dir(".grok"),
            opencode_config_dir: dir(".config/opencode"),
            openclaw_config_dir: dir(".openclaw"),
            hermes_config_dir: dir(".hermes"),
            ..base
        })
        .expect("pin app config dirs");
        Self { home, _env: env }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.home.join(rel)
    }

    /// Seed a live file with an explicit 0644 mode so mode snapshots do not
    /// depend on the umask.
    fn seed(&self, rel: &str, content: &str) -> PathBuf {
        let path = self.path(rel);
        fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
        fs::write(&path, content).expect("seed live file");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("chmod seed");
        }
        path
    }

    fn redact(&self, text: &str) -> String {
        let home = self.home.to_string_lossy();
        text.replace(home.as_ref(), "<HOME>")
            .replace(&home.replace('\\', "/"), "<HOME>")
            .replace('\\', "/")
    }

    fn render_result<T, E: Display>(&self, label: &str, result: &Result<T, E>) -> String {
        match result {
            Ok(_) => format!("{label}: ok\n"),
            Err(e) => format!("{label}: error: {}\n", self.redact(&e.to_string())),
        }
    }

    /// Render files under HOME for a snapshot.
    fn render_files(&self, rels: &[&str]) -> String {
        let mut out = String::new();
        for rel in rels {
            out.push_str(&format!("=== {rel} ===\n"));
            match fs::read(self.path(rel)) {
                Ok(bytes) => out.push_str(&render_bytes(&bytes)),
                Err(_) => out.push_str("<absent>\n"),
            }
        }
        out
    }

    fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.path(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
    }
}

fn render_bytes(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes)
        .replace('\u{feff}', "<BOM>")
        .replace('\r', "<CR>");
    if text.ends_with('\n') {
        text
    } else {
        format!("{text}\n<no newline at end of file>\n")
    }
}

fn assert_snapshot(writer: &str, variant: Variant, text: &str) {
    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path(fixtures_root().join(writer));
    settings.set_prepend_module_to_snapshot(false);
    settings.set_omit_expression(true);
    settings.set_description(format!("{writer} writer, {} input", variant.name()));
    settings.bind(|| insta::assert_snapshot!(variant.name(), text));
}

/// `chunk` is copied verbatim from the input and must survive the write
/// byte-for-byte.
fn assert_preserved(writer: &str, variant: Variant, input: &str, output: &str, chunk: &str) {
    let chunk = variant.apply(chunk);
    let chunk = chunk.trim_start_matches('\u{feff}');
    assert!(
        input.contains(chunk),
        "{writer}/{}: probe is not part of the input: {chunk:?}",
        variant.name()
    );
    assert!(
        output.contains(chunk),
        "{writer}/{}: untouched section changed; expected verbatim {chunk:?} in:\n{output}",
        variant.name()
    );
}

fn provider(id: &str, name: &str, settings: Value) -> Provider {
    Provider::with_id(id.to_string(), name.to_string(), settings, None)
}

fn write_live(app_type: AppType, writer: &str, id: &str, name: &str) -> Result<(), String> {
    let db = Database::memory().expect("memory db");
    let provider = provider(id, name, fixture_json(writer, "provider.json"));
    kit::write_provider_live(&db, &app_type, &provider).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Live-config writers
// ---------------------------------------------------------------------------

#[test]
fn claude_settings_json() {
    const W: &str = "claude_settings";
    for variant in VARIANTS {
        let env = LiveEnv::new();
        env.seed(
            ".claude/settings.json",
            &variant.apply(&fixture(W, "settings.json")),
        );
        let result = write_live(AppType::Claude, W, "relay", "Relay");
        let text =
            env.render_result("write", &result) + &env.render_files(&[".claude/settings.json"]);
        assert_snapshot(W, variant, &text);
    }
}

#[test]
fn claude_mcp_json() {
    const W: &str = "claude_mcp";
    let servers: HashMap<String, Value> =
        serde_json::from_value(fixture_json(W, "servers.json")).expect("servers map");
    // `set_mcp_servers_map` iterates a HashMap, so more than one server would
    // make the output order (and the snapshot) nondeterministic.
    assert_eq!(servers.len(), 1);
    for variant in VARIANTS {
        let env = LiveEnv::new();
        let input = variant.apply(&fixture(W, "claude.json"));
        env.seed(".claude.json", &input);
        let result = kit::set_claude_mcp_servers_map(&servers);
        let text = env.render_result("write", &result) + &env.render_files(&[".claude.json"]);
        assert_snapshot(W, variant, &text);
        if variant == Variant::Lf {
            let output = env.read(".claude.json");
            for chunk in [
                "  \"numStartups\": 42,\n",
                "  \"projects\": {\n    \"/work/repo\": {\n      \"allowedTools\": [],\n      \"mcpServers\": {\n        \"project-local\": {\n          \"command\": \"uvx\",\n          \"args\": [\n            \"project-local-mcp\"\n          ]\n        }\n      }\n    }\n  },\n",
                "  \"futureClaudeJsonKey\": \"keep-me\",\n  \"oauthAccount\": {\n    \"emailAddress\": \"user@example.com\"\n  }\n",
            ] {
                assert_preserved(W, variant, &input, &output, chunk);
            }
        }
    }
}

#[test]
fn codex_config_toml_and_auth_json() {
    const W: &str = "codex";
    for variant in VARIANTS {
        let env = LiveEnv::new();
        env.seed(".codex/auth.json", &variant.apply(&fixture(W, "auth.json")));
        env.seed(
            ".codex/config.toml",
            &variant.apply(&fixture(W, "config.toml")),
        );
        let result = write_live(AppType::Codex, W, "relay", "Relay");
        let text = env.render_result("write", &result)
            + &env.render_files(&[".codex/auth.json", ".codex/config.toml"]);
        assert_snapshot(W, variant, &text);
    }
}

#[test]
fn codex_preserves_official_auth_json() {
    const W: &str = "codex_preserve_auth";
    for variant in VARIANTS {
        let env = LiveEnv::with_settings(AppSettings {
            preserve_codex_official_auth_on_switch: true,
            ..AppSettings::default()
        });
        let auth = variant.apply(&fixture(W, "auth.json"));
        env.seed(".codex/auth.json", &auth);
        env.seed(
            ".codex/config.toml",
            &variant.apply(&fixture(W, "config.toml")),
        );
        let result = write_live(AppType::Codex, W, "relay", "Relay");
        let text = env.render_result("write", &result)
            + &env.render_files(&[".codex/auth.json", ".codex/config.toml"]);
        assert_snapshot(W, variant, &text);
        // The ChatGPT login cache must not be rewritten at all.
        assert_eq!(
            fs::read(env.path(".codex/auth.json")).expect("read auth.json"),
            auth.as_bytes(),
            "{W}/{}: auth.json changed",
            variant.name()
        );
    }
}

#[test]
fn gemini_env_and_settings_json() {
    const W: &str = "gemini";
    for variant in VARIANTS {
        let env = LiveEnv::new();
        env.seed(".gemini/.env", &variant.apply(&fixture(W, "env")));
        env.seed(
            ".gemini/settings.json",
            &variant.apply(&fixture(W, "settings.json")),
        );
        let result = write_live(AppType::Gemini, W, "relay", "Relay");
        let text = env.render_result("write", &result)
            + &env.render_files(&[".gemini/.env", ".gemini/settings.json"]);
        assert_snapshot(W, variant, &text);
    }
}

#[test]
fn grok_config_toml() {
    const W: &str = "grok";
    for variant in VARIANTS {
        let env = LiveEnv::new();
        env.seed(
            ".grok/config.toml",
            &variant.apply(&fixture(W, "config.toml")),
        );
        let result = write_live(AppType::GrokBuild, W, "relay", "Relay");
        let text = env.render_result("write", &result) + &env.render_files(&[".grok/config.toml"]);
        assert_snapshot(W, variant, &text);
    }
}

#[test]
fn opencode_json() {
    const W: &str = "opencode";
    for variant in VARIANTS {
        let env = LiveEnv::new();
        env.seed(
            ".config/opencode/opencode.json",
            &variant.apply(&fixture(W, "opencode.json")),
        );
        let result = write_live(AppType::OpenCode, W, "relay", "Relay");
        let text = env.render_result("write", &result)
            + &env.render_files(&[".config/opencode/opencode.json"]);
        assert_snapshot(W, variant, &text);
    }
}

#[test]
fn openclaw_json5() {
    const W: &str = "openclaw";
    for variant in VARIANTS {
        let env = LiveEnv::new();
        let input = variant.apply(&fixture(W, "openclaw.json"));
        env.seed(".openclaw/openclaw.json", &input);
        let result = write_live(AppType::OpenClaw, W, "relay", "Relay");
        let text =
            env.render_result("write", &result) + &env.render_files(&[".openclaw/openclaw.json"]);
        assert_snapshot(W, variant, &text);
        let output = env.read(".openclaw/openclaw.json");
        assert_preserved(
            W,
            variant,
            &input,
            &output,
            "  // OpenClaw config written by the user\n  agents: {\n    defaults: {\n      workspace: \"~/work\", // trailing comment\n    },\n  },\n",
        );
        // KNOWN BUG (pinned): the json-five 0.3.1 round-trip serializer drops
        // the `/` that closes a block comment, so the rewritten file no longer
        // parses and the write reports an error after the file was replaced
        // (the previous file is kept under ~/.cc-switch/backups/openclaw).
        // When this starts failing, the bug is fixed: turn it into an
        // `assert_preserved` and accept the new snapshots.
        let block_comment = variant.apply("  /* block comment before an unknown section */\n");
        assert!(
            input.contains(block_comment.trim_start_matches('\u{feff}'))
                && !output.contains(block_comment.trim_start_matches('\u{feff}'))
                && result.is_err(),
            "{W}/{}: OpenClaw block comments now survive the write; update the pinned known bug",
            variant.name()
        );
    }
}

#[test]
fn hermes_config_yaml() {
    const W: &str = "hermes";
    for variant in VARIANTS {
        let env = LiveEnv::new();
        let input = variant.apply(&fixture(W, "config.yaml"));
        env.seed(".hermes/config.yaml", &input);
        let result = write_live(AppType::Hermes, W, "relay", "Relay");
        let text =
            env.render_result("write", &result) + &env.render_files(&[".hermes/config.yaml"]);
        assert_snapshot(W, variant, &text);
        if result.is_ok() {
            let output = env.read(".hermes/config.yaml");
            for chunk in [
                "# Hermes config written by the user\nmodel:\n  default: gpt-5\n  provider: custom\n",
                "mcp_servers:\n  docs:\n    command: uvx\n    args: [docs-mcp]   # inline comment\n\nfuture_hermes_key:\n  nested: true\n",
            ] {
                assert_preserved(W, variant, &input, &output, chunk);
            }
        }
    }
}

#[test]
fn claude_desktop_config_files() {
    const W: &str = "claude_desktop";
    const NORMAL: &str = "Claude/claude_desktop_config.json";
    const THREEP: &str = "Claude-3p/claude_desktop_config.json";
    const META: &str = "Claude-3p/configLibrary/_meta.json";
    const PROFILE: &str = "Claude-3p/configLibrary/00000000-0000-4000-8000-000000157210.json";
    for variant in VARIANTS {
        let env = LiveEnv::new();
        env.seed(NORMAL, &variant.apply(&fixture(W, "normal.json")));
        env.seed(THREEP, &variant.apply(&fixture(W, "threep.json")));
        env.seed(META, &variant.apply(&fixture(W, "meta.json")));
        let db = Database::memory().expect("memory db");
        let provider = provider("relay", "Relay", fixture_json(W, "provider.json"));
        let result = kit::apply_claude_desktop_provider_at(
            &db,
            &provider,
            &env.path("Claude"),
            &env.path("Claude-3p"),
        );
        let text = env.render_result("apply", &result)
            + &env.render_files(&[NORMAL, THREEP, META, PROFILE]);
        assert_snapshot(W, variant, &text);
    }
}

#[test]
fn claude_plugin_config_json() {
    const W: &str = "claude_plugin";
    for variant in VARIANTS {
        let env = LiveEnv::new();
        env.seed(
            ".claude/config.json",
            &variant.apply(&fixture(W, "config.json")),
        );
        let write = kit::write_claude_plugin_config();
        let mut text = env.render_result("write", &write);
        text.push_str(&env.render_files(&[".claude/config.json"]));
        let clear = kit::clear_claude_plugin_config();
        text.push_str(&env.render_result("clear", &clear));
        text.push_str(&env.render_files(&[".claude/config.json"]));
        assert_snapshot(W, variant, &text);
    }
}

// ---------------------------------------------------------------------------
// OAuth token stores
// ---------------------------------------------------------------------------

#[tokio::test]
async fn copilot_token_store() {
    const W: &str = "copilot_auth";
    const STORE: &str = ".cc-switch/copilot_auth.json";
    for variant in VARIANTS {
        let env = LiveEnv::new();
        env.seed(STORE, &variant.apply(&fixture(W, "copilot_auth.json")));
        let manager = kit::CopilotAuthManager::new(env.path(".cc-switch"));
        let result = manager.set_default_account("1001").await;
        let text = env.render_result("set_default_account", &result) + &env.render_files(&[STORE]);
        assert_snapshot(W, variant, &text);
        if result.is_ok() {
            assert_private_mode(&env.path(STORE));
        }
    }
}

#[tokio::test]
async fn codex_oauth_token_store() {
    const W: &str = "codex_oauth";
    const STORE: &str = ".cc-switch/codex_oauth_auth.json";
    for variant in VARIANTS {
        let env = LiveEnv::new();
        env.seed(STORE, &variant.apply(&fixture(W, "codex_oauth_auth.json")));
        let manager = kit::CodexOAuthManager::new(env.path(".cc-switch"));
        let result = manager.set_default_account("acct-1").await;
        let text = env.render_result("set_default_account", &result) + &env.render_files(&[STORE]);
        assert_snapshot(W, variant, &text);
        if result.is_ok() {
            assert_private_mode(&env.path(STORE));
        }
    }
}

#[cfg(unix)]
fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .unwrap_or_else(|e| panic!("stat {}: {e}", path.display()))
        .permissions()
        .mode()
        & 0o777
}

/// Token stores hold long-lived refresh tokens: they must be owner-only.
fn assert_private_mode(path: &Path) {
    #[cfg(unix)]
    assert_eq!(
        mode_of(path),
        0o600,
        "{} must be written with mode 0600",
        path.display()
    );
    #[cfg(not(unix))]
    let _ = path;
}

/// File modes after each writer: token stores and the Gemini `.env` are
/// forced to owner-only, every other writer keeps the mode of the file it
/// replaces (seeded as 0644).
#[cfg(unix)]
#[tokio::test]
async fn written_file_modes() {
    let env = LiveEnv::new();
    let mut seeded = Vec::new();
    for (writer, name, rel) in [
        ("claude_settings", "settings.json", ".claude/settings.json"),
        ("claude_mcp", "claude.json", ".claude.json"),
        ("codex", "auth.json", ".codex/auth.json"),
        ("codex", "config.toml", ".codex/config.toml"),
        ("gemini", "env", ".gemini/.env"),
        ("gemini", "settings.json", ".gemini/settings.json"),
        ("grok", "config.toml", ".grok/config.toml"),
        (
            "opencode",
            "opencode.json",
            ".config/opencode/opencode.json",
        ),
        ("openclaw", "openclaw.json", ".openclaw/openclaw.json"),
        ("hermes", "config.yaml", ".hermes/config.yaml"),
        ("claude_plugin", "config.json", ".claude/config.json"),
        (
            "copilot_auth",
            "copilot_auth.json",
            ".cc-switch/copilot_auth.json",
        ),
        (
            "codex_oauth",
            "codex_oauth_auth.json",
            ".cc-switch/codex_oauth_auth.json",
        ),
    ] {
        env.seed(rel, &fixture(writer, name));
        seeded.push(rel);
    }

    let servers: HashMap<String, Value> =
        serde_json::from_value(fixture_json("claude_mcp", "servers.json")).expect("servers");
    write_live(AppType::Claude, "claude_settings", "relay", "Relay").expect("claude");
    kit::set_claude_mcp_servers_map(&servers).expect("claude mcp");
    write_live(AppType::Codex, "codex", "relay", "Relay").expect("codex");
    write_live(AppType::Gemini, "gemini", "relay", "Relay").expect("gemini");
    write_live(AppType::GrokBuild, "grok", "relay", "Relay").expect("grok");
    write_live(AppType::OpenCode, "opencode", "relay", "Relay").expect("opencode");
    // Fails after the file is replaced (block-comment known bug, see
    // `openclaw_json5`); the mode of the replaced file is still pinned.
    let _ = write_live(AppType::OpenClaw, "openclaw", "relay", "Relay");
    write_live(AppType::Hermes, "hermes", "relay", "Relay").expect("hermes");
    kit::write_claude_plugin_config().expect("claude plugin");
    kit::CopilotAuthManager::new(env.path(".cc-switch"))
        .set_default_account("1001")
        .await
        .expect("copilot");
    kit::CodexOAuthManager::new(env.path(".cc-switch"))
        .set_default_account("acct-1")
        .await
        .expect("codex oauth");

    let mut text = String::new();
    for rel in seeded.iter().copied().chain([".gemini"]) {
        text.push_str(&format!("{:o} {rel}\n", mode_of(&env.path(rel))));
    }
    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path(fixtures_root());
    settings.set_prepend_module_to_snapshot(false);
    settings.set_omit_expression(true);
    settings.set_description("file modes after every writer (seeded files are 0644)");
    settings.bind(|| insta::assert_snapshot!("file_modes", text));
}

// ---------------------------------------------------------------------------
// Codex state DB (codex_history_migration)
// ---------------------------------------------------------------------------

#[test]
fn codex_state_db_provider_bucket_migration() {
    const W: &str = "codex_state_db";
    const SESSION: &str = ".codex/sessions/2026/01/01/rollout-fixture.jsonl";
    for variant in VARIANTS {
        let env = LiveEnv::new();
        env.seed(SESSION, &variant.apply(&fixture(W, "session.jsonl")));
        let state_db = env.path(&format!(".codex/{}", kit::CODEX_STATE_DB_FILENAME));
        {
            let conn = rusqlite::Connection::open(&state_db).expect("open state db");
            conn.execute_batch(&fixture(W, "seed.sql"))
                .expect("seed state db");
        }

        let db = Database::memory().expect("memory db");
        db.save_provider(
            "codex",
            &provider("aihubmix", "AIHubMix", fixture_json(W, "provider.json")),
        )
        .expect("save provider");
        // A provider still on the legacy `ccswitch` bucket; the id is only
        // trusted when the config also defines `[model_providers.ccswitch]`.
        db.save_provider(
            "codex",
            &provider(
                "legacy",
                "Legacy",
                serde_json::json!({
                    "auth": {},
                    "config": "model_provider = \"ccswitch\"\n\n[model_providers.ccswitch]\nname = \"CC Switch\"\nbase_url = \"https://legacy.example.com/v1\"\n"
                }),
            ),
        )
        .expect("save legacy provider");

        let outcome = kit::maybe_migrate_codex_third_party_history_provider_bucket(&db);
        let mut text = match &outcome {
            Ok(outcome) => format!(
                "migrate: ok source_provider_ids={:?} migrated_jsonl_files={} migrated_state_rows={} skipped_reason={:?}\n",
                outcome.source_provider_ids,
                outcome.migrated_jsonl_files,
                outcome.migrated_state_rows,
                outcome.skipped_reason
            ),
            Err(e) => format!("migrate: error: {}\n", env.redact(&e.to_string())),
        };
        text.push_str(&format!(
            "=== threads (target id {:?}) ===\n",
            kit::CC_SWITCH_CODEX_MODEL_PROVIDER_ID
        ));
        let conn = rusqlite::Connection::open(&state_db).expect("reopen state db");
        let mut stmt = conn
            .prepare("SELECT id, model_provider, title FROM threads ORDER BY id")
            .expect("prepare");
        let rows = stmt
            .query_map([], |row| {
                Ok(format!(
                    "{} | {} | {}",
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?
                ))
            })
            .expect("query threads");
        for row in rows {
            text.push_str(&row.expect("row"));
            text.push('\n');
        }
        text.push_str(&env.render_files(&[SESSION]));
        text.push_str("=== settings.json localMigrations ===\n");
        text.push_str(&local_migrations_marker(&env));
        assert_snapshot(W, variant, &text);

        if outcome.as_ref().is_ok_and(|o| o.migrated_state_rows > 0) {
            assert!(
                state_db_backup_exists(&env),
                "{W}/{}: state DB was migrated without a backup",
                variant.name()
            );
        }
    }
}

/// The settings.json migration marker, with the timestamp redacted.
fn local_migrations_marker(env: &LiveEnv) -> String {
    let raw = fs::read_to_string(env.path(".cc-switch/settings.json")).expect("read settings");
    let settings: Value = serde_json::from_str(&raw).expect("parse settings");
    let mut marker = settings
        .get("localMigrations")
        .cloned()
        .unwrap_or(Value::Null);
    if let Some(migrations) = marker.as_object_mut() {
        for entry in migrations.values_mut() {
            if let Some(completed_at) = entry.get_mut("completedAt") {
                *completed_at = Value::String("<redacted>".to_string());
            }
        }
    }
    serde_json::to_string_pretty(&marker).expect("render marker") + "\n"
}

fn state_db_backup_exists(env: &LiveEnv) -> bool {
    let root = env.path(".cc-switch/backups/codex-history-provider-migration-v1");
    fs::read_dir(&root)
        .into_iter()
        .flatten()
        .flatten()
        .any(|generation| {
            generation
                .path()
                .join("state")
                .join(kit::CODEX_STATE_DB_FILENAME)
                .exists()
        })
}

/// Guard: every writer listed in the ticket has a fixture directory with a
/// snapshot per variant, so a writer cannot silently drop out of coverage.
#[test]
fn every_writer_has_fixtures_and_snapshots() {
    const WRITERS: [&str; 14] = [
        // 11 live-config writers (Codex and Gemini each write two files)
        "claude_settings",
        "claude_mcp",
        "codex",
        "codex_preserve_auth",
        "gemini",
        "grok",
        "opencode",
        "openclaw",
        "hermes",
        "claude_desktop",
        "claude_plugin",
        // 2 OAuth token stores
        "copilot_auth",
        "codex_oauth",
        // Codex state DB
        "codex_state_db",
    ];
    for writer in WRITERS {
        let dir = fixtures_root().join(writer);
        assert!(dir.is_dir(), "missing fixtures for {writer}");
        for variant in VARIANTS {
            let snap = dir.join(format!("{}.snap", variant.name()));
            assert!(snap.is_file(), "missing snapshot {}", snap.display());
        }
    }
}
