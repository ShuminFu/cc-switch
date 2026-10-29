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

    Database::create_tables_on_conn(&conn).expect("create tables");
    assert_eq!(
        Database::get_user_version(&conn).expect("read version before"),
        0
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
fn schema_create_tables_repairs_dev_global_profile_marker() {
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

    Database::create_tables_on_conn(&conn).expect("create tables should repair marker");

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
    Database::create_tables_on_conn(&conn).expect("repair is idempotent");
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
fn schema_create_tables_repairs_legacy_proxy_config_singleton_to_per_app() {
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

    Database::create_tables_on_conn(&conn).expect("create tables should repair proxy_config");

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
    }

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

/// 在隔离的 home 下写入一个备份文件，返回其文件名
fn write_backup_file(
    home: &std::path::Path,
    filename: &str,
    version: i32,
    provider_id: Option<&str>,
) -> String {
    let backup_dir = home.join(".cc-switch").join("backups");
    std::fs::create_dir_all(&backup_dir).expect("create backup dir");
    let conn = Connection::open(backup_dir.join(filename)).expect("open backup db");
    Database::create_tables_on_conn(&conn).expect("create backup tables");
    if let Some(id) = provider_id {
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
             VALUES (?1, 'claude', 'Backup Provider', '{}', '{}')",
            params![id],
        )
        .expect("insert backup provider");
    }
    Database::set_user_version(&conn, version).expect("set backup version");
    filename.to_string()
}

fn seed_main_provider(db: &Database) {
    let conn = db.conn.lock().expect("lock conn");
    conn.execute(
        "INSERT INTO providers (id, app_type, name, settings_config, meta)
         VALUES ('main-provider', 'claude', 'Main Provider', '{}', '{}')",
        [],
    )
    .expect("insert main provider");
    Database::set_user_version(&conn, SCHEMA_VERSION).expect("set main version");
}

fn provider_ids(db: &Database) -> Vec<String> {
    let conn = db.conn.lock().expect("lock conn");
    let mut stmt = conn
        .prepare("SELECT id FROM providers ORDER BY id")
        .expect("prepare");
    let ids = stmt
        .query_map([], |row| row.get(0))
        .expect("query providers")
        .collect::<Result<Vec<String>, _>>()
        .expect("collect providers");
    ids
}

fn main_user_version(db: &Database) -> i32 {
    let conn = db.conn.lock().expect("lock conn");
    Database::get_user_version(&conn).expect("read main version")
}

#[test]
fn restore_from_backup_rejects_newer_version_and_keeps_main_db() {
    let home = tempfile::tempdir().expect("tempdir");
    let _paths = crate::paths::scoped(crate::paths::Paths::with_home(home.path()));
    let db = Database::memory().expect("create memory db");
    seed_main_provider(&db);
    db.set_setting("local_marker", "kept").expect("set marker");

    let future = write_backup_file(
        home.path(),
        "future.db",
        SCHEMA_VERSION + 1,
        Some("future-provider"),
    );
    let err = db
        .restore_from_backup(&future)
        .expect_err("restoring a backup from a newer version must fail");
    match &err {
        AppError::Localized { key, .. } => assert_eq!(*key, "error.backupFromNewerVersion"),
        other => panic!("unexpected error: {other}"),
    }

    assert_eq!(main_user_version(&db), SCHEMA_VERSION);
    assert_eq!(provider_ids(&db), vec!["main-provider".to_string()]);
    assert_eq!(
        db.get_setting("local_marker")
            .expect("read marker")
            .as_deref(),
        Some("kept")
    );
}

#[test]
fn restore_from_backup_rejects_invalid_backup_and_keeps_main_db() {
    let home = tempfile::tempdir().expect("tempdir");
    let _paths = crate::paths::scoped(crate::paths::Paths::with_home(home.path()));
    let db = Database::memory().expect("create memory db");
    seed_main_provider(&db);

    // 空库（无供应商/MCP）在临时库校验阶段被拒绝
    let empty = write_backup_file(home.path(), "empty.db", SCHEMA_VERSION, None);
    assert!(db.restore_from_backup(&empty).is_err());

    // 非 SQLite 文件
    let garbage_path = home
        .path()
        .join(".cc-switch")
        .join("backups")
        .join("garbage.db");
    std::fs::write(&garbage_path, b"definitely not a sqlite database file").expect("write garbage");
    assert!(db.restore_from_backup("garbage.db").is_err());

    assert_eq!(main_user_version(&db), SCHEMA_VERSION);
    assert_eq!(provider_ids(&db), vec!["main-provider".to_string()]);
}

#[test]
fn restore_from_backup_replaces_main_db_after_temp_validation() {
    let home = tempfile::tempdir().expect("tempdir");
    let _paths = crate::paths::scoped(crate::paths::Paths::with_home(home.path()));
    let db = Database::memory().expect("create memory db");
    seed_main_provider(&db);

    let good = write_backup_file(
        home.path(),
        "good.db",
        SCHEMA_VERSION,
        Some("backup-provider"),
    );
    db.restore_from_backup(&good).expect("restore valid backup");

    assert_eq!(main_user_version(&db), SCHEMA_VERSION);
    assert_eq!(provider_ids(&db), vec!["backup-provider".to_string()]);
}

fn journal_mode(db: &Database) -> String {
    let conn = db.conn.lock().expect("lock conn");
    journal::current_journal_mode(&conn).expect("read journal_mode")
}

#[test]
fn init_uses_wal_with_busy_timeout_on_local_dir() {
    let home = tempfile::tempdir().expect("tempdir");
    let _paths = crate::paths::scoped(crate::paths::Paths::with_home(home.path()));
    let _settings = crate::settings::scoped_settings_store();

    let db = Database::init().expect("init database");
    assert_eq!(journal_mode(&db), "wal");
    {
        let conn = db.conn.lock().expect("lock conn");
        let sync: i64 = conn
            .query_row("PRAGMA synchronous;", [], |r| r.get(0))
            .expect("synchronous");
        assert_eq!(sync, 1, "WAL database should use synchronous=NORMAL");
        let timeout: i64 = conn
            .query_row("PRAGMA busy_timeout;", [], |r| r.get(0))
            .expect("busy_timeout");
        assert_eq!(timeout, 5000);
        // 切换 WAL 不能影响新库的 incremental auto_vacuum
        assert_eq!(
            Database::get_auto_vacuum_mode(&conn).expect("auto_vacuum"),
            2
        );
    }

    // WAL 下读者不被未提交的写事务阻塞
    let db_path = home.path().join(".cc-switch").join("cc-switch.db");
    {
        let conn = db.conn.lock().expect("lock conn");
        conn.execute_batch(
            "BEGIN IMMEDIATE; INSERT INTO settings (key, value) VALUES ('pending', 'x');",
        )
        .expect("open write transaction");
        let reader = Connection::open(&db_path).expect("open reader");
        let count: i64 = reader
            .query_row(
                "SELECT COUNT(*) FROM settings WHERE key = 'pending'",
                [],
                |r| r.get(0),
            )
            .expect("read while writer holds the lock");
        assert_eq!(count, 0);
        conn.execute_batch("COMMIT;").expect("commit");
    }

    // 快照备份文件不应保持 WAL（只读打开时会留下 -wal / -shm）
    let backup = db
        .backup_database_file()
        .expect("backup")
        .expect("backup path");
    let backup_conn = Connection::open(&backup).expect("open backup");
    assert_eq!(
        journal::current_journal_mode(&backup_conn).expect("backup journal_mode"),
        "delete"
    );
    drop(backup_conn);

    // 内存快照（WebDAV / S3 导出路径）可见已提交的写入
    let snapshot = db.snapshot_to_memory().expect("snapshot");
    let count: i64 = snapshot
        .query_row(
            "SELECT COUNT(*) FROM settings WHERE key = 'pending'",
            [],
            |r| r.get(0),
        )
        .expect("query snapshot");
    assert_eq!(count, 1);
    let sql = db.export_sql_string_for_sync().expect("export for sync");
    assert!(sql.contains("'pending'"));
}

#[test]
fn init_keeps_delete_journal_in_cloud_sync_dir() {
    let root = tempfile::tempdir().expect("tempdir");
    // 模拟 HOME 位于 Dropbox 中：数据目录为 <tmp>/Dropbox/.cc-switch
    let home = root.path().join("Dropbox");
    std::fs::create_dir_all(&home).expect("create simulated cloud home");
    let _paths = crate::paths::scoped(crate::paths::Paths::with_home(&home));
    let _settings = crate::settings::scoped_settings_store();

    let db = Database::init().expect("init database");
    assert_eq!(journal_mode(&db), "delete");
    {
        let conn = db.conn.lock().expect("lock conn");
        let timeout: i64 = conn
            .query_row("PRAGMA busy_timeout;", [], |r| r.get(0))
            .expect("busy_timeout");
        assert_eq!(timeout, 5000);
    }
    drop(db);
    let dir = home.join(".cc-switch");
    assert!(!dir.join("cc-switch.db-wal").exists());
    assert!(!dir.join("cc-switch.db-shm").exists());
}

/// 把 SQL 导出中的 `PRAGMA user_version=N;` 改写为 `version`，模拟更新版本的导出
fn with_dump_user_version(sql: &str, version: i32) -> String {
    assert!(
        sql.lines()
            .any(|line| line.starts_with("PRAGMA user_version=")),
        "dump has no user_version"
    );
    sql.lines()
        .map(|line| {
            if line.starts_with("PRAGMA user_version=") {
                format!("PRAGMA user_version={version};")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn import_of_newer_schema_dump_is_rejected_and_leaves_db_unchanged() {
    let home = tempfile::tempdir().expect("tempdir");
    let _paths = crate::paths::scoped(crate::paths::Paths::with_home(home.path()));
    let db = Database::memory().expect("create memory db");
    seed_main_provider(&db);

    let remote = Database::memory().expect("create remote db");
    {
        let conn = remote.conn.lock().expect("lock conn");
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
             VALUES ('remote-provider', 'claude', 'Remote Provider', '{}', '{}')",
            [],
        )
        .expect("insert remote provider");
    }
    let newer = with_dump_user_version(
        &remote.export_sql_string().expect("export"),
        SCHEMA_VERSION + 1,
    );

    for (label, result) in [
        ("manual import", db.import_sql_string(&newer)),
        ("sync import", db.import_sql_string_for_sync(&newer)),
    ] {
        match result {
            Err(AppError::Localized { key, en, .. }) => {
                assert_eq!(key, "backup.sql.newer_schema", "{label}");
                assert!(en.contains("No local data was changed"), "{label}: {en}");
            }
            other => panic!("{label}: unexpected result: {other:?}"),
        }
    }
    assert_eq!(main_user_version(&db), SCHEMA_VERSION);
    assert_eq!(provider_ids(&db), vec!["main-provider".to_string()]);

    // 头部被改写为受支持版本、但实际执行后 user_version 更新的导出同样被拒绝
    let forged = format!(
        "{}\nPRAGMA user_version={};\n",
        remote.export_sql_string().expect("export"),
        SCHEMA_VERSION + 1
    );
    match db.import_sql_string(&forged) {
        Err(AppError::Localized { key, .. }) => assert_eq!(key, "backup.sql.newer_schema"),
        other => panic!("forged dump: unexpected result: {other:?}"),
    }
    assert_eq!(provider_ids(&db), vec!["main-provider".to_string()]);
}

/// 模拟「新版应用迁移后回退到旧版」：磁盘上的库比当前应用新，backups 里有
/// 新版应用迁移前留下的快照（以及一个同样过新、不可恢复的备份）。
fn setup_newer_db_with_premigration_backup(home: &std::path::Path) -> std::path::PathBuf {
    let db = Database::init().expect("init database");
    seed_main_provider(&db);
    drop(db);
    let dir = home.join(".cc-switch");
    let db_path = dir.join("cc-switch.db");
    let backup_dir = dir.join("backups");
    std::fs::create_dir_all(&backup_dir).expect("create backup dir");
    {
        let conn = Connection::open(&db_path).expect("open main db");
        let mut dest =
            Connection::open(backup_dir.join("db_backup_20260101_000000.db")).expect("open dest");
        rusqlite::backup::Backup::new(&conn, &mut dest)
            .and_then(|b| b.step(-1))
            .expect("copy pre-migration backup");
        Database::set_user_version(&conn, SCHEMA_VERSION + 1).expect("bump main version");
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
             VALUES ('newer-provider', 'claude', 'Newer Provider', '{}', '{}')",
            [],
        )
        .expect("insert newer provider");
    }
    write_backup_file(
        home,
        "db_backup_20260102_000000.db",
        SCHEMA_VERSION + 1,
        None,
    );
    std::fs::write(
        backup_dir.join("db_backup_20260103_000000.db"),
        b"not sqlite",
    )
    .expect("write corrupt backup");
    db_path
}

#[test]
fn db_version_too_new_payload_lists_restorable_premigration_backups() {
    let home = tempfile::tempdir().expect("tempdir");
    let _paths = crate::paths::scoped(crate::paths::Paths::with_home(home.path()));
    let _settings = crate::settings::scoped_settings_store();
    let db_path = setup_newer_db_with_premigration_backup(home.path());

    let version = Database::stored_user_version_exceeds_supported(&db_path)
        .expect("precheck")
        .expect("database must be flagged as too new");
    let payload = crate::init_status::db_version_too_new_payload(&db_path, version);

    assert_eq!(
        payload.kind.as_deref(),
        Some(crate::init_status::DB_VERSION_TOO_NEW)
    );
    assert_eq!(payload.db_version, Some(SCHEMA_VERSION + 1));
    assert_eq!(payload.supported_version, Some(SCHEMA_VERSION));
    let backups = payload.restorable_backups.expect("backups listed");
    let names: Vec<&str> = backups.iter().map(|b| b.filename.as_str()).collect();
    assert_eq!(names, vec!["db_backup_20260101_000000.db"]);
    assert_eq!(backups[0].db_version, SCHEMA_VERSION);

    let json = serde_json::to_value(crate::init_status::db_version_too_new_payload(
        &db_path, version,
    ))
    .expect("serialize payload");
    assert_eq!(
        json["restorable_backups"][0]["filename"],
        json!("db_backup_20260101_000000.db")
    );
    assert_eq!(
        json["restorable_backups"][0]["dbVersion"],
        json!(SCHEMA_VERSION)
    );
}

#[test]
fn restore_backup_over_newer_db_downgrades_and_keeps_newer_copy() {
    let home = tempfile::tempdir().expect("tempdir");
    let _paths = crate::paths::scoped(crate::paths::Paths::with_home(home.path()));
    let _settings = crate::settings::scoped_settings_store();
    let db_path = setup_newer_db_with_premigration_backup(home.path());

    match Database::restore_backup_over_newer_db("db_backup_20260102_000000.db") {
        Err(AppError::Localized { key, .. }) => assert_eq!(key, "error.backupFromNewerVersion"),
        other => panic!("unexpected result: {other:?}"),
    }
    assert!(Database::restore_backup_over_newer_db("../cc-switch.db").is_err());
    assert_eq!(
        Database::stored_user_version_exceeds_supported(&db_path).expect("precheck"),
        Some(SCHEMA_VERSION + 1),
        "a refused restore must leave the newer database in place"
    );

    let preserved = Database::restore_backup_over_newer_db("db_backup_20260101_000000.db")
        .expect("restore pre-migration backup");
    assert!(preserved.starts_with(&format!("db_newer_v{}_", SCHEMA_VERSION + 1)));
    assert!(preserved.ends_with(".sqlite"));
    let backup_dir = home.path().join(".cc-switch").join("backups");
    let kept = Connection::open(backup_dir.join(&preserved)).expect("open preserved db");
    assert_eq!(
        Database::get_user_version(&kept).expect("preserved version"),
        SCHEMA_VERSION + 1
    );
    let newer_rows: i64 = kept
        .query_row(
            "SELECT COUNT(*) FROM providers WHERE id = 'newer-provider'",
            [],
            |r| r.get(0),
        )
        .expect("count newer rows");
    assert_eq!(newer_rows, 1, "the newer database must be preserved");
    // 保留的新版库不是 .db 备份：不会被列出，也不会被备份保留数清理
    assert!(Database::list_backups()
        .expect("list backups")
        .iter()
        .all(|b| b.filename != preserved));

    assert_eq!(
        Database::stored_user_version_exceeds_supported(&db_path).expect("precheck"),
        None
    );
    let db = Database::init().expect("app starts after downgrade restore");
    assert_eq!(main_user_version(&db), SCHEMA_VERSION);
    assert_eq!(provider_ids(&db), vec!["main-provider".to_string()]);
}
