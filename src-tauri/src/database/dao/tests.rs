//! DAO round-trip and invariant tests on an in-memory database (TEST-11).
//!
//! One section per DAO that previously had no direct test: failover, settings,
//! skills, stream_check, universal_providers, prompts and mcp. Everything runs
//! on `Database::memory()`, so no test touches the filesystem or needs
//! `#[serial]`.

use crate::app_config::{McpApps, McpServer};
use crate::database::Database;
use crate::prompt::Prompt;
use crate::provider::{Provider, UniversalProvider};
use crate::services::skill::SkillRepo;
use crate::services::stream_check::{HealthStatus, StreamCheckConfig, StreamCheckResult};
use serde_json::json;

fn db() -> Database {
    Database::memory().expect("in-memory database")
}

fn provider(id: &str, sort_index: Option<usize>) -> Provider {
    let mut p = Provider::with_id(
        id.to_string(),
        format!("Provider {id}"),
        json!({ "env": { "ANTHROPIC_BASE_URL": "https://example.com" } }),
        None,
    );
    p.sort_index = sort_index;
    p
}

// ---------------------------------------------------------------- failover

#[test]
fn failover_queue_add_remove_and_membership() {
    let db = db();
    for (id, idx) in [("a", Some(2)), ("b", Some(1)), ("c", None)] {
        db.save_provider("claude", &provider(id, idx)).unwrap();
    }
    assert!(db.get_failover_queue("claude").unwrap().is_empty());

    db.add_to_failover_queue("claude", "a").unwrap();
    db.add_to_failover_queue("claude", "b").unwrap();
    db.add_to_failover_queue("claude", "c").unwrap();
    assert!(db.is_in_failover_queue("claude", "a").unwrap());

    // Ordered by sort_index, entries without one last.
    let ids: Vec<_> = db
        .get_failover_queue("claude")
        .unwrap()
        .into_iter()
        .map(|i| i.provider_id)
        .collect();
    assert_eq!(ids, ["b", "a", "c"]);

    db.remove_from_failover_queue("claude", "a").unwrap();
    assert!(!db.is_in_failover_queue("claude", "a").unwrap());
    let available: Vec<_> = db
        .get_available_providers_for_failover("claude")
        .unwrap()
        .into_iter()
        .map(|p| p.id)
        .collect();
    assert_eq!(available, ["a"]);
    assert_eq!(db.get_failover_providers("claude").unwrap().len(), 2);
}

#[test]
fn failover_queue_is_scoped_per_app_and_ignores_unknown_ids() {
    let db = db();
    db.save_provider("claude", &provider("shared", Some(0)))
        .unwrap();
    db.save_provider("codex", &provider("shared", Some(0)))
        .unwrap();
    db.add_to_failover_queue("claude", "shared").unwrap();
    assert!(db.is_in_failover_queue("claude", "shared").unwrap());
    assert!(!db.is_in_failover_queue("codex", "shared").unwrap());

    // Unknown ids are a no-op, not an error.
    db.add_to_failover_queue("claude", "missing").unwrap();
    assert!(!db.is_in_failover_queue("claude", "missing").unwrap());
}

#[test]
fn clear_failover_queue_only_touches_one_app() {
    let db = db();
    db.save_provider("claude", &provider("x", None)).unwrap();
    db.save_provider("codex", &provider("y", None)).unwrap();
    db.add_to_failover_queue("claude", "x").unwrap();
    db.add_to_failover_queue("codex", "y").unwrap();
    db.clear_failover_queue("claude").unwrap();
    assert!(db.get_failover_queue("claude").unwrap().is_empty());
    assert_eq!(db.get_failover_queue("codex").unwrap().len(), 1);
}

// ---------------------------------------------------------------- settings

#[test]
fn bool_flag_accepts_true_and_one_only() {
    let db = db();
    assert!(!db.get_bool_flag("flag").unwrap());
    for (value, expected) in [
        ("true", true),
        ("1", true),
        ("false", false),
        ("yes", false),
    ] {
        db.set_setting("flag", value).unwrap();
        assert_eq!(db.get_bool_flag("flag").unwrap(), expected, "{value}");
    }
}

#[test]
fn config_snippet_and_cleared_flag_drive_auto_extract() {
    let db = db();
    assert!(db.should_auto_extract_config_snippet("claude").unwrap());

    db.set_config_snippet("claude", Some("{\"a\":1}".into()))
        .unwrap();
    assert_eq!(
        db.get_config_snippet("claude").unwrap().as_deref(),
        Some("{\"a\":1}")
    );
    assert!(!db.should_auto_extract_config_snippet("claude").unwrap());

    // Explicitly cleared by the user: never auto-extract again.
    db.set_config_snippet("claude", None).unwrap();
    db.set_config_snippet_cleared("claude", true).unwrap();
    assert!(db.is_config_snippet_cleared("claude").unwrap());
    assert!(!db.should_auto_extract_config_snippet("claude").unwrap());

    db.set_config_snippet_cleared("claude", false).unwrap();
    assert!(db.should_auto_extract_config_snippet("claude").unwrap());
    // Other apps are independent.
    assert!(db.should_auto_extract_config_snippet("codex").unwrap());
}

#[test]
fn global_proxy_url_trims_and_clears_on_blank() {
    let db = db();
    assert_eq!(db.get_global_proxy_url().unwrap(), None);
    db.set_global_proxy_url(Some("  http://127.0.0.1:7890  "))
        .unwrap();
    assert_eq!(
        db.get_global_proxy_url().unwrap().as_deref(),
        Some("http://127.0.0.1:7890")
    );
    db.set_global_proxy_url(Some("   ")).unwrap();
    assert_eq!(db.get_global_proxy_url().unwrap(), None);
}

#[test]
#[allow(deprecated)]
fn legacy_proxy_takeover_flags_round_trip() {
    let db = db();
    assert!(!db.get_proxy_takeover_enabled("claude").unwrap());
    assert!(!db.has_any_proxy_takeover().unwrap());
    db.set_proxy_takeover_enabled("claude", true).unwrap();
    assert!(db.get_proxy_takeover_enabled("claude").unwrap());
    assert!(db.has_any_proxy_takeover().unwrap());
    db.clear_all_proxy_takeover().unwrap();
    assert!(!db.get_proxy_takeover_enabled("claude").unwrap());
    assert!(!db.has_any_proxy_takeover().unwrap());
}

#[test]
fn legacy_common_config_migrated_flag_round_trips() {
    let db = db();
    assert!(!db.is_legacy_common_config_migrated().unwrap());
    db.set_legacy_common_config_migrated(true).unwrap();
    assert!(db.is_legacy_common_config_migrated().unwrap());
    db.set_legacy_common_config_migrated(false).unwrap();
    assert!(!db.is_legacy_common_config_migrated().unwrap());
}

// ---------------------------------------------------------------- skills

fn repo(owner: &str, name: &str) -> SkillRepo {
    SkillRepo {
        owner: owner.into(),
        name: name.into(),
        branch: "main".into(),
        enabled: true,
    }
}

#[test]
fn skill_repos_crud_is_sorted_and_upserts() {
    let db = db();
    db.save_skill_repo(&repo("zed", "skills")).unwrap();
    db.save_skill_repo(&repo("acme", "b")).unwrap();
    db.save_skill_repo(&repo("acme", "a")).unwrap();
    let names: Vec<_> = db
        .get_skill_repos()
        .unwrap()
        .into_iter()
        .map(|r| format!("{}/{}", r.owner, r.name))
        .collect();
    assert_eq!(names, ["acme/a", "acme/b", "zed/skills"]);

    let mut disabled = repo("acme", "a");
    disabled.enabled = false;
    disabled.branch = "dev".into();
    db.save_skill_repo(&disabled).unwrap();
    let repos = db.get_skill_repos().unwrap();
    assert_eq!(repos.len(), 3, "same (owner, name) upserts");
    assert!(!repos[0].enabled);
    assert_eq!(repos[0].branch, "dev");

    db.delete_skill_repo("acme", "a").unwrap();
    assert_eq!(db.get_skill_repos().unwrap().len(), 2);
}

#[test]
fn default_skill_repos_seed_once_and_respect_deletions() {
    let db = db();
    let seeded = db.init_default_skill_repos().unwrap();
    assert!(seeded > 0);
    for r in db.get_skill_repos().unwrap() {
        db.delete_skill_repo(&r.owner, &r.name).unwrap();
    }
    // Deleted defaults stay deleted (edea624).
    assert_eq!(db.init_default_skill_repos().unwrap(), 0);
    assert!(db.get_skill_repos().unwrap().is_empty());
}

#[test]
fn default_skill_repos_are_not_added_to_existing_user_repos() {
    let db = db();
    db.save_skill_repo(&repo("me", "mine")).unwrap();
    assert_eq!(db.init_default_skill_repos().unwrap(), 0);
    assert_eq!(db.get_skill_repos().unwrap().len(), 1);
}

// ---------------------------------------------------------------- stream_check

fn check_result(tested_at: i64) -> StreamCheckResult {
    StreamCheckResult {
        status: HealthStatus::Operational,
        success: true,
        message: "ok".into(),
        response_time_ms: Some(120),
        http_status: Some(200),
        model_used: String::new(),
        tested_at,
        retry_count: 0,
        error_category: None,
    }
}

#[test]
fn stream_check_config_defaults_and_round_trips() {
    let db = db();
    let default = db.get_stream_check_config().unwrap();
    assert_eq!(
        default.timeout_secs,
        StreamCheckConfig::default().timeout_secs
    );

    let custom = StreamCheckConfig {
        timeout_secs: 3,
        max_retries: 4,
        degraded_threshold_ms: 900,
    };
    db.save_stream_check_config(&custom).unwrap();
    let read = db.get_stream_check_config().unwrap();
    assert_eq!(
        (
            read.timeout_secs,
            read.max_retries,
            read.degraded_threshold_ms
        ),
        (3, 4, 900)
    );
}

#[test]
fn stream_check_config_with_corrupt_json_is_an_error_not_a_panic() {
    let db = db();
    db.set_setting("stream_check_config", "{not json").unwrap();
    assert!(db.get_stream_check_config().is_err());
}

#[test]
fn stream_check_log_cleanup_keeps_recent_rows() {
    let db = db();
    let now = chrono::Utc::now().timestamp();
    let first = db
        .save_stream_check_log("p", "P", "claude", &check_result(now - 40 * 86400))
        .unwrap();
    let second = db
        .save_stream_check_log("p", "P", "claude", &check_result(now))
        .unwrap();
    assert!(second > first, "row ids increase");
    assert_eq!(db.cleanup_old_stream_check_logs(30).unwrap(), 1);
    assert_eq!(db.cleanup_old_stream_check_logs(30).unwrap(), 0);
}

// ---------------------------------------------------------------- universal providers

#[test]
fn universal_provider_crud_round_trip() {
    let db = db();
    assert!(db.get_all_universal_providers().unwrap().is_empty());
    let up = UniversalProvider::new(
        "u1".into(),
        "Relay".into(),
        "newapi".into(),
        "https://relay.example.com".into(),
        "sk-universal".into(),
    );
    db.save_universal_provider(&up).unwrap();
    let read = db.get_universal_provider("u1").unwrap().expect("saved");
    assert_eq!(read.name, "Relay");
    assert_eq!(read.base_url, "https://relay.example.com");
    assert_eq!(read.api_key, "sk-universal", "reads return the real key");
    assert_eq!(db.get_all_universal_providers().unwrap().len(), 1);

    assert!(db.delete_universal_provider("u1").unwrap());
    assert!(!db.delete_universal_provider("u1").unwrap());
    assert!(db.get_universal_provider("u1").unwrap().is_none());
}

#[test]
fn universal_providers_with_corrupt_blob_is_an_error() {
    let db = db();
    db.set_setting("universal_providers", "[1,2,3]").unwrap();
    assert!(db.get_all_universal_providers().is_err());
}

// ---------------------------------------------------------------- prompts

fn prompt(id: &str, created_at: i64) -> Prompt {
    Prompt {
        id: id.into(),
        name: format!("Prompt {id}"),
        content: format!("# {id}"),
        description: Some("desc".into()),
        enabled: false,
        created_at: Some(created_at),
        updated_at: None,
    }
}

#[test]
fn prompts_are_scoped_per_app_and_ordered_by_creation() {
    let db = db();
    db.save_prompt("claude", &prompt("late", 20)).unwrap();
    db.save_prompt("claude", &prompt("early", 10)).unwrap();
    db.save_prompt("codex", &prompt("other", 5)).unwrap();
    let ids: Vec<_> = db.get_prompts("claude").unwrap().into_keys().collect();
    assert_eq!(ids, ["early", "late"]);
    assert_eq!(db.get_prompts("codex").unwrap().len(), 1);
}

#[test]
fn prompt_upsert_and_delete() {
    let db = db();
    db.save_prompt("claude", &prompt("p", 1)).unwrap();
    let mut edited = prompt("p", 1);
    edited.enabled = true;
    edited.content = "changed".into();
    db.save_prompt("claude", &edited).unwrap();
    let prompts = db.get_prompts("claude").unwrap();
    assert_eq!(prompts.len(), 1);
    assert!(prompts["p"].enabled);
    assert_eq!(prompts["p"].content, "changed");
    db.delete_prompt("claude", "p").unwrap();
    assert!(db.get_prompts("claude").unwrap().is_empty());
}

// ---------------------------------------------------------------- mcp

fn mcp(id: &str) -> McpServer {
    McpServer {
        id: id.into(),
        name: id.to_uppercase(),
        server: json!({ "type": "stdio", "command": "npx", "args": ["-y", id] }),
        apps: McpApps {
            claude: true,
            ..Default::default()
        },
        description: None,
        homepage: None,
        docs: None,
        tags: vec!["t".into()],
    }
}

#[test]
fn mcp_servers_round_trip_with_json_blob_and_apps() {
    let db = db();
    db.save_mcp_server(&mcp("fs")).unwrap();
    db.save_mcp_server(&mcp("git")).unwrap();
    let all = db.get_all_mcp_servers().unwrap();
    assert_eq!(all.len(), 2);
    let fs = &all["fs"];
    assert_eq!(fs.name, "FS");
    assert_eq!(fs.server["args"], json!(["-y", "fs"]));
    assert!(fs.apps.claude && !fs.apps.codex);
    assert_eq!(fs.tags, ["t"]);
}

#[test]
fn mcp_server_upsert_and_delete() {
    let db = db();
    db.save_mcp_server(&mcp("fs")).unwrap();
    let mut edited = mcp("fs");
    edited.apps.codex = true;
    db.save_mcp_server(&edited).unwrap();
    let all = db.get_all_mcp_servers().unwrap();
    assert_eq!(all.len(), 1);
    assert!(all["fs"].apps.codex);
    db.delete_mcp_server("fs").unwrap();
    assert!(db.get_all_mcp_servers().unwrap().is_empty());
    // Deleting a missing id is not an error.
    db.delete_mcp_server("fs").unwrap();
}

// ---------------------------------------------------------------- migrations (TEST-42)

/// A failure in the middle of the migration chain rolls back every earlier
/// step, including v13 -> v14's destructive DROP TABLE proxy_config + RENAME:
/// the database keeps its version, its data and its old table layout, and
/// the next attempt (once the cause is gone) migrates cleanly.
#[test]
fn mid_chain_migration_failure_rolls_back_every_step() {
    use crate::database::schema::{LATEST_SCHEMA_VERSION, MIGRATION_LEDGER_TABLE};
    use rusqlite::Connection;

    let conn = Connection::open_in_memory().unwrap();
    Database::create_tables_on_conn(&conn).unwrap();
    // Pretend the database was left at v13 by an older release.
    conn.execute(
        &format!("DELETE FROM {MIGRATION_LEDGER_TABLE} WHERE version > 13"),
        [],
    )
    .unwrap();
    Database::set_user_version(&conn, 13).unwrap();
    conn.execute(
        "UPDATE proxy_config SET listen_port = 9999 WHERE app_type = 'claude'",
        [],
    )
    .unwrap();
    let proxy_sql = |conn: &Connection| -> String {
        conn.query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'proxy_config'",
            [],
            |row| row.get(0),
        )
        .unwrap()
    };
    let sql_before = proxy_sql(&conn);
    let ledger_before: i64 = conn
        .query_row(
            &format!("SELECT COUNT(*) FROM {MIGRATION_LEDGER_TABLE}"),
            [],
            |row| row.get(0),
        )
        .unwrap();

    // Inject a failure after v14 has run: recording v15 in the ledger aborts.
    conn.execute_batch(&format!(
        "CREATE TRIGGER fail_v15 BEFORE INSERT ON {MIGRATION_LEDGER_TABLE}
         WHEN NEW.version = 15 BEGIN SELECT RAISE(ABORT, 'injected v15 failure'); END;"
    ))
    .unwrap();

    let err = Database::apply_schema_migrations_on_conn(&conn).unwrap_err();
    assert!(err.to_string().contains("injected v15 failure"), "{err}");

    assert_eq!(Database::get_user_version(&conn).unwrap(), 13);
    let port: i64 = conn
        .query_row(
            "SELECT listen_port FROM proxy_config WHERE app_type = 'claude'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(port, 9999, "proxy_config rows survive the rolled-back DROP");
    assert_eq!(proxy_sql(&conn), sql_before, "v14 table rebuild was undone");
    let leftover: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name = 'proxy_config_v14'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(leftover, 0);
    let ledger_after: i64 = conn
        .query_row(
            &format!("SELECT COUNT(*) FROM {MIGRATION_LEDGER_TABLE}"),
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        ledger_after, ledger_before,
        "no ledger rows from the failed run"
    );

    // Once the cause is gone, the same database migrates all the way.
    conn.execute_batch("DROP TRIGGER fail_v15;").unwrap();
    Database::apply_schema_migrations_on_conn(&conn).unwrap();
    assert_eq!(
        Database::get_user_version(&conn).unwrap(),
        LATEST_SCHEMA_VERSION
    );
    let port: i64 = conn
        .query_row(
            "SELECT listen_port FROM proxy_config WHERE app_type = 'claude'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(port, 9999);
}
