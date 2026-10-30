//! MCP 服务器 / Skill 的应用启用关联表（CCS-156）
//!
//! 启用状态以 `(owner_id, app_type)` 行存在关联表 `mcp_server_apps` / `skill_apps` 中，
//! 是唯一的读取来源；新增应用不需要改 DAO，也不需要 Schema 迁移。
//!
//! 为了混合版本的 WebDAV/S3 同步与降级，写入时仍同步更新 v17 之前的每应用列
//! `enabled_<slug>`（见 [`LEGACY_FLAG_APPS`]），保留一个版本后移除。

use std::collections::HashMap;

use rusqlite::{params, params_from_iter, types::Value, Connection};

use crate::app_config::{AppSet, AppType};
use crate::apps::registry::{descriptor, with_caps};
use crate::apps::AppCaps;
use crate::error::AppError;

/// v17 之前主表上每应用一列的旧启用列 `enabled_<slug>`（冻结，新增应用不再加列）
pub(crate) const LEGACY_FLAG_APPS: &[&str] = &[
    "claude",
    "codex",
    "gemini",
    "grokbuild",
    "opencode",
    "hermes",
];

/// 一张应用启用关联表
pub(crate) struct AppJoinTable {
    /// 关联表名
    pub(crate) table: &'static str,
    /// 关联表中指向主表 `id` 的列
    pub(crate) owner_column: &'static str,
    /// 主表名（`id` 为主键，带旧的 `enabled_<slug>` 列）
    pub(crate) owner_table: &'static str,
    /// 应用必须具备的能力位：`AppSet` 由 MCP 与 Skills 共用，没有该能力的应用
    ///（例如只有 MCP 能力的 OpenClaw 之于 `skill_apps`）在本表中视为本版本不认识
    pub(crate) cap: AppCaps,
}

pub(crate) const MCP_SERVER_APPS: AppJoinTable = AppJoinTable {
    table: "mcp_server_apps",
    owner_column: "server_id",
    owner_table: "mcp_servers",
    cap: AppCaps::MCP,
};

pub(crate) const SKILL_APPS: AppJoinTable = AppJoinTable {
    table: "skill_apps",
    owner_column: "skill_id",
    owner_table: "skills",
    cap: AppCaps::SKILLS,
};

fn db_err(e: rusqlite::Error) -> AppError {
    AppError::Database(e.to_string())
}

impl AppJoinTable {
    /// 关联表中的 app_type 解析为应用；本版本不认识的应用（更新版本写入）或
    /// 不具备本表能力的应用跳过
    fn parse_app(&self, slug: &str) -> Option<AppType> {
        match slug.parse::<AppType>() {
            Ok(app) if descriptor(&app).has(self.cap) => Some(app),
            _ => {
                log::debug!("{}: 忽略未知应用 '{slug}'", self.table);
                None
            }
        }
    }

    /// 读取全部主表行的启用集合（没有任何关联行的 id 不出现在结果中）
    pub(crate) fn read_all(&self, conn: &Connection) -> Result<HashMap<String, AppSet>, AppError> {
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {owner} AS owner_id, app_type FROM {table}",
                owner = self.owner_column,
                table = self.table
            ))
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>("owner_id")?,
                    row.get::<_, String>("app_type")?,
                ))
            })
            .map_err(db_err)?;

        let mut sets: HashMap<String, AppSet> = HashMap::new();
        for row in rows {
            let (id, slug) = row.map_err(db_err)?;
            if let Some(app) = self.parse_app(&slug) {
                sets.entry(id).or_default().set_enabled_for(&app, true);
            }
        }
        Ok(sets)
    }

    /// 读取单个主表行的启用集合
    pub(crate) fn read(&self, conn: &Connection, id: &str) -> Result<AppSet, AppError> {
        let mut stmt = conn
            .prepare(&format!(
                "SELECT app_type FROM {table} WHERE {owner} = ?1",
                owner = self.owner_column,
                table = self.table
            ))
            .map_err(db_err)?;
        let rows = stmt
            .query_map([id], |row| row.get::<_, String>("app_type"))
            .map_err(db_err)?;

        let mut apps = AppSet::default();
        for slug in rows {
            if let Some(app) = self.parse_app(&slug.map_err(db_err)?) {
                apps.set_enabled_for(&app, true);
            }
        }
        Ok(apps)
    }

    /// 用 `apps` 替换主表行 `id` 的启用集合，并双写旧的 `enabled_<slug>` 列
    ///
    /// 只替换本版本具备本表能力的应用的行：同一 Schema 版本下更新的构建可能已为
    /// 新应用（或新获得 MCP / Skills 能力的已有应用）写入行（不需要迁移），这些行必须原样保留；
    /// `apps` 中不具备本表能力的应用不写入。
    /// 调用方负责主表行已存在，并把本调用与主表写入放在同一事务中。
    pub(crate) fn write(&self, conn: &Connection, id: &str, apps: &AppSet) -> Result<(), AppError> {
        let known: Vec<&str> = with_caps(self.cap).map(|d| d.slug).collect();
        let placeholders = (0..known.len())
            .map(|i| format!("?{}", i + 2))
            .collect::<Vec<_>>()
            .join(", ");
        let mut values = vec![Value::Text(id.to_string())];
        values.extend(known.iter().map(|slug| Value::Text(slug.to_string())));
        conn.execute(
            &format!(
                "DELETE FROM {table} WHERE {owner} = ?1 AND app_type IN ({placeholders})",
                owner = self.owner_column,
                table = self.table
            ),
            params_from_iter(values),
        )
        .map_err(db_err)?;

        let mut insert = conn
            .prepare(&format!(
                "INSERT INTO {table} ({owner}, app_type) VALUES (?1, ?2)",
                owner = self.owner_column,
                table = self.table
            ))
            .map_err(db_err)?;
        for app in apps.iter().filter(|app| descriptor(app).has(self.cap)) {
            insert.execute(params![id, app.as_str()]).map_err(db_err)?;
        }

        let assignments = LEGACY_FLAG_APPS
            .iter()
            .enumerate()
            .map(|(i, slug)| format!("enabled_{slug} = ?{}", i + 1))
            .collect::<Vec<_>>()
            .join(", ");
        let mut values: Vec<Value> = LEGACY_FLAG_APPS
            .iter()
            .map(|slug| {
                let enabled = apps.iter().any(|app| app.as_str() == *slug);
                Value::Integer(i64::from(enabled))
            })
            .collect();
        values.push(Value::Text(id.to_string()));
        conn.execute(
            &format!(
                "UPDATE {owner_table} SET {assignments} WHERE id = ?{}",
                LEGACY_FLAG_APPS.len() + 1,
                owner_table = self.owner_table
            ),
            params_from_iter(values),
        )
        .map_err(db_err)?;
        Ok(())
    }

    /// 删除主表行 `id` 的全部关联行（含本版本不认识的应用）
    pub(crate) fn delete(&self, conn: &Connection, id: &str) -> Result<(), AppError> {
        conn.execute(
            &format!(
                "DELETE FROM {table} WHERE {owner} = ?1",
                owner = self.owner_column,
                table = self.table
            ),
            [id],
        )
        .map_err(db_err)?;
        Ok(())
    }

    /// 清空关联表
    pub(crate) fn clear(&self, conn: &Connection) -> Result<(), AppError> {
        conn.execute(&format!("DELETE FROM {}", self.table), [])
            .map_err(db_err)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_config::{InstalledSkill, McpServer};
    use crate::database::{lock_conn, Database};

    fn server(id: &str, apps: AppSet) -> McpServer {
        McpServer {
            id: id.to_string(),
            name: id.to_string(),
            server: serde_json::json!({ "type": "stdio", "command": "echo" }),
            apps,
            description: None,
            homepage: None,
            docs: None,
            tags: Vec::new(),
        }
    }

    fn skill(id: &str, apps: AppSet) -> InstalledSkill {
        InstalledSkill {
            id: id.to_string(),
            name: id.to_string(),
            description: None,
            directory: id.to_string(),
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            readme_url: None,
            apps,
            installed_at: 1,
            content_hash: None,
            updated_at: 0,
        }
    }

    fn join_rows(db: &Database, table: &AppJoinTable, id: &str) -> Result<Vec<String>, AppError> {
        let conn = lock_conn!(db.conn);
        let mut stmt = conn.prepare(&format!(
            "SELECT app_type FROM {} WHERE {} = ?1 ORDER BY app_type",
            table.table, table.owner_column
        ))?;
        let rows = stmt
            .query_map([id], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(rows)
    }

    /// 旧版本 DAO 读取的每应用列（`enabled_<slug>`），按 [`LEGACY_FLAG_APPS`] 顺序
    fn legacy_flags(db: &Database, owner_table: &str, id: &str) -> Result<Vec<bool>, AppError> {
        let conn = lock_conn!(db.conn);
        let columns = LEGACY_FLAG_APPS
            .iter()
            .map(|slug| format!("enabled_{slug}"))
            .collect::<Vec<_>>()
            .join(", ");
        let flags = conn.query_row(
            &format!("SELECT {columns} FROM {owner_table} WHERE id = ?1"),
            [id],
            |row| {
                (0..LEGACY_FLAG_APPS.len())
                    .map(|i| row.get::<_, bool>(i))
                    .collect::<rusqlite::Result<Vec<bool>>>()
            },
        )?;
        Ok(flags)
    }

    #[test]
    fn mcp_apps_live_in_the_join_table_and_dual_write_legacy_columns() -> Result<(), AppError> {
        let db = Database::memory()?;
        db.save_mcp_server(&server(
            "echo",
            AppSet::from_apps(&[AppType::Claude, AppType::Hermes]),
        ))?;

        assert_eq!(
            join_rows(&db, &MCP_SERVER_APPS, "echo")?,
            ["claude", "hermes"]
        );
        assert_eq!(
            legacy_flags(&db, "mcp_servers", "echo")?,
            [true, false, false, false, false, true]
        );
        let loaded = db.get_all_mcp_servers()?;
        assert_eq!(
            loaded["echo"].apps,
            AppSet::from_apps(&[AppType::Claude, AppType::Hermes])
        );

        db.save_mcp_server(&server("echo", AppSet::only(&AppType::Codex)))?;
        assert_eq!(join_rows(&db, &MCP_SERVER_APPS, "echo")?, ["codex"]);
        assert_eq!(
            legacy_flags(&db, "mcp_servers", "echo")?,
            [false, true, false, false, false, false]
        );

        db.delete_mcp_server("echo")?;
        assert!(join_rows(&db, &MCP_SERVER_APPS, "echo")?.is_empty());
        Ok(())
    }

    #[test]
    fn skill_apps_live_in_the_join_table_and_dual_write_legacy_columns() -> Result<(), AppError> {
        let db = Database::memory()?;
        db.save_skill(&skill("s1", AppSet::only(&AppType::OpenCode)))?;
        assert_eq!(join_rows(&db, &SKILL_APPS, "s1")?, ["opencode"]);
        assert_eq!(
            legacy_flags(&db, "skills", "s1")?,
            [false, false, false, false, true, false]
        );

        let apps = AppSet::from_apps(&[AppType::Gemini, AppType::GrokBuild]);
        assert!(db.update_skill_apps("s1", &apps)?);
        assert!(!db.update_skill_apps("missing", &apps)?);
        assert!(join_rows(&db, &SKILL_APPS, "missing")?.is_empty());
        assert_eq!(
            legacy_flags(&db, "skills", "s1")?,
            [false, false, true, true, false, false]
        );
        assert_eq!(db.get_installed_skill("s1")?.expect("s1").apps, apps);
        assert_eq!(db.get_all_installed_skills()?["s1"].apps, apps);

        db.clear_skills()?;
        assert!(join_rows(&db, &SKILL_APPS, "s1")?.is_empty());
        Ok(())
    }

    /// 同一 Schema 版本下更新的构建为新应用写入的行：读取时忽略，保存时保留
    #[test]
    fn rows_for_apps_this_build_does_not_know_survive_a_save() -> Result<(), AppError> {
        let db = Database::memory()?;
        db.save_mcp_server(&server("echo", AppSet::only(&AppType::Claude)))?;
        db.save_skill(&skill("s1", AppSet::only(&AppType::Claude)))?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO mcp_server_apps (server_id, app_type) VALUES ('echo', 'ninth')",
                [],
            )?;
            conn.execute(
                "INSERT INTO skill_apps (skill_id, app_type) VALUES ('s1', 'ninth')",
                [],
            )?;
            // 本版本认识但本表能力不接纳的应用（例如 OpenClaw 在更新的构建中获得 Skills 能力）
            conn.execute(
                "INSERT INTO skill_apps (skill_id, app_type) VALUES ('s1', 'openclaw')",
                [],
            )?;
        }

        assert_eq!(
            db.get_all_mcp_servers()?["echo"].apps,
            AppSet::only(&AppType::Claude)
        );
        db.save_mcp_server(&server("echo", AppSet::only(&AppType::Codex)))?;
        assert_eq!(
            join_rows(&db, &MCP_SERVER_APPS, "echo")?,
            ["codex", "ninth"]
        );

        assert_eq!(
            db.get_installed_skill("s1")?.expect("s1").apps,
            AppSet::only(&AppType::Claude)
        );
        db.update_skill_apps("s1", &AppSet::default())?;
        assert_eq!(join_rows(&db, &SKILL_APPS, "s1")?, ["ninth", "openclaw"]);
        assert!(db.get_installed_skill("s1")?.expect("s1").apps.is_empty());
        Ok(())
    }

    /// OpenClaw 只有 MCP 能力（CCS-164）：MCP 关联表存取它，Skill 关联表不写入它
    #[test]
    fn each_join_table_only_stores_apps_with_its_capability() -> Result<(), AppError> {
        let db = Database::memory()?;
        let apps = AppSet::from_apps(&[AppType::Claude, AppType::OpenClaw]);
        assert!(apps.is_enabled_for(&AppType::OpenClaw));

        db.save_mcp_server(&server("echo", apps.clone()))?;
        assert_eq!(
            join_rows(&db, &MCP_SERVER_APPS, "echo")?,
            ["claude", "openclaw"]
        );
        assert_eq!(db.get_all_mcp_servers()?["echo"].apps, apps);

        db.save_skill(&skill("s1", apps))?;
        assert_eq!(join_rows(&db, &SKILL_APPS, "s1")?, ["claude"]);
        assert_eq!(
            db.get_installed_skill("s1")?.expect("s1").apps,
            AppSet::only(&AppType::Claude)
        );
        Ok(())
    }
}
