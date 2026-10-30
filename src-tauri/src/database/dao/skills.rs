//! Skills 数据访问对象
//!
//! 提供 Skills 和 Skill Repos 的 CRUD 操作。
//!
//! v3.10.0+ 统一管理架构：
//! - Skills 使用统一的 id 主键，应用启用状态存于关联表 `skill_apps`
//!   （见 [`super::app_sets`]），本模块不按应用命名任何列
//! - 实际文件存储在 ~/.cc-switch/skills/，同步到各应用目录

use super::app_sets::SKILL_APPS;
use crate::app_config::{AppSet, InstalledSkill};
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::services::skill::SkillRepo;
use indexmap::IndexMap;
use rusqlite::{params, Row};

const SKILL_COLUMNS: &str = "id, name, description, directory, repo_owner, repo_name, repo_branch,
                        readme_url, installed_at, content_hash, updated_at";

/// 按列名读取一行 skills（`apps` 由调用方从关联表填入）
fn skill_from_row(row: &Row<'_>) -> rusqlite::Result<InstalledSkill> {
    Ok(InstalledSkill {
        id: row.get("id")?,
        name: row.get("name")?,
        description: row.get("description")?,
        directory: row.get("directory")?,
        repo_owner: row.get("repo_owner")?,
        repo_name: row.get("repo_name")?,
        repo_branch: row.get("repo_branch")?,
        readme_url: row.get("readme_url")?,
        apps: AppSet::default(),
        installed_at: row.get("installed_at")?,
        content_hash: row.get("content_hash")?,
        updated_at: row.get::<_, i64>("updated_at").unwrap_or(0),
    })
}

impl Database {
    // ========== InstalledSkill CRUD ==========

    /// 获取所有已安装的 Skills
    pub fn get_all_installed_skills(&self) -> Result<IndexMap<String, InstalledSkill>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut app_sets = SKILL_APPS.read_all(&conn)?;
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {SKILL_COLUMNS} FROM skills ORDER BY name ASC"
            ))
            .map_err(|e| AppError::Database(e.to_string()))?;

        let skill_iter = stmt
            .query_map([], skill_from_row)
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut skills = IndexMap::new();
        for skill_res in skill_iter {
            let mut skill = skill_res.map_err(|e| AppError::Database(e.to_string()))?;
            skill.apps = app_sets.remove(&skill.id).unwrap_or_default();
            skills.insert(skill.id.clone(), skill);
        }
        Ok(skills)
    }

    /// 获取单个已安装的 Skill
    pub fn get_installed_skill(&self, id: &str) -> Result<Option<InstalledSkill>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(&format!("SELECT {SKILL_COLUMNS} FROM skills WHERE id = ?1"))
            .map_err(|e| AppError::Database(e.to_string()))?;

        let result = stmt.query_row([id], skill_from_row);

        match result {
            Ok(mut skill) => {
                skill.apps = SKILL_APPS.read(&conn, id)?;
                Ok(Some(skill))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }

    /// 保存 Skill（添加或更新）
    pub fn save_skill(&self, skill: &InstalledSkill) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .savepoint()
            .map_err(|e| AppError::Database(e.to_string()))?;
        tx.execute(
            "INSERT OR REPLACE INTO skills
             (id, name, description, directory, repo_owner, repo_name, repo_branch,
              readme_url, installed_at, content_hash, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                skill.id,
                skill.name,
                skill.description,
                skill.directory,
                skill.repo_owner,
                skill.repo_name,
                skill.repo_branch,
                skill.readme_url,
                skill.installed_at,
                skill.content_hash,
                skill.updated_at,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        SKILL_APPS.write(&tx, &skill.id, &skill.apps)?;
        tx.commit().map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 删除 Skill
    pub fn delete_skill(&self, id: &str) -> Result<bool, AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .savepoint()
            .map_err(|e| AppError::Database(e.to_string()))?;
        let affected = tx
            .execute("DELETE FROM skills WHERE id = ?1", params![id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        SKILL_APPS.delete(&tx, id)?;
        tx.commit().map_err(|e| AppError::Database(e.to_string()))?;
        Ok(affected > 0)
    }

    /// 清空所有 Skills（用于迁移）
    pub fn clear_skills(&self) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .savepoint()
            .map_err(|e| AppError::Database(e.to_string()))?;
        tx.execute("DELETE FROM skills", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        SKILL_APPS.clear(&tx)?;
        tx.commit().map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 更新 Skill 的应用启用状态
    pub fn update_skill_apps(&self, id: &str, apps: &AppSet) -> Result<bool, AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .savepoint()
            .map_err(|e| AppError::Database(e.to_string()))?;
        let exists = tx
            .query_row("SELECT 1 FROM skills WHERE id = ?1", [id], |_| Ok(()))
            .map(|_| true)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(false),
                e => Err(AppError::Database(e.to_string())),
            })?;
        if exists {
            SKILL_APPS.write(&tx, id, apps)?;
        }
        tx.commit().map_err(|e| AppError::Database(e.to_string()))?;
        Ok(exists)
    }

    /// 更新 Skill 的内容哈希和更新时间
    pub fn update_skill_hash(
        &self,
        id: &str,
        content_hash: &str,
        updated_at: i64,
    ) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let affected = conn
            .execute(
                "UPDATE skills SET content_hash = ?1, updated_at = ?2 WHERE id = ?3",
                params![content_hash, updated_at, id],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(affected > 0)
    }

    // ========== SkillRepo CRUD（保持原有） ==========

    /// 获取所有 Skill 仓库
    pub fn get_skill_repos(&self) -> Result<Vec<SkillRepo>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT owner, name, branch, enabled FROM skill_repos ORDER BY owner ASC, name ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let repo_iter = stmt
            .query_map([], |row| {
                Ok(SkillRepo {
                    owner: row.get(0)?,
                    name: row.get(1)?,
                    branch: row.get(2)?,
                    enabled: row.get(3)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut repos = Vec::new();
        for repo_res in repo_iter {
            repos.push(repo_res.map_err(|e| AppError::Database(e.to_string()))?);
        }
        Ok(repos)
    }

    /// 保存 Skill 仓库
    pub fn save_skill_repo(&self, repo: &SkillRepo) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO skill_repos (owner, name, branch, enabled) VALUES (?1, ?2, ?3, ?4)",
            params![repo.owner, repo.name, repo.branch, repo.enabled],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 删除 Skill 仓库
    pub fn delete_skill_repo(&self, owner: &str, name: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "DELETE FROM skill_repos WHERE owner = ?1 AND name = ?2",
            params![owner, name],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 初始化默认的 Skill 仓库（启动时调用，每个数据库仅执行一次）
    pub fn init_default_skill_repos(&self) -> Result<usize, AppError> {
        const INITIALIZED_KEY: &str = "default_skill_repos_initialized";

        if self.get_bool_flag(INITIALIZED_KEY)? {
            return Ok(0);
        }

        // 兼容升级前已经存在的用户选择，并记录初始化状态，避免以后删空后恢复默认值。
        if !self.get_skill_repos()?.is_empty() {
            self.set_setting(INITIALIZED_KEY, "true")?;
            return Ok(0);
        }

        let default_store = crate::services::skill::SkillStore::default();
        let mut count = 0;

        for repo in &default_store.repos {
            self.save_skill_repo(repo)?;
            count += 1;
            log::info!("初始化默认 Skill 仓库: {}/{}", repo.owner, repo.name);
        }

        self.set_setting(INITIALIZED_KEY, "true")?;
        Ok(count)
    }
}
