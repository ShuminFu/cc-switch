//! 迁移等价性测试（CCS-011）
//!
//! 对 `tests/fixtures/db/v{N}.sql` 中每个已发布（或曾在 main 上存在）的 schema 版本，
//! 以及 `tests/fixtures/legacy/*.json` 中的旧版 config.json，走一遍真实启动路径
//! （`Database::init()` / `migrate_from_json`），并断言：
//! - `user_version == SCHEMA_VERSION`
//! - 归一化后的 sqlite_master / table_info 与全新数据库一致
//! - 用户数据行被保留
//! - `schema_migrations` 审计表恰好记录本次执行的每个迁移步骤（CCS-145）
//! - 再次启动迁移是 no-op（数据与 schema 不变，不再创建迁移前备份）
//! - `SCHEMA_VERSION + 1` 的数据库会被当前应用拒绝
//!
//! DB fixture 由 `scripts/gen-db-fixture.sh <tag>` 从对应 tag 的旧代码生成。

mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use cc_switch_lib::{
    paths, update_settings, AppSettings, Database, MultiAppConfig, SCHEMA_VERSION,
};
use rusqlite::types::Value;
use rusqlite::{Connection, DatabaseName};
use support::TestEnv;

/// 至少覆盖的不同起始版本数（验收标准）
const MIN_DB_FIXTURES: usize = 10;

/// 没有任何 commit 产出过的 schema 版本，因此没有 fixture。
///
/// v14：main 上曾短暂存在一个 v14（ac52c851，cache-write TTL 列），随后被回退为 v13；
/// 现在的 v14（Grok Build 代理行）与 v15 在同一个 commit 中引入，从未单独存在。
/// 那个未发布的 TTL 版 v14 数据库不在支持范围内（当前迁移链会把它当作新 v14，
/// 跳过 v13→v14），见 CCS-011 实现说明。
const VERSIONS_WITHOUT_FIXTURE: &[i32] = &[14];

/// 启动清理会删除超过保留期的日志（stream_check_logs 7 天、proxy_request_logs 30 天
/// 汇总进 rollup）。fixture 是静态的，加载后把这些时间戳刷新为当前时间，避免
/// 「数据保留」断言随时间推移而失效。
const FRESH_TIMESTAMP_COLUMNS: &[(&str, &str)] = &[
    ("proxy_request_logs", "created_at"),
    ("stream_check_logs", "tested_at"),
];

/// fixture 中的 proxy_config 是旧版本 seed 的默认值；加载后改成非默认值，
/// 这样「数据保留」断言才能发现迁移把用户配置重置为默认值的问题。
const CUSTOMIZED_PROXY_LISTEN_PORT: i64 = 23456;

/// 需要逐行校验保留情况的用户数据表。
///
/// 不在列表中的表：`model_pricing`（启动时重新 seed / 修复）。`skills` 在 v3 起
/// 结构整体更换，见 [`assert_rows_preserved`] 中的特殊处理。
const USER_DATA_TABLES: &[&str] = &[
    "providers",
    "provider_endpoints",
    "mcp_servers",
    "prompts",
    "skill_repos",
    "settings",
    "proxy_request_logs",
    "usage_daily_rollups",
    "stream_check_logs",
    "proxy_live_backup",
    "session_log_sync",
    "profiles",
    "skills",
    "proxy_config",
    "schema_migrations",
];

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// 按版本号排序的 DB fixture 列表
fn db_fixtures() -> Vec<(i32, PathBuf)> {
    let dir = fixtures_dir().join("db");
    let mut fixtures: Vec<(i32, PathBuf)> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .filter_map(|entry| {
            let path = entry.expect("dir entry").path();
            let name = path.file_name()?.to_str()?;
            let version = name.strip_prefix('v')?.strip_suffix(".sql")?.parse().ok()?;
            Some((version, path))
        })
        .collect();
    fixtures.sort();
    fixtures
}

fn db_path() -> PathBuf {
    paths::current().app_config_dir().join("cc-switch.db")
}

fn backup_count() -> usize {
    let dir = paths::current().app_config_dir().join("backups");
    std::fs::read_dir(dir).map(|it| it.count()).unwrap_or(0)
}

/// backups 目录中以 `suffix` 结尾的文件（按文件名排序）
fn backup_files(suffix: &str) -> Vec<PathBuf> {
    let dir = paths::current().app_config_dir().join("backups");
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|it| {
            it.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.to_string_lossy().ends_with(suffix))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

/// 唯一的迁移前数据库备份（auto-vacuum 重建前的备份是迁移后的 SCHEMA_VERSION，据此区分）
fn premigration_db_backup(label: &str) -> PathBuf {
    let backups: Vec<PathBuf> = backup_files(".db")
        .into_iter()
        .filter(|p| user_version(&open(p)) < SCHEMA_VERSION)
        .collect();
    assert_eq!(
        backups.len(),
        1,
        "{label}: expected exactly one pre-migration backup, found {backups:?}"
    );
    backups.into_iter().next().expect("one backup")
}

/// 迁移前备份必须与升级前的数据库完全一致（版本、schema、全部数据行）
fn assert_backup_matches(label: &str, pre: &(i32, SchemaShape, BTreeMap<String, Vec<Row>>)) {
    let backup = full_snapshot(&open(&premigration_db_backup(label)));
    assert_eq!(backup.0, pre.0, "{label}: backup user_version");
    assert_schema_eq(&format!("{label} (backup)"), &backup.1, &pre.1);
    assert_eq!(
        backup.2, pre.2,
        "{label}: backup data differs from pre-upgrade state"
    );
}

fn open(path: &Path) -> Connection {
    Connection::open(path).unwrap_or_else(|e| panic!("open {}: {e}", path.display()))
}

fn user_version(conn: &Connection) -> i32 {
    conn.query_row("PRAGMA user_version;", [], |row| row.get(0))
        .expect("read user_version")
}

fn table_exists(conn: &Connection, table: &str) -> bool {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [table],
        |row| row.get::<_, i64>(0),
    )
    .expect("query sqlite_master")
        > 0
}

fn user_tables(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .expect("prepare table list");
    stmt.query_map([], |row| row.get(0))
        .expect("query table list")
        .collect::<Result<_, _>>()
        .expect("read table list")
}

/// 加载 fixture，并把易过期的时间戳刷新为当前时间
fn load_fixture(conn: &Connection, sql_path: &Path) {
    let sql = std::fs::read_to_string(sql_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", sql_path.display()));
    // dump 按表名顺序建表，子表的数据可能先于父表出现，加载期间关闭外键检查
    conn.execute_batch("PRAGMA foreign_keys = OFF;")
        .expect("disable foreign keys");
    conn.execute_batch(&sql)
        .unwrap_or_else(|e| panic!("load {}: {e}", sql_path.display()));
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .expect("enable foreign keys");
    let now = chrono::Utc::now().timestamp();
    for (table, column) in FRESH_TIMESTAMP_COLUMNS {
        if table_exists(conn, table) {
            conn.execute(&format!("UPDATE \"{table}\" SET \"{column}\" = ?1"), [now])
                .expect("refresh fixture timestamps");
        }
    }
    if table_exists(conn, "proxy_config") {
        conn.execute(
            "UPDATE proxy_config SET listen_port = ?1",
            [CUSTOMIZED_PROXY_LISTEN_PORT],
        )
        .expect("customize proxy_config");
    }
}

// ---------------------------------------------------------------------------
// Schema 归一化
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
struct ColumnShape {
    r#type: String,
    notnull: bool,
    default: Option<String>,
    pk: i64,
}

/// (unique, origin, partial, 列, 显式索引名；自动索引为 None)
type IndexShape = (bool, String, bool, Vec<String>, Option<String>);

#[derive(Debug, PartialEq, Eq)]
struct TableShape {
    /// 列按名称排序：ALTER TABLE ADD COLUMN 追加在末尾，列顺序不影响行为
    columns: BTreeMap<String, ColumnShape>,
    indexes: BTreeSet<IndexShape>,
    /// (目标表, 本表列, 目标列, ON UPDATE, ON DELETE)
    foreign_keys: BTreeSet<(String, String, String, String, String)>,
    /// 归一化后的 CHECK 约束
    checks: BTreeSet<String>,
}

type SchemaShape = BTreeMap<String, TableShape>;

fn normalize_sql_fragment(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn normalize_default(default: Option<String>) -> Option<String> {
    default.map(|s| {
        let mut s = s.trim().to_string();
        // `(datetime('now'))` 与 `datetime('now')`、`'0'` 与 `0` 等价写法统一
        while s.starts_with('(') && s.ends_with(')') {
            s = s[1..s.len() - 1].trim().to_string();
        }
        normalize_sql_fragment(s.trim_matches('\'').trim_matches('"'))
    })
}

/// 从 CREATE TABLE 语句中提取 CHECK(...) 约束
fn extract_checks(sql: &str) -> BTreeSet<String> {
    let lower = sql.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut checks = BTreeSet::new();
    let mut from = 0;
    while let Some(pos) = lower[from..].find("check") {
        let start = from + pos + "check".len();
        let Some(open) = lower[start..].find('(').map(|p| start + p) else {
            break;
        };
        if !lower[start..open].trim().is_empty() {
            from = start;
            continue;
        }
        let mut depth = 0;
        let mut end = open;
        for (i, b) in bytes.iter().enumerate().skip(open) {
            match b {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i;
                        break;
                    }
                }
                _ => {}
            }
        }
        checks.insert(normalize_sql_fragment(&sql[open + 1..end]));
        from = end + 1;
    }
    checks
}

fn schema_shape(conn: &Connection) -> SchemaShape {
    let mut shape = SchemaShape::new();
    for table in user_tables(conn) {
        let mut columns = BTreeMap::new();
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info(\"{table}\");"))
            .expect("prepare table_info");
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(1)?,
                    ColumnShape {
                        r#type: row.get::<_, String>(2)?.to_ascii_uppercase(),
                        notnull: row.get::<_, i64>(3)? != 0,
                        default: normalize_default(row.get::<_, Option<String>>(4)?),
                        pk: row.get(5)?,
                    },
                ))
            })
            .expect("query table_info");
        for row in rows {
            let (name, column) = row.expect("read table_info");
            columns.insert(name, column);
        }

        let mut indexes = BTreeSet::new();
        let mut stmt = conn
            .prepare(&format!("PRAGMA index_list(\"{table}\");"))
            .expect("prepare index_list");
        let list: Vec<(String, bool, String, bool)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(1)?,
                    row.get::<_, i64>(2)? != 0,
                    row.get(3)?,
                    row.get::<_, i64>(4)? != 0,
                ))
            })
            .expect("query index_list")
            .collect::<Result<_, _>>()
            .expect("read index_list");
        for (name, unique, origin, partial) in list {
            let mut stmt = conn
                .prepare(&format!("PRAGMA index_info(\"{name}\");"))
                .expect("prepare index_info");
            let cols: Vec<String> = stmt
                .query_map([], |row| row.get::<_, Option<String>>(2))
                .expect("query index_info")
                .map(|c| c.expect("read index_info").unwrap_or_default())
                .collect();
            let explicit = (origin == "c").then_some(name);
            indexes.insert((unique, origin, partial, cols, explicit));
        }

        let mut stmt = conn
            .prepare(&format!("PRAGMA foreign_key_list(\"{table}\");"))
            .expect("prepare foreign_key_list");
        let foreign_keys = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                ))
            })
            .expect("query foreign_key_list")
            .collect::<Result<_, _>>()
            .expect("read foreign_key_list");

        let sql: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [&table],
                |row| row.get(0),
            )
            .expect("read table sql");

        shape.insert(
            table,
            TableShape {
                columns,
                indexes,
                foreign_keys,
                checks: extract_checks(&sql),
            },
        );
    }
    shape
}

/// 逐表比较，失败信息只列出不一致的表，便于定位
fn assert_schema_eq(label: &str, actual: &SchemaShape, expected: &SchemaShape) {
    let mut problems = Vec::new();
    for name in expected.keys().filter(|t| !actual.contains_key(*t)) {
        problems.push(format!("missing table {name}"));
    }
    for name in actual.keys().filter(|t| !expected.contains_key(*t)) {
        problems.push(format!("unexpected table {name}"));
    }
    for (name, want) in expected {
        let Some(got) = actual.get(name) else {
            continue;
        };
        if got != want {
            problems.push(format!("table {name}:\n  got:  {got:?}\n  want: {want:?}"));
        }
    }
    assert!(
        problems.is_empty(),
        "{label}: schema differs from a fresh database:\n{}",
        problems.join("\n")
    );
}

// ---------------------------------------------------------------------------
// 数据快照
// ---------------------------------------------------------------------------

type Row = BTreeMap<String, Value>;

fn table_rows(conn: &Connection, table: &str) -> Vec<Row> {
    let mut stmt = conn
        .prepare(&format!("SELECT * FROM \"{table}\""))
        .expect("prepare select");
    let names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
    let mut rows: Vec<Row> = stmt
        .query_map([], |row| {
            let mut map = Row::new();
            for (i, name) in names.iter().enumerate() {
                map.insert(name.clone(), row.get::<_, Value>(i)?);
            }
            Ok(map)
        })
        .expect("query rows")
        .collect::<Result<_, _>>()
        .expect("read rows");
    rows.sort_by_key(|row| format!("{row:?}"));
    rows
}

/// 整库快照：schema + 全部数据行（用于验证二次迁移为 no-op）
fn full_snapshot(conn: &Connection) -> (i32, SchemaShape, BTreeMap<String, Vec<Row>>) {
    let data = user_tables(conn)
        .into_iter()
        .map(|t| {
            let rows = table_rows(conn, &t);
            (t, rows)
        })
        .collect();
    (user_version(conn), schema_shape(conn), data)
}

/// fixture 中的每一行在迁移后仍存在，且两边都有的列取值不变。
///
/// skills 表在 v3 从「安装状态」（v1 以 key、v2 以 (directory, app_type) 为主键）
/// 换成统一结构（id 主键），旧行不原样保留，而是写入
/// `settings.skills_ssot_migration_snapshot` 供启动后重新导入，因此改为校验快照。
fn assert_rows_preserved(label: &str, before: &Connection, after: &Connection) {
    for table in USER_DATA_TABLES {
        if !table_exists(before, table) {
            continue;
        }
        assert!(
            table_exists(after, table),
            "{label}: table {table} disappeared during migration"
        );
        let old_rows = table_rows(before, table);
        if *table == "skills" && old_rows.first().is_some_and(|r| !r.contains_key("id")) {
            assert_legacy_skills_snapshotted(label, &old_rows, after);
            continue;
        }
        let new_rows = table_rows(after, table);
        for old in &old_rows {
            let found = new_rows.iter().any(|new| {
                old.iter()
                    .all(|(col, value)| new.get(col).is_none_or(|v| v == value))
            });
            assert!(
                found,
                "{label}: row not preserved in {table}: {old:?}\nrows after migration: {new_rows:#?}"
            );
        }
    }
}

/// v3 之前的已安装 skill 必须出现在迁移快照中，并标记为待重新导入
fn assert_legacy_skills_snapshotted(label: &str, old_rows: &[Row], after: &Connection) {
    let setting = |key: &str| -> String {
        after
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .unwrap_or_else(|e| panic!("{label}: read setting {key}: {e}"))
    };
    assert_eq!(
        setting("skills_ssot_migration_pending"),
        "true",
        "{label}: legacy skills must be marked for re-import"
    );
    let snapshot: Vec<serde_json::Value> =
        serde_json::from_str(&setting("skills_ssot_migration_snapshot"))
            .unwrap_or_else(|e| panic!("{label}: parse skills snapshot: {e}"));
    let installed = old_rows
        .iter()
        .filter(|r| r.get("installed") == Some(&Value::Integer(1)))
        .count();
    assert!(installed > 0, "{label}: fixture has no installed skill");
    assert_eq!(
        snapshot.len(),
        installed,
        "{label}: skills snapshot {snapshot:?} does not cover {old_rows:?}"
    );
    assert!(
        snapshot
            .iter()
            .any(|s| s["directory"] == "fixture-skill" && s["app_type"] == "claude"),
        "{label}: fixture skill missing from snapshot {snapshot:?}"
    );
}

/// 用当前代码创建一个全新的数据库，作为等价性比较的基准
fn fresh_schema() -> SchemaShape {
    let _env = TestEnv::new();
    let db = Database::init().expect("init fresh database");
    drop(db);
    let conn = open(&db_path());
    assert_eq!(user_version(&conn), SCHEMA_VERSION);
    schema_shape(&conn)
}

/// `schema_migrations` 审计表中版本大于 `version` 的记录（按执行顺序）
fn applied_migrations_after(conn: &Connection, version: i32) -> Vec<i32> {
    let mut stmt = conn
        .prepare("SELECT version FROM schema_migrations WHERE version > ?1 ORDER BY id")
        .expect("prepare schema_migrations query");
    stmt.query_map([version], |row| row.get(0))
        .expect("query schema_migrations")
        .collect::<Result<_, _>>()
        .expect("read schema_migrations")
}

/// 在当前测试环境中再次启动，断言没有任何变化，也不会再做迁移前备份
fn assert_second_init_is_noop(label: &str) {
    let before = full_snapshot(&open(&db_path()));
    let backups = backup_count();
    drop(Database::init().unwrap_or_else(|e| panic!("{label}: second init failed: {e}")));
    let after = full_snapshot(&open(&db_path()));
    assert_eq!(
        before.0, after.0,
        "{label}: user_version changed on second init"
    );
    assert_schema_eq(&format!("{label} (second init)"), &after.1, &before.1);
    assert_eq!(before.2, after.2, "{label}: data changed on second init");
    assert_eq!(
        backup_count(),
        backups,
        "{label}: second init created a pre-migration backup"
    );
}

#[test]
fn db_fixtures_cover_every_schema_version() {
    let versions: Vec<i32> = db_fixtures().into_iter().map(|(v, _)| v).collect();
    assert!(
        versions.len() >= MIN_DB_FIXTURES,
        "need at least {MIN_DB_FIXTURES} start versions, found {versions:?}"
    );
    let missing: Vec<i32> = (1..=SCHEMA_VERSION)
        .filter(|v| !versions.contains(v) && !VERSIONS_WITHOUT_FIXTURE.contains(v))
        .collect();
    assert!(
        missing.is_empty(),
        "missing DB fixtures for schema versions {missing:?}; \
         regenerate with scripts/gen-db-fixture.sh <tag-or-commit>"
    );
    assert!(
        versions.iter().all(|v| *v <= SCHEMA_VERSION),
        "fixture newer than SCHEMA_VERSION {SCHEMA_VERSION}: {versions:?}"
    );
}

#[test]
fn every_released_schema_migrates_to_current() {
    let expected = fresh_schema();
    let fixtures = db_fixtures();
    assert!(!fixtures.is_empty(), "no DB fixtures found");

    for (version, sql_path) in fixtures {
        let label = format!("v{version} fixture");
        let _env = TestEnv::new();
        let path = db_path();
        std::fs::create_dir_all(path.parent().expect("db dir")).expect("create db dir");

        load_fixture(&open(&path), &sql_path);
        // 从同一份已加载的文件复制对照库，而不是再加载一次 fixture：两次加载跨秒时
        // 刷新后的时间戳不同，会让「数据保留」断言偶发失败
        let mut before = Connection::open_in_memory().expect("open memory db");
        before
            .restore(
                DatabaseName::Main,
                &path,
                None::<fn(rusqlite::backup::Progress)>,
            )
            .expect("copy fixture db into memory");
        assert_eq!(
            user_version(&before),
            version,
            "{label}: fixture user_version"
        );
        let pre = full_snapshot(&open(&path));

        drop(Database::init().unwrap_or_else(|e| panic!("{label}: init failed: {e}")));
        if version < SCHEMA_VERSION {
            assert_backup_matches(&label, &pre);
        }

        let after = open(&path);
        assert_eq!(
            user_version(&after),
            SCHEMA_VERSION,
            "{label}: user_version"
        );
        assert_schema_eq(&label, &schema_shape(&after), &expected);
        assert_rows_preserved(&label, &before, &after);
        assert_eq!(
            applied_migrations_after(&after, version),
            ((version + 1)..=SCHEMA_VERSION).collect::<Vec<_>>(),
            "{label}: schema_migrations must record each applied step once"
        );
        drop(after);

        assert_second_init_is_noop(&label);
    }
}

/// v0（尚未设置 user_version 的数据库）同样要在任何 DDL 之前做迁移前备份
#[test]
fn v0_database_gets_premigration_backup() {
    let _env = TestEnv::new();
    let path = db_path();
    std::fs::create_dir_all(path.parent().expect("db dir")).expect("create db dir");
    {
        let conn = open(&path);
        load_fixture(&conn, &fixtures_dir().join("db/v1.sql"));
        conn.execute_batch("PRAGMA user_version = 0;")
            .expect("reset user_version");
    }
    let pre = full_snapshot(&open(&path));
    assert_eq!(pre.0, 0);

    drop(Database::init().expect("init v0 database"));
    assert_eq!(user_version(&open(&path)), SCHEMA_VERSION);
    assert_backup_matches("v0", &pre);
    assert_second_init_is_noop("v0");
}

/// WAL 模式下尚未 checkpoint 的写入只存在于 -wal 文件中：直接复制主库文件会丢失它们，
/// 迁移前备份（Online Backup API）必须包含这些写入
#[test]
fn premigration_backup_includes_uncheckpointed_wal_writes() {
    let _env = TestEnv::new();
    let path = db_path();
    std::fs::create_dir_all(path.parent().expect("db dir")).expect("create db dir");
    {
        let conn = open(&path);
        load_fixture(&conn, &fixtures_dir().join("db/v13.sql"));
        let mode: String = conn
            .query_row("PRAGMA journal_mode = WAL;", [], |row| row.get(0))
            .expect("enable WAL");
        assert_eq!(mode, "wal");
    }

    // 保持连接打开，关闭自动 checkpoint，让写入停留在 WAL 中
    let writer = open(&path);
    writer
        .execute_batch("PRAGMA wal_autocheckpoint = 0;")
        .expect("disable autocheckpoint");
    writer
        .execute(
            "INSERT INTO settings (key, value) VALUES ('wal_marker', 'uncheckpointed')",
            [],
        )
        .expect("write marker");
    let wal_path = PathBuf::from(format!("{}-wal", path.display()));
    assert!(
        std::fs::metadata(&wal_path).map(|m| m.len()).unwrap_or(0) > 0,
        "marker write must still be in the WAL"
    );

    // 对照：只复制主库文件拿不到 WAL 中的写入
    let scratch = tempfile::tempdir().expect("tempdir");
    let copied = scratch.path().join("copy.db");
    std::fs::copy(&path, &copied).expect("copy main db file");
    let marker_in = |conn: &Connection| -> bool {
        conn.query_row(
            "SELECT COUNT(*) FROM settings WHERE key = 'wal_marker' AND value = 'uncheckpointed'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("query marker")
            > 0
    };
    assert!(
        !marker_in(&open(&copied)),
        "a plain file copy unexpectedly saw the WAL write"
    );

    drop(Database::init().expect("init WAL database"));
    let backup = open(&premigration_db_backup("wal"));
    assert_eq!(user_version(&backup), 13);
    assert!(
        marker_in(&backup),
        "pre-migration backup is missing the uncheckpointed WAL write"
    );
    drop(writer);
}

/// 备份保留数为最小值 1 时，同一次启动中 auto-vacuum 重建不得再备份而把迁移前快照清理掉
#[test]
fn premigration_backup_survives_minimum_retention() {
    let _env = TestEnv::new();
    update_settings(AppSettings {
        backup_retain_count: Some(1),
        ..AppSettings::default()
    })
    .expect("set backup retention to 1");
    let path = db_path();
    std::fs::create_dir_all(path.parent().expect("db dir")).expect("create db dir");
    {
        let conn = open(&path);
        load_fixture(&conn, &fixtures_dir().join("db/v1.sql"));
        let mode: i64 = conn
            .query_row("PRAGMA auto_vacuum;", [], |row| row.get(0))
            .expect("read auto_vacuum");
        assert_ne!(mode, 2, "fixture must need an auto-vacuum rebuild");
    }
    let pre = full_snapshot(&open(&path));

    drop(Database::init().expect("init v1 database"));
    assert_eq!(user_version(&open(&path)), SCHEMA_VERSION);
    assert_backup_matches("retain=1", &pre);
}

/// 迁移失败时，启动错误信息中必须包含迁移前备份 ID，且该备份文件存在
#[test]
fn failed_migration_error_names_the_backup_id() {
    let _env = TestEnv::new();
    let path = db_path();
    std::fs::create_dir_all(path.parent().expect("db dir")).expect("create db dir");
    // providers 是视图而非表：v0 → v1 的 ALTER TABLE ADD COLUMN 必然失败
    open(&path)
        .execute_batch("CREATE VIEW providers AS SELECT 1 AS id; PRAGMA user_version = 0;")
        .expect("create broken v0 database");

    let err = match Database::init() {
        Ok(_) => panic!("migration of a broken v0 database must fail"),
        Err(e) => e.to_string(),
    };
    let backup = premigration_db_backup("broken v0");
    let backup_id = backup
        .file_stem()
        .and_then(|s| s.to_str())
        .expect("backup id")
        .to_string();
    assert!(
        err.contains(&backup_id),
        "startup error must name the backup id {backup_id}: {err}"
    );
    let view: String = open(&backup)
        .query_row(
            "SELECT type FROM sqlite_master WHERE name = 'providers'",
            [],
            |row| row.get(0),
        )
        .expect("backup keeps pre-upgrade schema");
    assert_eq!(view, "view");
}

#[test]
fn newer_schema_version_is_rejected() {
    let _env = TestEnv::new();
    drop(Database::init().expect("init fresh database"));
    let path = db_path();
    let future = SCHEMA_VERSION + 1;
    open(&path)
        .execute_batch(&format!("PRAGMA user_version = {future};"))
        .expect("bump user_version");

    assert_eq!(
        Database::stored_user_version_exceeds_supported(&path).expect("precheck"),
        Some(future),
        "startup precheck must flag a v{future} database"
    );
    let err = match Database::init() {
        Ok(_) => panic!("v{SCHEMA_VERSION} app must refuse a v{future} database"),
        Err(e) => e.to_string(),
    };
    assert!(err.contains("数据库版本过新"), "unexpected error: {err}");
    assert_eq!(
        user_version(&open(&path)),
        future,
        "a refused database must keep its user_version"
    );
}

// ---------------------------------------------------------------------------
// 旧版 config.json → SQLite
// ---------------------------------------------------------------------------

/// 模拟应用启动：config.json 存在而数据库不存在时，先备份 config.json 并加载 JSON，再 init + migrate_from_json
fn migrate_legacy_json(name: &str) -> Connection {
    let src = fixtures_dir().join("legacy").join(name);
    let dir = paths::current().app_config_dir();
    std::fs::create_dir_all(&dir).expect("create app config dir");
    std::fs::copy(&src, dir.join("config.json"))
        .unwrap_or_else(|e| panic!("copy {}: {e}", src.display()));
    assert!(!db_path().exists());

    let backup_id = Database::backup_legacy_config_json()
        .unwrap_or_else(|e| panic!("{name}: config.json backup failed: {e}"))
        .expect("config.json exists, a backup id is expected");
    let config = MultiAppConfig::load().unwrap_or_else(|e| panic!("{name}: load failed: {e}"));
    let db = Database::init().expect("init database");
    db.migrate_from_json(&config)
        .unwrap_or_else(|e| panic!("{name}: migrate_from_json failed: {e}"));
    drop(db);

    // JSON → SQLite 升级前，旧版 config.json 被原样备份
    let json_backups = backup_files(".config.json");
    assert_eq!(
        json_backups,
        vec![dir.join("backups").join(format!("{backup_id}.config.json"))],
        "{name}: expected exactly the config.json backup {backup_id}"
    );
    assert_eq!(
        std::fs::read(&json_backups[0]).expect("read config.json backup"),
        std::fs::read(&src).expect("read fixture"),
        "{name}: config.json backup differs from the pre-upgrade file"
    );

    let conn = open(&db_path());
    assert_eq!(user_version(&conn), SCHEMA_VERSION, "{name}: user_version");
    conn
}

fn query_strings(conn: &Connection, sql: &str) -> Vec<String> {
    let mut stmt = conn.prepare(sql).expect("prepare");
    stmt.query_map([], |row| row.get(0))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("read")
}

#[test]
fn legacy_config_json_v37_unified_mcp_migrates() {
    let expected = fresh_schema();
    let _env = TestEnv::new();
    let name = "config-v3.7-unified-mcp.json";
    let conn = migrate_legacy_json(name);
    assert_schema_eq(name, &schema_shape(&conn), &expected);

    assert_eq!(
        query_strings(
            &conn,
            "SELECT app_type || '/' || id || '/' || name || '/' || is_current FROM providers ORDER BY app_type, id"
        ),
        vec![
            "claude/claude-official/Claude Official/0",
            "claude/claude-relay/Claude Relay/1",
            "codex/codex-relay/Codex Relay/1",
            "gemini/gemini-main/Gemini Main/1",
        ]
    );
    let settings: String = conn
        .query_row(
            "SELECT settings_config FROM providers WHERE id = 'claude-relay' AND app_type = 'claude'",
            [],
            |row| row.get(0),
        )
        .expect("read settings_config");
    let settings: serde_json::Value = serde_json::from_str(&settings).expect("settings json");
    assert_eq!(
        settings["env"]["ANTHROPIC_BASE_URL"],
        "https://relay.example.com"
    );
    let meta: String = conn
        .query_row(
            "SELECT meta FROM providers WHERE id = 'claude-relay' AND app_type = 'claude'",
            [],
            |row| row.get(0),
        )
        .expect("read meta");
    assert!(
        !meta.contains("custom_endpoints"),
        "custom endpoints must move to provider_endpoints, meta = {meta}"
    );
    assert_eq!(
        query_strings(
            &conn,
            "SELECT provider_id || ' ' || url FROM provider_endpoints ORDER BY url"
        ),
        vec![
            "claude-relay https://relay-2.example.com",
            "claude-relay https://relay-3.example.com",
        ]
    );

    assert_eq!(
        query_strings(
            &conn,
            "SELECT id || ':' || enabled_claude || enabled_codex || enabled_gemini FROM mcp_servers ORDER BY id"
        ),
        vec!["fetch:110", "filesystem:101"]
    );
    let tags: String = conn
        .query_row("SELECT tags FROM mcp_servers WHERE id = 'fetch'", [], |r| {
            r.get(0)
        })
        .expect("read tags");
    assert_eq!(tags, r#"["web"]"#);

    assert_eq!(
        query_strings(
            &conn,
            "SELECT app_type || '/' || id || '/' || enabled FROM prompts ORDER BY app_type, id"
        ),
        vec!["claude/rules/1", "codex/codex-rules/0"]
    );
    assert_eq!(
        query_strings(
            &conn,
            "SELECT owner || '/' || name || '@' || branch || ':' || enabled FROM skill_repos ORDER BY owner"
        ),
        vec!["fixture-org/fixture-skills@dev:0"]
    );
    assert_eq!(
        query_strings(
            &conn,
            "SELECT key || '=' || value FROM settings WHERE key LIKE 'common_config_%' ORDER BY key"
        ),
        vec![
            r#"common_config_claude={"includeCoAuthoredBy":false}"#,
            "common_config_codex=model_reasoning_effort = \"high\"",
        ]
    );
    drop(conn);

    assert_second_init_is_noop(name);
}

#[test]
fn legacy_config_json_v36_per_app_mcp_migrates() {
    let expected = fresh_schema();
    let _env = TestEnv::new();
    let name = "config-v3.6-per-app-mcp.json";
    let conn = migrate_legacy_json(name);
    assert_schema_eq(name, &schema_shape(&conn), &expected);

    // 没有 gemini 段的旧配置也能迁移；current 指向的供应商被标记
    assert_eq!(
        query_strings(
            &conn,
            "SELECT app_type || '/' || id || '/' || is_current FROM providers ORDER BY app_type, id"
        ),
        vec![
            "claude/claude-a/0",
            "claude/claude-b/1",
            "codex/codex-a/1",
        ]
    );
    // v3.6 按客户端分开的 MCP（mcp.claude / mcp.codex）合并为统一结构，
    // 同 id 的服务器在两个应用都启用
    assert_eq!(
        query_strings(
            &conn,
            "SELECT id || ':' || enabled_claude || enabled_codex || enabled_gemini FROM mcp_servers ORDER BY id"
        ),
        vec!["claude-only:100", "codex-only:010", "shared:110"]
    );
    let shared: String = conn
        .query_row(
            "SELECT server_config FROM mcp_servers WHERE id = 'shared'",
            [],
            |r| r.get(0),
        )
        .expect("read server_config");
    let shared: serde_json::Value = serde_json::from_str(&shared).expect("server json");
    assert_eq!(shared["command"], "npx");
    // 旧字段 claude_common_config_snippet 迁移为 common_config_claude
    assert_eq!(
        query_strings(
            &conn,
            "SELECT value FROM settings WHERE key = 'common_config_claude'"
        ),
        vec![r#"{"permissions":{"allow":["Bash"]}}"#]
    );
    drop(conn);

    assert_second_init_is_noop(name);
}
