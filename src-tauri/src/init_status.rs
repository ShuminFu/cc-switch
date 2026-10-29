use serde::Serialize;
use std::sync::{OnceLock, RwLock};

#[derive(Debug, Clone, Serialize)]
pub struct InitErrorPayload {
    pub path: String,
    pub error: String,
    /// 错误类别。`Some("db_version_too_new")` 表示数据库版本过新（应用过旧），
    /// 前端据此展示「升级应用」恢复界面而非直接退出。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// 磁盘上数据库的 user_version（数据库版本过新时填充）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub db_version: Option<i32>,
    /// 当前应用支持的 SCHEMA_VERSION（数据库版本过新时填充）。
    /// 当升级到最新版后 db_version 仍 > supported_version，说明可能由第三方客户端创建。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported_version: Option<i32>,
    /// 数据库版本过新时可由当前应用恢复的备份（user_version ≤ SCHEMA_VERSION，
    /// 即更新版应用迁移前留下的快照），新到旧排序；前端据此提供「恢复迁移前备份」。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restorable_backups: Option<Vec<crate::database::backup::RestorableBackup>>,
}

/// 构造 `db_version_too_new` 启动错误：磁盘上的数据库版本 `version` 高于
/// 当前应用支持的 SCHEMA_VERSION，附带可恢复的迁移前备份列表。
pub fn db_version_too_new_payload(db_path: &std::path::Path, version: i32) -> InitErrorPayload {
    let supported = crate::database::SCHEMA_VERSION;
    let restorable_backups =
        crate::database::Database::list_restorable_backups().unwrap_or_else(|e| {
            log::warn!("列出可恢复的数据库备份失败: {e}");
            Vec::new()
        });
    InitErrorPayload {
        path: db_path.display().to_string(),
        error: format!(
            "数据库版本过新（{version}），当前应用仅支持 {supported}，请升级应用后再尝试。"
        ),
        kind: Some(DB_VERSION_TOO_NEW.to_string()),
        db_version: Some(version),
        supported_version: Some(supported),
        restorable_backups: Some(restorable_backups),
    }
}

/// [`InitErrorPayload::kind`]：数据库版本过新（应用过旧）
pub const DB_VERSION_TOO_NEW: &str = "db_version_too_new";

static INIT_ERROR: OnceLock<RwLock<Option<InitErrorPayload>>> = OnceLock::new();

fn cell() -> &'static RwLock<Option<InitErrorPayload>> {
    INIT_ERROR.get_or_init(|| RwLock::new(None))
}

pub fn set_init_error(payload: InitErrorPayload) {
    #[allow(clippy::unwrap_used)]
    if let Ok(mut guard) = cell().write() {
        *guard = Some(payload);
    }
}

pub fn get_init_error() -> Option<InitErrorPayload> {
    cell().read().ok()?.clone()
}

// ============================================================
// 迁移结果状态
// ============================================================

static MIGRATION_SUCCESS: OnceLock<RwLock<bool>> = OnceLock::new();

fn migration_cell() -> &'static RwLock<bool> {
    MIGRATION_SUCCESS.get_or_init(|| RwLock::new(false))
}

pub fn set_migration_success() {
    if let Ok(mut guard) = migration_cell().write() {
        *guard = true;
    }
}

/// 获取并消费迁移成功状态（只返回一次 true，之后返回 false）
pub fn take_migration_success() -> bool {
    if let Ok(mut guard) = migration_cell().write() {
        let val = *guard;
        *guard = false;
        val
    } else {
        false
    }
}

// ============================================================
// Skills SSOT 迁移结果状态
// ============================================================

#[derive(Debug, Clone, Serialize)]
pub struct SkillsMigrationPayload {
    pub count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

static SKILLS_MIGRATION_RESULT: OnceLock<RwLock<Option<SkillsMigrationPayload>>> = OnceLock::new();

fn skills_migration_cell() -> &'static RwLock<Option<SkillsMigrationPayload>> {
    SKILLS_MIGRATION_RESULT.get_or_init(|| RwLock::new(None))
}

pub fn set_skills_migration_result(count: usize) {
    if let Ok(mut guard) = skills_migration_cell().write() {
        *guard = Some(SkillsMigrationPayload { count, error: None });
    }
}

pub fn set_skills_migration_error(error: String) {
    if let Ok(mut guard) = skills_migration_cell().write() {
        *guard = Some(SkillsMigrationPayload {
            count: 0,
            error: Some(error),
        });
    }
}

/// 获取并消费 Skills 迁移结果（只返回一次 Some，之后返回 None）
pub fn take_skills_migration_result() -> Option<SkillsMigrationPayload> {
    if let Ok(mut guard) = skills_migration_cell().write() {
        guard.take()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_error_roundtrip() {
        let payload = InitErrorPayload {
            path: "/tmp/config.json".into(),
            error: "broken json".into(),
            kind: None,
            db_version: None,
            supported_version: None,
            restorable_backups: None,
        };
        set_init_error(payload.clone());
        let got = get_init_error().expect("should get payload back");
        assert_eq!(got.path, payload.path);
        assert_eq!(got.error, payload.error);
    }
}
