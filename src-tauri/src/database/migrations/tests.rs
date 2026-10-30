//! 迁移框架测试：MIGRATIONS 表的结构约束、审计表，以及单个步骤的行为

use super::*;
use crate::services::sync_protocol::{DB_COMPAT_VERSION, LEGACY_DB_COMPAT_VERSION};

/// 最后一个允许声明 [`DbCompatImpact::PrePolicy`] 的版本（兼容策略之前发布的步骤）
const LAST_PRE_POLICY_VERSION: i32 = 15;

fn audit_versions(conn: &Connection) -> Result<Vec<i32>, AppError> {
    let mut stmt = conn.prepare("SELECT version FROM schema_migrations ORDER BY id")?;
    let versions = stmt
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<i32>, _>>()?;
    Ok(versions)
}

fn setting(conn: &Connection, key: &str) -> Result<Option<String>, AppError> {
    let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
    let mut rows = stmt.query([key])?;
    Ok(match rows.next()? {
        Some(row) => row.get(0)?,
        None => None,
    })
}

fn index_exists(conn: &Connection, name: &str) -> Result<bool, AppError> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = ?1",
        [name],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

#[test]
fn migrations_are_contiguous_and_end_at_schema_version() {
    let versions: Vec<i32> = MIGRATIONS.iter().map(|m| m.version).collect();
    let expected: Vec<i32> = (1..=SCHEMA_VERSION).collect();
    assert_eq!(
        versions, expected,
        "MIGRATIONS must hold exactly one step per version 1..=SCHEMA_VERSION, in order"
    );
    assert_eq!(MIGRATIONS.last().map(|m| m.version), Some(SCHEMA_VERSION));
    for migration in MIGRATIONS {
        assert!(
            !migration.name.trim().is_empty(),
            "v{} needs a name",
            migration.version
        );
    }
}

#[test]
fn migrations_declare_db_compat_impact_per_policy() {
    let mut last_breaking = LEGACY_DB_COMPAT_VERSION;
    for migration in MIGRATIONS {
        match migration.db_compat {
            DbCompatImpact::Additive => {}
            DbCompatImpact::PrePolicy => assert!(
                migration.version <= LAST_PRE_POLICY_VERSION,
                "v{}: PrePolicy is only for steps released before docs/dev/DB_COMPATIBILITY.md; \
                 declare Additive or Breaking",
                migration.version
            ),
            DbCompatImpact::Breaking { db_compat_version } => {
                assert!(
                    db_compat_version > LEGACY_DB_COMPAT_VERSION
                        && db_compat_version <= DB_COMPAT_VERSION,
                    "v{}: a breaking step must ship with a DB_COMPAT_VERSION bump \
                     (declared {db_compat_version}, current {DB_COMPAT_VERSION})",
                    migration.version
                );
                assert!(
                    db_compat_version >= last_breaking,
                    "v{}: DB_COMPAT_VERSION of breaking steps must not decrease",
                    migration.version
                );
                last_breaking = db_compat_version;
            }
        }
    }
}

#[test]
fn fresh_database_runs_every_step_and_records_it() -> Result<(), AppError> {
    let conn = Connection::open_in_memory()?;
    Database::apply_schema_migrations_on_conn(&conn)?;

    assert_eq!(Database::get_user_version(&conn)?, SCHEMA_VERSION);
    assert_eq!(
        audit_versions(&conn)?,
        (1..=SCHEMA_VERSION).collect::<Vec<_>>()
    );
    let (name, db_compat, app_version): (String, String, String) = conn.query_row(
        "SELECT name, db_compat, app_version FROM schema_migrations WHERE version = ?1",
        [SCHEMA_VERSION],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!(name, MIGRATIONS[MIGRATIONS.len() - 1].name);
    assert_eq!(db_compat, "additive");
    assert_eq!(app_version, env!("CARGO_PKG_VERSION"));

    // 全新安装不应触发「从应用目录导入旧版 Skills」
    assert_eq!(setting(&conn, "skills_ssot_migration_pending")?, None);
    assert!(Database::has_column(&conn, "skills", "enabled_grokbuild")?);
    let proxy_rows: i64 =
        conn.query_row("SELECT COUNT(*) FROM proxy_config", [], |row| row.get(0))?;
    assert_eq!(proxy_rows, 4);
    assert!(index_exists(&conn, "idx_request_logs_provider_created_at")?);

    // 已是最新版本：再次迁移不执行任何步骤，也不追加审计记录
    Database::apply_schema_migrations_on_conn(&conn)?;
    assert_eq!(audit_versions(&conn)?.len(), SCHEMA_VERSION as usize);
    Ok(())
}

#[test]
fn upgrade_records_only_the_steps_it_applies() -> Result<(), AppError> {
    let conn = Connection::open_in_memory()?;
    Database::apply_schema_migrations_up_to(&conn, 13)?;
    assert_eq!(Database::get_user_version(&conn)?, 13);
    conn.execute("DELETE FROM schema_migrations", [])?;

    Database::apply_schema_migrations_on_conn(&conn)?;

    assert_eq!(
        audit_versions(&conn)?,
        (14..=SCHEMA_VERSION).collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn v15_database_gets_spend_limit_index_from_numbered_step() -> Result<(), AppError> {
    let conn = Connection::open_in_memory()?;
    Database::apply_schema_migrations_up_to(&conn, 15)?;
    // v15 发布版的数据库没有这个索引（它曾由每次启动的 create_tables 补建）
    conn.execute(
        "DROP INDEX IF EXISTS idx_request_logs_dedup_lookup_expr",
        [],
    )?;
    assert!(!index_exists(
        &conn,
        "idx_request_logs_provider_created_at"
    )?);

    Database::apply_schema_migrations_on_conn(&conn)?;

    assert_eq!(Database::get_user_version(&conn)?, SCHEMA_VERSION);
    assert!(index_exists(&conn, "idx_request_logs_provider_created_at")?);
    assert!(index_exists(&conn, "idx_request_logs_dedup_lookup_expr")?);
    Ok(())
}

#[test]
fn failing_step_propagates_error_and_rolls_back() -> Result<(), AppError> {
    let conn = Connection::open_in_memory()?;
    Database::apply_schema_migrations_up_to(&conn, 15)?;
    let audit_before = audit_versions(&conn)?;
    // 与索引同名的表让 v16 的 CREATE INDEX 必然失败；旧的 create_tables 会吞掉这类错误
    conn.execute_batch(
        "DROP INDEX IF EXISTS idx_request_logs_provider_created_at;
         CREATE TABLE idx_request_logs_provider_created_at (x INTEGER);
         INSERT INTO settings (key, value) VALUES ('current_profile_id', 'p1');",
    )?;

    let err = Database::apply_schema_migrations_on_conn(&conn)
        .expect_err("a failing step must not be swallowed");
    assert!(
        err.to_string().contains("v15 -> v16"),
        "unexpected error: {err}"
    );

    assert_eq!(Database::get_user_version(&conn)?, 15);
    assert_eq!(audit_versions(&conn)?, audit_before);
    // 同一步骤中已执行的修改也被回滚
    assert_eq!(setting(&conn, "current_profile_id")?.as_deref(), Some("p1"));
    Ok(())
}

#[test]
fn v16_repairs_dev_global_profile_marker() -> Result<(), AppError> {
    let conn = Connection::open_in_memory()?;
    // 模拟跑过未发布开发版的库：current 标记还是全局 key（现按应用分组）
    Database::apply_schema_migrations_up_to(&conn, 15)?;
    conn.execute_batch(
        "INSERT INTO profiles (id, name, payload) VALUES ('p1', 'Project A', '{}');
         INSERT INTO settings (key, value) VALUES ('current_profile_id', 'p1');",
    )?;

    Database::apply_schema_migrations_on_conn(&conn)?;

    assert_eq!(
        setting(&conn, "current_profile_id_claude")?.as_deref(),
        Some("p1")
    );
    assert_eq!(setting(&conn, "current_profile_id")?, None);
    Ok(())
}

#[test]
fn v16_restores_v2_objects_missing_from_beta_v2() -> Result<(), AppError> {
    let conn = Connection::open_in_memory()?;
    // 模拟测试版 v2：user_version=2，但缺少 v2 引入的 failover 列和代理状态表
    // （原先由每次启动的 create_tables 补建，v2 步骤不会对已是 v2 的库重跑）
    Database::apply_schema_migrations_up_to(&conn, 2)?;
    conn.execute_batch(
        "DROP INDEX idx_providers_failover;
         ALTER TABLE providers DROP COLUMN in_failover_queue;
         DROP TABLE provider_health;
         DROP TABLE stream_check_logs;
         DROP TABLE proxy_live_backup;
         DROP INDEX idx_request_logs_model;",
    )?;

    Database::apply_schema_migrations_on_conn(&conn)?;

    assert_eq!(Database::get_user_version(&conn)?, SCHEMA_VERSION);
    assert!(Database::has_column(
        &conn,
        "providers",
        "in_failover_queue"
    )?);
    assert!(index_exists(&conn, "idx_providers_failover")?);
    for table in ["provider_health", "stream_check_logs", "proxy_live_backup"] {
        assert!(Database::table_exists(&conn, table)?, "{table} missing");
    }
    assert!(index_exists(&conn, "idx_stream_check_logs_provider")?);
    assert!(index_exists(&conn, "idx_request_logs_model")?);
    Ok(())
}

#[test]
fn v14_converts_singleton_proxy_config_left_by_beta_v2() -> Result<(), AppError> {
    let conn = Connection::open_in_memory()?;
    // 模拟测试版 v2：user_version=2，但 proxy_config 仍是单例结构（无 app_type）
    Database::apply_schema_migrations_up_to(&conn, 2)?;
    conn.execute_batch(
        r#"
        DROP TABLE proxy_config;
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
        INSERT INTO proxy_config (id, enabled, listen_port) VALUES (1, 1, 5123);
        "#,
    )?;

    Database::apply_schema_migrations_on_conn(&conn)?;

    assert!(Database::has_column(&conn, "proxy_config", "app_type")?);
    let mut stmt =
        conn.prepare("SELECT app_type, listen_port FROM proxy_config ORDER BY app_type")?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        rows,
        ["claude", "codex", "gemini", "grokbuild"]
            .iter()
            .map(|app| (app.to_string(), 5123))
            .collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn migrate_v12_to_v13_adds_input_token_semantics_columns() -> Result<(), AppError> {
    let conn = Connection::open_in_memory()?;
    conn.execute(
        "CREATE TABLE proxy_request_logs (request_id TEXT PRIMARY KEY)",
        [],
    )?;
    conn.execute(
        "CREATE TABLE usage_daily_rollups (date TEXT PRIMARY KEY)",
        [],
    )?;
    Database::set_user_version(&conn, 12)?;

    Database::apply_schema_migrations_on_conn(&conn)?;

    assert_eq!(Database::get_user_version(&conn)?, SCHEMA_VERSION);
    assert!(Database::has_column(
        &conn,
        "proxy_request_logs",
        "input_token_semantics"
    )?);
    assert!(Database::has_column(
        &conn,
        "usage_daily_rollups",
        "input_token_semantics"
    )?);
    let log_default: i64 = conn.query_row(
        "SELECT dflt_value = '0' FROM pragma_table_info('proxy_request_logs')
         WHERE name = 'input_token_semantics'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(log_default, 1);

    Ok(())
}

#[test]
fn migrate_v13_to_v14_adds_grokbuild_proxy_row_and_preserves_values() -> Result<(), AppError> {
    let conn = Connection::open_in_memory()?;
    Database::apply_schema_migrations_up_to(&conn, 13)?;
    conn.execute("DELETE FROM proxy_config WHERE app_type = 'grokbuild'", [])?;
    conn.execute(
        "UPDATE proxy_config SET enabled = 1, max_retries = 9 WHERE app_type = 'codex'",
        [],
    )?;
    Database::set_user_version(&conn, 13)?;

    Database::apply_schema_migrations_on_conn(&conn)?;

    assert_eq!(Database::get_user_version(&conn)?, SCHEMA_VERSION);
    let grok_rows: i64 = conn.query_row(
        "SELECT COUNT(*) FROM proxy_config WHERE app_type = 'grokbuild'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(grok_rows, 1);
    let codex_values: (i64, i64) = conn.query_row(
        "SELECT enabled, max_retries FROM proxy_config WHERE app_type = 'codex'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    assert_eq!(codex_values, (1, 9));

    Ok(())
}

#[test]
fn migrate_v14_to_v15_adds_grokbuild_skill_and_mcp_flags() -> Result<(), AppError> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch(
        "CREATE TABLE mcp_servers (
            id TEXT PRIMARY KEY,
            enabled_codex BOOLEAN NOT NULL DEFAULT 0
        );
        CREATE TABLE skills (
            id TEXT PRIMARY KEY,
            enabled_codex BOOLEAN NOT NULL DEFAULT 0
        );",
    )?;
    conn.execute(
        "INSERT INTO mcp_servers (id, enabled_codex) VALUES ('mcp-1', 1)",
        [],
    )?;
    conn.execute(
        "INSERT INTO skills (id, enabled_codex) VALUES ('skill-1', 1)",
        [],
    )?;
    Database::set_user_version(&conn, 14)?;

    Database::apply_schema_migrations_on_conn(&conn)?;

    assert_eq!(Database::get_user_version(&conn)?, SCHEMA_VERSION);
    assert!(Database::has_column(
        &conn,
        "mcp_servers",
        "enabled_grokbuild"
    )?);
    assert!(Database::has_column(&conn, "skills", "enabled_grokbuild")?);
    let mcp_values: (i64, i64) = conn.query_row(
        "SELECT enabled_codex, enabled_grokbuild FROM mcp_servers WHERE id = 'mcp-1'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let skill_values: (i64, i64) = conn.query_row(
        "SELECT enabled_codex, enabled_grokbuild FROM skills WHERE id = 'skill-1'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    assert_eq!(mcp_values, (1, 0));
    assert_eq!(skill_values, (1, 0));

    Ok(())
}

#[test]
fn migrate_v16_to_v17_backfills_join_tables_and_drops_proxy_config_check() -> Result<(), AppError> {
    let conn = Connection::open_in_memory()?;
    Database::apply_schema_migrations_up_to(&conn, 16)?;
    conn.execute_batch(
        "INSERT INTO mcp_servers (id, name, server_config, enabled_claude, enabled_codex, enabled_hermes)
            VALUES ('m1', 'm1', '{}', 1, 0, 1), ('m2', 'm2', '{}', 0, 0, 0);
         INSERT INTO skills (id, name, directory, installed_at, enabled_gemini, enabled_grokbuild)
            VALUES ('s1', 's1', 's1', 1, 1, 1);
         UPDATE proxy_config SET listen_port = 23456, max_retries = 9 WHERE app_type = 'codex';",
    )?;
    let proxy_before: Vec<(String, i64, i64)> = conn
        .prepare("SELECT app_type, listen_port, max_retries FROM proxy_config ORDER BY app_type")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<Result<_, _>>()?;
    assert!(
        conn.execute("INSERT INTO proxy_config (app_type) VALUES ('ninth')", [])
            .is_err(),
        "v16 still has the app_type CHECK"
    );

    Database::apply_schema_migrations_on_conn(&conn)?;

    let pairs = |sql: &str| -> Result<Vec<String>, AppError> {
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(rows)
    };
    assert_eq!(
        pairs("SELECT server_id || ':' || app_type FROM mcp_server_apps ORDER BY 1")?,
        ["m1:claude", "m1:hermes"]
    );
    assert_eq!(
        pairs("SELECT skill_id || ':' || app_type FROM skill_apps ORDER BY 1")?,
        ["s1:gemini", "s1:grokbuild"]
    );
    // 旧列保留不变（双写一个版本）
    let legacy: (i64, i64) = conn.query_row(
        "SELECT enabled_claude, enabled_hermes FROM mcp_servers WHERE id = 'm1'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    assert_eq!(legacy, (1, 1));

    let proxy_after: Vec<(String, i64, i64)> = conn
        .prepare("SELECT app_type, listen_port, max_retries FROM proxy_config ORDER BY app_type")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<Result<_, _>>()?;
    assert_eq!(
        proxy_after, proxy_before,
        "proxy_config rows survive the rebuild"
    );
    let ddl: String = conn.query_row(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'proxy_config'",
        [],
        |row| row.get(0),
    )?;
    assert!(!ddl.contains("CHECK"), "CHECK must be gone: {ddl}");
    conn.execute("INSERT INTO proxy_config (app_type) VALUES ('ninth')", [])?;

    Ok(())
}

/// 旧库可能缺少部分 `enabled_<slug>` 列（例如 v15 之前没有 grokbuild）：回填只用存在的列
#[test]
fn migrate_v16_to_v17_backfill_tolerates_missing_legacy_columns() -> Result<(), AppError> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch(
        "CREATE TABLE mcp_servers (id TEXT PRIMARY KEY, enabled_codex BOOLEAN NOT NULL DEFAULT 0);
         INSERT INTO mcp_servers (id, enabled_codex) VALUES ('m1', 1);",
    )?;
    Database::set_user_version(&conn, 16)?;

    Database::apply_schema_migrations_on_conn(&conn)?;

    let app: String = conn.query_row(
        "SELECT app_type FROM mcp_server_apps WHERE server_id = 'm1'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(app, "codex");
    assert!(Database::table_exists(&conn, "skill_apps")?);
    Ok(())
}

/// v18：未改过的内置行（当前价或旧价）标为 builtin，改过价格或显示名的内置行与自定义模型标为 user
#[test]
fn migrate_v17_to_v18_classifies_pricing_provenance() -> Result<(), AppError> {
    let conn = Connection::open_in_memory()?;
    Database::apply_schema_migrations_up_to(&conn, 17)?;
    conn.execute_batch(
        "DELETE FROM model_pricing WHERE model_id = 'claude-sonnet-5';
         UPDATE model_pricing SET cache_creation_cost_per_million = '0'
            WHERE model_id = 'gpt-5.6-sol';
         UPDATE model_pricing SET input_cost_per_million = '99'
            WHERE model_id = 'claude-opus-4-8';
         UPDATE model_pricing SET display_name = 'Renamed only'
            WHERE model_id = 'claude-haiku-4-5-20251001';
         INSERT INTO model_pricing (model_id, display_name, input_cost_per_million,
            output_cost_per_million) VALUES ('my-custom-model', 'Custom', '1', '2');",
    )?;

    Database::apply_schema_migrations_on_conn(&conn)?;

    let row = |id: &str| -> Result<(String, Option<i64>, Option<i64>, i64), AppError> {
        Ok(conn.query_row(
            "SELECT source, catalog_version, updated_at, deleted
             FROM model_pricing WHERE model_id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?)
    };
    let version = i64::from(crate::pricing::catalog::catalog().version);
    assert_eq!(
        row("claude-opus-4-7")?,
        ("builtin".into(), Some(version), None, 0)
    );
    // 仅改了显示名的行也是用户行：否则启动同步会把显示名改回目录值
    assert_eq!(
        row("claude-haiku-4-5-20251001")?,
        ("user".into(), None, None, 0)
    );
    // 等于目录旧价：未改过，但不属于任何目录版本
    assert_eq!(row("gpt-5.6-sol")?, ("builtin".into(), None, None, 0));
    assert_eq!(row("claude-opus-4-8")?, ("user".into(), None, None, 0));
    assert_eq!(row("my-custom-model")?, ("user".into(), None, None, 0));
    let missing: i64 = conn.query_row(
        "SELECT COUNT(*) FROM model_pricing WHERE model_id = 'claude-sonnet-5'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(missing, 0, "the migration itself writes no rows");
    Ok(())
}
