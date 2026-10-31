//! 数据库模块测试
//!
//! 包含 Schema 迁移和基本功能的测试。

use super::*;
use crate::app_config::MultiAppConfig;
use crate::provider::{Provider, ProviderManager};
use indexmap::IndexMap;
use rusqlite::{params, Connection};
use serde_json::json;
use std::collections::HashMap;
use tempfile::NamedTempFile;

const LEGACY_SCHEMA_SQL: &str = r#"
    CREATE TABLE providers (
        id TEXT NOT NULL,
        app_type TEXT NOT NULL,
        name TEXT NOT NULL,
        settings_config TEXT NOT NULL,
        PRIMARY KEY (id, app_type)
    );
    CREATE TABLE provider_endpoints (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        provider_id TEXT NOT NULL,
        app_type TEXT NOT NULL,
        url TEXT NOT NULL
    );
    CREATE TABLE mcp_servers (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        server_config TEXT NOT NULL
    );
    CREATE TABLE prompts (
        id TEXT NOT NULL,
        app_type TEXT NOT NULL,
        name TEXT NOT NULL,
        content TEXT NOT NULL,
        PRIMARY KEY (id, app_type)
    );
    CREATE TABLE skills (
        key TEXT PRIMARY KEY,
        installed BOOLEAN NOT NULL DEFAULT 0
    );
    CREATE TABLE skill_repos (
        owner TEXT NOT NULL,
        name TEXT NOT NULL,
        PRIMARY KEY (owner, name)
    );
    CREATE TABLE settings (
        key TEXT PRIMARY KEY,
        value TEXT
    );
"#;

// v3.8.x（schema v1）的真实表结构快照：用于验证从 v3.8.* 升级到当前版本的迁移链路
// 参考：tag v3.8.3 的 src-tauri/src/database/schema.rs
const V3_8_SCHEMA_V1_SQL: &str = r#"
    CREATE TABLE providers (
        id TEXT NOT NULL,
        app_type TEXT NOT NULL,
        name TEXT NOT NULL,
        settings_config TEXT NOT NULL,
        website_url TEXT,
        category TEXT,
        created_at INTEGER,
        sort_index INTEGER,
        notes TEXT,
        icon TEXT,
        icon_color TEXT,
        meta TEXT NOT NULL DEFAULT '{}',
        is_current BOOLEAN NOT NULL DEFAULT 0,
        PRIMARY KEY (id, app_type)
    );
    CREATE TABLE provider_endpoints (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        provider_id TEXT NOT NULL,
        app_type TEXT NOT NULL,
        url TEXT NOT NULL,
        added_at INTEGER,
        FOREIGN KEY (provider_id, app_type) REFERENCES providers(id, app_type) ON DELETE CASCADE
    );
    CREATE TABLE mcp_servers (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        server_config TEXT NOT NULL,
        description TEXT,
        homepage TEXT,
        docs TEXT,
        tags TEXT NOT NULL DEFAULT '[]',
        enabled_claude BOOLEAN NOT NULL DEFAULT 0,
        enabled_codex BOOLEAN NOT NULL DEFAULT 0,
        enabled_gemini BOOLEAN NOT NULL DEFAULT 0
    );
    CREATE TABLE prompts (
        id TEXT NOT NULL,
        app_type TEXT NOT NULL,
        name TEXT NOT NULL,
        content TEXT NOT NULL,
        description TEXT,
        enabled BOOLEAN NOT NULL DEFAULT 1,
        created_at INTEGER,
        updated_at INTEGER,
        PRIMARY KEY (id, app_type)
    );
    CREATE TABLE skills (
        key TEXT PRIMARY KEY,
        installed BOOLEAN NOT NULL DEFAULT 0,
        installed_at INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE skill_repos (
        owner TEXT NOT NULL,
        name TEXT NOT NULL,
        branch TEXT NOT NULL DEFAULT 'main',
        enabled BOOLEAN NOT NULL DEFAULT 1,
        PRIMARY KEY (owner, name)
    );
    CREATE TABLE settings (
        key TEXT PRIMARY KEY,
        value TEXT
    );
"#;

#[derive(Debug)]
struct ColumnInfo {
    r#type: String,
    notnull: i64,
    default: Option<String>,
}

fn get_column_info(conn: &Connection, table: &str, column: &str) -> ColumnInfo {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info(\"{table}\");"))
        .expect("prepare pragma");
    let mut rows = stmt.query([]).expect("query pragma");
    while let Some(row) = rows.next().expect("read row") {
        let column_name: String = row.get(1).expect("name");
        if column_name.eq_ignore_ascii_case(column) {
            return ColumnInfo {
                r#type: row.get::<_, String>(2).expect("type"),
                notnull: row.get::<_, i64>(3).expect("notnull"),
                default: row.get::<_, Option<String>>(4).ok().flatten(),
            };
        }
    }
    panic!("column {table}.{column} not found");
}

fn normalize_default(default: &Option<String>) -> Option<String> {
    default
        .as_ref()
        .map(|s| s.trim_matches('\'').trim_matches('"').to_string())
}

#[test]
fn deleted_default_skill_repo_is_not_restored() {
    let db = Database::memory().expect("create memory db");

    assert_eq!(db.init_default_skill_repos().expect("initialize repos"), 4);
    for repo in db.get_skill_repos().expect("get initialized repos") {
        db.delete_skill_repo(&repo.owner, &repo.name)
            .expect("delete repo");
    }
    assert!(db.get_skill_repos().expect("get deleted repos").is_empty());

    assert_eq!(
        db.init_default_skill_repos().expect("reinitialize repos"),
        0
    );
    assert!(db.get_skill_repos().expect("get repos").is_empty());
}

#[test]
fn existing_skill_repo_selection_is_not_supplemented() {
    let db = Database::memory().expect("create memory db");
    let default_store = crate::services::skill::SkillStore::default();
    db.save_skill_repo(&default_store.repos[0])
        .expect("save existing repo");

    assert_eq!(db.init_default_skill_repos().expect("initialize repos"), 0);
    assert_eq!(db.get_skill_repos().expect("get repos").len(), 1);
    assert!(db
        .get_bool_flag("default_skill_repos_initialized")
        .expect("get initialized flag"));
}

#[test]
fn schema_migration_sets_user_version_when_missing() {
    let conn = Connection::open_in_memory().expect("open memory db");

    // A brand-new database is created directly at the current version (it used to be
    // left at user_version=0 until apply_schema_migrations ran the whole chain).
    Database::create_tables_on_conn(&conn).expect("create tables");
    assert_eq!(
        Database::get_user_version(&conn).expect("read version before"),
        SCHEMA_VERSION
    );

    Database::apply_schema_migrations_on_conn(&conn).expect("apply migration");

    assert_eq!(
        Database::get_user_version(&conn).expect("read version after"),
        SCHEMA_VERSION
    );
}

#[test]
fn schema_migration_rejects_future_version() {
    let conn = Connection::open_in_memory().expect("open memory db");
    Database::create_tables_on_conn(&conn).expect("create tables");
    Database::set_user_version(&conn, SCHEMA_VERSION + 1).expect("set future version");

    let err =
        Database::apply_schema_migrations_on_conn(&conn).expect_err("should reject higher version");
    assert!(
        err.to_string().contains("数据库版本过新"),
        "unexpected error: {err}"
    );
}

/// DB-13: every timestamp-like column is documented with its unit
/// (docs/dev/database.md); new ones must be INTEGER (Unix ms for new columns).
#[test]
fn timestamp_columns_use_known_units() {
    // (table, column, unit) of every existing timestamp column.
    const KNOWN: &[(&str, &str, &str)] = &[
        ("providers", "created_at", "ms"),
        ("providers", "sync_updated_at", "ms"),
        ("provider_endpoints", "added_at", "ms"),
        ("mcp_servers", "sync_updated_at", "ms"),
        ("prompts", "created_at", "s"),
        ("prompts", "updated_at", "s"),
        ("prompts", "sync_updated_at", "ms"),
        ("skills", "installed_at", "s"),
        ("skills", "updated_at", "s"),
        ("skills", "sync_updated_at", "ms"),
        ("profiles", "created_at", "s"),
        ("profiles", "updated_at", "s"),
        ("profiles", "sync_updated_at", "ms"),
        ("proxy_request_logs", "created_at", "s"),
        ("stream_check_logs", "tested_at", "s"),
        ("session_log_sync", "last_modified", "ns"),
        ("session_log_sync", "last_synced_at", "s"),
        ("schema_migrations", "applied_at", "s"),
        ("sync_tombstones", "deleted_at", "ms"),
        // Legacy TEXT timestamps (documented; do not add more).
        ("proxy_config", "created_at", "text:datetime"),
        ("proxy_config", "updated_at", "text:datetime"),
        ("provider_health", "last_success_at", "text:rfc3339"),
        ("provider_health", "last_failure_at", "text:rfc3339"),
        ("provider_health", "updated_at", "text:rfc3339"),
        ("proxy_live_backup", "backed_up_at", "text:rfc3339"),
        ("usage_daily_rollups", "date", "text:local-date"),
    ];
    let looks_like_timestamp = |name: &str| {
        name.ends_with("_at")
            || name.ends_with("_time")
            || name == "date"
            || name == "last_modified"
            || name.contains("timestamp")
    };
    let conn = Connection::open_in_memory().expect("open memory db");
    Database::create_tables_on_conn(&conn).expect("fresh schema");
    let tables = query_strings(
        &conn,
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
    );
    let mut seen = Vec::new();
    for table in tables.iter().map(|t| t[0].clone()) {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info(\"{table}\")"))
            .expect("table_info");
        let cols: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get(1)?, row.get(2)?)))
            .expect("columns")
            .collect::<Result<_, _>>()
            .expect("columns");
        for (column, ty) in cols {
            if !looks_like_timestamp(&column) {
                continue;
            }
            let known = KNOWN
                .iter()
                .find(|(t, c, _)| *t == table && *c == column)
                .unwrap_or_else(|| {
                    panic!(
                        "timestamp column {table}.{column} ({ty}) is not documented: add it to \
                         KNOWN here and to docs/dev/database.md (new columns: INTEGER Unix ms)"
                    )
                });
            if known.2.starts_with("text:") {
                assert!(ty.eq_ignore_ascii_case("TEXT"), "{table}.{column}: {ty}");
            } else {
                assert!(
                    ty.eq_ignore_ascii_case("INTEGER"),
                    "{table}.{column} must be INTEGER, is {ty}"
                );
            }
            seen.push((table.clone(), column));
        }
    }
    for (t, c, _) in KNOWN {
        assert!(
            seen.iter().any(|(st, sc)| st == t && sc == c),
            "documented column {t}.{c} no longer exists"
        );
    }
}

/// DB-30: opening a database from a newer build leaves the file untouched.
#[test]
fn opening_a_newer_database_does_not_modify_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("cc-switch.db");
    {
        let conn = Connection::open(&path).expect("create");
        conn.execute_batch(
            "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT);
             INSERT INTO settings (key, value) VALUES ('k', 'v');",
        )
        .expect("seed");
        Database::set_user_version(&conn, SCHEMA_VERSION + 1).expect("future version");
    }
    let before = std::fs::read(&path).expect("read before");
    let err = Database::open_file(&path)
        .err()
        .expect("newer database is rejected");
    assert!(err.to_string().contains("数据库版本过新"), "{err}");
    let conn = Connection::open(&path).expect("reopen");
    assert!(Database::create_tables_on_conn(&conn).is_err());
    drop(conn);
    assert_eq!(std::fs::read(&path).expect("read after"), before);
}

#[test]
fn schema_migration_adds_missing_columns_for_providers() {
    let conn = Connection::open_in_memory().expect("open memory db");

    // 创建旧版 providers 表，缺少新增列
    conn.execute_batch(LEGACY_SCHEMA_SQL)
        .expect("seed old schema");

    Database::apply_schema_migrations_on_conn(&conn).expect("apply migrations");

    // 验证关键新增列已补齐
    for (table, column) in [
        ("providers", "meta"),
        ("providers", "is_current"),
        ("provider_endpoints", "added_at"),
        ("mcp_servers", "enabled_gemini"),
        ("prompts", "updated_at"),
        ("skills", "installed_at"),
        ("skill_repos", "enabled"),
    ] {
        assert!(
            Database::has_column(&conn, table, column).expect("check column"),
            "{table}.{column} should exist after migration"
        );
    }

    // 验证 meta 列约束保持一致
    let meta = get_column_info(&conn, "providers", "meta");
    assert_eq!(meta.notnull, 1, "meta should be NOT NULL");
    assert_eq!(
        normalize_default(&meta.default).as_deref(),
        Some("{}"),
        "meta default should be '{{}}'"
    );

    assert_eq!(
        Database::get_user_version(&conn).expect("version after migration"),
        SCHEMA_VERSION
    );
}

#[test]
fn schema_migration_aligns_column_defaults_and_types() {
    let conn = Connection::open_in_memory().expect("open memory db");
    conn.execute_batch(LEGACY_SCHEMA_SQL)
        .expect("seed old schema");

    Database::apply_schema_migrations_on_conn(&conn).expect("apply migrations");

    let is_current = get_column_info(&conn, "providers", "is_current");
    assert_eq!(is_current.r#type, "BOOLEAN");
    assert_eq!(is_current.notnull, 1);
    assert_eq!(normalize_default(&is_current.default).as_deref(), Some("0"));

    let tags = get_column_info(&conn, "mcp_servers", "tags");
    assert_eq!(tags.r#type, "TEXT");
    assert_eq!(tags.notnull, 1);
    assert_eq!(normalize_default(&tags.default).as_deref(), Some("[]"));

    let enabled = get_column_info(&conn, "prompts", "enabled");
    assert_eq!(enabled.r#type, "BOOLEAN");
    assert_eq!(enabled.notnull, 1);
    assert_eq!(normalize_default(&enabled.default).as_deref(), Some("1"));

    let installed_at = get_column_info(&conn, "skills", "installed_at");
    assert_eq!(installed_at.r#type, "INTEGER");
    assert_eq!(installed_at.notnull, 1);
    assert_eq!(
        normalize_default(&installed_at.default).as_deref(),
        Some("0")
    );

    let branch = get_column_info(&conn, "skill_repos", "branch");
    assert_eq!(branch.r#type, "TEXT");
    assert_eq!(normalize_default(&branch.default).as_deref(), Some("main"));

    let skill_repo_enabled = get_column_info(&conn, "skill_repos", "enabled");
    assert_eq!(skill_repo_enabled.r#type, "BOOLEAN");
    assert_eq!(skill_repo_enabled.notnull, 1);
    assert_eq!(
        normalize_default(&skill_repo_enabled.default).as_deref(),
        Some("1")
    );
}

#[test]
fn schema_create_tables_include_pricing_model_columns() {
    let conn = Connection::open_in_memory().expect("open memory db");
    Database::create_tables_on_conn(&conn).expect("create tables");

    let multiplier = get_column_info(&conn, "proxy_config", "default_cost_multiplier");
    assert_eq!(multiplier.r#type, "TEXT");
    assert_eq!(multiplier.notnull, 1);
    assert_eq!(normalize_default(&multiplier.default).as_deref(), Some("1"));

    let pricing_source = get_column_info(&conn, "proxy_config", "pricing_model_source");
    assert_eq!(pricing_source.r#type, "TEXT");
    assert_eq!(pricing_source.notnull, 1);
    assert_eq!(
        normalize_default(&pricing_source.default).as_deref(),
        Some("response")
    );

    let request_model = get_column_info(&conn, "proxy_request_logs", "request_model");
    assert_eq!(request_model.r#type, "TEXT");
    assert_eq!(request_model.notnull, 0);
}

#[test]
fn schema_migration_v4_adds_pricing_model_columns() {
    let conn = Connection::open_in_memory().expect("open memory db");
    conn.execute_batch(
        r#"
        CREATE TABLE providers (
            id TEXT NOT NULL,
            app_type TEXT NOT NULL,
            name TEXT NOT NULL,
            settings_config TEXT NOT NULL DEFAULT '{}',
            meta TEXT NOT NULL DEFAULT '{}',
            PRIMARY KEY (id, app_type)
        );
        CREATE TABLE proxy_config (app_type TEXT PRIMARY KEY);
        CREATE TABLE proxy_request_logs (request_id TEXT PRIMARY KEY, model TEXT NOT NULL);
        CREATE TABLE mcp_servers (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            server_config TEXT NOT NULL,
            enabled_claude INTEGER NOT NULL DEFAULT 0,
            enabled_codex INTEGER NOT NULL DEFAULT 0,
            enabled_gemini INTEGER NOT NULL DEFAULT 0,
            enabled_opencode INTEGER NOT NULL DEFAULT 0
        );
        "#,
    )
    .expect("seed v4 schema");

    Database::set_user_version(&conn, 4).expect("set user_version=4");
    Database::apply_schema_migrations_on_conn(&conn).expect("apply migrations");

    let multiplier = get_column_info(&conn, "proxy_config", "default_cost_multiplier");
    assert_eq!(multiplier.r#type, "TEXT");
    assert_eq!(multiplier.notnull, 1);
    assert_eq!(normalize_default(&multiplier.default).as_deref(), Some("1"));

    let pricing_source = get_column_info(&conn, "proxy_config", "pricing_model_source");
    assert_eq!(pricing_source.r#type, "TEXT");
    assert_eq!(pricing_source.notnull, 1);
    assert_eq!(
        normalize_default(&pricing_source.default).as_deref(),
        Some("response")
    );

    let request_model = get_column_info(&conn, "proxy_request_logs", "request_model");
    assert_eq!(request_model.r#type, "TEXT");
    assert_eq!(request_model.notnull, 0);

    assert_eq!(
        Database::get_user_version(&conn).expect("version after migration"),
        SCHEMA_VERSION
    );
}

#[test]
fn migration_v10_to_v11_rebuilds_rollups_with_request_model_dimension() {
    let conn = Connection::open_in_memory().expect("open memory db");

    // 模拟 v10 形状的 rollup 表（主键不含 request_model）+ 一行历史聚合数据，
    // 以及 v10 形状的明细表（无 pricing_model 列）
    conn.execute_batch(
        r#"
        CREATE TABLE proxy_request_logs (
            request_id TEXT PRIMARY KEY,
            model TEXT NOT NULL,
            request_model TEXT
        );
        CREATE TABLE usage_daily_rollups (
            date TEXT NOT NULL,
            app_type TEXT NOT NULL,
            provider_id TEXT NOT NULL,
            model TEXT NOT NULL,
            request_count INTEGER NOT NULL DEFAULT 0,
            success_count INTEGER NOT NULL DEFAULT 0,
            input_tokens INTEGER NOT NULL DEFAULT 0,
            output_tokens INTEGER NOT NULL DEFAULT 0,
            cache_read_tokens INTEGER NOT NULL DEFAULT 0,
            cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
            total_cost_usd TEXT NOT NULL DEFAULT '0',
            avg_latency_ms INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (date, app_type, provider_id, model)
        );
        INSERT INTO usage_daily_rollups
            (date, app_type, provider_id, model, request_count, success_count,
             input_tokens, output_tokens, total_cost_usd, avg_latency_ms)
        VALUES ('2026-05-01', 'claude', 'p1', 'kimi-k2', 7, 7, 1000, 500, '0.07', 120);
        "#,
    )
    .expect("seed v10 rollup table");

    Database::set_user_version(&conn, 10).expect("set user_version=10");
    Database::apply_schema_migrations_on_conn(&conn).expect("apply migrations");

    // 新列存在且 NOT NULL DEFAULT ''
    let request_model = get_column_info(&conn, "usage_daily_rollups", "request_model");
    assert_eq!(request_model.r#type, "TEXT");
    assert_eq!(request_model.notnull, 1);
    let rollup_pricing_model = get_column_info(&conn, "usage_daily_rollups", "pricing_model");
    assert_eq!(rollup_pricing_model.r#type, "TEXT");
    assert_eq!(rollup_pricing_model.notnull, 1);

    // 明细表补上 pricing_model 列（可空，历史行 NULL）
    let pricing_model = get_column_info(&conn, "proxy_request_logs", "pricing_model");
    assert_eq!(pricing_model.r#type, "TEXT");
    assert_eq!(pricing_model.notnull, 0);

    // 历史行保留，request_model 填 ''（未知）
    let (rm, count, input, cost): (String, i64, i64, String) = conn
        .query_row(
            "SELECT request_model, request_count, input_tokens, total_cost_usd
             FROM usage_daily_rollups WHERE model = 'kimi-k2'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("migrated row");
    assert_eq!(rm, "");
    assert_eq!(count, 7);
    assert_eq!(input, 1000);
    assert_eq!(cost, "0.07");

    // 主键包含 request_model：同 model 不同别名可共存
    conn.execute(
        "INSERT INTO usage_daily_rollups
            (date, app_type, provider_id, model, request_model, request_count)
         VALUES ('2026-05-01', 'claude', 'p1', 'kimi-k2', 'claude-sonnet-4-6', 1)",
        [],
    )
    .expect("insert row with same model but different request_model");

    assert_eq!(
        Database::get_user_version(&conn).expect("version after migration"),
        SCHEMA_VERSION
    );
}

#[test]
fn schema_migration_repairs_dev_global_profile_marker() {
    let conn = Connection::open_in_memory().expect("open memory db");

    // 模拟跑过未发布开发版的库：user_version 已是 12（迁移不会再跑），
    // 但 current 标记还是全局 key（现按应用分组）
    conn.execute_batch(
        r#"
        CREATE TABLE profiles (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            payload TEXT NOT NULL,
            sort_order INTEGER,
            created_at INTEGER,
            updated_at INTEGER
        );
        INSERT INTO profiles (id, name, payload) VALUES ('p1', 'Project A', '{}');
        CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT);
        INSERT INTO settings (key, value) VALUES ('current_profile_id', 'p1');
        "#,
    )
    .expect("seed dev v12 shape");
    Database::set_user_version(&conn, 12).expect("set user_version=12");

    // Formerly repaired by create_tables on every start; now by the v16 -> v17 migration.
    Database::create_tables_on_conn(&conn).expect("create tables");
    Database::apply_schema_migrations_on_conn(&conn).expect("migration should repair marker");

    // 全局 current 标记改名为 claude 组标记，旧 key 删除
    let claude_marker: String = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'current_profile_id_claude'",
            [],
            |row| row.get(0),
        )
        .expect("scoped current marker");
    assert_eq!(claude_marker, "p1");
    let old_marker: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM settings WHERE key = 'current_profile_id'",
            [],
            |row| row.get(0),
        )
        .expect("count old marker");
    assert_eq!(old_marker, 0);

    // 修复必须幂等：再跑一遍不应破坏已迁移的标记
    Database::create_tables_on_conn(&conn).expect("create tables again");
    Database::apply_schema_migrations_on_conn(&conn).expect("repair is idempotent");
    let claude_marker: String = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'current_profile_id_claude'",
            [],
            |row| row.get(0),
        )
        .expect("scoped current marker survives");
    assert_eq!(claude_marker, "p1");
}

#[test]
fn schema_migration_repairs_legacy_proxy_config_singleton_to_per_app() {
    let conn = Connection::open_in_memory().expect("open memory db");

    // 模拟测试版 v2：user_version=2，但 proxy_config 仍是单例结构（无 app_type）
    Database::set_user_version(&conn, 2).expect("set user_version");
    conn.execute_batch(
        r#"
        CREATE TABLE proxy_config (
            id INTEGER PRIMARY KEY,
            enabled INTEGER NOT NULL DEFAULT 0,
            listen_address TEXT NOT NULL DEFAULT '127.0.0.1',
            listen_port INTEGER NOT NULL DEFAULT 5000,
            max_retries INTEGER NOT NULL DEFAULT 3,
            request_timeout INTEGER NOT NULL DEFAULT 300,
            enable_logging INTEGER NOT NULL DEFAULT 1,
            target_app TEXT NOT NULL DEFAULT 'claude',
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        INSERT INTO proxy_config (id, enabled) VALUES (1, 1);
        "#,
    )
    .expect("seed legacy proxy_config");

    // Formerly repaired by create_tables on every start; now by the one-time pre-v17
    // preflight that runs before the migration chain.
    Database::create_tables_on_conn(&conn).expect("create tables");
    Database::apply_schema_migrations_on_conn(&conn).expect("migration should repair proxy_config");

    assert!(
        Database::has_column(&conn, "proxy_config", "app_type").expect("check app_type"),
        "proxy_config should be migrated to per-app structure"
    );

    let count: i32 = conn
        .query_row("SELECT COUNT(*) FROM proxy_config", [], |r| r.get(0))
        .expect("count rows");
    assert_eq!(count, 4, "per-app proxy_config should have 4 rows");

    // 新结构下应能按 app_type 查询
    let _: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM proxy_config WHERE app_type = 'claude'",
            [],
            |r| r.get(0),
        )
        .expect("query by app_type");

    // The legacy columns are added before the conversion reads the singleton row, so the
    // old listen port (5000) survives instead of falling back to defaults.
    let port: i64 = conn
        .query_row(
            "SELECT listen_port FROM proxy_config WHERE app_type = 'codex'",
            [],
            |r| r.get(0),
        )
        .expect("listen port");
    assert_eq!(port, 5000);
}

#[test]
fn migration_from_v3_8_schema_v1_to_current_schema_v3() {
    let conn = Connection::open_in_memory().expect("open memory db");
    conn.execute("PRAGMA foreign_keys = ON;", [])
        .expect("enable foreign keys");

    // 模拟 v3.8.* 用户的数据库（schema v1）
    conn.execute_batch(V3_8_SCHEMA_V1_SQL)
        .expect("seed v3.8 schema v1");
    Database::set_user_version(&conn, 1).expect("set user_version=1");

    // 插入一条旧版 Provider + Skill（用于验证迁移不会破坏既有数据）
    conn.execute(
        "INSERT INTO providers (
            id, app_type, name, settings_config, website_url, category,
            created_at, sort_index, notes, icon, icon_color, meta, is_current
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            "p1",
            "claude",
            "Test Provider",
            serde_json::to_string(&json!({ "anthropicApiKey": "sk-test" })).unwrap(),
            Option::<String>::None,
            Option::<String>::None,
            Option::<i64>::None,
            Option::<usize>::None,
            Option::<String>::None,
            Option::<String>::None,
            Option::<String>::None,
            "{}",
            1,
        ],
    )
    .expect("seed provider");

    conn.execute(
        "INSERT INTO skills (key, installed, installed_at) VALUES (?1, ?2, ?3)",
        params!["claude:demo-skill", 1, 1700000000i64],
    )
    .expect("seed legacy skill");

    // 按应用启动流程：先 create_tables（补齐新增表），再 apply_schema_migrations（按 user_version 迁移）
    Database::create_tables_on_conn(&conn).expect("create tables");
    Database::apply_schema_migrations_on_conn(&conn).expect("apply migrations");

    assert_eq!(
        Database::get_user_version(&conn).expect("user_version after migration"),
        SCHEMA_VERSION
    );

    // v1 -> v2：providers 新增字段必须补齐
    for column in [
        "cost_multiplier",
        "limit_daily_usd",
        "limit_monthly_usd",
        "provider_type",
        "in_failover_queue",
    ] {
        assert!(
            Database::has_column(&conn, "providers", column).expect("check column"),
            "providers.{column} should exist after migration"
        );
    }

    // 旧 provider 不应丢失，且新增字段应有默认值
    let provider_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM providers WHERE id = 'p1' AND app_type = 'claude'",
            [],
            |r| r.get(0),
        )
        .expect("count providers");
    assert_eq!(provider_count, 1);

    let cost_multiplier: String = conn
        .query_row(
            "SELECT cost_multiplier FROM providers WHERE id = 'p1' AND app_type = 'claude'",
            [],
            |r| r.get(0),
        )
        .expect("read cost_multiplier");
    assert_eq!(cost_multiplier, "1.0");

    // v2 -> v3：skills 表重建为统一结构，并设置 pending 标记（后续由启动时扫描文件系统重建数据）
    assert!(
        Database::has_column(&conn, "skills", "enabled_claude").expect("check skills v3 column"),
        "skills table should be migrated to v3 structure"
    );
    let skills_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM skills", [], |r| r.get(0))
        .expect("count skills");
    assert_eq!(skills_count, 0, "skills table should be rebuilt empty");

    let pending: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'skills_ssot_migration_pending'",
            [],
            |r| r.get(0),
        )
        .ok();
    assert!(
        matches!(pending.as_deref(), Some("true") | Some("1")),
        "skills_ssot_migration_pending should be set after v2->v3 migration"
    );
    let snapshot: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'skills_ssot_migration_snapshot'",
            [],
            |r| r.get(0),
        )
        .ok();
    let snapshot = snapshot.expect("skills migration snapshot should be recorded");
    let snapshot_rows: serde_json::Value =
        serde_json::from_str(&snapshot).expect("parse skills migration snapshot");
    assert!(
        snapshot_rows
            .as_array()
            .is_some_and(|rows| rows.iter().any(|row| {
                row.get("directory").and_then(|v| v.as_str()) == Some("demo-skill")
                    && row.get("app_type").and_then(|v| v.as_str()) == Some("claude")
            })),
        "skills migration snapshot should preserve legacy app mapping"
    );

    // v3.9+ 新增：proxy_config 三行 seed 必须存在（否则 UI 会查不到默认值）
    let proxy_rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM proxy_config", [], |r| r.get(0))
        .expect("count proxy_config rows");
    assert_eq!(proxy_rows, 4);

    // model_pricing 应具备默认数据（迁移时会 seed）
    let pricing_rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM model_pricing", [], |r| r.get(0))
        .expect("count model_pricing rows");
    assert!(pricing_rows > 0, "model_pricing should be seeded");
}

/// DB-23: re-running the JSON migration over an existing provider updates it in
/// place instead of REPLACE-deleting it (which cascaded to provider_health and
/// provider_endpoints).
#[test]
fn json_migration_upserts_providers_without_cascading_children() {
    let db = Database::memory().expect("memory db");
    let mut manager = ProviderManager::default();
    manager.providers.insert(
        "p1".to_string(),
        crate::provider::Provider::with_id(
            "p1".to_string(),
            "Original".to_string(),
            json!({}),
            None,
        ),
    );
    let mut apps = HashMap::new();
    apps.insert("claude".to_string(), manager);
    let config = MultiAppConfig {
        version: 2,
        apps,
        mcp: Default::default(),
        prompts: Default::default(),
        skills: Default::default(),
        common_config_snippets: Default::default(),
        claude_common_config_snippet: None,
    };
    db.migrate_from_json(&config).expect("first migration");
    {
        let conn = db.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO provider_health (provider_id, app_type, is_healthy, updated_at)
             VALUES ('p1', 'claude', 0, '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
    }
    db.migrate_from_json(&config).expect("second migration");
    let conn = db.conn.lock().unwrap();
    let health: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM provider_health WHERE provider_id = 'p1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(health, 1, "provider_health survives the re-migration");
}

#[test]
fn schema_dry_run_does_not_write_to_disk() {
    // Create minimal valid config for migration
    let mut apps = HashMap::new();
    apps.insert("claude".to_string(), ProviderManager::default());

    let config = MultiAppConfig {
        version: 2,
        apps,
        mcp: Default::default(),
        prompts: Default::default(),
        skills: Default::default(),
        common_config_snippets: Default::default(),
        claude_common_config_snippet: None,
    };

    // Dry-run should succeed without any file I/O errors
    let result = Database::migrate_from_json_dry_run(&config);
    assert!(
        result.is_ok(),
        "Dry-run should succeed with valid config: {result:?}"
    );
}

#[test]
fn dry_run_validates_schema_compatibility() {
    // Create config with actual provider data
    let mut providers = IndexMap::new();
    providers.insert(
        "test-provider".to_string(),
        Provider {
            id: "test-provider".to_string(),
            name: "Test Provider".to_string(),
            settings_config: json!({
                "anthropicApiKey": "sk-test-123",
            }),
            website_url: None,
            category: None,
            created_at: Some(1234567890),
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        },
    );

    let manager = ProviderManager {
        providers,
        current: "test-provider".to_string(),
    };

    let mut apps = HashMap::new();
    apps.insert("claude".to_string(), manager);

    let config = MultiAppConfig {
        version: 2,
        apps,
        mcp: Default::default(),
        prompts: Default::default(),
        skills: Default::default(),
        common_config_snippets: Default::default(),
        claude_common_config_snippet: None,
    };

    // Dry-run should validate the full migration path
    let result = Database::migrate_from_json_dry_run(&config);
    assert!(
        result.is_ok(),
        "Dry-run should succeed with provider data: {result:?}"
    );
}

/// DB-19: a user-edited price survives a seed-version bump; built-in rows are
/// refreshed; an up-to-date database is not written by seeding.
#[test]
fn user_edited_pricing_survives_seed_bumps() {
    let db = Database::memory().expect("create memory db");
    db.upsert_model_pricing(&crate::services::usage_stats::ModelPricingInfo {
        model_id: "glm-5.1".to_string(),
        display_name: "My GLM".to_string(),
        input_cost_per_million: "7".to_string(),
        output_cost_per_million: "7".to_string(),
        cache_read_cost_per_million: "0".to_string(),
        cache_creation_cost_per_million: "0".to_string(),
        currency: "USD".to_string(),
        input_modalities: None,
    })
    .expect("user edit");
    {
        let conn = db.conn.lock().unwrap();
        // Simulate an older applied seed with a stale built-in row.
        conn.execute_batch(
            "UPDATE settings SET value = '0' WHERE key = 'model_pricing_seed_version';
             UPDATE model_pricing SET seed_version = 0, input_cost_per_million = '1.68'
             WHERE model_id = 'deepseek-v4-pro';",
        )
        .unwrap();
    }
    db.ensure_model_pricing_seeded().expect("seed");
    {
        let conn = db.conn.lock().unwrap();
        let glm: (String, String, String) = conn
            .query_row(
                "SELECT display_name, input_cost_per_million, source FROM model_pricing WHERE model_id = 'glm-5.1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(glm, ("My GLM".into(), "7".into(), "user".into()));
        let deepseek: String = conn
            .query_row(
                "SELECT input_cost_per_million FROM model_pricing WHERE model_id = 'deepseek-v4-pro'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(deepseek, "0.435");
    }

    // Up to date: seeding is a read-only no-op.
    let before: i64 = db
        .conn
        .lock()
        .unwrap()
        .query_row("SELECT total_changes()", [], |r| r.get(0))
        .unwrap();
    db.ensure_model_pricing_seeded().expect("seed again");
    let after: i64 = db
        .conn
        .lock()
        .unwrap()
        .query_row("SELECT total_changes()", [], |r| r.get(0))
        .unwrap();
    assert_eq!(before, after, "no writes when the seed is current");
}

#[test]
fn schema_model_pricing_is_seeded_on_init() {
    let db = Database::memory().expect("create memory db");

    let conn = db.conn.lock().expect("lock conn");

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM model_pricing", [], |row| row.get(0))
        .expect("count pricing");

    assert!(
        count > 0,
        "模型定价数据应该在初始化时自动填充，实际数量: {}",
        count
    );

    // 验证包含 Claude 模型
    let claude_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM model_pricing WHERE model_id LIKE 'claude-%'",
            [],
            |row| row.get(0),
        )
        .expect("check claude");
    assert!(
        claude_count > 0,
        "应该包含 Claude 模型定价，实际数量: {}",
        claude_count
    );

    // 验证包含 GPT 模型
    let gpt_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM model_pricing WHERE model_id LIKE 'gpt-%'",
            [],
            |row| row.get(0),
        )
        .expect("check gpt");
    assert!(
        gpt_count > 0,
        "应该包含 GPT 模型定价，实际数量: {}",
        gpt_count
    );

    // 验证包含 Gemini 模型
    let gemini_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM model_pricing WHERE model_id LIKE 'gemini-%'",
            [],
            |row| row.get(0),
        )
        .expect("check gemini");
    assert!(
        gemini_count > 0,
        "应该包含 Gemini 模型定价，实际数量: {}",
        gemini_count
    );
}

/// DB-19: upgrading a v19 database (no `source` column) refreshes rows that
/// still hold a previous built-in price and keeps user-edited prices.
#[test]
fn model_pricing_seed_repairs_known_outdated_builtin_prices() {
    let db = Database::memory().expect("create memory db");

    {
        let conn = db.conn.lock().expect("lock conn");
        conn.execute(
            "UPDATE model_pricing
             SET input_cost_per_million = '1.68',
                 output_cost_per_million = '3.36',
                 cache_read_cost_per_million = '0.14',
                 cache_creation_cost_per_million = '0'
             WHERE model_id = 'deepseek-v4-pro'",
            [],
        )
        .expect("restore old DeepSeek price");
        conn.execute(
            "UPDATE model_pricing
             SET input_cost_per_million = '9',
                 output_cost_per_million = '9',
                 cache_read_cost_per_million = '9',
                 cache_creation_cost_per_million = '0'
             WHERE model_id = 'glm-5.1'",
            [],
        )
        .expect("set custom GLM price");
        // Back to the v19 shape: no source/seed_version, no applied seed.
        conn.execute_batch(
            "ALTER TABLE model_pricing DROP COLUMN source;
             ALTER TABLE model_pricing DROP COLUMN seed_version;
             DELETE FROM settings WHERE key = 'model_pricing_seed_version';
             PRAGMA user_version = 19;",
        )
        .expect("downgrade to v19");
    }

    db.apply_schema_migrations().expect("migrate to current");
    db.ensure_model_pricing_seeded()
        .expect("ensure pricing seeded");

    let conn = db.conn.lock().expect("lock conn");
    let deepseek: (String, String, String) = conn
        .query_row(
            "SELECT input_cost_per_million, output_cost_per_million, cache_read_cost_per_million
             FROM model_pricing WHERE model_id = 'deepseek-v4-pro'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("query DeepSeek price");
    assert_eq!(
        deepseek,
        (
            "0.435".to_string(),
            "0.87".to_string(),
            "0.003625".to_string()
        )
    );

    let glm: (String, String, String) = conn
        .query_row(
            "SELECT input_cost_per_million, output_cost_per_million, cache_read_cost_per_million
             FROM model_pricing WHERE model_id = 'glm-5.1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("query GLM price");
    assert_eq!(glm, ("9".to_string(), "9".to_string(), "9".to_string()));
}

#[test]
fn ensure_incremental_auto_vacuum_rebuilds_existing_file_db() {
    let temp = NamedTempFile::new().expect("create temp db file");
    let path = temp.path().to_path_buf();

    let conn = Connection::open(&path).expect("open temp db");
    conn.execute("PRAGMA auto_vacuum = NONE;", [])
        .expect("set none auto_vacuum");
    Database::create_tables_on_conn(&conn).expect("create tables");

    assert_eq!(
        Database::get_auto_vacuum_mode(&conn).expect("auto_vacuum before rebuild"),
        0,
        "existing file db should start with NONE auto_vacuum"
    );

    let rebuilt =
        Database::ensure_incremental_auto_vacuum_on_conn(&conn).expect("enable incremental mode");
    assert!(rebuilt, "existing db should require rebuild via VACUUM");
    drop(conn);

    let reopened = Connection::open(&path).expect("reopen temp db");
    assert_eq!(
        Database::get_auto_vacuum_mode(&reopened).expect("auto_vacuum after rebuild"),
        2,
        "file db should persist INCREMENTAL auto_vacuum after VACUUM rebuild"
    );
}

// ─── Backup restore / rotation (TEST-34, TEST-35, DB-10) ──────────────────

mod backup_restore {
    use super::super::backup::BackupKind;
    use super::*;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime};

    /// A private config dir with an in-memory live database. Uses the explicit
    /// `*_in` backup APIs so tests never touch process-global env or settings.
    struct TestHome {
        dir: tempfile::TempDir,
        retain: usize,
        db: Database,
    }

    impl TestHome {
        fn new(retain: usize) -> Self {
            let dir = tempfile::tempdir().expect("temp config dir");
            // backup_database_file_in only runs when the live DB file exists.
            std::fs::write(dir.path().join("cc-switch.db"), b"").expect("touch db");
            Self {
                dir,
                retain,
                db: Database::memory().expect("memory db"),
            }
        }

        fn backups_dir(&self) -> PathBuf {
            self.dir.path().join("backups")
        }

        fn backup(&self, kind: BackupKind) -> PathBuf {
            self.db
                .backup_database_file_in(self.dir.path(), self.retain, kind, None)
                .expect("backup")
                .expect("backup path")
        }

        fn restore(&self, filename: &str) -> Result<String, AppError> {
            self.db
                .restore_from_backup_in(self.dir.path(), self.retain, filename)
        }
    }

    fn insert_provider(db: &Database, id: &str) {
        let conn = db.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
             VALUES (?1, 'claude', ?1, '{}', '{}')",
            [id],
        )
        .expect("insert provider");
    }

    fn provider_ids(db: &Database) -> Vec<String> {
        let conn = db.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id FROM providers ORDER BY id")
            .expect("prepare");
        let ids = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .expect("query")
            .collect::<Result<Vec<_>, _>>()
            .expect("rows");
        ids
    }

    fn set_mtime(path: &Path, secs_ago: u64) {
        let file = std::fs::File::options()
            .write(true)
            .open(path)
            .expect("open backup");
        file.set_modified(SystemTime::now() - Duration::from_secs(secs_ago))
            .expect("set mtime");
    }

    fn file_name(path: &Path) -> String {
        path.file_name().unwrap().to_string_lossy().into_owned()
    }

    fn backup_names(home: &TestHome) -> Vec<String> {
        let mut names = std::fs::read_dir(home.backups_dir())
            .expect("read backups")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".db"))
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    #[test]
    fn restoring_oldest_backup_at_retention_limit_restores_its_data() {
        let home = TestHome::new(2);
        let db = &home.db;

        insert_provider(db, "p-old");
        let oldest = home.backup(BackupKind::Safety);
        set_mtime(&oldest, 3600);

        insert_provider(db, "p-new");
        let newer = home.backup(BackupKind::Safety);
        set_mtime(&newer, 1800);
        insert_provider(db, "p-live");

        // The safety backup taken by the restore pushes the safety pool over the
        // retention limit; the oldest file is the one being restored.
        home.restore(&file_name(&oldest)).expect("restore oldest");

        assert_eq!(provider_ids(db), vec!["p-old".to_string()]);
        assert!(
            oldest.exists(),
            "the restored backup must not be rotated away"
        );
        assert_eq!(
            backup_names(&home).len(),
            2,
            "rotation still enforces the retention limit"
        );
    }

    #[test]
    fn restoring_a_missing_backup_errors_and_creates_nothing() {
        let home = TestHome::new(10);
        let db = &home.db;
        insert_provider(db, "p-live");
        std::fs::create_dir_all(home.backups_dir()).unwrap();

        let err = home
            .restore("db_backup_20200101_000000.db")
            .expect_err("missing backup must fail");
        assert!(err.to_string().contains("not found"), "{err}");
        assert!(!home
            .backups_dir()
            .join("db_backup_20200101_000000.db")
            .exists());
        assert_eq!(provider_ids(db), vec!["p-live".to_string()]);
    }

    #[test]
    fn restoring_a_future_version_backup_leaves_live_db_unchanged() {
        let home = TestHome::new(10);
        let db = &home.db;
        insert_provider(db, "p-backup");
        let backup = home.backup(BackupKind::Manual);
        {
            let conn = Connection::open(&backup).unwrap();
            Database::set_user_version(&conn, SCHEMA_VERSION + 1).unwrap();
        }
        insert_provider(db, "p-live");
        let before = backup_names(&home);
        let version_before = Database::get_user_version(&db.conn.lock().unwrap()).unwrap();

        let err = home
            .restore(&file_name(&backup))
            .expect_err("future version must be rejected");
        assert!(err.to_string().contains("newer"), "{err}");

        assert_eq!(
            provider_ids(db),
            vec!["p-backup".to_string(), "p-live".to_string()]
        );
        let conn = db.conn.lock().unwrap();
        assert_eq!(Database::get_user_version(&conn).unwrap(), version_before);
        drop(conn);
        assert_eq!(backup_names(&home), before, "no safety backup on rejection");
    }

    #[test]
    fn restoring_a_garbage_file_leaves_live_db_unchanged() {
        let home = TestHome::new(10);
        let db = &home.db;
        insert_provider(db, "p-live");
        std::fs::create_dir_all(home.backups_dir()).unwrap();
        std::fs::write(
            home.backups_dir().join("garbage.db"),
            b"this is definitely not an sqlite database file, just some bytes",
        )
        .unwrap();

        home.restore("garbage.db")
            .expect_err("garbage must be rejected");
        assert_eq!(provider_ids(db), vec!["p-live".to_string()]);
    }

    #[test]
    fn restoring_a_non_cc_switch_database_is_rejected() {
        let home = TestHome::new(10);
        let db = &home.db;
        insert_provider(db, "p-live");
        std::fs::create_dir_all(home.backups_dir()).unwrap();
        {
            let conn = Connection::open(home.backups_dir().join("other.db")).unwrap();
            conn.execute("CREATE TABLE notes (x TEXT)", []).unwrap();
        }

        home.restore("other.db")
            .expect_err("foreign database must be rejected");
        assert_eq!(provider_ids(db), vec!["p-live".to_string()]);
    }

    #[test]
    fn restore_keeps_device_takeover_state_and_live_backups() {
        let home = TestHome::new(10);
        let db = &home.db;
        insert_provider(db, "p-backup");
        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "UPDATE proxy_config SET enabled = 1, listen_port = 16000, max_retries = 7
                 WHERE app_type = 'claude'",
                [],
            )
            .unwrap();
        }
        let backup = home.backup(BackupKind::Manual);

        // Live state on this device after the backup was taken.
        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "UPDATE proxy_config SET enabled = 0, listen_port = 15721, max_retries = 3
                 WHERE app_type = 'claude'",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO proxy_live_backup (app_type, original_config, backed_up_at)
                 VALUES ('codex', 'live-original', '2026-01-01')",
                [],
            )
            .unwrap();
        }

        home.restore(&file_name(&backup)).expect("restore");

        let conn = db.conn.lock().unwrap();
        let (enabled, port, retries): (i64, i64, i64) = conn
            .query_row(
                "SELECT enabled, listen_port, max_retries FROM proxy_config WHERE app_type = 'claude'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(enabled, 0, "takeover flag comes from this device");
        assert_eq!(port, 15721, "listen port comes from this device");
        assert_eq!(retries, 7, "shared proxy preferences come from the backup");
        let live_backup: String = conn
            .query_row(
                "SELECT original_config FROM proxy_live_backup WHERE app_type = 'codex'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(live_backup, "live-original");
    }

    fn insert_usage_rows(db: &Database, request_id: &str) {
        let conn = db.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO proxy_request_logs (request_id, provider_id, app_type, model,
                input_tokens, output_tokens, total_cost_usd, latency_ms, status_code, created_at)
             VALUES (?1, 'p-backup', 'claude', 'm', 1, 1, '0.01', 1, 200, 1000)",
            [request_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO usage_daily_rollups (date, app_type, provider_id, model, request_count,
                success_count, input_tokens, output_tokens, cache_read_tokens,
                cache_creation_tokens, total_cost_usd, avg_latency_ms)
             VALUES ('2026-03-01', 'claude', 'p-backup', 'm', 1, 1, 1, 1, 0, 0, '0.01', 1)",
            [],
        )
        .unwrap();
    }

    fn count(db: &Database, sql: &str) -> i64 {
        let conn = db.conn.lock().unwrap();
        conn.query_row(sql, [], |row| row.get(0)).unwrap()
    }

    #[test]
    fn restore_keeps_usage_recorded_after_the_backup_by_default() {
        let home = TestHome::new(10);
        let db = &home.db;
        insert_provider(db, "p-backup");
        let backup = home.backup(BackupKind::Manual);
        insert_usage_rows(db, "req-after-backup");

        home.restore(&file_name(&backup)).expect("restore");
        assert_eq!(
            count(
                db,
                "SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = 'req-after-backup'"
            ),
            1,
            "logs recorded after the backup survive the restore (DB-42)"
        );
        assert_eq!(count(db, "SELECT COUNT(*) FROM usage_daily_rollups"), 1);
    }

    #[test]
    fn restore_without_usage_preservation_takes_the_backups_usage() {
        let home = TestHome::new(10);
        let db = &home.db;
        insert_provider(db, "p-backup");
        let backup = home.backup(BackupKind::Manual);
        insert_usage_rows(db, "req-after-backup");

        db.restore_from_backup_in_with(home.dir.path(), home.retain, &file_name(&backup), false)
            .expect("restore");
        assert_eq!(count(db, "SELECT COUNT(*) FROM proxy_request_logs"), 0);
        assert_eq!(count(db, "SELECT COUNT(*) FROM usage_daily_rollups"), 0);
    }

    #[test]
    fn backup_files_are_written_in_rollback_journal_mode() {
        let dir = tempfile::tempdir().expect("tempdir");
        let live = Database::open_file(&dir.path().join("cc-switch.db")).expect("open file db");
        insert_provider(&live, "p1");
        let path = live
            .backup_database_file_in(dir.path(), 5, BackupKind::Manual, None)
            .expect("backup")
            .expect("backup path");
        let conn = rusqlite::Connection::open(&path).expect("open backup");
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode.to_ascii_lowercase(), "delete");
    }

    #[test]
    fn rotation_never_deletes_manual_named_or_other_kinds() {
        let home = TestHome::new(3);
        let db = &home.db;
        insert_provider(db, "p1");

        let manual = home.backup(BackupKind::Manual);
        let pre = home.backup(BackupKind::PreMigration);
        let safety = home.backup(BackupKind::Safety);
        let to_rename = home.backup(BackupKind::Auto);
        let renamed = "before-big-change.db".to_string();
        std::fs::rename(&to_rename, home.backups_dir().join(&renamed)).expect("rename");
        for path in [&manual, &pre, &safety] {
            set_mtime(path, 7200);
        }
        set_mtime(&home.backups_dir().join(&renamed), 7200);

        for _ in 0..20 {
            home.backup(BackupKind::Auto);
        }

        let names = backup_names(&home);
        for kept in [
            file_name(&manual),
            file_name(&pre),
            file_name(&safety),
            renamed.clone(),
        ] {
            assert!(
                names.contains(&kept),
                "{kept} must survive rotation: {names:?}"
            );
        }
        let auto_count = names
            .iter()
            .filter(|n| BackupKind::from_filename(n) == Some(BackupKind::Auto))
            .count();
        assert_eq!(
            auto_count, 3,
            "auto backups are rotated to the retain count"
        );
    }

    #[test]
    fn sql_import_accepts_export_from_v3_8_schema() {
        // SEC-30: the import authorizer must still accept exports made by
        // older app versions (legacy schema, legacy tables).
        let legacy = Connection::open_in_memory().expect("open memory db");
        legacy
            .execute_batch(V3_8_SCHEMA_V1_SQL)
            .expect("seed v3.8 schema v1");
        Database::set_user_version(&legacy, 1).expect("set user_version=1");
        legacy
            .execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta, is_current)
                 VALUES ('legacy-p', 'claude', 'Legacy', '{}', '{}', 1)",
                [],
            )
            .expect("seed provider");
        let sql = Database::dump_sql(&legacy, &[]).expect("dump legacy");

        let target = Database::memory().expect("memory db");
        target
            .import_sql_string(&sql)
            .expect("import legacy export");
        assert_eq!(provider_ids(&target), vec!["legacy-p".to_string()]);
    }

    #[test]
    fn backup_kind_is_parsed_from_filename() {
        assert_eq!(
            BackupKind::from_filename("db_backup_20260101_120000.db"),
            Some(BackupKind::Auto)
        );
        assert_eq!(
            BackupKind::from_filename("db_backup_20260101_120000_3.db"),
            Some(BackupKind::Auto)
        );
        assert_eq!(
            BackupKind::from_filename("db_manual_20260101_120000.db"),
            Some(BackupKind::Manual)
        );
        assert_eq!(
            BackupKind::from_filename("db_safety_20260101_120000.db"),
            Some(BackupKind::Safety)
        );
        assert_eq!(
            BackupKind::from_filename("db_premigration_20260101_120000_1.db"),
            Some(BackupKind::PreMigration)
        );
        assert_eq!(BackupKind::from_filename("db_backup_mine.db"), None);
        assert_eq!(
            BackupKind::from_filename("db_backup_20260101_120000_.db"),
            None
        );
        assert_eq!(BackupKind::from_filename("before-big-change.db"), None);
    }
}

// ─── Fresh schema vs. migrated historical schemas (DB-07 / DB-08) ───────────

use std::collections::BTreeMap;

fn normalize_sql(sql: &str) -> String {
    let collapsed = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed
        .replace("( ", "(")
        .replace(" )", ")")
        .replace(" ,", ",")
}

fn query_strings(conn: &Connection, sql: &str) -> Vec<Vec<String>> {
    let mut stmt = conn.prepare(sql).expect("prepare schema query");
    let width = stmt.column_count();
    stmt.query_map([], |row| {
        (0..width)
            .map(|i| {
                let value: rusqlite::types::Value = row.get(i)?;
                Ok(match value {
                    rusqlite::types::Value::Null => "NULL".to_string(),
                    rusqlite::types::Value::Integer(v) => v.to_string(),
                    rusqlite::types::Value::Real(v) => v.to_string(),
                    rusqlite::types::Value::Text(v) => v,
                    rusqlite::types::Value::Blob(_) => "<blob>".to_string(),
                })
            })
            .collect::<Result<Vec<_>, _>>()
    })
    .expect("run schema query")
    .collect::<Result<Vec<_>, _>>()
    .expect("read schema rows")
}

/// Structural snapshot of a database schema: every table's columns (name, declared type,
/// NOT NULL, default, primary-key position) and foreign keys, every index (explicit ones
/// by normalized SQL, automatic ones by table/columns/uniqueness) and every trigger.
/// Column order is ignored: `ALTER TABLE ADD COLUMN` always appends, so a migrated table
/// legitimately lists its columns in a different order than a freshly created one.
fn schema_snapshot(conn: &Connection) -> BTreeMap<String, String> {
    let mut snapshot = BTreeMap::new();
    let objects = query_strings(
        conn,
        "SELECT type, name, tbl_name, COALESCE(sql, '') FROM sqlite_master
         WHERE name NOT LIKE 'sqlite\\_%' ESCAPE '\\' ORDER BY type, name",
    );
    for object in objects {
        let (kind, name, table, sql) = (&object[0], &object[1], &object[2], &object[3]);
        match kind.as_str() {
            "table" => {
                let mut columns = query_strings(
                    conn,
                    &format!(
                        "SELECT name, upper(type), \"notnull\", dflt_value, pk
                         FROM pragma_table_info('{name}')"
                    ),
                )
                .into_iter()
                .map(|c| c.join("|"))
                .collect::<Vec<_>>();
                columns.sort();
                let mut fks = query_strings(
                    conn,
                    &format!(
                        "SELECT \"table\", \"from\", \"to\", on_update, on_delete
                         FROM pragma_foreign_key_list('{name}')"
                    ),
                )
                .into_iter()
                .map(|c| c.join("|"))
                .collect::<Vec<_>>();
                fks.sort();
                snapshot.insert(
                    format!("table {name}"),
                    format!("columns: {columns:?}; foreign keys: {fks:?}"),
                );
                for index in query_strings(
                    conn,
                    &format!(
                        "SELECT name, \"unique\", origin FROM pragma_index_list('{name}')
                         WHERE origin != 'c'"
                    ),
                ) {
                    let cols = query_strings(
                        conn,
                        &format!("SELECT name FROM pragma_index_info('{}')", index[0]),
                    )
                    .into_iter()
                    .map(|c| c.join(""))
                    .collect::<Vec<_>>();
                    snapshot.insert(
                        format!("auto-index {name}({})", cols.join(",")),
                        format!("unique={} origin={}", index[1], index[2]),
                    );
                }
            }
            "index" if !sql.is_empty() => {
                snapshot.insert(format!("index {name} on {table}"), normalize_sql(sql));
            }
            "index" => {}
            other => {
                snapshot.insert(format!("{other} {name} on {table}"), normalize_sql(sql));
            }
        }
    }
    snapshot
}

fn assert_same_schema(
    expected: &BTreeMap<String, String>,
    actual: &BTreeMap<String, String>,
    label: &str,
) {
    let mut diffs = Vec::new();
    for (key, value) in expected {
        match actual.get(key) {
            None => diffs.push(format!("missing {key}: {value}")),
            Some(other) if other != value => diffs.push(format!(
                "differs {key}:\n  fresh:    {value}\n  migrated: {other}"
            )),
            _ => {}
        }
    }
    for (key, value) in actual {
        if !expected.contains_key(key) {
            diffs.push(format!("unexpected {key}: {value}"));
        }
    }
    assert!(
        diffs.is_empty(),
        "{label}: migrated schema differs from fresh schema:\n{}",
        diffs.join("\n")
    );
}

fn fresh_schema() -> BTreeMap<String, String> {
    let conn = Connection::open_in_memory().expect("open memory db");
    Database::create_tables_on_conn(&conn).expect("create fresh schema");
    schema_snapshot(&conn)
}

/// Startup order: create_tables (a no-op for existing databases apart from the app's
/// triggers), then the migration chain.
fn upgrade_like_startup(conn: &Connection) {
    Database::create_tables_on_conn(conn).expect("create tables");
    Database::apply_schema_migrations_on_conn(conn).expect("apply migrations");
    assert_eq!(
        Database::get_user_version(conn).expect("user_version"),
        SCHEMA_VERSION
    );
}

fn ledger_sources(conn: &Connection) -> Vec<(i64, String)> {
    query_strings(
        conn,
        "SELECT version, source FROM schema_migrations ORDER BY version",
    )
    .into_iter()
    .map(|r| (r[0].parse().expect("version"), r[1].clone()))
    .collect()
}

fn assert_ledger(conn: &Connection, migrated_from: i64) {
    let expected: Vec<(i64, String)> = (1..=SCHEMA_VERSION as i64)
        .map(|v| {
            let source = if v <= migrated_from {
                "backfilled"
            } else {
                "migrated"
            };
            (v, source.to_string())
        })
        .collect();
    assert_eq!(ledger_sources(conn), expected);
    for (version, name, checksum) in Database::read_migration_ledger(conn).expect("ledger") {
        let registered = schema::MIGRATIONS
            .iter()
            .find(|m| m.version == version)
            .expect("registered");
        assert_eq!(
            (name.as_str(), checksum.as_str()),
            (registered.name, registered.checksum)
        );
    }
}

fn drop_columns(conn: &Connection, table: &str, columns: &[&str]) {
    for column in columns {
        conn.execute(&format!("ALTER TABLE {table} DROP COLUMN {column}"), [])
            .unwrap_or_else(|e| panic!("drop {table}.{column}: {e}"));
    }
}

/// Undo the schema effect of migration `version` on a database that is at `version`,
/// reproducing the shape a database had when it was last opened by an app at
/// `version - 1` (tables that were only ever created by the old every-start
/// `create_tables` stay in place, as they did in real installations).
fn undo_migration(conn: &Connection, version: i32) {
    match version {
        27 => drop_columns(conn, "skills", &["enabled_openclaw"]),
        26 => {
            conn.execute_batch(
                "DROP INDEX idx_request_logs_zero_cost_nanos;
                 DROP TRIGGER trg_proxy_request_logs_cost_nanos_insert;
                 DROP TRIGGER trg_proxy_request_logs_cost_nanos_update;
                 DROP TRIGGER trg_usage_daily_rollups_cost_nanos_insert;
                 DROP TRIGGER trg_usage_daily_rollups_cost_nanos_update;
                 ALTER TABLE proxy_request_logs DROP COLUMN total_cost_nanos;
                 ALTER TABLE usage_daily_rollups DROP COLUMN total_cost_nanos;
                 CREATE INDEX idx_request_logs_zero_cost ON proxy_request_logs(created_at)
                     WHERE CAST(total_cost_usd AS REAL) <= 0;",
            )
            .expect("undo v26");
        }
        25 => drop_columns(conn, "model_pricing", &["currency"]),
        24 => drop_columns(conn, "model_pricing", &["input_modalities"]),
        23 => conn
            .execute_batch(
                "CREATE INDEX idx_request_logs_status ON proxy_request_logs(status_code);",
            )
            .expect("undo v23"),
        22 => {
            for table in ["providers", "mcp_servers", "prompts", "skills", "profiles"] {
                for kind in ["insert", "update", "delete"] {
                    conn.execute(&format!("DROP TRIGGER trg_sync_{table}_{kind}"), [])
                        .expect("undo v22 trigger");
                }
                drop_columns(conn, table, &["sync_rev", "sync_updated_at"]);
            }
            conn.execute_batch(
                "DROP TABLE sync_tombstones;
                 DELETE FROM schema_migrations WHERE version = 22;",
            )
            .expect("undo v22");
        }
        21 => {
            let sql: String = conn
                .query_row(
                    "SELECT sql FROM sqlite_master WHERE name = 'proxy_config'",
                    [],
                    |row| row.get(0),
                )
                .expect("proxy_config sql");
            let old = sql.replacen("proxy_config", "proxy_config_v20", 1).replace(
                "app_type TEXT PRIMARY KEY NOT NULL,",
                "app_type TEXT PRIMARY KEY CHECK (app_type IN ('claude','codex','gemini','grokbuild')),",
            );
            assert_ne!(old, sql);
            conn.execute_batch(&format!(
                "{old};
                 INSERT INTO proxy_config_v20 SELECT * FROM proxy_config;
                 DROP TABLE proxy_config;
                 ALTER TABLE proxy_config_v20 RENAME TO proxy_config;
                 DELETE FROM schema_migrations WHERE version = 21;"
            ))
            .expect("undo v21");
        }
        20 => {
            drop_columns(conn, "model_pricing", &["source", "seed_version"]);
            conn.execute("DELETE FROM schema_migrations WHERE version = 20", [])
                .expect("undo v20 ledger row");
        }
        19 => conn
            .execute_batch(
                "DROP INDEX idx_provider_endpoints_unique;
                 DELETE FROM schema_migrations WHERE version = 19;",
            )
            .expect("undo v19"),
        18 => {
            drop_columns(conn, "session_log_sync", &["byte_offset", "prefix_hash"]);
            conn.execute("DELETE FROM schema_migrations WHERE version = 18", [])
                .expect("undo v18 ledger row");
        }
        17 => conn
            .execute_batch("DROP TABLE schema_migrations;")
            .expect("undo v17"),
        16 => {
            conn.execute_batch(
                "DROP TRIGGER trg_request_logs_dedup_session_insert;
                 DROP TRIGGER trg_request_logs_dedup_proxy_insert;
                 DROP INDEX idx_request_logs_provider_created_at;
                 DROP INDEX idx_request_logs_zero_cost;",
            )
            .expect("undo v16 objects");
            drop_columns(conn, "proxy_request_logs", &["dedup_hidden"]);
            conn.execute_batch(
                "CREATE INDEX idx_request_logs_provider ON proxy_request_logs(provider_id, app_type);
                 CREATE INDEX idx_request_logs_model ON proxy_request_logs(model);",
            )
            .expect("undo v16 indexes");
        }
        15 => {
            drop_columns(conn, "mcp_servers", &["enabled_grokbuild"]);
            drop_columns(conn, "skills", &["enabled_grokbuild"]);
        }
        14 => {
            let sql: String = conn
                .query_row(
                    "SELECT sql FROM sqlite_master WHERE name = 'proxy_config'",
                    [],
                    |row| row.get(0),
                )
                .expect("proxy_config sql");
            let old = sql
                .replacen("proxy_config", "proxy_config_v13", 1)
                .replace(",'grokbuild'", "");
            assert_ne!(old, sql);
            conn.execute_batch(&format!(
                "{old};
                 INSERT INTO proxy_config_v13 SELECT * FROM proxy_config WHERE app_type != 'grokbuild';
                 DROP TABLE proxy_config;
                 ALTER TABLE proxy_config_v13 RENAME TO proxy_config;"
            ))
            .expect("undo v14");
        }
        13 => {
            drop_columns(conn, "proxy_request_logs", &["input_token_semantics"]);
            drop_columns(conn, "usage_daily_rollups", &["input_token_semantics"]);
        }
        12 => conn
            .execute_batch("DROP TABLE profiles;")
            .expect("undo v12"),
        11 => {
            drop_columns(conn, "proxy_request_logs", &["pricing_model"]);
            conn.execute_batch(
                "CREATE TABLE usage_daily_rollups_v10 (
                    date TEXT NOT NULL,
                    app_type TEXT NOT NULL,
                    provider_id TEXT NOT NULL,
                    model TEXT NOT NULL,
                    request_count INTEGER NOT NULL DEFAULT 0,
                    success_count INTEGER NOT NULL DEFAULT 0,
                    input_tokens INTEGER NOT NULL DEFAULT 0,
                    output_tokens INTEGER NOT NULL DEFAULT 0,
                    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
                    cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
                    total_cost_usd TEXT NOT NULL DEFAULT '0',
                    avg_latency_ms INTEGER NOT NULL DEFAULT 0,
                    PRIMARY KEY (date, app_type, provider_id, model)
                );
                INSERT INTO usage_daily_rollups_v10
                    SELECT date, app_type, provider_id, model, request_count, success_count,
                           input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                           total_cost_usd, avg_latency_ms
                    FROM usage_daily_rollups;
                DROP TABLE usage_daily_rollups;
                ALTER TABLE usage_daily_rollups_v10 RENAME TO usage_daily_rollups;",
            )
            .expect("undo v11");
        }
        10 => {
            drop_columns(conn, "mcp_servers", &["enabled_hermes"]);
            drop_columns(conn, "skills", &["enabled_hermes"]);
        }
        9 => {}
        8 => {
            conn.execute_batch(
                "DROP INDEX idx_request_logs_dedup_lookup_expr;
                 DROP INDEX idx_request_logs_app_created_at;
                 DROP TABLE session_log_sync;",
            )
            .expect("undo v8");
            drop_columns(conn, "proxy_request_logs", &["data_source"]);
        }
        7 => drop_columns(conn, "skills", &["content_hash", "updated_at"]),
        6 => conn
            .execute_batch("DROP TABLE usage_daily_rollups;")
            .expect("undo v6"),
        5 => {
            drop_columns(
                conn,
                "proxy_config",
                &["default_cost_multiplier", "pricing_model_source"],
            );
            drop_columns(conn, "proxy_request_logs", &["request_model"]);
        }
        4 => {
            drop_columns(conn, "mcp_servers", &["enabled_opencode"]);
            drop_columns(conn, "skills", &["enabled_opencode"]);
        }
        3 => conn
            .execute_batch(
                "DROP TABLE skills;
                 CREATE TABLE skills (
                     directory TEXT NOT NULL,
                     app_type TEXT NOT NULL,
                     installed BOOLEAN NOT NULL DEFAULT 0,
                     installed_at INTEGER NOT NULL DEFAULT 0,
                     PRIMARY KEY (directory, app_type)
                 );
                 INSERT INTO skills (directory, app_type, installed, installed_at)
                 VALUES ('demo-skill', 'claude', 1, 1700000000);",
            )
            .expect("undo v3"),
        other => panic!("no synthetic shape for undoing v{other}"),
    }
}

/// A database shaped like one last opened by an app at schema `version` (2..=16), with
/// a little data in it.
fn database_at_version(version: i32) -> Connection {
    let conn = Connection::open_in_memory().expect("open memory db");
    Database::create_tables_on_conn(&conn).expect("create fresh schema");
    conn.execute_batch(
        "INSERT INTO providers (id, app_type, name, settings_config, meta, is_current)
             VALUES ('p1', 'claude', 'Provider', '{}', '{}', 1);
         INSERT INTO mcp_servers (id, name, server_config, enabled_claude)
             VALUES ('m1', 'MCP', '{}', 1);
         INSERT INTO proxy_request_logs
             (request_id, provider_id, app_type, model, input_tokens, output_tokens,
              latency_ms, status_code, created_at)
             VALUES ('r1', 'p1', 'claude', 'claude-sonnet-4', 10, 20, 100, 200, 1700000000);
         UPDATE proxy_config SET max_retries = 9 WHERE app_type = 'codex';",
    )
    .expect("seed data");
    for undo in ((version + 1)..=SCHEMA_VERSION).rev() {
        undo_migration(&conn, undo);
    }
    Database::set_user_version(&conn, version).expect("set user_version");
    conn
}

#[test]
fn fresh_database_is_created_at_current_version_with_baseline_ledger() {
    let conn = Connection::open_in_memory().expect("open memory db");
    Database::create_tables_on_conn(&conn).expect("create fresh schema");
    assert_eq!(
        Database::get_user_version(&conn).expect("user_version"),
        SCHEMA_VERSION
    );
    let ledger = ledger_sources(&conn);
    assert_eq!(ledger.len(), SCHEMA_VERSION as usize);
    // Versions up to the baseline are recorded as such; later migrations run
    // on top of the baseline DDL.
    assert!(ledger.iter().all(|(version, source)| {
        if *version <= i64::from(schema::BASELINE_SCHEMA_VERSION) {
            source == "baseline"
        } else {
            source == "migrated"
        }
    }));
    let proxy_rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM proxy_config", [], |row| row.get(0))
        .expect("count proxy_config");
    assert_eq!(proxy_rows, 4);

    // Re-running both entry points on the now-existing database changes nothing.
    let before = schema_snapshot(&conn);
    upgrade_like_startup(&conn);
    assert_eq!(schema_snapshot(&conn), before);
    assert_eq!(ledger_sources(&conn), ledger);
}

#[test]
fn memory_database_is_fully_migrated() {
    let db = Database::memory().expect("memory db");
    let conn = db.conn.lock().expect("lock");
    assert_eq!(
        Database::get_user_version(&conn).expect("user_version"),
        SCHEMA_VERSION
    );
    assert_same_schema(
        &fresh_schema(),
        &schema_snapshot(&conn),
        "Database::memory()",
    );
}

#[test]
fn empty_database_migrated_through_full_chain_matches_fresh_schema() {
    let conn = Connection::open_in_memory().expect("open memory db");
    // Bypass the fresh path: run the legacy preflight and v1..vN on an empty database.
    Database::apply_schema_migrations_on_conn(&conn).expect("apply migrations");
    assert_same_schema(&fresh_schema(), &schema_snapshot(&conn), "empty v0");
    assert_ledger(&conn, 0);
}

#[test]
fn v3_8_database_upgrade_matches_fresh_schema() {
    let conn = Connection::open_in_memory().expect("open memory db");
    conn.execute_batch(V3_8_SCHEMA_V1_SQL)
        .expect("seed v3.8 schema");
    conn.execute(
        "INSERT INTO skills (key, installed, installed_at) VALUES ('claude:demo', 1, 1)",
        [],
    )
    .expect("seed legacy skill");
    Database::set_user_version(&conn, 1).expect("set user_version");
    upgrade_like_startup(&conn);
    assert_same_schema(&fresh_schema(), &schema_snapshot(&conn), "v3.8 (v1)");
    assert_ledger(&conn, 1);
}

#[test]
fn every_historical_version_upgrades_to_the_fresh_schema() {
    let fresh = fresh_schema();
    for version in 2..SCHEMA_VERSION {
        let conn = database_at_version(version);
        upgrade_like_startup(&conn);
        assert_same_schema(&fresh, &schema_snapshot(&conn), &format!("v{version}"));
        if version >= schema::BASELINE_SCHEMA_VERSION {
            // Created after the ledger existed: earlier rows keep their source.
            let ledger = ledger_sources(&conn);
            assert_eq!(ledger.len(), SCHEMA_VERSION as usize);
            assert!(ledger
                .iter()
                .filter(|(v, _)| *v > version as i64)
                .all(|(_, source)| source == "migrated"));
        } else {
            assert_ledger(&conn, version as i64);
        }

        // Data survives the upgrade.
        let providers: i64 = conn
            .query_row("SELECT COUNT(*) FROM providers", [], |row| row.get(0))
            .expect("count providers");
        assert_eq!(providers, 1, "v{version}: provider kept");
        let logs: i64 = conn
            .query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| {
                row.get(0)
            })
            .expect("count logs");
        assert_eq!(logs, 1, "v{version}: request log kept");
        let codex_retries: i64 = conn
            .query_row(
                "SELECT max_retries FROM proxy_config WHERE app_type = 'codex'",
                [],
                |row| row.get(0),
            )
            .expect("codex proxy config");
        assert_eq!(codex_retries, 9, "v{version}: proxy_config values kept");
        let proxy_rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM proxy_config", [], |row| row.get(0))
            .expect("count proxy_config");
        assert_eq!(proxy_rows, 4, "v{version}: one proxy_config row per app");

        // A second start is a no-op.
        upgrade_like_startup(&conn);
        assert_same_schema(
            &fresh,
            &schema_snapshot(&conn),
            &format!("v{version} (rerun)"),
        );
    }
}

#[test]
fn beta_v2_singleton_proxy_config_upgrade_matches_fresh_schema() {
    let conn = Connection::open_in_memory().expect("open memory db");
    conn.execute_batch(
        r#"
        CREATE TABLE proxy_config (
            id INTEGER PRIMARY KEY,
            enabled INTEGER NOT NULL DEFAULT 0,
            listen_address TEXT NOT NULL DEFAULT '127.0.0.1',
            listen_port INTEGER NOT NULL DEFAULT 5000,
            max_retries INTEGER NOT NULL DEFAULT 3,
            request_timeout INTEGER NOT NULL DEFAULT 300,
            enable_logging INTEGER NOT NULL DEFAULT 1,
            target_app TEXT NOT NULL DEFAULT 'claude',
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        INSERT INTO proxy_config (id, enabled) VALUES (1, 1);
        "#,
    )
    .expect("seed legacy proxy_config");
    Database::set_user_version(&conn, 2).expect("set user_version");
    upgrade_like_startup(&conn);
    assert_same_schema(
        &fresh_schema(),
        &schema_snapshot(&conn),
        "beta v2 singleton",
    );
}

#[test]
fn imported_dump_without_ledger_is_backfilled() {
    // SQL exports never contain the ledger; importing a current dump must rebuild it.
    let conn = Connection::open_in_memory().expect("open memory db");
    Database::create_tables_on_conn(&conn).expect("create fresh schema");
    conn.execute_batch(
        "DROP TABLE schema_migrations;
         DROP TRIGGER trg_request_logs_dedup_session_insert;
         DROP TRIGGER trg_request_logs_dedup_proxy_insert;",
    )
    .expect("simulate imported dump");
    upgrade_like_startup(&conn);
    assert_same_schema(&fresh_schema(), &schema_snapshot(&conn), "imported dump");
    assert_ledger(&conn, SCHEMA_VERSION as i64);
}

#[test]
fn failed_migration_rolls_back_schema_and_ledger() {
    let conn = database_at_version(15);
    // v16 -> v17 drops the legacy failover_queue *table*; a view with that name makes
    // the DROP fail after the preflight and v15 -> v16 have already run. The error used
    // to be swallowed (its result was discarded); now it aborts the whole upgrade.
    conn.execute_batch("CREATE VIEW failover_queue AS SELECT 1 AS x;")
        .expect("seed blocking view");
    let before = schema_snapshot(&conn);
    let err = Database::apply_schema_migrations_on_conn(&conn).expect_err("migration must fail");
    assert!(
        err.to_string().contains("failover_queue"),
        "unexpected error: {err}"
    );
    assert_eq!(Database::get_user_version(&conn).expect("user_version"), 15);
    assert_eq!(
        schema_snapshot(&conn),
        before,
        "failed migration must roll back"
    );
}

#[test]
fn baseline_schema_checksum_is_pinned() {
    use sha2::{Digest, Sha256};
    // Hash of the frozen baseline DDL as SQLite stores it (tables + indexes, excluding the
    // dedup triggers whose SQL lives in services::usage_stats). The baseline is shared by
    // fresh databases and the pre-v17 upgrade path, so it must never change: add a
    // migration instead. The hash covers the fresh schema, i.e. the baseline plus
    // later migrations (v18 added session_log_sync.byte_offset / prefix_hash; v19-v22
    // added the endpoint unique index, pricing source, CHECK-free proxy_config and row versions;
    // v23 dropped the unused status_code request-log index; v24/v25 added the
    // model_pricing catalog columns; v26 the integer cost columns and index;
    // v27 skills.enabled_openclaw).
    let conn = Connection::open_in_memory().expect("open memory db");
    Database::create_tables_on_conn(&conn).expect("create fresh schema");
    let objects = query_strings(
        &conn,
        "SELECT type, name, sql FROM sqlite_master
         WHERE type IN ('table', 'index') AND sql IS NOT NULL ORDER BY type, name",
    );
    let text = objects
        .iter()
        .map(|o| format!("{} {} {}", o[0], o[1], normalize_sql(&o[2])))
        .collect::<Vec<_>>()
        .join("\n");
    let checksum = format!("{:x}", Sha256::digest(text.as_bytes()));
    assert_eq!(
        checksum,
        "9bf327fdf49c0ed4241d20f542438db91304a3eae278d69c55c057522ebe3b13",
        "baseline schema changed; it is frozen at v{}:\n{text}",
        schema::BASELINE_SCHEMA_VERSION
    );
}
