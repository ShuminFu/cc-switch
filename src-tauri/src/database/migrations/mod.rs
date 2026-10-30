//! 版本化 Schema 迁移框架
//!
//! - [`MIGRATIONS`]：按版本排序的迁移表，第 N 项把数据库从 `user_version = N-1`
//!   迁移到 N，最后一项必须是 [`SCHEMA_VERSION`]（见本模块测试）。
//! - v1 是冻结的基线（[`baseline`]）：v3.8.3 发布时的建表语句。全新数据库也从
//!   v0 依次执行每一步，因此「全新库」与「从任意旧版本升级的库」走同一条链路。
//! - 每一步都必须声明对 `DB_COMPAT_VERSION` 的影响（[`DbCompatImpact`]，规则见
//!   docs/dev/DB_COMPATIBILITY.md）。
//! - 每次执行的步骤记录在 `schema_migrations` 审计表中。
//!
//! 迁移步骤一旦发布即不可修改（只能追加新步骤），错误必须向上传播，不得吞掉。

mod baseline;
mod legacy;
mod v016;
mod v017;
mod v018;

use super::{Database, SCHEMA_VERSION};
use crate::error::AppError;
use rusqlite::{params, Connection};

/// 一次迁移运行的上下文
pub(crate) struct MigrationContext {
    /// 迁移开始时数据库为空（v0 且没有任何用户表），即全新安装
    pub(crate) fresh_database: bool,
}

type MigrationFn = fn(&Connection, &MigrationContext) -> Result<(), AppError>;

/// 迁移对同步 `db.sql` 代际（`DB_COMPAT_VERSION`）的影响
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DbCompatImpact {
    /// 纯增量（新表、带默认值的新列、新索引、不改变含义的数据修复），
    /// 无需升级 `DB_COMPAT_VERSION`
    Additive,
    /// 非增量变更，随 `db_compat_version` 这一代同步格式发布；
    /// 必须与 `DB_COMPAT_VERSION` 的升级放在同一个 PR 中
    #[allow(dead_code)] // 目前还没有步骤需要；新的非增量步骤必须用它声明
    Breaking { db_compat_version: u32 },
    /// 兼容策略（CCS-091）之前发布的非增量变更（v1–v15），只允许用于历史步骤
    PrePolicy,
}

impl DbCompatImpact {
    fn audit_label(self) -> String {
        match self {
            Self::Additive => "additive".to_string(),
            Self::Breaking { db_compat_version } => format!("breaking:{db_compat_version}"),
            Self::PrePolicy => "pre-policy".to_string(),
        }
    }
}

/// 单个迁移步骤：把数据库从 `version - 1` 迁移到 `version`
pub(crate) struct Migration {
    pub(crate) version: i32,
    pub(crate) name: &'static str,
    pub(crate) db_compat: DbCompatImpact,
    up: MigrationFn,
}

/// 按版本排序的全部迁移步骤
pub(crate) const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "frozen v1 baseline (v3.8.3 tables) and pre-version column top-up",
        db_compat: DbCompatImpact::Additive,
        up: |conn, _| baseline::apply(conn),
    },
    Migration {
        version: 2,
        name: "usage statistics, proxy tables, per-app proxy_config, skills keyed by app",
        db_compat: DbCompatImpact::PrePolicy,
        up: |conn, _| Database::migrate_v1_to_v2(conn),
    },
    Migration {
        version: 3,
        name: "unified skills table (SSOT)",
        db_compat: DbCompatImpact::PrePolicy,
        up: Database::migrate_v2_to_v3,
    },
    Migration {
        version: 4,
        name: "OpenCode enablement columns",
        db_compat: DbCompatImpact::Additive,
        up: |conn, _| Database::migrate_v3_to_v4(conn),
    },
    Migration {
        version: 5,
        name: "cost multiplier, pricing source and request_model columns",
        db_compat: DbCompatImpact::Additive,
        up: |conn, _| Database::migrate_v4_to_v5(conn),
    },
    Migration {
        version: 6,
        name: "usage_daily_rollups table and github_copilot template type",
        db_compat: DbCompatImpact::PrePolicy,
        up: |conn, _| Database::migrate_v5_to_v6(conn),
    },
    Migration {
        version: 7,
        name: "skills content_hash and updated_at",
        db_compat: DbCompatImpact::Additive,
        up: |conn, _| Database::migrate_v6_to_v7(conn),
    },
    Migration {
        version: 8,
        name: "session log usage tracking and CNY pricing fixes",
        db_compat: DbCompatImpact::Additive,
        up: |conn, _| Database::migrate_v7_to_v8(conn),
    },
    Migration {
        version: 9,
        name: "reseed model pricing",
        db_compat: DbCompatImpact::Additive,
        up: |conn, _| Database::migrate_v8_to_v9(conn),
    },
    Migration {
        version: 10,
        name: "Hermes enablement columns",
        db_compat: DbCompatImpact::Additive,
        up: |conn, _| Database::migrate_v9_to_v10(conn),
    },
    Migration {
        version: 11,
        name: "request_model/pricing_model rollup dimensions",
        db_compat: DbCompatImpact::PrePolicy,
        up: |conn, _| Database::migrate_v10_to_v11(conn),
    },
    Migration {
        version: 12,
        name: "profiles table",
        db_compat: DbCompatImpact::Additive,
        up: |conn, _| Database::migrate_v11_to_v12(conn),
    },
    Migration {
        version: 13,
        name: "input token semantics columns",
        db_compat: DbCompatImpact::Additive,
        up: |conn, _| Database::migrate_v12_to_v13(conn),
    },
    Migration {
        version: 14,
        name: "Grok Build proxy_config row",
        db_compat: DbCompatImpact::PrePolicy,
        up: |conn, _| Database::migrate_v13_to_v14(conn),
    },
    Migration {
        version: 15,
        name: "Grok Build enablement for skills and MCP",
        db_compat: DbCompatImpact::Additive,
        up: |conn, _| Database::migrate_v14_to_v15(conn),
    },
    Migration {
        version: 16,
        name: "startup repairs as a numbered step and spend-limit window index",
        db_compat: DbCompatImpact::Additive,
        up: |conn, _| v016::apply(conn),
    },
    Migration {
        version: 17,
        name:
            "mcp_server_apps / skill_apps join tables and proxy_config without the app_type CHECK",
        db_compat: DbCompatImpact::Additive,
        up: |conn, _| v017::apply(conn),
    },
    Migration {
        version: 18,
        name:
            "model_pricing provenance (source, catalog_version, updated_at) and deleted tombstones",
        db_compat: DbCompatImpact::Additive,
        up: |conn, _| v018::apply(conn),
    },
];

impl Database {
    /// 在指定连接上应用 Schema 迁移（全新数据库也走这里，从 v0 开始）
    pub(crate) fn apply_schema_migrations_on_conn(conn: &Connection) -> Result<(), AppError> {
        Self::migrate_on_conn(conn, SCHEMA_VERSION)
    }

    /// 迁移到 `target` 版本为止（测试用于构造某个历史版本的真实数据库）
    #[cfg(test)]
    pub(crate) fn apply_schema_migrations_up_to(
        conn: &Connection,
        target: i32,
    ) -> Result<(), AppError> {
        Self::migrate_on_conn(conn, target)
    }

    fn migrate_on_conn(conn: &Connection, target: i32) -> Result<(), AppError> {
        conn.execute("SAVEPOINT schema_migration;", [])
            .map_err(|e| AppError::Database(format!("开启迁移 savepoint 失败: {e}")))?;

        let version = Self::get_user_version(conn)?;

        if version > SCHEMA_VERSION {
            conn.execute("ROLLBACK TO schema_migration;", []).ok();
            conn.execute("RELEASE schema_migration;", []).ok();
            return Err(AppError::Database(format!(
                "数据库版本过新（{version}），当前应用仅支持 {SCHEMA_VERSION}，请升级应用后再尝试。"
            )));
        }

        let result = (|| {
            let ctx = MigrationContext {
                fresh_database: version == 0 && !Self::has_user_tables(conn)?,
            };
            Self::ensure_migration_audit_table(conn)?;

            for migration in MIGRATIONS
                .iter()
                .filter(|m| m.version > version && m.version <= target)
            {
                let current = Self::get_user_version(conn)?;
                if current != migration.version - 1 {
                    return Err(AppError::Database(format!(
                        "未知的数据库版本 {current}，无法迁移到 {SCHEMA_VERSION}"
                    )));
                }
                log::info!(
                    "迁移数据库从 v{current} 到 v{}（{}）",
                    migration.version,
                    migration.name
                );
                (migration.up)(conn, &ctx).map_err(|e| {
                    AppError::Database(format!(
                        "数据库迁移 v{current} -> v{} 失败: {e}",
                        migration.version
                    ))
                })?;
                Self::set_user_version(conn, migration.version)?;
                Self::record_migration(conn, migration)?;
            }

            let reached = Self::get_user_version(conn)?;
            if reached != target.max(version) {
                return Err(AppError::Database(format!(
                    "未知的数据库版本 {reached}，无法迁移到 {SCHEMA_VERSION}"
                )));
            }
            Ok(())
        })();

        match result {
            Ok(_) => {
                conn.execute("RELEASE schema_migration;", [])
                    .map_err(|e| AppError::Database(format!("提交迁移 savepoint 失败: {e}")))?;
                Ok(())
            }
            Err(e) => {
                conn.execute("ROLLBACK TO schema_migration;", []).ok();
                conn.execute("RELEASE schema_migration;", []).ok();
                Err(e)
            }
        }
    }

    /// 迁移审计表：由迁移框架维护，记录每次执行的迁移步骤（每台设备独立，不随同步覆盖）
    fn ensure_migration_audit_table(conn: &Connection) -> Result<(), AppError> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                id INTEGER PRIMARY KEY,
                version INTEGER NOT NULL,
                name TEXT NOT NULL,
                db_compat TEXT NOT NULL,
                app_version TEXT NOT NULL,
                applied_at INTEGER NOT NULL
            )",
            [],
        )
        .map_err(|e| AppError::Database(format!("创建 schema_migrations 表失败: {e}")))?;
        Ok(())
    }

    fn record_migration(conn: &Connection, migration: &Migration) -> Result<(), AppError> {
        conn.execute(
            "INSERT INTO schema_migrations (version, name, db_compat, app_version, applied_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                migration.version,
                migration.name,
                migration.db_compat.audit_label(),
                env!("CARGO_PKG_VERSION"),
                chrono::Utc::now().timestamp(),
            ],
        )
        .map_err(|e| {
            AppError::Database(format!(
                "记录迁移 v{} 到 schema_migrations 失败: {e}",
                migration.version
            ))
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
