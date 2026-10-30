//! Schema 定义和迁移
//!
//! 负责数据库表结构的创建和版本迁移。
//!
//! ## 两条路径，一套迁移
//!
//! - **全新数据库**（无用户表且 `user_version = 0`）：[`Database::create_tables_on_conn`]
//!   在一个 savepoint 内执行冻结的基线 DDL（[`BASELINE_SCHEMA_VERSION`] 时的完整 schema），
//!   写入默认数据行，把基线及之前的迁移以 `baseline` 记入迁移台账，再执行基线之后
//!   注册的迁移，最终 `user_version = SCHEMA_VERSION`。
//! - **已有数据库**：表结构只通过 [`MIGRATIONS`] 注册表按版本前进
//!   （[`Database::apply_schema_migrations_on_conn`]），每一步记录到
//!   [`MIGRATION_LEDGER_TABLE`] 台账（版本、名称、校验和、应用版本、时间、来源）。
//!   低于基线版本的旧库在迁移链之前执行一次冻结的兼容预处理
//!   （`prepare_pre_baseline_database`，即 v17 之前每次启动都会执行的建表/补列逻辑），
//!   升级到基线之后不再执行。
//!
//! 修改 schema 时：**不要**修改基线 DDL 或已发布的迁移，而是在 [`MIGRATIONS`] 末尾追加
//! `migrate_v{N-1}_to_v{N}`。`SCHEMA_VERSION` 与同步的 `DB_COMPAT_VERSION` 都由注册表推导；
//! 测试会校验注册表完整性、已发布迁移的校验和、基线 schema 校验和，以及
//! 「全新库 == 各历史版本升级后的库」。

use super::{lock_conn, Database, SCHEMA_VERSION};
use crate::error::AppError;
use crate::services::usage_stats::{backfill_dedup_hidden_sql, request_logs_dedup_trigger_sqls};
use rusqlite::{params, Connection};
use serde::Serialize;

#[derive(Serialize)]
struct LegacySkillMigrationRow {
    directory: String,
    app_type: String,
}

// ─── Migration registry ──────────────────────────────────────

/// 迁移台账表名。它描述的是「这个数据库文件」的迁移历史，因此不进入 SQL 导出/同步快照
/// （导入时由迁移框架按 `user_version` 回填）。
pub(crate) const MIGRATION_LEDGER_TABLE: &str = "schema_migrations";

/// 冻结的基线 DDL 对应的 schema 版本，也是引入迁移台账的版本。
/// 低于该版本的数据库在迁移链之前执行一次兼容预处理。
pub(crate) const BASELINE_SCHEMA_VERSION: i32 = 17;

/// 引入迁移台账时（schema v17）同步协议的数据库兼容版本（`db-v6`）。
/// 之后每个 `breaks_sync: true` 的迁移使其加一，见 [`SYNC_DB_COMPAT_VERSION`]。
const SYNC_DB_COMPAT_AT_BASELINE: u32 = 6;

/// 一个已注册的 schema 迁移：把数据库从 `version - 1` 升级到 `version`。
pub(crate) struct Migration {
    /// 迁移完成后的 `user_version`。注册表必须从 1 开始连续递增（编译期校验）。
    pub(crate) version: i32,
    /// 稳定的英文名称，写入台账。
    pub(crate) name: &'static str,
    /// 迁移函数 `migrate_v{version-1}_to_v{version}` 源码（去掉注释和空白后）的 SHA-256。
    /// 已发布的迁移不可修改；源码变化时测试会失败。
    pub(crate) checksum: &'static str,
    /// 该迁移是否破坏同步快照兼容性（旧客户端无法导入新快照，或反之）。
    /// 为 `true` 时同步的 `DB_COMPAT_VERSION` 加一。基线及之前的迁移不参与推导
    /// （其影响已包含在 [`SYNC_DB_COMPAT_AT_BASELINE`] 中）。
    pub(crate) breaks_sync: bool,
    pub(crate) up: fn(&Connection) -> Result<(), AppError>,
}

/// 所有 schema 迁移，按版本排序。新增迁移只能追加到末尾。
pub(crate) const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "backfill_missing_columns",
        checksum: "ec508829247b959ace6a9a8011208e440bb9eef51d890ce91a7d0c3739a61cac",
        breaks_sync: false,
        up: Database::migrate_v0_to_v1,
    },
    Migration {
        version: 2,
        name: "usage_stats_and_per_app_proxy_config",
        checksum: "b4d97a3523eaaeec92ee01f65e84d049937e4d81d292bee79c663f4843b0d7bf",
        breaks_sync: false,
        up: Database::migrate_v1_to_v2,
    },
    Migration {
        version: 3,
        name: "unified_skills",
        checksum: "175a543126fe5423f8e6002506e93bd90c731d8adfa1eb2e15a4e6aba475a5e5",
        breaks_sync: false,
        up: Database::migrate_v2_to_v3,
    },
    Migration {
        version: 4,
        name: "opencode_support",
        checksum: "3b8be73a7712643d45792433f8c7053e25f6c3f0dbb95f3365a94347507f8477",
        breaks_sync: false,
        up: Database::migrate_v3_to_v4,
    },
    Migration {
        version: 5,
        name: "pricing_model_source",
        checksum: "dff73d696934859f37b854ca9710a0b3c6f2f23183437aaf276ef79a3d3de197",
        breaks_sync: false,
        up: Database::migrate_v4_to_v5,
    },
    Migration {
        version: 6,
        name: "usage_daily_rollups_and_copilot_template",
        checksum: "dc806a76c299155e87abb083c9626e61a560540951aba56107a313b8572fb464",
        breaks_sync: false,
        up: Database::migrate_v5_to_v6,
    },
    Migration {
        version: 7,
        name: "skill_update_detection",
        checksum: "d2507422d60fe164e7dd481fe568ee7cf37140d5fe20bdd6ceefaff38bbe6fdd",
        breaks_sync: false,
        up: Database::migrate_v6_to_v7,
    },
    Migration {
        version: 8,
        name: "session_log_usage_and_cny_pricing_fix",
        checksum: "63e8a2a060e29fde2a03fe954f68213b6edf67c09a4a2eef40c8f500aa0a4b82",
        breaks_sync: false,
        up: Database::migrate_v7_to_v8,
    },
    Migration {
        version: 9,
        name: "refresh_model_pricing",
        checksum: "d21f2b6359faae213beed261d341282e350dc965e70417fdc840e040d2b02eba",
        breaks_sync: false,
        up: Database::migrate_v8_to_v9,
    },
    Migration {
        version: 10,
        name: "hermes_support",
        checksum: "af3a5a125e83207fc70fd162ac3314f74ca70df0f37cbc487da0c4b977a3eb14",
        breaks_sync: false,
        up: Database::migrate_v9_to_v10,
    },
    Migration {
        version: 11,
        name: "rollup_request_model_dimension",
        checksum: "5a9c758d21e14a54926524d65155e6db3053d25ab5a5cd3278a824a35b254e2e",
        breaks_sync: false,
        up: Database::migrate_v10_to_v11,
    },
    Migration {
        version: 12,
        name: "profiles",
        checksum: "058132d5ecd4ed5934904c28e41517e10ee61bf9ae8e6c40b1911fa827f5583e",
        breaks_sync: false,
        up: Database::migrate_v11_to_v12,
    },
    Migration {
        version: 13,
        name: "input_token_semantics",
        checksum: "0a2341bccdeb7fa467be73ec958be6b85ace524dc94a51f28c20c810871c91a1",
        breaks_sync: false,
        up: Database::migrate_v12_to_v13,
    },
    Migration {
        version: 14,
        name: "grokbuild_proxy_config",
        checksum: "6f2a3c81e624e651bda720d381922477a1ec0d6986069300ed9d4e824bbe8056",
        breaks_sync: false,
        up: Database::migrate_v13_to_v14,
    },
    Migration {
        version: 15,
        name: "grokbuild_skills_and_mcp",
        checksum: "0610c23277e016af33349588c108cef726aff42550799b4fac2c3a2b6cf3ed89",
        breaks_sync: false,
        up: Database::migrate_v14_to_v15,
    },
    Migration {
        version: 16,
        name: "request_log_dedup_flag_and_index_trim",
        checksum: "b842694101b79b4896b1bf9b1c54202d4b21a39d1dec41276532b73381d78f05",
        breaks_sync: false,
        up: Database::migrate_v15_to_v16,
    },
    Migration {
        version: 17,
        name: "migration_ledger_and_startup_repairs",
        checksum: "6a88c17bbd2b5bf87dc456ea9353670249acfb98dda5285e7fa256a01333632e",
        breaks_sync: false,
        up: Database::migrate_v16_to_v17,
    },
];

/// 注册表中最新的 schema 版本（即 `SCHEMA_VERSION`）。
pub(crate) const LATEST_SCHEMA_VERSION: i32 = MIGRATIONS[MIGRATIONS.len() - 1].version;

/// 同步协议的数据库兼容版本，由注册表推导：基线值 + 基线之后 `breaks_sync` 的迁移数。
pub(crate) const SYNC_DB_COMPAT_VERSION: u32 = {
    let mut compat = SYNC_DB_COMPAT_AT_BASELINE;
    let mut i = 0;
    while i < MIGRATIONS.len() {
        if MIGRATIONS[i].version > BASELINE_SCHEMA_VERSION && MIGRATIONS[i].breaks_sync {
            compat += 1;
        }
        i += 1;
    }
    compat
};

// 编译期校验：版本从 1 开始连续递增，基线版本已注册。
const _: () = {
    let mut i = 0;
    while i < MIGRATIONS.len() {
        assert!(
            MIGRATIONS[i].version == i as i32 + 1,
            "MIGRATIONS must be ordered and contiguous starting at version 1"
        );
        i += 1;
    }
    assert!(
        BASELINE_SCHEMA_VERSION >= 1 && BASELINE_SCHEMA_VERSION <= LATEST_SCHEMA_VERSION,
        "BASELINE_SCHEMA_VERSION must be a registered migration"
    );
};

/// 台账中一行的来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LedgerSource {
    /// 迁移在这个数据库上实际执行过。
    Migrated,
    /// 全新数据库直接以基线 schema 创建，基线及之前的迁移无需执行。
    Baseline,
    /// 该版本在台账出现之前（或数据库由 SQL 导入重建之前）就已应用，执行细节未知。
    Backfilled,
}

impl LedgerSource {
    fn as_str(self) -> &'static str {
        match self {
            LedgerSource::Migrated => "migrated",
            LedgerSource::Baseline => "baseline",
            LedgerSource::Backfilled => "backfilled",
        }
    }
}

// ─── Frozen baseline schema (v17) ────────────────────────────

/// 基线 schema（[`BASELINE_SCHEMA_VERSION`]）的建表语句。**冻结**：
/// 旧库的兼容预处理也执行这些语句来补齐缺失的表，修改它们会改变历史升级路径，
/// 测试中的基线校验和也会失败。新的 schema 变更请写成迁移（全新库也会执行基线之后的迁移）。
const BASELINE_TABLE_SQL: &[&str] = &[
    // 1. Providers 表（cost_multiplier..provider_type 由 v1→v2 引入，全新库直接包含）
    "CREATE TABLE IF NOT EXISTS providers (
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
        in_failover_queue BOOLEAN NOT NULL DEFAULT 0,
        cost_multiplier TEXT NOT NULL DEFAULT '1.0',
        limit_daily_usd TEXT,
        limit_monthly_usd TEXT,
        provider_type TEXT,
        PRIMARY KEY (id, app_type)
    )",
    // 2. Provider Endpoints 表
    "CREATE TABLE IF NOT EXISTS provider_endpoints (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        provider_id TEXT NOT NULL,
        app_type TEXT NOT NULL,
        url TEXT NOT NULL,
        added_at INTEGER,
        FOREIGN KEY (provider_id, app_type) REFERENCES providers(id, app_type) ON DELETE CASCADE
    )",
    // 3. MCP Servers 表
    "CREATE TABLE IF NOT EXISTS mcp_servers (
        id TEXT PRIMARY KEY, name TEXT NOT NULL, server_config TEXT NOT NULL,
        description TEXT, homepage TEXT, docs TEXT, tags TEXT NOT NULL DEFAULT '[]',
        enabled_claude BOOLEAN NOT NULL DEFAULT 0, enabled_codex BOOLEAN NOT NULL DEFAULT 0,
        enabled_gemini BOOLEAN NOT NULL DEFAULT 0, enabled_grokbuild BOOLEAN NOT NULL DEFAULT 0,
        enabled_opencode BOOLEAN NOT NULL DEFAULT 0,
        enabled_hermes BOOLEAN NOT NULL DEFAULT 0
    )",
    // 4. Prompts 表
    "CREATE TABLE IF NOT EXISTS prompts (
        id TEXT NOT NULL, app_type TEXT NOT NULL, name TEXT NOT NULL, content TEXT NOT NULL,
        description TEXT, enabled BOOLEAN NOT NULL DEFAULT 1, created_at INTEGER, updated_at INTEGER,
        PRIMARY KEY (id, app_type)
    )",
    // 5. Skills 表（v3.10.0+ 统一结构）
    "CREATE TABLE IF NOT EXISTS skills (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        description TEXT,
        directory TEXT NOT NULL,
        repo_owner TEXT,
        repo_name TEXT,
        repo_branch TEXT DEFAULT 'main',
        readme_url TEXT,
        enabled_claude BOOLEAN NOT NULL DEFAULT 0,
        enabled_codex BOOLEAN NOT NULL DEFAULT 0,
        enabled_gemini BOOLEAN NOT NULL DEFAULT 0,
        enabled_grokbuild BOOLEAN NOT NULL DEFAULT 0,
        enabled_opencode BOOLEAN NOT NULL DEFAULT 0,
        enabled_hermes BOOLEAN NOT NULL DEFAULT 0,
        installed_at INTEGER NOT NULL DEFAULT 0,
        content_hash TEXT,
        updated_at INTEGER NOT NULL DEFAULT 0
    )",
    // 6. Skill Repos 表
    "CREATE TABLE IF NOT EXISTS skill_repos (
        owner TEXT NOT NULL, name TEXT NOT NULL, branch TEXT NOT NULL DEFAULT 'main',
        enabled BOOLEAN NOT NULL DEFAULT 1, PRIMARY KEY (owner, name)
    )",
    // 7. Settings 表
    "CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT)",
    // 8. Proxy Config 表（每应用一行，app_type 主键）
    "CREATE TABLE IF NOT EXISTS proxy_config (
        app_type TEXT PRIMARY KEY CHECK (app_type IN ('claude','codex','gemini','grokbuild')),
        proxy_enabled INTEGER NOT NULL DEFAULT 0, listen_address TEXT NOT NULL DEFAULT '127.0.0.1',
        listen_port INTEGER NOT NULL DEFAULT 15721, enable_logging INTEGER NOT NULL DEFAULT 1,
        enabled INTEGER NOT NULL DEFAULT 0, auto_failover_enabled INTEGER NOT NULL DEFAULT 0,
        max_retries INTEGER NOT NULL DEFAULT 3, streaming_first_byte_timeout INTEGER NOT NULL DEFAULT 60,
        streaming_idle_timeout INTEGER NOT NULL DEFAULT 120, non_streaming_timeout INTEGER NOT NULL DEFAULT 600,
        circuit_failure_threshold INTEGER NOT NULL DEFAULT 4, circuit_success_threshold INTEGER NOT NULL DEFAULT 2,
        circuit_timeout_seconds INTEGER NOT NULL DEFAULT 60, circuit_error_rate_threshold REAL NOT NULL DEFAULT 0.6,
        circuit_min_requests INTEGER NOT NULL DEFAULT 10,
        default_cost_multiplier TEXT NOT NULL DEFAULT '1',
        pricing_model_source TEXT NOT NULL DEFAULT 'response',
        live_takeover_active INTEGER NOT NULL DEFAULT 0,
        created_at TEXT NOT NULL DEFAULT (datetime('now')), updated_at TEXT NOT NULL DEFAULT (datetime('now'))
    )",
    // 9. Provider Health 表
    "CREATE TABLE IF NOT EXISTS provider_health (
        provider_id TEXT NOT NULL, app_type TEXT NOT NULL, is_healthy INTEGER NOT NULL DEFAULT 1,
        consecutive_failures INTEGER NOT NULL DEFAULT 0, last_success_at TEXT, last_failure_at TEXT,
        last_error TEXT, updated_at TEXT NOT NULL,
        PRIMARY KEY (provider_id, app_type),
        FOREIGN KEY (provider_id, app_type) REFERENCES providers(id, app_type) ON DELETE CASCADE
    )",
    // 10. Proxy Request Logs 表
    // pricing_model = 写入时实际用于计价的模型名（pricing_model_source 解析结果），
    // 回填按它重算；NULL 表示 v11 之前的历史行，'' 表示未计价的错误行。
    "CREATE TABLE IF NOT EXISTS proxy_request_logs (
        request_id TEXT PRIMARY KEY, provider_id TEXT NOT NULL, app_type TEXT NOT NULL, model TEXT NOT NULL,
        request_model TEXT,
        pricing_model TEXT,
        input_tokens INTEGER NOT NULL DEFAULT 0, output_tokens INTEGER NOT NULL DEFAULT 0,
        cache_read_tokens INTEGER NOT NULL DEFAULT 0, cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
        input_token_semantics INTEGER NOT NULL DEFAULT 0,
        input_cost_usd TEXT NOT NULL DEFAULT '0', output_cost_usd TEXT NOT NULL DEFAULT '0',
        cache_read_cost_usd TEXT NOT NULL DEFAULT '0', cache_creation_cost_usd TEXT NOT NULL DEFAULT '0',
        total_cost_usd TEXT NOT NULL DEFAULT '0', latency_ms INTEGER NOT NULL, first_token_ms INTEGER,
        duration_ms INTEGER, status_code INTEGER NOT NULL, error_message TEXT, session_id TEXT,
        provider_type TEXT, is_streaming INTEGER NOT NULL DEFAULT 0,
        cost_multiplier TEXT NOT NULL DEFAULT '1.0', created_at INTEGER NOT NULL,
        data_source TEXT NOT NULL DEFAULT 'proxy',
        dedup_hidden INTEGER NOT NULL DEFAULT 0
    )",
    // 11. Model Pricing 表
    "CREATE TABLE IF NOT EXISTS model_pricing (
        model_id TEXT PRIMARY KEY, display_name TEXT NOT NULL,
        input_cost_per_million TEXT NOT NULL, output_cost_per_million TEXT NOT NULL,
        cache_read_cost_per_million TEXT NOT NULL DEFAULT '0',
        cache_creation_cost_per_million TEXT NOT NULL DEFAULT '0'
    )",
    // 12. Stream Check Logs 表
    "CREATE TABLE IF NOT EXISTS stream_check_logs (
        id INTEGER PRIMARY KEY AUTOINCREMENT, provider_id TEXT NOT NULL, provider_name TEXT NOT NULL,
        app_type TEXT NOT NULL, status TEXT NOT NULL, success INTEGER NOT NULL, message TEXT NOT NULL,
        response_time_ms INTEGER, http_status INTEGER, model_used TEXT,
        retry_count INTEGER DEFAULT 0, tested_at INTEGER NOT NULL
    )",
    // 注意：circuit_breaker_config 已合并到 proxy_config 表中
    // 13. Proxy Live Backup 表 (Live 配置备份)
    "CREATE TABLE IF NOT EXISTS proxy_live_backup (
        app_type TEXT PRIMARY KEY, original_config TEXT NOT NULL, backed_up_at TEXT NOT NULL
    )",
    // 14. Usage Daily Rollups 表 (日聚合统计)
    // request_model 保留路由接管的「客户端别名 → 真实模型」映射维度，
    // pricing_model 保留写入时的计价基准（request 计价模式下与 model 分叉），
    // 否则明细被 prune 后接管计费不可审计；历史行迁移时填 ''（未知）。
    "CREATE TABLE IF NOT EXISTS usage_daily_rollups (
        date TEXT NOT NULL,
        app_type TEXT NOT NULL,
        provider_id TEXT NOT NULL,
        model TEXT NOT NULL,
        request_model TEXT NOT NULL DEFAULT '',
        pricing_model TEXT NOT NULL DEFAULT '',
        request_count INTEGER NOT NULL DEFAULT 0,
        success_count INTEGER NOT NULL DEFAULT 0,
        input_tokens INTEGER NOT NULL DEFAULT 0,
        output_tokens INTEGER NOT NULL DEFAULT 0,
        cache_read_tokens INTEGER NOT NULL DEFAULT 0,
        cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
        input_token_semantics INTEGER NOT NULL DEFAULT 0,
        total_cost_usd TEXT NOT NULL DEFAULT '0',
        avg_latency_ms INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (date, app_type, provider_id, model, request_model, pricing_model)
    )",
    // 15. Session Log Sync 表 (会话日志同步状态)
    "CREATE TABLE IF NOT EXISTS session_log_sync (
        file_path TEXT PRIMARY KEY,
        last_modified INTEGER NOT NULL,
        last_line_offset INTEGER NOT NULL DEFAULT 0,
        last_synced_at INTEGER NOT NULL
    )",
    // 16. Profiles 表（全应用共享的项目实体，payload 按 app 分槽快照
    //     供应商/MCP/Skills/Prompt；各应用分组的 current 标记在 settings 表）
    "CREATE TABLE IF NOT EXISTS profiles (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        payload TEXT NOT NULL,
        sort_order INTEGER,
        created_at INTEGER,
        updated_at INTEGER
    )",
];

/// 每个应用的默认代理配置行：(app_type, max_retries, streaming_first_byte_timeout,
/// streaming_idle_timeout, non_streaming_timeout, circuit_failure_threshold,
/// circuit_success_threshold, circuit_timeout_seconds, circuit_error_rate_threshold,
/// circuit_min_requests)
type ProxyConfigDefaults = (&'static str, i64, i64, i64, i64, i64, i64, i64, f64, i64);
const DEFAULT_PROXY_CONFIG_ROWS: &[ProxyConfigDefaults] = &[
    ("claude", 6, 90, 180, 600, 8, 3, 90, 0.7, 15),
    ("codex", 3, 60, 120, 600, 4, 2, 60, 0.6, 10),
    ("gemini", 5, 60, 120, 600, 4, 2, 60, 0.6, 10),
    ("grokbuild", 3, 60, 120, 600, 4, 2, 60, 0.6, 10),
];

/// v17 之前每次启动都会尝试补到 proxy_config 上的列（兼容 v3.9.0-2 等早期结构）。
const LEGACY_PROXY_CONFIG_COLUMNS: &[(&str, &str)] = &[
    ("live_takeover_active", "INTEGER NOT NULL DEFAULT 0"),
    ("proxy_enabled", "INTEGER NOT NULL DEFAULT 0"),
    ("listen_address", "TEXT NOT NULL DEFAULT '127.0.0.1'"),
    ("listen_port", "INTEGER NOT NULL DEFAULT 15721"),
    ("enable_logging", "INTEGER NOT NULL DEFAULT 1"),
    (
        "streaming_first_byte_timeout",
        "INTEGER NOT NULL DEFAULT 60",
    ),
    ("streaming_idle_timeout", "INTEGER NOT NULL DEFAULT 120"),
    ("non_streaming_timeout", "INTEGER NOT NULL DEFAULT 600"),
];

fn db_err(context: &'static str) -> impl Fn(rusqlite::Error) -> AppError {
    move |e| AppError::Database(format!("{context}: {e}"))
}

impl Database {
    /// 创建所有数据库表（见 [`Self::create_tables_on_conn`]）
    pub(crate) fn create_tables(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        Self::create_tables_on_conn(&conn)
    }

    /// 为全新数据库创建当前 schema；对已有数据库不做任何表结构变更。
    ///
    /// - 全新数据库（无用户表且 `user_version = 0`）：在一个 savepoint 内创建完整 schema、
    ///   写入默认行和迁移台账，并把 `user_version` 设为 `SCHEMA_VERSION`。
    /// - 已有数据库：表结构只通过 [`Self::apply_schema_migrations_on_conn`] 前进。这里只重建
    ///   应用自有的派生对象（`trg_request_logs_dedup_*` 触发器）：SQL 导出不含触发器，
    ///   SQL 导入/备份恢复会先删除所有触发器，再调用本函数恢复应用自有的触发器。
    pub(crate) fn create_tables_on_conn(conn: &Connection) -> Result<(), AppError> {
        if Self::is_brand_new_database(conn)? {
            return Self::in_savepoint(conn, "create_schema", || Self::create_fresh_schema(conn));
        }
        Self::create_request_logs_dedup_triggers_if_supported(conn)?;
        Ok(())
    }

    fn is_brand_new_database(conn: &Connection) -> Result<bool, AppError> {
        Ok(Self::get_user_version(conn)? == 0 && !Self::has_user_tables(conn)?)
    }

    fn create_fresh_schema(conn: &Connection) -> Result<(), AppError> {
        Self::create_baseline_schema(conn)?;
        Self::seed_default_proxy_config_rows(conn)?;
        Self::ensure_migration_ledger(conn)?;
        for migration in MIGRATIONS
            .iter()
            .filter(|m| m.version <= BASELINE_SCHEMA_VERSION)
        {
            Self::record_migration(conn, migration, LedgerSource::Baseline)?;
        }
        Self::set_user_version(conn, BASELINE_SCHEMA_VERSION)?;
        Self::run_pending_migrations(conn, BASELINE_SCHEMA_VERSION)
    }

    /// 执行冻结的基线 DDL（全部 `IF NOT EXISTS`）。旧表缺少所需列时跳过对应的索引/触发器，
    /// 由后续迁移补齐列后再创建。
    fn create_baseline_schema(conn: &Connection) -> Result<(), AppError> {
        for sql in BASELINE_TABLE_SQL {
            conn.execute(sql, []).map_err(db_err("创建表失败"))?;
        }

        // (provider_id, app_type, created_at) 取代旧的 (provider_id, app_type) 索引，
        // 让按 provider 的时间范围查询（额度检查）可以直接界定扫描范围。
        // 旧索引与无人使用的 idx_request_logs_model 在 v15→v16 迁移中删除。
        Self::create_request_logs_provider_index_if_supported(conn)?;
        for (index, column) in [
            ("idx_request_logs_created_at", "created_at"),
            ("idx_request_logs_session", "session_id"),
            ("idx_request_logs_status", "status_code"),
        ] {
            if Self::has_column(conn, "proxy_request_logs", column)? {
                conn.execute(
                    &format!("CREATE INDEX IF NOT EXISTS {index} ON proxy_request_logs({column})"),
                    [],
                )
                .map_err(|e| AppError::Database(format!("创建索引 {index} 失败: {e}")))?;
            }
        }
        Self::create_request_logs_usage_indexes_if_supported(conn)?;
        Self::create_request_logs_dedup_triggers_if_supported(conn)?;

        if Self::has_columns(
            conn,
            "stream_check_logs",
            &["app_type", "provider_id", "tested_at"],
        )? {
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_stream_check_logs_provider
                 ON stream_check_logs(app_type, provider_id, tested_at DESC)",
                [],
            )
            .map_err(db_err("创建 stream_check_logs 索引失败"))?;
        }

        Self::create_providers_failover_index_if_supported(conn)
    }

    /// 为故障转移队列创建索引（基于 providers 表）；旧表缺列时跳过。
    fn create_providers_failover_index_if_supported(conn: &Connection) -> Result<(), AppError> {
        if Self::has_columns(
            conn,
            "providers",
            &["app_type", "in_failover_queue", "sort_index"],
        )? {
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_providers_failover
                 ON providers(app_type, in_failover_queue, sort_index)",
                [],
            )
            .map_err(db_err("创建 failover 索引失败"))?;
        }
        Ok(())
    }

    /// 补齐每个应用的默认代理配置行（`INSERT OR IGNORE`，不覆盖已有行）。
    /// 旧版单例结构（无 app_type 列）由迁移转换后再补。
    fn seed_default_proxy_config_rows(conn: &Connection) -> Result<(), AppError> {
        if !Self::has_column(conn, "proxy_config", "app_type")? {
            return Ok(());
        }
        for (
            app,
            retries,
            first_byte,
            idle,
            non_streaming,
            cb_fail,
            cb_ok,
            cb_timeout,
            cb_rate,
            cb_min,
        ) in DEFAULT_PROXY_CONFIG_ROWS
        {
            conn.execute(
                "INSERT OR IGNORE INTO proxy_config (app_type, max_retries,
                streaming_first_byte_timeout, streaming_idle_timeout, non_streaming_timeout,
                circuit_failure_threshold, circuit_success_threshold, circuit_timeout_seconds,
                circuit_error_rate_threshold, circuit_min_requests)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    app,
                    retries,
                    first_byte,
                    idle,
                    non_streaming,
                    cb_fail,
                    cb_ok,
                    cb_timeout,
                    cb_rate,
                    cb_min
                ],
            )
            .map_err(|e| AppError::Database(format!("初始化 {app} 代理配置失败: {e}")))?;
        }
        Ok(())
    }

    /// 低于 [`BASELINE_SCHEMA_VERSION`] 的数据库在迁移链之前执行一次的兼容预处理。
    ///
    /// v17 之前，`create_tables_on_conn` 在每次启动、导入和恢复时都会执行这段逻辑，
    /// 历史迁移依赖它先补齐缺失的表（以当时的完整结构）和 proxy_config 的早期列。
    /// 这里保持相同的效果（只是不再吞掉 DDL 错误、ALTER 先检查列），使任意历史版本的
    /// 数据库升级结果不变；其余启动修复（默认行、旧 failover_queue 等）在 v17 迁移中执行。
    /// 基线版本及之后的数据库不再执行本函数。
    fn prepare_pre_baseline_database(conn: &Connection) -> Result<(), AppError> {
        Self::create_baseline_schema(conn)?;

        // 补齐 proxy_config 的早期列（兼容 v3.9.0-2 升级）
        if Self::table_exists(conn, "proxy_config")? {
            for (column, definition) in LEGACY_PROXY_CONFIG_COLUMNS {
                Self::add_column_if_missing(conn, "proxy_config", column, definition)?;
            }
        }

        // 兼容：若旧版 proxy_config 仍为单例结构（无 app_type），先转换为每应用一行。
        // 说明：测试版 user_version=2 的库不会再触发 v1->v2 迁移，但后续迁移依赖 app_type 列。
        Self::migrate_proxy_config_to_per_app(conn)
    }

    /// 应用 Schema 迁移
    pub(crate) fn apply_schema_migrations(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        Self::apply_schema_migrations_on_conn(&conn)
    }

    /// 在指定连接上应用 Schema 迁移：把数据库从当前 `user_version` 按注册表前进到
    /// `SCHEMA_VERSION`。整个过程在一个 savepoint 内，任何一步失败都会完整回滚。
    pub(crate) fn apply_schema_migrations_on_conn(conn: &Connection) -> Result<(), AppError> {
        let version = Self::get_user_version(conn)?;

        // 在执行任何 DDL 之前拒绝更新版本的数据库
        if version > SCHEMA_VERSION {
            return Err(AppError::Database(format!(
                "数据库版本过新（{version}），当前应用仅支持 {SCHEMA_VERSION}，请升级应用后再尝试。"
            )));
        }
        if version < 0 {
            return Err(AppError::Database(format!(
                "未知的数据库版本 {version}，无法迁移到 {SCHEMA_VERSION}"
            )));
        }

        Self::in_savepoint(conn, "schema_migration", || {
            if version < BASELINE_SCHEMA_VERSION {
                Self::prepare_pre_baseline_database(conn)?;
            }
            Self::ensure_migration_ledger(conn)?;
            // 台账出现之前（或数据库由 SQL 导入重建之前）已应用的版本：回填记录
            for migration in MIGRATIONS.iter().filter(|m| m.version <= version) {
                Self::record_migration_if_missing(conn, migration, LedgerSource::Backfilled)?;
            }
            Self::run_pending_migrations(conn, version)?;
            Self::warn_on_ledger_mismatch(conn);
            Ok(())
        })
    }

    /// 依次执行版本大于 `from` 的迁移，每一步更新 `user_version` 并写入台账。
    fn run_pending_migrations(conn: &Connection, from: i32) -> Result<(), AppError> {
        for migration in MIGRATIONS.iter().filter(|m| m.version > from) {
            log::info!(
                "迁移数据库从 v{} 到 v{}（{}）",
                migration.version - 1,
                migration.version,
                migration.name
            );
            (migration.up)(conn)?;
            Self::set_user_version(conn, migration.version)?;
            Self::record_migration(conn, migration, LedgerSource::Migrated)?;
        }
        Ok(())
    }

    /// 在 savepoint 中执行 `f`：成功则提交，失败则回滚并返回原错误。
    fn in_savepoint(
        conn: &Connection,
        name: &str,
        f: impl FnOnce() -> Result<(), AppError>,
    ) -> Result<(), AppError> {
        conn.execute(&format!("SAVEPOINT {name};"), [])
            .map_err(|e| AppError::Database(format!("开启 savepoint {name} 失败: {e}")))?;
        match f() {
            Ok(()) => {
                conn.execute(&format!("RELEASE {name};"), [])
                    .map_err(|e| AppError::Database(format!("提交 savepoint {name} 失败: {e}")))?;
                Ok(())
            }
            Err(e) => {
                conn.execute(&format!("ROLLBACK TO {name};"), []).ok();
                conn.execute(&format!("RELEASE {name};"), []).ok();
                Err(e)
            }
        }
    }

    // --- 迁移台账 ---

    fn ensure_migration_ledger(conn: &Connection) -> Result<(), AppError> {
        conn.execute(
            &format!(
                "CREATE TABLE IF NOT EXISTS {MIGRATION_LEDGER_TABLE} (
                    version INTEGER PRIMARY KEY,
                    name TEXT NOT NULL,
                    checksum TEXT NOT NULL,
                    app_version TEXT NOT NULL,
                    applied_at INTEGER NOT NULL,
                    source TEXT NOT NULL
                )"
            ),
            [],
        )
        .map_err(db_err("创建迁移台账失败"))?;
        Ok(())
    }

    fn record_migration(
        conn: &Connection,
        migration: &Migration,
        source: LedgerSource,
    ) -> Result<(), AppError> {
        conn.execute(
            &format!(
                "INSERT OR REPLACE INTO {MIGRATION_LEDGER_TABLE}
                    (version, name, checksum, app_version, applied_at, source)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
            ),
            params![
                migration.version,
                migration.name,
                migration.checksum,
                env!("CARGO_PKG_VERSION"),
                chrono::Utc::now().timestamp(),
                source.as_str()
            ],
        )
        .map_err(db_err("写入迁移台账失败"))?;
        Ok(())
    }

    fn record_migration_if_missing(
        conn: &Connection,
        migration: &Migration,
        source: LedgerSource,
    ) -> Result<(), AppError> {
        let exists: bool = conn
            .query_row(
                &format!(
                    "SELECT EXISTS(SELECT 1 FROM {MIGRATION_LEDGER_TABLE} WHERE version = ?1)"
                ),
                [migration.version],
                |row| row.get(0),
            )
            .map_err(db_err("读取迁移台账失败"))?;
        if exists {
            return Ok(());
        }
        Self::record_migration(conn, migration, source)
    }

    /// 台账中的名称/校验和与注册表不一致时记录警告（用于诊断，不阻止启动）。
    fn warn_on_ledger_mismatch(conn: &Connection) {
        let rows = match Self::read_migration_ledger(conn) {
            Ok(rows) => rows,
            Err(e) => {
                log::warn!("读取迁移台账失败: {e}");
                return;
            }
        };
        for (version, name, checksum) in rows {
            match MIGRATIONS.iter().find(|m| m.version == version) {
                Some(m) if m.name == name && m.checksum == checksum => {}
                Some(m) => log::warn!(
                    "迁移台账 v{version} ({name}, {checksum}) 与当前注册的迁移 ({}, {}) 不一致",
                    m.name,
                    m.checksum
                ),
                None => log::warn!("迁移台账包含未注册的版本 v{version} ({name})"),
            }
        }
    }

    /// 读取迁移台账：(version, name, checksum)，按版本排序。
    pub(crate) fn read_migration_ledger(
        conn: &Connection,
    ) -> Result<Vec<(i32, String, String)>, AppError> {
        if !Self::table_exists(conn, MIGRATION_LEDGER_TABLE)? {
            return Ok(Vec::new());
        }
        let mut stmt = conn
            .prepare(&format!(
                "SELECT version, name, checksum FROM {MIGRATION_LEDGER_TABLE} ORDER BY version"
            ))
            .map_err(db_err("读取迁移台账失败"))?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(db_err("读取迁移台账失败"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err("解析迁移台账失败"))?;
        Ok(rows)
    }

    /// v0 -> v1 迁移：补齐所有缺失列
    fn migrate_v0_to_v1(conn: &Connection) -> Result<(), AppError> {
        // providers 表
        Self::add_column_if_missing(conn, "providers", "category", "TEXT")?;
        Self::add_column_if_missing(conn, "providers", "created_at", "INTEGER")?;
        Self::add_column_if_missing(conn, "providers", "sort_index", "INTEGER")?;
        Self::add_column_if_missing(conn, "providers", "notes", "TEXT")?;
        Self::add_column_if_missing(conn, "providers", "icon", "TEXT")?;
        Self::add_column_if_missing(conn, "providers", "icon_color", "TEXT")?;
        Self::add_column_if_missing(conn, "providers", "meta", "TEXT NOT NULL DEFAULT '{}'")?;
        Self::add_column_if_missing(
            conn,
            "providers",
            "is_current",
            "BOOLEAN NOT NULL DEFAULT 0",
        )?;

        // provider_endpoints 表
        Self::add_column_if_missing(conn, "provider_endpoints", "added_at", "INTEGER")?;

        // mcp_servers 表
        Self::add_column_if_missing(conn, "mcp_servers", "description", "TEXT")?;
        Self::add_column_if_missing(conn, "mcp_servers", "homepage", "TEXT")?;
        Self::add_column_if_missing(conn, "mcp_servers", "docs", "TEXT")?;
        Self::add_column_if_missing(conn, "mcp_servers", "tags", "TEXT NOT NULL DEFAULT '[]'")?;
        Self::add_column_if_missing(
            conn,
            "mcp_servers",
            "enabled_codex",
            "BOOLEAN NOT NULL DEFAULT 0",
        )?;
        Self::add_column_if_missing(
            conn,
            "mcp_servers",
            "enabled_gemini",
            "BOOLEAN NOT NULL DEFAULT 0",
        )?;

        // prompts 表
        Self::add_column_if_missing(conn, "prompts", "description", "TEXT")?;
        Self::add_column_if_missing(conn, "prompts", "enabled", "BOOLEAN NOT NULL DEFAULT 1")?;
        Self::add_column_if_missing(conn, "prompts", "created_at", "INTEGER")?;
        Self::add_column_if_missing(conn, "prompts", "updated_at", "INTEGER")?;

        // skills 表
        Self::add_column_if_missing(conn, "skills", "installed_at", "INTEGER NOT NULL DEFAULT 0")?;

        // skill_repos 表
        Self::add_column_if_missing(
            conn,
            "skill_repos",
            "branch",
            "TEXT NOT NULL DEFAULT 'main'",
        )?;
        Self::add_column_if_missing(conn, "skill_repos", "enabled", "BOOLEAN NOT NULL DEFAULT 1")?;
        // 注意: skills_path 字段已被移除，因为现在支持全仓库递归扫描

        Ok(())
    }

    /// v1 -> v2 迁移：添加使用统计表和完整字段，重构 skills 表
    fn migrate_v1_to_v2(conn: &Connection) -> Result<(), AppError> {
        // providers 表字段
        Self::add_column_if_missing(
            conn,
            "providers",
            "cost_multiplier",
            "TEXT NOT NULL DEFAULT '1.0'",
        )?;
        Self::add_column_if_missing(conn, "providers", "limit_daily_usd", "TEXT")?;
        Self::add_column_if_missing(conn, "providers", "limit_monthly_usd", "TEXT")?;
        Self::add_column_if_missing(conn, "providers", "provider_type", "TEXT")?;
        Self::add_column_if_missing(
            conn,
            "providers",
            "in_failover_queue",
            "BOOLEAN NOT NULL DEFAULT 0",
        )?;

        // 添加代理超时配置字段
        if Self::table_exists(conn, "proxy_config")? {
            // 兼容旧版本缺失的基础字段
            Self::add_column_if_missing(
                conn,
                "proxy_config",
                "proxy_enabled",
                "INTEGER NOT NULL DEFAULT 0",
            )?;
            Self::add_column_if_missing(
                conn,
                "proxy_config",
                "listen_address",
                "TEXT NOT NULL DEFAULT '127.0.0.1'",
            )?;
            Self::add_column_if_missing(
                conn,
                "proxy_config",
                "listen_port",
                "INTEGER NOT NULL DEFAULT 15721",
            )?;
            Self::add_column_if_missing(
                conn,
                "proxy_config",
                "enable_logging",
                "INTEGER NOT NULL DEFAULT 1",
            )?;

            Self::add_column_if_missing(
                conn,
                "proxy_config",
                "streaming_first_byte_timeout",
                "INTEGER NOT NULL DEFAULT 60",
            )?;
            Self::add_column_if_missing(
                conn,
                "proxy_config",
                "streaming_idle_timeout",
                "INTEGER NOT NULL DEFAULT 120",
            )?;
            Self::add_column_if_missing(
                conn,
                "proxy_config",
                "non_streaming_timeout",
                "INTEGER NOT NULL DEFAULT 600",
            )?;
        }

        // 删除旧的 failover_queue 表（如果存在）
        conn.execute("DROP INDEX IF EXISTS idx_failover_queue_order", [])
            .map_err(|e| AppError::Database(format!("删除 failover_queue 索引失败: {e}")))?;
        conn.execute("DROP TABLE IF EXISTS failover_queue", [])
            .map_err(|e| AppError::Database(format!("删除 failover_queue 表失败: {e}")))?;

        // 创建 failover 索引
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_providers_failover
             ON providers(app_type, in_failover_queue, sort_index)",
            [],
        )
        .map_err(|e| AppError::Database(format!("创建 failover 索引失败: {e}")))?;

        // proxy_request_logs 表
        conn.execute("CREATE TABLE IF NOT EXISTS proxy_request_logs (
            request_id TEXT PRIMARY KEY, provider_id TEXT NOT NULL, app_type TEXT NOT NULL, model TEXT NOT NULL,
            request_model TEXT,
            input_tokens INTEGER NOT NULL DEFAULT 0, output_tokens INTEGER NOT NULL DEFAULT 0,
            cache_read_tokens INTEGER NOT NULL DEFAULT 0, cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
            input_token_semantics INTEGER NOT NULL DEFAULT 0,
            input_cost_usd TEXT NOT NULL DEFAULT '0', output_cost_usd TEXT NOT NULL DEFAULT '0',
            cache_read_cost_usd TEXT NOT NULL DEFAULT '0', cache_creation_cost_usd TEXT NOT NULL DEFAULT '0',
            total_cost_usd TEXT NOT NULL DEFAULT '0', latency_ms INTEGER NOT NULL, first_token_ms INTEGER,
            duration_ms INTEGER, status_code INTEGER NOT NULL, error_message TEXT, session_id TEXT,
            provider_type TEXT, is_streaming INTEGER NOT NULL DEFAULT 0,
            cost_multiplier TEXT NOT NULL DEFAULT '1.0', created_at INTEGER NOT NULL
        )", [])?;

        // 为已存在的表添加新字段
        Self::add_column_if_missing(conn, "proxy_request_logs", "provider_type", "TEXT")?;
        Self::add_column_if_missing(
            conn,
            "proxy_request_logs",
            "is_streaming",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        Self::add_column_if_missing(
            conn,
            "proxy_request_logs",
            "cost_multiplier",
            "TEXT NOT NULL DEFAULT '1.0'",
        )?;
        Self::add_column_if_missing(conn, "proxy_request_logs", "first_token_ms", "INTEGER")?;
        Self::add_column_if_missing(conn, "proxy_request_logs", "duration_ms", "INTEGER")?;

        // model_pricing 表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS model_pricing (
            model_id TEXT PRIMARY KEY, display_name TEXT NOT NULL,
            input_cost_per_million TEXT NOT NULL, output_cost_per_million TEXT NOT NULL,
            cache_read_cost_per_million TEXT NOT NULL DEFAULT '0',
            cache_creation_cost_per_million TEXT NOT NULL DEFAULT '0'
        )",
            [],
        )?;

        // 清空并重新插入模型定价
        conn.execute("DELETE FROM model_pricing", [])
            .map_err(|e| AppError::Database(format!("清空模型定价失败: {e}")))?;
        Self::seed_model_pricing(conn)?;

        // 重构 skills 表（添加 app_type 字段）
        Self::migrate_skills_table(conn)?;

        // 重构 proxy_config 为三行结构（每应用独立配置）
        Self::migrate_proxy_config_to_per_app(conn)?;

        Ok(())
    }

    /// 将 proxy_config 迁移为三行结构（每应用独立配置）
    fn migrate_proxy_config_to_per_app(conn: &Connection) -> Result<(), AppError> {
        // 检查是否已经是新表结构（幂等性）
        if !Self::table_exists(conn, "proxy_config")? {
            // 表不存在，跳过迁移（新安装）
            return Ok(());
        }

        if Self::has_column(conn, "proxy_config", "app_type")? {
            // 已经是三行结构，跳过迁移
            log::info!("proxy_config 已经是三行结构，跳过迁移");
            return Ok(());
        }

        // 读取旧配置
        let old_config = conn
            .query_row(
                "SELECT listen_address, listen_port, max_retries, enable_logging,
                    streaming_first_byte_timeout, streaming_idle_timeout, non_streaming_timeout
             FROM proxy_config WHERE id = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i32>(1)?,
                        row.get::<_, i32>(2)?,
                        row.get::<_, i32>(3)?,
                        row.get::<_, i32>(4).unwrap_or(30),
                        row.get::<_, i32>(5).unwrap_or(60),
                        row.get::<_, i32>(6).unwrap_or(300),
                    ))
                },
            )
            .unwrap_or_else(|_| ("127.0.0.1".to_string(), 5000, 3, 1, 30, 60, 300));

        let old_cb = conn.query_row(
            "SELECT failure_threshold, success_threshold, timeout_seconds, error_rate_threshold, min_requests
             FROM circuit_breaker_config WHERE id = 1", [],
            |row| Ok((row.get::<_, i32>(0)?, row.get::<_, i32>(1)?, row.get::<_, i64>(2)?,
                      row.get::<_, f64>(3)?, row.get::<_, i32>(4)?))
        ).unwrap_or((5, 2, 60, 0.5, 10));

        let get_bool = |key: &str| -> bool {
            conn.query_row("SELECT value FROM settings WHERE key = ?", [key], |r| {
                r.get::<_, String>(0)
            })
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false)
        };

        let apps = [
            (
                "claude",
                get_bool("proxy_takeover_claude"),
                get_bool("auto_failover_enabled_claude"),
                6,
                45,
                90,
                8,
                3,
                90,
                0.6,
                15,
            ),
            (
                "codex",
                get_bool("proxy_takeover_codex"),
                get_bool("auto_failover_enabled_codex"),
                3,
                old_config.4,
                old_config.5,
                old_cb.0,
                old_cb.1,
                old_cb.2,
                old_cb.3,
                old_cb.4,
            ),
            (
                "gemini",
                get_bool("proxy_takeover_gemini"),
                get_bool("auto_failover_enabled_gemini"),
                5,
                old_config.4,
                old_config.5,
                old_cb.0,
                old_cb.1,
                old_cb.2,
                old_cb.3,
                old_cb.4,
            ),
            (
                "grokbuild",
                false,
                false,
                3,
                old_config.4,
                old_config.5,
                old_cb.0,
                old_cb.1,
                old_cb.2,
                old_cb.3,
                old_cb.4,
            ),
        ];

        // 创建新表
        conn.execute("DROP TABLE IF EXISTS proxy_config_new", [])?;
        conn.execute("CREATE TABLE proxy_config_new (
            app_type TEXT PRIMARY KEY CHECK (app_type IN ('claude','codex','gemini','grokbuild')),
            proxy_enabled INTEGER NOT NULL DEFAULT 0, listen_address TEXT NOT NULL DEFAULT '127.0.0.1',
            listen_port INTEGER NOT NULL DEFAULT 15721, enable_logging INTEGER NOT NULL DEFAULT 1,
            enabled INTEGER NOT NULL DEFAULT 0, auto_failover_enabled INTEGER NOT NULL DEFAULT 0,
            max_retries INTEGER NOT NULL DEFAULT 3, streaming_first_byte_timeout INTEGER NOT NULL DEFAULT 60,
            streaming_idle_timeout INTEGER NOT NULL DEFAULT 120, non_streaming_timeout INTEGER NOT NULL DEFAULT 600,
            circuit_failure_threshold INTEGER NOT NULL DEFAULT 4, circuit_success_threshold INTEGER NOT NULL DEFAULT 2,
            circuit_timeout_seconds INTEGER NOT NULL DEFAULT 60, circuit_error_rate_threshold REAL NOT NULL DEFAULT 0.6,
            circuit_min_requests INTEGER NOT NULL DEFAULT 10,
            default_cost_multiplier TEXT NOT NULL DEFAULT '1',
            pricing_model_source TEXT NOT NULL DEFAULT 'response',
            live_takeover_active INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now')), updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        )", [])?;

        // 插入三行配置
        for (app, takeover, failover, retries, fb, idle, cb_f, cb_s, cb_t, cb_r, cb_m) in apps {
            conn.execute(
                "INSERT INTO proxy_config_new (app_type, proxy_enabled, listen_address, listen_port, enable_logging,
                 enabled, auto_failover_enabled, max_retries, streaming_first_byte_timeout, streaming_idle_timeout,
                 non_streaming_timeout, circuit_failure_threshold, circuit_success_threshold, circuit_timeout_seconds,
                 circuit_error_rate_threshold, circuit_min_requests)
                 VALUES (?1, 0, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                rusqlite::params![app, old_config.0, old_config.1, old_config.3,
                    if takeover { 1 } else { 0 }, if failover { 1 } else { 0 },
                    retries, fb, idle, old_config.6, cb_f, cb_s, cb_t, cb_r, cb_m]
            ).map_err(|e| AppError::Database(format!("插入 {app} 配置失败: {e}")))?;
        }

        // 替换表并清理
        conn.execute("DROP TABLE IF EXISTS proxy_config", [])?;
        conn.execute("ALTER TABLE proxy_config_new RENAME TO proxy_config", [])?;
        conn.execute("DROP TABLE IF EXISTS circuit_breaker_config", [])?;
        conn.execute("DELETE FROM settings WHERE key LIKE 'proxy_takeover_%'", [])?;
        conn.execute(
            "DELETE FROM settings WHERE key LIKE 'auto_failover_enabled_%'",
            [],
        )?;

        log::info!("proxy_config 已迁移为三行结构");
        Ok(())
    }

    /// 迁移 skills 表：从单 key 主键改为 (directory, app_type) 复合主键
    fn migrate_skills_table(conn: &Connection) -> Result<(), AppError> {
        // v3 结构（统一管理架构）已经是更高版本的 skills 表：
        // - 主键为 id
        // - 包含 enabled_claude / enabled_codex / enabled_gemini 等列
        // 在这种情况下，不应再执行 v1 -> v2 的迁移逻辑，否则会因列不匹配而失败。
        if Self::has_column(conn, "skills", "enabled_claude")?
            || Self::has_column(conn, "skills", "id")?
        {
            log::info!("skills 表已经是 v3 结构，跳过 v1 -> v2 迁移");
            return Ok(());
        }

        // 检查是否已经是新表结构
        if Self::has_column(conn, "skills", "app_type")? {
            log::info!("skills 表已经包含 app_type 字段，跳过迁移");
            return Ok(());
        }

        log::info!("开始迁移 skills 表...");

        // 1. 重命名旧表
        conn.execute("ALTER TABLE skills RENAME TO skills_old", [])
            .map_err(|e| AppError::Database(format!("重命名旧 skills 表失败: {e}")))?;

        // 2. 创建新表
        conn.execute(
            "CREATE TABLE skills (
                directory TEXT NOT NULL,
                app_type TEXT NOT NULL,
                installed BOOLEAN NOT NULL DEFAULT 0,
                installed_at INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (directory, app_type)
            )",
            [],
        )
        .map_err(|e| AppError::Database(format!("创建新 skills 表失败: {e}")))?;

        // 3. 迁移数据：解析 key 格式（如 "claude:my-skill" 或 "codex:foo"）
        //    旧数据如果没有前缀，默认为 claude
        let mut stmt = conn
            .prepare("SELECT key, installed, installed_at FROM skills_old")
            .map_err(|e| AppError::Database(format!("查询旧 skills 数据失败: {e}")))?;

        let old_skills: Vec<(String, bool, i64)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, bool>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .map_err(|e| AppError::Database(format!("读取旧 skills 数据失败: {e}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(format!("解析旧 skills 数据失败: {e}")))?;

        let count = old_skills.len();

        for (key, installed, installed_at) in old_skills {
            // 解析 key: "app:directory" 或 "directory"（默认 claude）
            let (app_type, directory) = if let Some(idx) = key.find(':') {
                let (app, dir) = key.split_at(idx);
                (app.to_string(), dir[1..].to_string()) // 跳过冒号
            } else {
                ("claude".to_string(), key.clone())
            };

            conn.execute(
                "INSERT INTO skills (directory, app_type, installed, installed_at) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![directory, app_type, installed, installed_at],
            )
            .map_err(|e| {
                AppError::Database(format!("迁移 skill {key} 到新表失败: {e}"))
            })?;
        }

        // 4. 删除旧表
        conn.execute("DROP TABLE skills_old", [])
            .map_err(|e| AppError::Database(format!("删除旧 skills 表失败: {e}")))?;

        log::info!("skills 表迁移完成，共迁移 {count} 条记录");
        Ok(())
    }

    /// v2 -> v3 迁移：Skills 统一管理架构
    ///
    /// 将 skills 表从 (directory, app_type) 复合主键结构迁移到统一的 id 主键结构，
    /// 支持三应用启用标志（enabled_claude, enabled_codex, enabled_gemini）。
    ///
    /// 迁移策略：
    /// 1. 旧数据库只存储安装记录，真正的 skill 文件在文件系统
    /// 2. 直接重建新表结构，后续由 SkillService 在首次启动时扫描文件系统重建数据
    fn migrate_v2_to_v3(conn: &Connection) -> Result<(), AppError> {
        // 检查是否已经是新结构（通过检查是否有 enabled_claude 列）
        if Self::has_column(conn, "skills", "enabled_claude")? {
            log::info!("skills 表已经是 v3 结构，跳过迁移");
            return Ok(());
        }

        log::info!("开始迁移 skills 表到 v3 结构（统一管理架构）...");

        // 1. 备份旧数据（用于日志和后续启动迁移）
        let old_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM skills", [], |row| row.get(0))
            .unwrap_or(0);
        log::info!("旧 skills 表有 {old_count} 条记录");

        let mut stmt = conn
            .prepare(
                "SELECT directory, app_type FROM skills
                 WHERE installed = 1",
            )
            .map_err(|e| AppError::Database(format!("查询旧 skills 快照失败: {e}")))?;
        let snapshot_rows: Vec<LegacySkillMigrationRow> = stmt
            .query_map([], |row| {
                Ok(LegacySkillMigrationRow {
                    directory: row.get(0)?,
                    app_type: row.get(1)?,
                })
            })
            .map_err(|e| AppError::Database(format!("读取旧 skills 快照失败: {e}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(format!("解析旧 skills 快照失败: {e}")))?;
        let snapshot_json = serde_json::to_string(&snapshot_rows)
            .map_err(|e| AppError::Database(format!("序列化旧 skills 快照失败: {e}")))?;

        // 标记：需要在启动后从文件系统扫描并重建 Skills 数据
        // 说明：v3 结构将 Skills 的 SSOT 迁移到 ~/.cc-switch/skills/，
        // 旧表只存“安装记录”，无法直接无损迁移到新结构，因此改为启动后扫描 app 目录导入。
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('skills_ssot_migration_pending', 'true')",
            [],
        )
        .map_err(|e| AppError::Database(format!("写入 skills 迁移标记失败: {e}")))?;
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('skills_ssot_migration_snapshot', ?1)",
            [snapshot_json],
        )
        .map_err(|e| AppError::Database(format!("写入 skills 迁移快照失败: {e}")))?;

        // 2. 删除旧表
        conn.execute("DROP TABLE IF EXISTS skills", [])
            .map_err(|e| AppError::Database(format!("删除旧 skills 表失败: {e}")))?;

        // 3. 创建新表
        conn.execute(
            "CREATE TABLE skills (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                directory TEXT NOT NULL,
                repo_owner TEXT,
                repo_name TEXT,
                repo_branch TEXT DEFAULT 'main',
                readme_url TEXT,
                enabled_claude BOOLEAN NOT NULL DEFAULT 0,
                enabled_codex BOOLEAN NOT NULL DEFAULT 0,
                enabled_gemini BOOLEAN NOT NULL DEFAULT 0,
                installed_at INTEGER NOT NULL DEFAULT 0
            )",
            [],
        )
        .map_err(|e| AppError::Database(format!("创建新 skills 表失败: {e}")))?;

        log::info!(
            "skills 表已迁移到 v3 结构。\n\
             注意：旧的安装记录已清除，首次启动时将自动扫描文件系统重建数据。"
        );

        Ok(())
    }

    /// v3 -> v4 迁移：添加 OpenCode 支持
    ///
    /// 为 mcp_servers 和 skills 表添加 enabled_opencode 列。
    fn migrate_v3_to_v4(conn: &Connection) -> Result<(), AppError> {
        // 为 mcp_servers 表添加 enabled_opencode 列
        Self::add_column_if_missing(
            conn,
            "mcp_servers",
            "enabled_opencode",
            "BOOLEAN NOT NULL DEFAULT 0",
        )?;

        // 为 skills 表添加 enabled_opencode 列
        Self::add_column_if_missing(
            conn,
            "skills",
            "enabled_opencode",
            "BOOLEAN NOT NULL DEFAULT 0",
        )?;

        log::info!("v3 -> v4 迁移完成：已添加 OpenCode 支持");
        Ok(())
    }

    /// v4 -> v5 迁移：新增计费模式配置与请求模型字段
    fn migrate_v4_to_v5(conn: &Connection) -> Result<(), AppError> {
        if Self::table_exists(conn, "proxy_config")? {
            Self::add_column_if_missing(
                conn,
                "proxy_config",
                "default_cost_multiplier",
                "TEXT NOT NULL DEFAULT '1'",
            )?;
            Self::add_column_if_missing(
                conn,
                "proxy_config",
                "pricing_model_source",
                "TEXT NOT NULL DEFAULT 'response'",
            )?;
        }
        if Self::table_exists(conn, "proxy_request_logs")? {
            Self::add_column_if_missing(conn, "proxy_request_logs", "request_model", "TEXT")?;
        }

        log::info!("v4 -> v5 迁移完成：已添加计费模式与请求模型字段");
        Ok(())
    }

    /// v5 -> v6 迁移：添加使用量日聚合表 + 统一 Copilot 模板类型
    fn migrate_v5_to_v6(conn: &Connection) -> Result<(), AppError> {
        // 1. 添加使用量日聚合表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS usage_daily_rollups (
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
            )",
            [],
        )
        .map_err(|e| AppError::Database(format!("创建 usage_daily_rollups 表失败: {e}")))?;

        // 2. 统一 Copilot 模板类型为 github_copilot
        let mut stmt = conn
            .prepare("SELECT id, app_type, meta FROM providers")
            .map_err(|e| AppError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut updates = Vec::new();
        for row in rows {
            let (id, app_type, meta_str) = row.map_err(|e| AppError::Database(e.to_string()))?;

            if let Ok(mut meta) = serde_json::from_str::<serde_json::Value>(&meta_str) {
                let mut updated = false;

                if let Some(usage_script) = meta.get_mut("usage_script") {
                    if let Some(template_type) = usage_script.get_mut("template_type") {
                        if template_type == "copilot" {
                            *template_type =
                                serde_json::Value::String("github_copilot".to_string());
                            updated = true;
                        }
                    }
                }

                if updated {
                    let new_meta_str = serde_json::to_string(&meta)
                        .map_err(|e| AppError::Database(e.to_string()))?;
                    updates.push((id, app_type, new_meta_str));
                }
            }
        }

        for (id, app_type, new_meta) in updates {
            conn.execute(
                "UPDATE providers SET meta = ?1 WHERE id = ?2 AND app_type = ?3",
                params![new_meta, id, app_type],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        }

        log::info!("v5 -> v6 迁移完成：已添加使用量日聚合表，统一 copilot 模板类型");
        Ok(())
    }

    /// v6 -> v7: Skills 更新检测支持（content_hash + updated_at）
    fn migrate_v6_to_v7(conn: &Connection) -> Result<(), AppError> {
        if Self::table_exists(conn, "skills")? {
            Self::add_column_if_missing(conn, "skills", "content_hash", "TEXT")?;
            Self::add_column_if_missing(
                conn,
                "skills",
                "updated_at",
                "INTEGER NOT NULL DEFAULT 0",
            )?;
        }
        log::info!("v6 -> v7 迁移完成：已添加 content_hash 和 updated_at 列");
        Ok(())
    }

    /// v7 -> v8: 会话日志使用追踪（无代理模式统计支持）
    fn migrate_v7_to_v8(conn: &Connection) -> Result<(), AppError> {
        // 1. 为 proxy_request_logs 添加 data_source 列，区分数据来源
        if Self::table_exists(conn, "proxy_request_logs")? {
            Self::add_column_if_missing(
                conn,
                "proxy_request_logs",
                "data_source",
                "TEXT NOT NULL DEFAULT 'proxy'",
            )?;
            Self::create_request_logs_usage_indexes_if_supported(conn)?;
        }

        // 2. 创建会话日志同步状态表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS session_log_sync (
                file_path TEXT PRIMARY KEY,
                last_modified INTEGER NOT NULL,
                last_line_offset INTEGER NOT NULL DEFAULT 0,
                last_synced_at INTEGER NOT NULL
            )",
            [],
        )
        .map_err(|e| AppError::Database(format!("创建 session_log_sync 表失败: {e}")))?;

        // 3. 修正国产模型定价：之前误将 CNY 值存为 USD 字段，统一转换为 USD
        if Self::table_exists(conn, "model_pricing")? {
            let pricing_fixes: &[(&str, &str, &str, &str, &str)] = &[
                ("deepseek-v3.2", "0.28", "0.42", "0.028", "0"),
                ("deepseek-v3.1", "0.55", "1.67", "0.055", "0"),
                ("deepseek-v3", "0.28", "1.11", "0.028", "0"),
                ("doubao-seed-code", "0.17", "1.11", "0.02", "0"),
                ("kimi-k2-thinking", "0.55", "2.20", "0.10", "0"),
                ("kimi-k2-0905", "0.55", "2.20", "0.10", "0"),
                ("kimi-k2-turbo", "1.11", "8.06", "0.14", "0"),
                ("minimax-m2.1", "0.27", "0.95", "0.03", "0"),
                ("minimax-m2.1-lightning", "0.27", "2.33", "0.03", "0"),
                ("minimax-m2", "0.27", "0.95", "0.03", "0"),
                ("glm-4.7", "0.39", "1.75", "0.04", "0"),
                ("glm-4.6", "0.28", "1.11", "0.03", "0"),
                ("mimo-v2-flash", "0.09", "0.29", "0.009", "0"),
            ];
            for (model_id, input, output, cache_read, cache_creation) in pricing_fixes {
                conn.execute(
                    "UPDATE model_pricing SET
                        input_cost_per_million = ?2,
                        output_cost_per_million = ?3,
                        cache_read_cost_per_million = ?4,
                        cache_creation_cost_per_million = ?5
                     WHERE model_id = ?1",
                    rusqlite::params![model_id, input, output, cache_read, cache_creation],
                )
                .map_err(|e| AppError::Database(format!("更新模型 {model_id} 定价失败: {e}")))?;
            }
        }

        log::info!("v7 -> v8 迁移完成：data_source 列、session_log_sync 表、修正 13 个模型定价");
        Ok(())
    }

    /// v8 → v9: 全面补充模型定价（清空 + 重新 seed）
    fn migrate_v8_to_v9(conn: &Connection) -> Result<(), AppError> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS model_pricing (
                model_id TEXT PRIMARY KEY, display_name TEXT NOT NULL,
                input_cost_per_million TEXT NOT NULL, output_cost_per_million TEXT NOT NULL,
                cache_read_cost_per_million TEXT NOT NULL DEFAULT '0',
                cache_creation_cost_per_million TEXT NOT NULL DEFAULT '0'
            )",
            [],
        )
        .map_err(|e| AppError::Database(format!("创建 model_pricing 表失败: {e}")))?;
        conn.execute("DELETE FROM model_pricing", [])
            .map_err(|e| AppError::Database(format!("清空模型定价失败: {e}")))?;
        Self::seed_model_pricing(conn)?;
        log::info!("v8 -> v9 迁移完成：已刷新全部模型定价数据");
        Ok(())
    }

    /// v9 -> v10 迁移：添加 Hermes Agent 支持
    fn migrate_v9_to_v10(conn: &Connection) -> Result<(), AppError> {
        Self::add_column_if_missing(
            conn,
            "mcp_servers",
            "enabled_hermes",
            "BOOLEAN NOT NULL DEFAULT 0",
        )?;

        // skills table may not exist in databases migrated from very old versions
        if Self::table_exists(conn, "skills")? {
            Self::add_column_if_missing(
                conn,
                "skills",
                "enabled_hermes",
                "BOOLEAN NOT NULL DEFAULT 0",
            )?;
        }

        log::info!("v9 -> v10 迁移完成：已添加 Hermes Agent 支持");
        Ok(())
    }

    /// v10 -> v11：usage_daily_rollups 增加 request_model 维度（进入主键），
    /// proxy_request_logs 增加 pricing_model 列（写入时的计价基准，回填依据）。
    ///
    /// 路由接管下 model（真实上游模型）≠ request_model（客户端别名），
    /// 旧 rollup 只按 model 聚合，明细 prune 后映射关系永久丢失、计费不可审计。
    /// SQLite 改主键必须重建表；历史行的 request_model 已不可知，填 ''。
    fn migrate_v10_to_v11(conn: &Connection) -> Result<(), AppError> {
        // proxy_request_logs.pricing_model：NULL = v11 前的历史行（回填走
        // model → 占位符回退 request_model 的旧逻辑），'' = 未计价的错误行
        if Self::table_exists(conn, "proxy_request_logs")? {
            Self::add_column_if_missing(conn, "proxy_request_logs", "pricing_model", "TEXT")?;
        }

        if !Self::table_exists(conn, "usage_daily_rollups")? {
            log::info!("v10 -> v11：usage_daily_rollups 不存在，跳过重建");
            return Ok(());
        }

        conn.execute_batch(
            "ALTER TABLE usage_daily_rollups RENAME TO usage_daily_rollups_v10;
             CREATE TABLE usage_daily_rollups (
                 date TEXT NOT NULL,
                 app_type TEXT NOT NULL,
                 provider_id TEXT NOT NULL,
                 model TEXT NOT NULL,
                 request_model TEXT NOT NULL DEFAULT '',
                 pricing_model TEXT NOT NULL DEFAULT '',
                 request_count INTEGER NOT NULL DEFAULT 0,
                 success_count INTEGER NOT NULL DEFAULT 0,
                 input_tokens INTEGER NOT NULL DEFAULT 0,
                 output_tokens INTEGER NOT NULL DEFAULT 0,
                 cache_read_tokens INTEGER NOT NULL DEFAULT 0,
                 cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
                 total_cost_usd TEXT NOT NULL DEFAULT '0',
                 avg_latency_ms INTEGER NOT NULL DEFAULT 0,
                 PRIMARY KEY (date, app_type, provider_id, model, request_model, pricing_model)
             );
             INSERT INTO usage_daily_rollups
                 (date, app_type, provider_id, model, request_model, pricing_model,
                  request_count, success_count, input_tokens, output_tokens,
                  cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms)
             SELECT date, app_type, provider_id, model, '', '',
                  request_count, success_count, input_tokens, output_tokens,
                  cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
             FROM usage_daily_rollups_v10;
             DROP TABLE usage_daily_rollups_v10;",
        )
        .map_err(|e| {
            AppError::Database(format!("v10 -> v11 重建 usage_daily_rollups 失败: {e}"))
        })?;

        log::info!(
            "v10 -> v11 迁移完成：usage_daily_rollups 已保留 request_model/pricing_model 维度"
        );
        Ok(())
    }

    /// v11 -> v12 迁移：添加项目 Profiles 表
    /// 与 create_tables_on_conn 中的建表语句保持一致（IF NOT EXISTS 保证幂等）
    fn migrate_v11_to_v12(conn: &Connection) -> Result<(), AppError> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS profiles (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                payload TEXT NOT NULL,
                sort_order INTEGER,
                created_at INTEGER,
                updated_at INTEGER
            )",
            [],
        )
        .map_err(|e| AppError::Database(format!("v11 -> v12 创建 profiles 表失败: {e}")))?;
        Ok(())
    }

    /// v12 -> v13：记录 input_tokens 是否包含缓存写入。
    ///
    /// 默认 0 表示旧版/未知语义；旧 Codex 行只包含 cache read，不包含
    /// cache creation。新代理行会显式写入 1(total-inclusive) 或 2(fresh)。
    fn migrate_v12_to_v13(conn: &Connection) -> Result<(), AppError> {
        if Self::table_exists(conn, "proxy_request_logs")? {
            Self::add_column_if_missing(
                conn,
                "proxy_request_logs",
                "input_token_semantics",
                "INTEGER NOT NULL DEFAULT 0",
            )?;
        }
        if Self::table_exists(conn, "usage_daily_rollups")? {
            Self::add_column_if_missing(
                conn,
                "usage_daily_rollups",
                "input_token_semantics",
                "INTEGER NOT NULL DEFAULT 0",
            )?;
        }
        Ok(())
    }

    /// v13 -> v14: allow Grok Build to own an independent proxy configuration row.
    fn migrate_v13_to_v14(conn: &Connection) -> Result<(), AppError> {
        if !Self::table_exists(conn, "proxy_config")? {
            return Ok(());
        }

        conn.execute("DROP TABLE IF EXISTS proxy_config_v14", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "CREATE TABLE proxy_config_v14 (
                app_type TEXT PRIMARY KEY CHECK (app_type IN ('claude','codex','gemini','grokbuild')),
                proxy_enabled INTEGER NOT NULL DEFAULT 0,
                listen_address TEXT NOT NULL DEFAULT '127.0.0.1',
                listen_port INTEGER NOT NULL DEFAULT 15721,
                enable_logging INTEGER NOT NULL DEFAULT 1,
                enabled INTEGER NOT NULL DEFAULT 0,
                auto_failover_enabled INTEGER NOT NULL DEFAULT 0,
                max_retries INTEGER NOT NULL DEFAULT 3,
                streaming_first_byte_timeout INTEGER NOT NULL DEFAULT 60,
                streaming_idle_timeout INTEGER NOT NULL DEFAULT 120,
                non_streaming_timeout INTEGER NOT NULL DEFAULT 600,
                circuit_failure_threshold INTEGER NOT NULL DEFAULT 4,
                circuit_success_threshold INTEGER NOT NULL DEFAULT 2,
                circuit_timeout_seconds INTEGER NOT NULL DEFAULT 60,
                circuit_error_rate_threshold REAL NOT NULL DEFAULT 0.6,
                circuit_min_requests INTEGER NOT NULL DEFAULT 10,
                default_cost_multiplier TEXT NOT NULL DEFAULT '1',
                pricing_model_source TEXT NOT NULL DEFAULT 'response',
                live_takeover_active INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        let copied_columns = [
            ("app_type", "'claude'"),
            ("proxy_enabled", "0"),
            ("listen_address", "'127.0.0.1'"),
            ("listen_port", "15721"),
            ("enable_logging", "1"),
            ("enabled", "0"),
            ("auto_failover_enabled", "0"),
            ("max_retries", "3"),
            ("streaming_first_byte_timeout", "60"),
            ("streaming_idle_timeout", "120"),
            ("non_streaming_timeout", "600"),
            ("circuit_failure_threshold", "4"),
            ("circuit_success_threshold", "2"),
            ("circuit_timeout_seconds", "60"),
            ("circuit_error_rate_threshold", "0.6"),
            ("circuit_min_requests", "10"),
            ("default_cost_multiplier", "'1'"),
            ("pricing_model_source", "'response'"),
            ("live_takeover_active", "0"),
            ("created_at", "datetime('now')"),
            ("updated_at", "datetime('now')"),
        ]
        .into_iter()
        .map(|(column, fallback)| {
            Self::has_column(conn, "proxy_config", column).map(|exists| {
                if exists {
                    format!("\"{column}\"")
                } else {
                    fallback.into()
                }
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?
        .join(", ");

        let copy_sql = format!(
            "INSERT INTO proxy_config_v14 (
                app_type, proxy_enabled, listen_address, listen_port, enable_logging,
                enabled, auto_failover_enabled, max_retries,
                streaming_first_byte_timeout, streaming_idle_timeout, non_streaming_timeout,
                circuit_failure_threshold, circuit_success_threshold, circuit_timeout_seconds,
                circuit_error_rate_threshold, circuit_min_requests,
                default_cost_multiplier, pricing_model_source, live_takeover_active,
                created_at, updated_at
            )
            SELECT {copied_columns} FROM proxy_config"
        );
        conn.execute(&copy_sql, [])
            .map_err(|e| AppError::Database(e.to_string()))?;

        conn.execute("DROP TABLE proxy_config", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute("ALTER TABLE proxy_config_v14 RENAME TO proxy_config", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "INSERT OR IGNORE INTO proxy_config (app_type) VALUES ('grokbuild')",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(())
    }

    /// v14 -> v15: persist Grok Build enablement for unified Skills and MCP.
    fn migrate_v14_to_v15(conn: &Connection) -> Result<(), AppError> {
        if Self::table_exists(conn, "mcp_servers")? {
            Self::add_column_if_missing(
                conn,
                "mcp_servers",
                "enabled_grokbuild",
                "BOOLEAN NOT NULL DEFAULT 0",
            )?;
        }
        if Self::table_exists(conn, "skills")? {
            Self::add_column_if_missing(
                conn,
                "skills",
                "enabled_grokbuild",
                "BOOLEAN NOT NULL DEFAULT 0",
            )?;
        }
        Ok(())
    }

    /// v15 -> v16: persist the cross-source dedup decision and trim usage-log indexes.
    ///
    /// - 新增 `proxy_request_logs.dedup_hidden`，由 AFTER INSERT 触发器维护，并在此
    ///   一次性回填，读侧聚合不再逐行执行相关子查询（PERF-08）；
    /// - `idx_request_logs_provider(provider_id, app_type)` 被
    ///   `idx_request_logs_provider_created_at` 取代；`idx_request_logs_model`
    ///   没有任何查询使用（模型筛选走 COALESCE 表达式），删除以减少写放大（DB-04）；
    /// - 新增零成本部分索引，成本回填只扫描候选行（DB-09）。
    fn migrate_v15_to_v16(conn: &Connection) -> Result<(), AppError> {
        if !Self::table_exists(conn, "proxy_request_logs")? {
            return Ok(());
        }
        Self::add_column_if_missing(
            conn,
            "proxy_request_logs",
            "dedup_hidden",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        for index in ["idx_request_logs_provider", "idx_request_logs_model"] {
            conn.execute(&format!("DROP INDEX IF EXISTS {index}"), [])
                .map_err(|e| AppError::Database(format!("删除索引 {index} 失败: {e}")))?;
        }
        Self::create_request_logs_provider_index_if_supported(conn)?;
        Self::create_request_logs_usage_indexes_if_supported(conn)?;
        if !Self::create_request_logs_dedup_triggers_if_supported(conn)? {
            return Ok(());
        }

        let hidden = conn
            .execute(&backfill_dedup_hidden_sql(), [])
            .map_err(|e| AppError::Database(format!("回填跨源去重标记失败: {e}")))?;
        if hidden > 0 {
            log::info!("已标记 {hidden} 条与 proxy 日志重复的会话用量行");
        }
        Ok(())
    }

    /// v16 -> v17: introduce the migration ledger and run the startup repairs that
    /// `create_tables_on_conn` used to repeat on every start, one last time.
    ///
    /// The repairs are idempotent and order-independent with respect to v1..v16, so
    /// running them after the chain gives the same result as the old every-start code.
    fn migrate_v16_to_v17(conn: &Connection) -> Result<(), AppError> {
        Self::ensure_migration_ledger(conn)?;

        // 补齐缺失的默认代理配置行
        Self::seed_default_proxy_config_rows(conn)?;

        // 修复跑过未发布开发版的库：current 标记曾是全局 key，现按应用分组
        // （随 v12 定稿为 current_profile_id_<scope>）
        if Self::table_exists(conn, "settings")? {
            conn.execute(
                "INSERT OR REPLACE INTO settings (key, value)
                 SELECT 'current_profile_id_claude', value FROM settings
                 WHERE key = 'current_profile_id'",
                [],
            )
            .map_err(|e| AppError::Database(format!("迁移全局 profile 标记失败: {e}")))?;
            conn.execute("DELETE FROM settings WHERE key = 'current_profile_id'", [])
                .map_err(|e| AppError::Database(format!("删除全局 profile 标记失败: {e}")))?;
        }

        // 故障转移队列：providers.in_failover_queue 取代旧的 failover_queue 表
        if Self::table_exists(conn, "providers")? {
            Self::add_column_if_missing(
                conn,
                "providers",
                "in_failover_queue",
                "BOOLEAN NOT NULL DEFAULT 0",
            )?;
        }
        conn.execute("DROP INDEX IF EXISTS idx_failover_queue_order", [])
            .map_err(|e| AppError::Database(format!("删除 failover_queue 索引失败: {e}")))?;
        conn.execute("DROP TABLE IF EXISTS failover_queue", [])
            .map_err(|e| AppError::Database(format!("删除 failover_queue 表失败: {e}")))?;
        Self::create_providers_failover_index_if_supported(conn)?;
        Ok(())
    }

    /// 插入默认模型定价数据
    /// 格式: (model_id, display_name, input, output, cache_read, cache_creation)
    /// 注意: model_id 使用短横线格式（如 claude-haiku-4-5），与 API 返回的模型名称标准化后一致
    fn seed_model_pricing(conn: &Connection) -> Result<(), AppError> {
        let pricing_data = [
            // Claude Fable 5（Opus 之上的新档）
            (
                "claude-fable-5",
                "Claude Fable 5",
                "10",
                "50",
                "1.00",
                "12.50",
            ),
            (
                "claude-mythos-5",
                "Claude Mythos 5",
                "10",
                "50",
                "1.00",
                "12.50",
            ),
            // Claude 4.8 系列
            (
                "claude-opus-4-8",
                "Claude Opus 4.8",
                "5",
                "25",
                "0.50",
                "6.25",
            ),
            // Claude Sonnet 5（list 价，与 Sonnet 4.6 一致；促销 $2/$10 至 2026-08-31 不入表）
            (
                "claude-sonnet-5",
                "Claude Sonnet 5",
                "3",
                "15",
                "0.30",
                "3.75",
            ),
            // Claude 4.7 系列
            (
                "claude-opus-4-7",
                "Claude Opus 4.7",
                "5",
                "25",
                "0.50",
                "6.25",
            ),
            // Claude 4.6 系列
            (
                "claude-opus-4-6-20260206",
                "Claude Opus 4.6",
                "5",
                "25",
                "0.50",
                "6.25",
            ),
            (
                "claude-sonnet-4-6-20260217",
                "Claude Sonnet 4.6",
                "3",
                "15",
                "0.30",
                "3.75",
            ),
            // Claude 4.5 系列
            (
                "claude-opus-4-5-20251101",
                "Claude Opus 4.5",
                "5",
                "25",
                "0.50",
                "6.25",
            ),
            (
                "claude-sonnet-4-5-20250929",
                "Claude Sonnet 4.5",
                "3",
                "15",
                "0.30",
                "3.75",
            ),
            (
                "claude-haiku-4-5-20251001",
                "Claude Haiku 4.5",
                "1",
                "5",
                "0.10",
                "1.25",
            ),
            // Claude 4 系列 (Legacy Models)
            (
                "claude-opus-4-20250514",
                "Claude Opus 4",
                "15",
                "75",
                "1.50",
                "18.75",
            ),
            (
                "claude-opus-4-1-20250805",
                "Claude Opus 4.1",
                "15",
                "75",
                "1.50",
                "18.75",
            ),
            (
                "claude-sonnet-4-20250514",
                "Claude Sonnet 4",
                "3",
                "15",
                "0.30",
                "3.75",
            ),
            // Claude 3.5 系列
            (
                "claude-3-5-haiku-20241022",
                "Claude 3.5 Haiku",
                "0.80",
                "4",
                "0.08",
                "1",
            ),
            (
                "claude-3-5-sonnet-20241022",
                "Claude 3.5 Sonnet",
                "3",
                "15",
                "0.30",
                "3.75",
            ),
            // GPT-5.6 系列（Sol / Terra / Luna，2026-06 发布）
            // 5.6 家族起 cache write 收 1.25× 输入价（此前 GPT 模型写缓存免费，勿回填旧系列）
            ("gpt-5.6-sol", "GPT-5.6 Sol", "5", "30", "0.50", "6.25"),
            (
                "gpt-5.6-terra",
                "GPT-5.6 Terra",
                "2.50",
                "15",
                "0.25",
                "3.125",
            ),
            ("gpt-5.6-luna", "GPT-5.6 Luna", "1", "6", "0.10", "1.25"),
            // 裸名 gpt-5.6 是 sol 的官方别名；effort 后缀对齐 gpt-5.5 系列的记账形态
            ("gpt-5.6", "GPT-5.6 Sol", "5", "30", "0.50", "6.25"),
            ("gpt-5.6-low", "GPT-5.6 Sol", "5", "30", "0.50", "6.25"),
            ("gpt-5.6-medium", "GPT-5.6 Sol", "5", "30", "0.50", "6.25"),
            ("gpt-5.6-high", "GPT-5.6 Sol", "5", "30", "0.50", "6.25"),
            ("gpt-5.6-xhigh", "GPT-5.6 Sol", "5", "30", "0.50", "6.25"),
            ("gpt-5.6-minimal", "GPT-5.6 Sol", "5", "30", "0.50", "6.25"),
            // GPT-5.5 系列
            ("gpt-5.5", "GPT-5.5", "5", "30", "0.50", "0"),
            ("gpt-5.5-low", "GPT-5.5", "5", "30", "0.50", "0"),
            ("gpt-5.5-medium", "GPT-5.5", "5", "30", "0.50", "0"),
            ("gpt-5.5-high", "GPT-5.5", "5", "30", "0.50", "0"),
            ("gpt-5.5-xhigh", "GPT-5.5", "5", "30", "0.50", "0"),
            ("gpt-5.5-minimal", "GPT-5.5", "5", "30", "0.50", "0"),
            // GPT-5.4 系列
            ("gpt-5.4", "GPT-5.4", "2.50", "15", "0.25", "0"),
            ("gpt-5.4-mini", "GPT-5.4 Mini", "0.75", "4.50", "0.075", "0"),
            ("gpt-5.4-nano", "GPT-5.4 Nano", "0.20", "1.25", "0.02", "0"),
            // GPT-5.2 系列
            ("gpt-5.2", "GPT-5.2", "1.75", "14", "0.175", "0"),
            ("gpt-5.2-low", "GPT-5.2", "1.75", "14", "0.175", "0"),
            ("gpt-5.2-medium", "GPT-5.2", "1.75", "14", "0.175", "0"),
            ("gpt-5.2-high", "GPT-5.2", "1.75", "14", "0.175", "0"),
            ("gpt-5.2-xhigh", "GPT-5.2", "1.75", "14", "0.175", "0"),
            ("gpt-5.2-codex", "GPT-5.2 Codex", "1.75", "14", "0.175", "0"),
            (
                "gpt-5.2-codex-low",
                "GPT-5.2 Codex",
                "1.75",
                "14",
                "0.175",
                "0",
            ),
            (
                "gpt-5.2-codex-medium",
                "GPT-5.2 Codex",
                "1.75",
                "14",
                "0.175",
                "0",
            ),
            (
                "gpt-5.2-codex-high",
                "GPT-5.2 Codex",
                "1.75",
                "14",
                "0.175",
                "0",
            ),
            (
                "gpt-5.2-codex-xhigh",
                "GPT-5.2 Codex",
                "1.75",
                "14",
                "0.175",
                "0",
            ),
            // GPT-5.3 Codex 系列
            ("gpt-5.3-codex", "GPT-5.3 Codex", "1.75", "14", "0.175", "0"),
            (
                "gpt-5.3-codex-low",
                "GPT-5.3 Codex",
                "1.75",
                "14",
                "0.175",
                "0",
            ),
            (
                "gpt-5.3-codex-medium",
                "GPT-5.3 Codex",
                "1.75",
                "14",
                "0.175",
                "0",
            ),
            (
                "gpt-5.3-codex-high",
                "GPT-5.3 Codex",
                "1.75",
                "14",
                "0.175",
                "0",
            ),
            (
                "gpt-5.3-codex-xhigh",
                "GPT-5.3 Codex",
                "1.75",
                "14",
                "0.175",
                "0",
            ),
            // GPT-5.1 系列
            ("gpt-5.1", "GPT-5.1", "1.25", "10", "0.125", "0"),
            ("gpt-5.1-low", "GPT-5.1", "1.25", "10", "0.125", "0"),
            ("gpt-5.1-medium", "GPT-5.1", "1.25", "10", "0.125", "0"),
            ("gpt-5.1-high", "GPT-5.1", "1.25", "10", "0.125", "0"),
            ("gpt-5.1-minimal", "GPT-5.1", "1.25", "10", "0.125", "0"),
            ("gpt-5.1-codex", "GPT-5.1 Codex", "1.25", "10", "0.125", "0"),
            (
                "gpt-5.1-codex-mini",
                "GPT-5.1 Codex",
                "1.25",
                "10",
                "0.125",
                "0",
            ),
            (
                "gpt-5.1-codex-max",
                "GPT-5.1 Codex",
                "1.25",
                "10",
                "0.125",
                "0",
            ),
            (
                "gpt-5.1-codex-max-high",
                "GPT-5.1 Codex",
                "1.25",
                "10",
                "0.125",
                "0",
            ),
            (
                "gpt-5.1-codex-max-xhigh",
                "GPT-5.1 Codex",
                "1.25",
                "10",
                "0.125",
                "0",
            ),
            // GPT-5 系列
            ("gpt-5", "GPT-5", "1.25", "10", "0.125", "0"),
            ("gpt-5-low", "GPT-5", "1.25", "10", "0.125", "0"),
            ("gpt-5-medium", "GPT-5", "1.25", "10", "0.125", "0"),
            ("gpt-5-high", "GPT-5", "1.25", "10", "0.125", "0"),
            ("gpt-5-minimal", "GPT-5", "1.25", "10", "0.125", "0"),
            ("gpt-5-codex", "GPT-5 Codex", "1.25", "10", "0.125", "0"),
            ("gpt-5-codex-low", "GPT-5 Codex", "1.25", "10", "0.125", "0"),
            (
                "gpt-5-codex-medium",
                "GPT-5 Codex",
                "1.25",
                "10",
                "0.125",
                "0",
            ),
            (
                "gpt-5-codex-high",
                "GPT-5 Codex",
                "1.25",
                "10",
                "0.125",
                "0",
            ),
            (
                "gpt-5-codex-mini",
                "GPT-5 Codex",
                "1.25",
                "10",
                "0.125",
                "0",
            ),
            (
                "gpt-5-codex-mini-medium",
                "GPT-5 Codex",
                "1.25",
                "10",
                "0.125",
                "0",
            ),
            (
                "gpt-5-codex-mini-high",
                "GPT-5 Codex",
                "1.25",
                "10",
                "0.125",
                "0",
            ),
            // OpenAI Reasoning 系列
            ("o3", "OpenAI o3", "2", "8", "0.50", "0"),
            ("o4-mini", "OpenAI o4-mini", "1.10", "4.40", "0.275", "0"),
            // GPT-4.1 系列
            ("gpt-4.1", "GPT-4.1", "2", "8", "0.50", "0"),
            ("gpt-4.1-mini", "GPT-4.1 Mini", "0.40", "1.60", "0.10", "0"),
            ("gpt-4.1-nano", "GPT-4.1 Nano", "0.10", "0.40", "0.025", "0"),
            // Gemini 3.5 系列
            (
                "gemini-3.5-flash",
                "Gemini 3.5 Flash",
                "1.50",
                "9.00",
                "0.15",
                "0",
            ),
            // Gemini 3.1 系列
            (
                "gemini-3.1-pro-preview",
                "Gemini 3.1 Pro Preview",
                "2",
                "12",
                "0.20",
                "0",
            ),
            (
                "gemini-3.1-flash-lite",
                "Gemini 3.1 Flash Lite",
                "0.25",
                "1.50",
                "0.025",
                "0",
            ),
            (
                "gemini-3.1-flash-lite-preview",
                "Gemini 3.1 Flash Lite Preview",
                "0.25",
                "1.50",
                "0.025",
                "0",
            ),
            // Gemini 3 系列
            (
                "gemini-3-pro-preview",
                "Gemini 3 Pro Preview",
                "2",
                "12",
                "0.2",
                "0",
            ),
            (
                "gemini-3-flash-preview",
                "Gemini 3 Flash Preview",
                "0.5",
                "3",
                "0.05",
                "0",
            ),
            // Gemini 2.5 系列
            (
                "gemini-2.5-pro",
                "Gemini 2.5 Pro",
                "1.25",
                "10",
                "0.125",
                "0",
            ),
            (
                "gemini-2.5-flash",
                "Gemini 2.5 Flash",
                "0.3",
                "2.5",
                "0.03",
                "0",
            ),
            (
                "gemini-2.5-flash-lite",
                "Gemini 2.5 Flash Lite",
                "0.10",
                "0.40",
                "0.01",
                "0",
            ),
            // Gemini 2.0 系列
            (
                "gemini-2.0-flash",
                "Gemini 2.0 Flash",
                "0.10",
                "0.40",
                "0.025",
                "0",
            ),
            // StepFun 系列
            (
                "step-3.7-flash",
                "Step 3.7 Flash",
                "0.19",
                "1.13",
                "0.04",
                "0",
            ),
            (
                "step-3.5-flash",
                "Step 3.5 Flash",
                "0.10",
                "0.30",
                "0.02",
                "0",
            ),
            (
                "step-3.5-flash-2603",
                "Step 3.5 Flash 2603",
                "0.10",
                "0.30",
                "0.02",
                "0",
            ),
            // ====== 国产模型 (USD/1M tokens) ======
            // Doubao (字节跳动)
            // Seed 2.1 系列（2026-06 火山引擎官方 list 价，CNY 按 ~7.14 折算）：
            //   pro   输入 6 元 / 输出 30 元 / 命中 1.2 元
            //   turbo 输入 3 元 / 输出 15 元 / 命中 0.6 元
            // 「缓存存储 0.017 元/M/小时」是按时长计费的存储费，与本表 cache_creation（按 token 写入价）口径不同，置 0。
            (
                "doubao-seed-2-1-pro",
                "Doubao Seed 2.1 Pro",
                "0.84",
                "4.2",
                "0.17",
                "0",
            ),
            (
                "doubao-seed-2-1-turbo",
                "Doubao Seed 2.1 Turbo",
                "0.42",
                "2.1",
                "0.08",
                "0",
            ),
            (
                "doubao-seed-code",
                "Doubao Seed Code",
                "0.17",
                "1.11",
                "0.02",
                "0",
            ),
            (
                "doubao-seed-2-0-pro",
                "Doubao Seed 2.0 Pro",
                "0.47",
                "2.37",
                "0.09",
                "0",
            ),
            (
                "doubao-seed-2-0-code",
                "Doubao Seed 2.0 Code",
                "0.47",
                "2.37",
                "0.09",
                "0",
            ),
            (
                "doubao-seed-2-0-code-preview-latest",
                "Doubao Seed 2.0 Code Preview",
                "0.47",
                "2.37",
                "0.09",
                "0",
            ),
            (
                "doubao-seed-2-0-lite",
                "Doubao Seed 2.0 Lite",
                "0.08",
                "0.50",
                "0.017",
                "0",
            ),
            (
                "doubao-seed-2-0-mini",
                "Doubao Seed 2.0 Mini",
                "0.03",
                "0.31",
                "0.0056",
                "0",
            ),
            // DeepSeek 系列
            (
                "deepseek-v3.2",
                "DeepSeek V3.2",
                "0.28",
                "0.42",
                "0.028",
                "0",
            ),
            (
                "deepseek-v3.1",
                "DeepSeek V3.1",
                "0.55",
                "1.67",
                "0.055",
                "0",
            ),
            ("deepseek-v3", "DeepSeek V3", "0.28", "1.11", "0.028", "0"),
            (
                "deepseek-chat",
                "DeepSeek Chat",
                "0.27",
                "1.10",
                "0.07",
                "0",
            ),
            (
                "deepseek-reasoner",
                "DeepSeek Reasoner",
                "0.55",
                "2.19",
                "0.14",
                "0",
            ),
            // DeepSeek V4 系列（官方 CNY 按 1 USD ≈ 7.14 折算）
            (
                "deepseek-v4-flash",
                "DeepSeek V4 Flash",
                "0.14",
                "0.28",
                "0.0028",
                "0",
            ),
            (
                "deepseek-v4-pro",
                "DeepSeek V4 Pro",
                "0.435",
                "0.87",
                "0.003625",
                "0",
            ),
            // Kimi (月之暗面)
            (
                "kimi-k2-thinking",
                "Kimi K2 Thinking",
                "0.55",
                "2.20",
                "0.10",
                "0",
            ),
            ("kimi-k2-0905", "Kimi K2", "0.55", "2.20", "0.10", "0"),
            (
                "kimi-k2-turbo",
                "Kimi K2 Turbo",
                "1.11",
                "8.06",
                "0.14",
                "0",
            ),
            ("kimi-k2.5", "Kimi K2.5", "0.60", "3.00", "0.10", "0"),
            ("kimi-k2.6", "Kimi K2.6", "0.95", "4.00", "0.16", "0"),
            (
                "kimi-k2.7-code",
                "Kimi K2.7 Code",
                "0.95",
                "4.00",
                "0.19",
                "0",
            ),
            // 腾讯混元 (Tencent Hunyuan)（官方 CNY 1/4/0.25 按 1 USD ≈ 7.14 折算；Hy3 阶梯计价取最低档）
            ("hunyuan-hy3", "Hunyuan Hy3", "0.14", "0.56", "0.035", "0"),
            ("hy3", "Hunyuan Hy3", "0.14", "0.56", "0.035", "0"),
            // MiniMax 系列
            ("minimax-m2.1", "MiniMax M2.1", "0.27", "0.95", "0.03", "0"),
            (
                "minimax-m2.1-lightning",
                "MiniMax M2.1 Lightning",
                "0.27",
                "2.33",
                "0.03",
                "0",
            ),
            ("minimax-m2", "MiniMax M2", "0.27", "0.95", "0.03", "0"),
            ("minimax-m2.5", "MiniMax M2.5", "0.15", "0.95", "0.03", "0"),
            (
                "minimax-m2.5-lightning",
                "MiniMax M2.5 Lightning",
                "0.30",
                "2.40",
                "0.03",
                "0",
            ),
            (
                "minimax-m2.7",
                "MiniMax M2.7",
                "0.30",
                "1.20",
                "0.06",
                "0.375",
            ),
            (
                "minimax-m2.7-highspeed",
                "MiniMax M2.7 Highspeed",
                "0.60",
                "2.40",
                "0.06",
                "0.375",
            ),
            ("minimax-m3", "MiniMax M3", "0.60", "2.40", "0.12", "0"),
            // GLM (智谱)
            ("glm-4.7", "GLM-4.7", "0.6", "2.2", "0.11", "0"),
            ("glm-4.6", "GLM-4.6", "0.6", "2.2", "0.11", "0"),
            ("glm-5", "GLM-5", "1", "3.2", "0.2", "0"),
            ("glm-5.1", "GLM-5.1", "1.4", "4.4", "0.26", "0"),
            ("glm-5.2", "GLM-5.2", "1.4", "4.4", "0.26", "0"),
            // MiMo (小米)
            (
                "mimo-v2-flash",
                "MiMo V2 Flash",
                "0.09",
                "0.29",
                "0.009",
                "0",
            ),
            ("mimo-v2-pro", "MiMo V2 Pro", "0.435", "0.87", "0.0036", "0"),
            ("mimo-v2.5", "MiMo V2.5", "0.14", "0.29", "0.0028", "0"),
            (
                "mimo-v2.5-pro",
                "MiMo V2.5 Pro",
                "0.435",
                "0.87",
                "0.0036",
                "0",
            ),
            // Qwen 系列 (阿里巴巴)
            ("qwen3.7-max", "Qwen3.7 Max", "2.50", "7.50", "0.25", "0"),
            ("qwen3.7-plus", "Qwen3.7 Plus", "0.40", "1.60", "0.08", "0"),
            (
                "qwen3.6-plus",
                "Qwen3.6 Plus",
                "0.325",
                "1.95",
                "0.065",
                "0",
            ),
            ("qwen3.5-plus", "Qwen3.5 Plus", "0.26", "1.56", "0.052", "0"),
            ("qwen3-max", "Qwen3 Max", "0.78", "3.90", "0", "0"),
            (
                "qwen3-235b-a22b",
                "Qwen3 235B-A22B",
                "0.70",
                "8.40",
                "0",
                "0",
            ),
            (
                "qwen3-coder-plus",
                "Qwen3 Coder Plus",
                "0.65",
                "3.25",
                "0.13",
                "0",
            ),
            (
                "qwen3-coder-480b",
                "Qwen3 Coder 480B",
                "0.65",
                "3.25",
                "0",
                "0",
            ),
            (
                "qwen3-coder-480b-a35b-instruct",
                "Qwen3 Coder 480B-A35B Instruct",
                "0.65",
                "3.25",
                "0",
                "0",
            ),
            (
                "qwen3-coder-flash",
                "Qwen3 Coder Flash",
                "0.195",
                "0.975",
                "0.039",
                "0",
            ),
            (
                "qwen3-coder-next",
                "Qwen3 Coder Next",
                "0.12",
                "0.75",
                "0",
                "0",
            ),
            ("qwq-plus", "QwQ Plus", "0.80", "2.40", "0", "0"),
            ("qwq-32b", "QwQ 32B", "0.20", "0.60", "0", "0"),
            ("qwen3-32b", "Qwen3 32B", "0.16", "0.64", "0", "0"),
            // Grok 系列 (xAI)
            ("grok-4.3", "Grok 4.3", "1.25", "2.50", "0.20", "0"),
            (
                "grok-4.20-0309-reasoning",
                "Grok 4.20 Reasoning",
                "1.25",
                "2.50",
                "0.20",
                "0",
            ),
            (
                "grok-4.20-0309-non-reasoning",
                "Grok 4.20",
                "1.25",
                "2.50",
                "0.20",
                "0",
            ),
            (
                "grok-4-1-fast-reasoning",
                "Grok 4.1 Fast Reasoning",
                "0.20",
                "0.50",
                "0.05",
                "0",
            ),
            (
                "grok-4-1-fast-non-reasoning",
                "Grok 4.1 Fast",
                "0.20",
                "0.50",
                "0.05",
                "0",
            ),
            ("grok-4", "Grok 4", "3", "15", "0.75", "0"),
            (
                "grok-code-fast-1",
                "Grok Build 0.1 (Code Fast Alias)",
                "1",
                "2",
                "0.20",
                "0",
            ),
            ("grok-build-0.1", "Grok Build 0.1", "1", "2", "0.20", "0"),
            ("grok-3", "Grok 3", "3", "15", "0.75", "0"),
            ("grok-3-mini", "Grok 3 Mini", "0.25", "0.50", "0.075", "0"),
            // Mistral 系列
            (
                "mistral-medium-3.5",
                "Mistral Medium 3.5",
                "1.50",
                "7.50",
                "0",
                "0",
            ),
            (
                "mistral-small-4",
                "Mistral Small 4",
                "0.10",
                "0.30",
                "0.01",
                "0",
            ),
            (
                "devstral-small-2-2512",
                "Devstral Small 2",
                "0.10",
                "0.30",
                "0.01",
                "0",
            ),
            (
                "magistral-small",
                "Magistral Small",
                "0.50",
                "1.50",
                "0",
                "0",
            ),
            ("codestral-2508", "Codestral", "0.30", "0.90", "0.03", "0"),
            (
                "devstral-small-1.1",
                "Devstral Small 1.1",
                "0.07",
                "0.28",
                "0.01",
                "0",
            ),
            ("devstral-2-2512", "Devstral 2", "0.40", "2", "0.04", "0"),
            (
                "devstral-medium",
                "Devstral Medium",
                "0.40",
                "2",
                "0.04",
                "0",
            ),
            (
                "mistral-large-3-2512",
                "Mistral Large 3",
                "0.50",
                "1.50",
                "0.05",
                "0",
            ),
            (
                "mistral-medium-3.1",
                "Mistral Medium 3.1",
                "0.40",
                "2",
                "0.04",
                "0",
            ),
            (
                "mistral-small-3.2-24b",
                "Mistral Small 3.2",
                "0.075",
                "0.20",
                "0.01",
                "0",
            ),
            ("magistral-medium", "Magistral Medium", "2", "5", "0", "0"),
            // Cohere 系列
            ("command-a", "Cohere Command A", "2.50", "10", "0", "0"),
            (
                "command-r-plus",
                "Cohere Command R+",
                "2.50",
                "10",
                "0",
                "0",
            ),
            ("command-r", "Cohere Command R", "0.15", "0.60", "0", "0"),
            // OpenAI 补充
            ("o3-pro", "OpenAI o3-pro", "20", "80", "0", "0"),
            ("o3-mini", "OpenAI o3-mini", "0.55", "2.20", "0.55", "0"),
            ("o1", "OpenAI o1", "15", "60", "7.50", "0"),
            ("o1-mini", "OpenAI o1-mini", "0.55", "2.20", "0.55", "0"),
            ("codex-mini", "Codex Mini", "0.75", "3", "0.025", "0"),
            ("gpt-5-mini", "GPT-5 Mini", "0.25", "2", "0.025", "0"),
            ("gpt-5-nano", "GPT-5 Nano", "0.05", "0.40", "0.005", "0"),
        ];

        let mut stmt = conn
            .prepare(
                "INSERT OR IGNORE INTO model_pricing (
                    model_id, display_name, input_cost_per_million, output_cost_per_million,
                    cache_read_cost_per_million, cache_creation_cost_per_million
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .map_err(|e| AppError::Database(format!("准备模型定价语句失败: {e}")))?;
        for (model_id, display_name, input, output, cache_read, cache_creation) in pricing_data {
            stmt.execute(rusqlite::params![
                model_id,
                display_name,
                input,
                output,
                cache_read,
                cache_creation
            ])
            .map_err(|e| AppError::Database(format!("插入模型定价失败: {e}")))?;
        }

        log::info!("已插入 {} 条默认模型定价数据", pricing_data.len());
        Ok(())
    }

    fn repair_current_model_pricing(conn: &Connection) -> Result<(), AppError> {
        let pricing_fixes = [
            // 2026-07-12 GPT-5.6 家族 cache write=1.25× 输入价（OpenAI 5.6 起的新规），
            // 修正早期 seed 的 0 值；只匹配未被用户改过的行
            (
                "gpt-5.6-sol",
                "GPT-5.6 Sol",
                "5",
                "30",
                "0.50",
                "6.25",
                "5",
                "30",
                "0.50",
                "0",
            ),
            (
                "gpt-5.6-terra",
                "GPT-5.6 Terra",
                "2.50",
                "15",
                "0.25",
                "3.125",
                "2.50",
                "15",
                "0.25",
                "0",
            ),
            (
                "gpt-5.6-luna",
                "GPT-5.6 Luna",
                "1",
                "6",
                "0.10",
                "1.25",
                "1",
                "6",
                "0.10",
                "0",
            ),
            // 2026-06-10 全量核价（厂商官方 list 价；CNY 按 ~7.14 折算）
            // GLM 4.6/4.7：旧值是中转/OpenRouter 折扣价，统一到 Z.ai 官方（与 glm-5/5.1 一致）
            (
                "glm-4.7", "GLM-4.7", "0.6", "2.2", "0.11", "0", "0.39", "1.75", "0.04", "0",
            ),
            (
                "glm-4.6", "GLM-4.6", "0.6", "2.2", "0.11", "0", "0.28", "1.11", "0.03", "0",
            ),
            // Grok 4.20：xAI 已降价 2/6 → 1.25/2.50
            (
                "grok-4.20-0309-reasoning",
                "Grok 4.20 Reasoning",
                "1.25",
                "2.50",
                "0.20",
                "0",
                "2",
                "6",
                "0.20",
                "0",
            ),
            (
                "grok-4.20-0309-non-reasoning",
                "Grok 4.20",
                "1.25",
                "2.50",
                "0.20",
                "0",
                "2",
                "6",
                "0.20",
                "0",
            ),
            // Kimi K2.5 官方 output 3.00
            (
                "kimi-k2.5",
                "Kimi K2.5",
                "0.60",
                "3.00",
                "0.10",
                "0",
                "0.60",
                "2.50",
                "0.10",
                "0",
            ),
            // MiniMax M2.5 input 0.15
            (
                "minimax-m2.5",
                "MiniMax M2.5",
                "0.15",
                "0.95",
                "0.03",
                "0",
                "0.12",
                "0.95",
                "0.03",
                "0",
            ),
            // Mistral Devstral 2 output 0.90 → 2（与同表 devstral-medium 一致）
            (
                "devstral-2-2512",
                "Devstral 2",
                "0.40",
                "2",
                "0.04",
                "0",
                "0.40",
                "0.90",
                "0.04",
                "0",
            ),
            // Doubao Seed 2.0：lite 旧价贵 3-4 倍 + 全系补 cache 命中价
            (
                "doubao-seed-2-0-lite",
                "Doubao Seed 2.0 Lite",
                "0.08",
                "0.50",
                "0.017",
                "0",
                "0.25",
                "2",
                "0",
                "0",
            ),
            (
                "doubao-seed-2-0-pro",
                "Doubao Seed 2.0 Pro",
                "0.47",
                "2.37",
                "0.09",
                "0",
                "0.47",
                "2.37",
                "0",
                "0",
            ),
            (
                "doubao-seed-2-0-code",
                "Doubao Seed 2.0 Code",
                "0.47",
                "2.37",
                "0.09",
                "0",
                "0.47",
                "2.37",
                "0",
                "0",
            ),
            (
                "doubao-seed-2-0-code-preview-latest",
                "Doubao Seed 2.0 Code Preview",
                "0.47",
                "2.37",
                "0.09",
                "0",
                "0.47",
                "2.37",
                "0",
                "0",
            ),
            (
                "doubao-seed-2-0-mini",
                "Doubao Seed 2.0 Mini",
                "0.03",
                "0.31",
                "0.0056",
                "0",
                "0.03",
                "0.31",
                "0",
                "0",
            ),
            // MiMo：5/27 永久降价，旧值是旧价
            (
                "mimo-v2-pro",
                "MiMo V2 Pro",
                "0.435",
                "0.87",
                "0.0036",
                "0",
                "1",
                "3",
                "0",
                "0",
            ),
            (
                "mimo-v2.5",
                "MiMo V2.5",
                "0.14",
                "0.29",
                "0.0028",
                "0",
                "0.09",
                "0.29",
                "0.009",
                "0",
            ),
            (
                "mimo-v2.5-pro",
                "MiMo V2.5 Pro",
                "0.435",
                "0.87",
                "0.0036",
                "0",
                "1",
                "3",
                "0",
                "0",
            ),
            // Qwen：官方"隐式缓存 = 输入 20%"补 cache 命中价
            (
                "qwen3.6-plus",
                "Qwen3.6 Plus",
                "0.325",
                "1.95",
                "0.065",
                "0",
                "0.325",
                "1.95",
                "0",
                "0",
            ),
            (
                "qwen3.5-plus",
                "Qwen3.5 Plus",
                "0.26",
                "1.56",
                "0.052",
                "0",
                "0.26",
                "1.56",
                "0",
                "0",
            ),
            (
                "qwen3-coder-plus",
                "Qwen3 Coder Plus",
                "0.65",
                "3.25",
                "0.13",
                "0",
                "0.65",
                "3.25",
                "0",
                "0",
            ),
            (
                "qwen3-coder-flash",
                "Qwen3 Coder Flash",
                "0.195",
                "0.975",
                "0.039",
                "0",
                "0.195",
                "0.975",
                "0",
                "0",
            ),
            (
                "deepseek-v4-flash",
                "DeepSeek V4 Flash",
                "0.14",
                "0.28",
                "0.0028",
                "0",
                "0.14",
                "0.28",
                "0.028",
                "0",
            ),
            (
                "deepseek-v4-pro",
                "DeepSeek V4 Pro",
                "0.435",
                "0.87",
                "0.003625",
                "0",
                "1.68",
                "3.36",
                "0.14",
                "0",
            ),
            (
                "glm-5", "GLM-5", "1", "3.2", "0.2", "0", "0.72", "2.30", "0", "0",
            ),
            (
                "glm-5.1", "GLM-5.1", "1.4", "4.4", "0.26", "0", "0.95", "3.15", "0", "0",
            ),
            (
                "grok-code-fast-1",
                "Grok Build 0.1 (Code Fast Alias)",
                "1",
                "2",
                "0.20",
                "0",
                "0.20",
                "1.50",
                "0.02",
                "0",
            ),
        ];

        for (
            model_id,
            display_name,
            input,
            output,
            cache_read,
            cache_creation,
            old_input,
            old_output,
            old_cache_read,
            old_cache_creation,
        ) in pricing_fixes
        {
            conn.execute(
                "UPDATE model_pricing SET
                    display_name = ?2,
                    input_cost_per_million = ?3,
                    output_cost_per_million = ?4,
                    cache_read_cost_per_million = ?5,
                    cache_creation_cost_per_million = ?6
                 WHERE model_id = ?1
                   AND input_cost_per_million = ?7
                   AND output_cost_per_million = ?8
                   AND cache_read_cost_per_million = ?9
                   AND cache_creation_cost_per_million = ?10",
                rusqlite::params![
                    model_id,
                    display_name,
                    input,
                    output,
                    cache_read,
                    cache_creation,
                    old_input,
                    old_output,
                    old_cache_read,
                    old_cache_creation
                ],
            )
            .map_err(|e| AppError::Database(format!("修复模型 {model_id} 定价失败: {e}")))?;
        }

        Ok(())
    }

    /// 确保模型定价表具备默认数据
    pub fn ensure_model_pricing_seeded(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        Self::ensure_model_pricing_seeded_on_conn(&conn)
    }

    fn ensure_model_pricing_seeded_on_conn(conn: &Connection) -> Result<(), AppError> {
        // 每次启动都执行 INSERT OR IGNORE，增量追加新模型；仅修复仍等于旧内置值的定价。
        // 数百条语句放进同一个 SAVEPOINT：只提交一次，而不是逐条 autocommit（DB-09）。
        conn.execute("SAVEPOINT seed_model_pricing;", [])
            .map_err(|e| AppError::Database(format!("开启定价种子 savepoint 失败: {e}")))?;
        let result =
            Self::seed_model_pricing(conn).and_then(|_| Self::repair_current_model_pricing(conn));
        match result {
            Ok(()) => conn
                .execute("RELEASE seed_model_pricing;", [])
                .map(|_| ())
                .map_err(|e| AppError::Database(format!("提交定价种子失败: {e}"))),
            Err(e) => {
                conn.execute("ROLLBACK TO seed_model_pricing;", []).ok();
                conn.execute("RELEASE seed_model_pricing;", []).ok();
                Err(e)
            }
        }
    }

    // --- 辅助方法 ---

    pub(crate) fn get_user_version(conn: &Connection) -> Result<i32, AppError> {
        conn.query_row("PRAGMA user_version;", [], |row| row.get(0))
            .map_err(|e| AppError::Database(format!("读取 user_version 失败: {e}")))
    }

    pub(crate) fn set_user_version(conn: &Connection, version: i32) -> Result<(), AppError> {
        if version < 0 {
            return Err(AppError::Database("user_version 不能为负数".to_string()));
        }
        let sql = format!("PRAGMA user_version = {version};");
        conn.execute(&sql, [])
            .map_err(|e| AppError::Database(format!("写入 user_version 失败: {e}")))?;
        Ok(())
    }

    fn create_request_logs_usage_indexes_if_supported(conn: &Connection) -> Result<(), AppError> {
        if !Self::table_exists(conn, "proxy_request_logs")? {
            return Ok(());
        }

        let has_app_type = Self::has_column(conn, "proxy_request_logs", "app_type")?;
        let has_created_at = Self::has_column(conn, "proxy_request_logs", "created_at")?;
        if has_app_type && has_created_at {
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_request_logs_app_created_at
                 ON proxy_request_logs(app_type, created_at DESC)",
                [],
            )
            .map_err(|e| AppError::Database(format!("创建使用量应用时间索引失败: {e}")))?;
        }

        let required_columns = [
            "app_type",
            "data_source",
            "input_tokens",
            "output_tokens",
            "cache_read_tokens",
            "created_at",
            "cache_creation_tokens",
        ];
        for column in required_columns {
            if !Self::has_column(conn, "proxy_request_logs", column)? {
                return Ok(());
            }
        }

        conn.execute("DROP INDEX IF EXISTS idx_request_logs_dedup_lookup", [])
            .map_err(|e| AppError::Database(format!("删除旧使用量去重索引失败: {e}")))?;

        // 查询层为了兼容历史 NULL data_source 行，会使用
        // COALESCE(data_source, 'proxy')。普通 data_source 索引无法匹配该表达式，
        // 会让跨源去重子查询退化成大量扫描；表达式索引让 SQLite 能按同一表达式查找。
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_logs_dedup_lookup_expr
             ON proxy_request_logs(app_type, COALESCE(data_source, 'proxy'), input_tokens,
                                   output_tokens, cache_read_tokens, created_at,
                                   cache_creation_tokens)",
            [],
        )
        .map_err(|e| AppError::Database(format!("创建使用量去重表达式索引失败: {e}")))?;

        if Self::has_column(conn, "proxy_request_logs", "total_cost_usd")? {
            // 部分索引：只收录零成本行。WHERE 必须与
            // `usage_stats::BACKFILL_ZERO_COST_CANDIDATES_SQL` 中的条件逐字一致，
            // 规划器才会用它代替全表扫描。
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_request_logs_zero_cost
                 ON proxy_request_logs(created_at)
                 WHERE CAST(total_cost_usd AS REAL) <= 0",
                [],
            )
            .map_err(|e| AppError::Database(format!("创建零成本用量索引失败: {e}")))?;
        }
        Ok(())
    }

    fn create_request_logs_provider_index_if_supported(conn: &Connection) -> Result<(), AppError> {
        for column in ["provider_id", "app_type", "created_at"] {
            if !Self::has_column(conn, "proxy_request_logs", column)? {
                return Ok(());
            }
        }
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_logs_provider_created_at
             ON proxy_request_logs(provider_id, app_type, created_at)",
            [],
        )
        .map_err(|e| AppError::Database(format!("创建 provider 时间索引失败: {e}")))?;
        Ok(())
    }

    /// 创建维护 `dedup_hidden` 的触发器；旧表缺少所需列时跳过（由迁移补齐后再创建）。
    /// 返回触发器是否已就绪。
    pub(crate) fn create_request_logs_dedup_triggers_if_supported(
        conn: &Connection,
    ) -> Result<bool, AppError> {
        if !Self::table_exists(conn, "proxy_request_logs")? {
            return Ok(false);
        }
        let required_columns = [
            "dedup_hidden",
            "data_source",
            "app_type",
            "model",
            "status_code",
            "input_tokens",
            "output_tokens",
            "cache_read_tokens",
            "cache_creation_tokens",
            "created_at",
        ];
        for column in required_columns {
            if !Self::has_column(conn, "proxy_request_logs", column)? {
                return Ok(false);
            }
        }
        for sql in request_logs_dedup_trigger_sqls() {
            conn.execute(&sql, [])
                .map_err(|e| AppError::Database(format!("创建跨源去重触发器失败: {e}")))?;
        }
        Ok(true)
    }

    fn validate_identifier(s: &str, kind: &str) -> Result<(), AppError> {
        if s.is_empty() {
            return Err(AppError::Database(format!("{kind} 不能为空")));
        }
        if !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(AppError::Database(format!(
                "非法{kind}: {s}，仅允许字母、数字和下划线"
            )));
        }
        Ok(())
    }

    pub(crate) fn table_exists(conn: &Connection, table: &str) -> Result<bool, AppError> {
        Self::validate_identifier(table, "表名")?;

        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table'")
            .map_err(|e| AppError::Database(format!("读取表名失败: {e}")))?;
        let mut rows = stmt
            .query([])
            .map_err(|e| AppError::Database(format!("查询表名失败: {e}")))?;
        while let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
            let name: String = row
                .get(0)
                .map_err(|e| AppError::Database(format!("解析表名失败: {e}")))?;
            if name.eq_ignore_ascii_case(table) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(crate) fn has_column(
        conn: &Connection,
        table: &str,
        column: &str,
    ) -> Result<bool, AppError> {
        Self::validate_identifier(table, "表名")?;
        Self::validate_identifier(column, "列名")?;

        let sql = format!("PRAGMA table_info(\"{table}\");");
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| AppError::Database(format!("读取表结构失败: {e}")))?;
        let mut rows = stmt
            .query([])
            .map_err(|e| AppError::Database(format!("查询表结构失败: {e}")))?;
        while let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
            let name: String = row
                .get(1)
                .map_err(|e| AppError::Database(format!("读取列名失败: {e}")))?;
            if name.eq_ignore_ascii_case(column) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// 表存在且包含全部 `columns` 时返回 true。
    fn has_columns(conn: &Connection, table: &str, columns: &[&str]) -> Result<bool, AppError> {
        for column in columns {
            if !Self::has_column(conn, table, column)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn add_column_if_missing(
        conn: &Connection,
        table: &str,
        column: &str,
        definition: &str,
    ) -> Result<bool, AppError> {
        Self::validate_identifier(table, "表名")?;
        Self::validate_identifier(column, "列名")?;

        if !Self::table_exists(conn, table)? {
            return Err(AppError::Database(format!(
                "表 {table} 不存在，无法添加列 {column}"
            )));
        }
        if Self::has_column(conn, table, column)? {
            return Ok(false);
        }

        let sql = format!("ALTER TABLE \"{table}\" ADD COLUMN \"{column}\" {definition};");
        conn.execute(&sql, [])
            .map_err(|e| AppError::Database(format!("为表 {table} 添加列 {column} 失败: {e}")))?;
        log::info!("已为表 {table} 添加缺失列 {column}");
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Migration registry ──────────────────────────────────

    const SCHEMA_SOURCE: &str = include_str!("schema.rs");

    /// Body of `fn {fn_name}(...)` with comments and insignificant whitespace removed:
    /// whitespace outside string literals is dropped, runs of whitespace inside string
    /// literals collapse to one space, and trailing commas are ignored, so rustfmt
    /// reflows and comment edits do not change the checksum.
    fn normalized_fn_body(source: &str, fn_name: &str) -> String {
        let signature = format!("fn {fn_name}(");
        let start = source
            .find(&signature)
            .unwrap_or_else(|| panic!("function `{fn_name}` not found in schema.rs"));
        let open = start + source[start..].find('{').expect("function body");
        let chars: Vec<char> = source[open..].chars().collect();

        let mut out = String::new();
        let mut depth = 0usize;
        let mut in_string = false;
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if in_string {
                if c == '\\' && i + 1 < chars.len() {
                    out.push(c);
                    out.push(chars[i + 1]);
                    i += 2;
                    continue;
                }
                if c.is_whitespace() {
                    if !out.ends_with(' ') {
                        out.push(' ');
                    }
                } else {
                    out.push(c);
                    if c == '"' {
                        in_string = false;
                    }
                }
            } else if c == '/' && chars.get(i + 1) == Some(&'/') {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                continue;
            } else if c == '"' {
                in_string = true;
                out.push(c);
            } else if !c.is_whitespace() {
                out.push(c);
                if c == '{' {
                    depth += 1;
                } else if c == '}' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
            i += 1;
        }
        out.replace(",)", ")").replace(",]", "]").replace(",}", "}")
    }

    fn migration_fn_name(version: i32) -> String {
        format!("migrate_v{}_to_v{}", version - 1, version)
    }

    fn source_checksum(version: i32) -> String {
        use sha2::{Digest, Sha256};
        let body = normalized_fn_body(SCHEMA_SOURCE, &migration_fn_name(version));
        format!("{:x}", Sha256::digest(body.as_bytes()))
    }

    #[test]
    fn registered_migration_checksums_match_source() {
        let mismatches: Vec<String> = MIGRATIONS
            .iter()
            .filter_map(|m| {
                let actual = source_checksum(m.version);
                (actual != m.checksum).then(|| {
                    format!(
                        "v{} {}: registered \"{}\", source \"{actual}\"",
                        m.version, m.name, m.checksum
                    )
                })
            })
            .collect();
        assert!(
            mismatches.is_empty(),
            "Released migrations must not change: a database that already ran a migration \
             will never run the edited version. Add a new migration instead. If the change \
             is intentional and does not alter behaviour (e.g. a log message), update the \
             registered checksum:\n{}",
            mismatches.join("\n")
        );
    }

    #[test]
    fn every_migration_function_is_registered() {
        let mut found = Vec::new();
        let mut rest = SCHEMA_SOURCE;
        while let Some(pos) = rest.find("fn migrate_v") {
            let tail = &rest[pos + "fn migrate_v".len()..];
            let end = tail.find('(').expect("signature");
            let name = &tail[..end];
            if let Some((from, to)) = name.split_once("_to_v") {
                if let (Ok(from), Ok(to)) = (from.parse::<i32>(), to.parse::<i32>()) {
                    // Skip test functions named after migrations.
                    if tail[end..].starts_with("(conn: &Connection)") {
                        assert_eq!(to, from + 1, "migrate_v{from}_to_v{to} must step by one");
                        found.push(to);
                    }
                }
            }
            rest = tail;
        }
        found.sort_unstable();
        let registered: Vec<i32> = MIGRATIONS.iter().map(|m| m.version).collect();
        assert_eq!(
            found, registered,
            "every migrate_vN_to_vM function must have exactly one MIGRATIONS entry"
        );
        assert_eq!(SCHEMA_VERSION, *registered.last().unwrap());
        let mut names: Vec<&str> = MIGRATIONS.iter().map(|m| m.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(
            names.len(),
            MIGRATIONS.len(),
            "migration names must be unique"
        );
    }

    #[test]
    fn schema_version_and_sync_compat_version_move_together() {
        // DB_COMPAT_VERSION is derived from the registry; this pin makes a schema bump
        // an explicit decision about sync compatibility. When adding a migration, decide
        // whether it sets `breaks_sync` (old clients can no longer import the new
        // snapshot, or vice versa) and update this pair accordingly.
        assert_eq!(
            (
                SCHEMA_VERSION,
                crate::services::sync_protocol::DB_COMPAT_VERSION
            ),
            (17, 6),
            "SCHEMA_VERSION changed without reviewing DB_COMPAT_VERSION (or vice versa)"
        );
        assert_eq!(
            crate::services::sync_protocol::DB_COMPAT_VERSION,
            SYNC_DB_COMPAT_VERSION
        );
        let derived = SYNC_DB_COMPAT_AT_BASELINE
            + MIGRATIONS
                .iter()
                .filter(|m| m.version > BASELINE_SCHEMA_VERSION && m.breaks_sync)
                .count() as u32;
        assert_eq!(SYNC_DB_COMPAT_VERSION, derived);
    }

    #[test]
    fn create_tables_propagates_ddl_errors() {
        // A read-only connection makes every DDL statement fail; the fresh path must
        // report it instead of silently leaving a half-created schema.
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("ro.db");
        Connection::open(&path).expect("create file");
        let conn = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("open read-only");
        assert!(Database::create_tables_on_conn(&conn).is_err());
        assert!(Database::apply_schema_migrations_on_conn(&conn).is_err());
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
        Database::create_tables_on_conn(&conn)?;
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

    fn request_log_index_names(conn: &Connection) -> Result<Vec<String>, AppError> {
        let mut stmt = conn.prepare(
            "SELECT name FROM sqlite_master
             WHERE type = 'index' AND tbl_name = 'proxy_request_logs' AND sql IS NOT NULL
             ORDER BY name",
        )?;
        let names = stmt
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(names)
    }

    const EXPECTED_REQUEST_LOG_INDEXES: [&str; 7] = [
        "idx_request_logs_app_created_at",
        "idx_request_logs_created_at",
        "idx_request_logs_dedup_lookup_expr",
        "idx_request_logs_provider_created_at",
        "idx_request_logs_session",
        "idx_request_logs_status",
        "idx_request_logs_zero_cost",
    ];

    #[test]
    fn fresh_schema_has_expected_request_log_indexes_and_triggers() -> Result<(), AppError> {
        let conn = Connection::open_in_memory()?;
        Database::create_tables_on_conn(&conn)?;
        Database::apply_schema_migrations_on_conn(&conn)?;
        assert_eq!(
            request_log_index_names(&conn)?,
            EXPECTED_REQUEST_LOG_INDEXES.to_vec()
        );
        let triggers: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'trigger' AND tbl_name = 'proxy_request_logs'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(triggers, 2);
        Ok(())
    }

    #[test]
    fn migrate_v15_to_v16_backfills_dedup_flag_and_trims_indexes() -> Result<(), AppError> {
        let conn = Connection::open_in_memory()?;
        // v15 形态：建表后去掉 dedup_hidden 相关对象，并恢复旧索引
        Database::create_tables_on_conn(&conn)?;
        conn.execute_batch(
            "DROP TRIGGER trg_request_logs_dedup_session_insert;
             DROP TRIGGER trg_request_logs_dedup_proxy_insert;
             DROP INDEX idx_request_logs_provider_created_at;
             DROP INDEX idx_request_logs_zero_cost;
             ALTER TABLE proxy_request_logs DROP COLUMN dedup_hidden;
             CREATE INDEX idx_request_logs_provider ON proxy_request_logs(provider_id, app_type);
             CREATE INDEX idx_request_logs_model ON proxy_request_logs(model);",
        )?;
        let insert = "INSERT INTO proxy_request_logs (
                request_id, provider_id, app_type, model, input_tokens, output_tokens,
                cache_read_tokens, cache_creation_tokens, latency_ms, status_code,
                created_at, data_source
            ) VALUES (?1, 'p1', 'claude', 'claude-sonnet-4', 10, 20, 0, 0, 100, 200, ?2, ?3)";
        conn.execute(insert, params!["proxy-1", 1_000_000i64, "proxy"])?;
        conn.execute(insert, params!["session-dup", 1_000_030i64, "session_log"])?;
        conn.execute(
            insert,
            params!["session-unique", 1_900_000i64, "session_log"],
        )?;
        Database::set_user_version(&conn, 15)?;

        // 与启动顺序一致：先 create_tables（旧表缺列时不得建触发器），再迁移
        Database::create_tables_on_conn(&conn)?;
        Database::apply_schema_migrations_on_conn(&conn)?;

        assert_eq!(Database::get_user_version(&conn)?, SCHEMA_VERSION);
        assert!(Database::has_column(
            &conn,
            "proxy_request_logs",
            "dedup_hidden"
        )?);
        assert_eq!(
            request_log_index_names(&conn)?,
            EXPECTED_REQUEST_LOG_INDEXES.to_vec()
        );
        let mut stmt = conn.prepare(
            "SELECT request_id FROM proxy_request_logs WHERE dedup_hidden = 1 ORDER BY request_id",
        )?;
        let hidden = stmt
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        assert_eq!(hidden, vec!["session-dup".to_string()]);

        // 迁移后创建的触发器对新写入生效
        conn.execute(insert, params!["proxy-2", 1_900_010i64, "proxy"])?;
        let unique_hidden: i64 = conn.query_row(
            "SELECT dedup_hidden FROM proxy_request_logs WHERE request_id = 'session-unique'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(unique_hidden, 1);
        Ok(())
    }
}
