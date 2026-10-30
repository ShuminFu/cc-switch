//! Skills 命令层
//!
//! v3.10.0+ 统一管理架构：
//! - 支持三应用开关（Claude/Codex/Gemini）
//! - SSOT 存储在 ~/.cc-switch/skills/

use crate::app_config::{AppType, InstalledSkill, UnmanagedSkill};
use crate::error::{ErrorCode, IpcError};
use crate::services::skill::{
    DiscoverableSkill, ImportSkillSelection, MigrationResult, Skill, SkillBackupEntry, SkillRepo,
    SkillService, SkillStorageLocation, SkillUninstallResult, SkillUpdateInfo,
    SkillsShSearchResult,
};
use crate::store::AppState;
use std::str::FromStr;
use std::sync::Arc;
use tauri::State;

/// SkillService 状态包装
pub struct SkillServiceState(pub Arc<SkillService>);

/// 解析 app 参数为 AppType
fn parse_app_type(app: &str) -> Result<AppType, String> {
    AppType::from_str(app).map_err(|e| e.to_string())
}

// ========== 统一管理命令 ==========

/// 获取所有已安装的 Skills
#[tauri::command]
pub fn get_installed_skills(
    app_state: State<'_, AppState>,
) -> Result<Vec<InstalledSkill>, IpcError> {
    SkillService::get_all_installed(&app_state.db).map_err(IpcError::from)
}

#[tauri::command]
pub fn get_skill_backups() -> Result<Vec<SkillBackupEntry>, IpcError> {
    SkillService::list_backups().map_err(IpcError::from)
}

#[tauri::command]
pub fn delete_skill_backup(backup_id: String) -> Result<bool, IpcError> {
    SkillService::delete_backup(&backup_id).map_err(IpcError::from)?;
    Ok(true)
}

/// 安装 Skill（新版统一安装）
///
/// 参数：
/// - skill: 从发现列表获取的技能信息
/// - current_app: 当前选中的应用，安装后默认启用该应用
#[tauri::command]
pub async fn install_skill_unified(
    skill: DiscoverableSkill,
    current_app: String,
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<InstalledSkill, IpcError> {
    let app_type = parse_app_type(&current_app)?;

    service
        .0
        .install(&app_state.db, &skill, &app_type)
        .await
        .map_err(IpcError::from)
}

/// 卸载 Skill（新版统一卸载）
#[tauri::command]
pub fn uninstall_skill_unified(
    id: String,
    app_state: State<'_, AppState>,
) -> Result<SkillUninstallResult, IpcError> {
    SkillService::uninstall(&app_state.db, &id).map_err(IpcError::from)
}

#[tauri::command]
pub fn restore_skill_backup(
    backup_id: String,
    current_app: String,
    app_state: State<'_, AppState>,
) -> Result<InstalledSkill, IpcError> {
    let app_type = parse_app_type(&current_app)?;
    SkillService::restore_from_backup(&app_state.db, &backup_id, &app_type).map_err(IpcError::from)
}

/// 切换 Skill 的应用启用状态
#[tauri::command]
pub fn toggle_skill_app(
    id: String,
    app: String,
    enabled: bool,
    app_state: State<'_, AppState>,
) -> Result<bool, IpcError> {
    let app_type = parse_app_type(&app)?;
    SkillService::toggle_app(&app_state.db, &id, &app_type, enabled).map_err(IpcError::from)?;
    Ok(true)
}

/// 扫描未管理的 Skills
#[tauri::command]
pub fn scan_unmanaged_skills(
    app_state: State<'_, AppState>,
) -> Result<Vec<UnmanagedSkill>, IpcError> {
    SkillService::scan_unmanaged(&app_state.db).map_err(IpcError::from)
}

/// 从应用目录导入 Skills
#[tauri::command]
pub fn import_skills_from_apps(
    imports: Vec<ImportSkillSelection>,
    app_state: State<'_, AppState>,
) -> Result<Vec<InstalledSkill>, IpcError> {
    SkillService::import_from_apps(&app_state.db, imports).map_err(IpcError::from)
}

// ========== 发现功能命令 ==========

/// 发现可安装的 Skills（从仓库获取）
#[tauri::command]
pub async fn discover_available_skills(
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<Vec<DiscoverableSkill>, IpcError> {
    let repos = SkillService::list_db_repos(&app_state.db).map_err(IpcError::from)?;
    service
        .0
        .discover_available(repos)
        .await
        .map_err(IpcError::from)
}

/// 检查 Skills 更新
#[tauri::command]
pub async fn check_skill_updates(
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<Vec<SkillUpdateInfo>, IpcError> {
    service
        .0
        .check_updates(&app_state.db)
        .await
        .map_err(IpcError::from)
}

/// 更新单个 Skill
#[tauri::command]
pub async fn update_skill(
    id: String,
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<InstalledSkill, IpcError> {
    service
        .0
        .update_skill(&app_state.db, &id)
        .await
        .map_err(IpcError::from)
}

/// 迁移 Skill 存储位置
#[tauri::command]
pub async fn migrate_skill_storage(
    target: SkillStorageLocation,
    app_state: State<'_, AppState>,
) -> Result<MigrationResult, IpcError> {
    SkillService::migrate_storage(&app_state.db, target).map_err(IpcError::from)
}

/// 搜索 skills.sh 公共目录
#[tauri::command]
pub async fn search_skills_sh(
    query: String,
    limit: usize,
    offset: usize,
) -> Result<SkillsShSearchResult, IpcError> {
    SkillService::search_skills_sh(&query, limit, offset)
        .await
        .map_err(IpcError::from)
}

// ========== 兼容旧 API 的命令 ==========

/// 获取技能列表（兼容旧 API）
#[tauri::command]
pub async fn get_skills(
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<Vec<Skill>, IpcError> {
    let repos = SkillService::list_db_repos(&app_state.db).map_err(IpcError::from)?;
    service
        .0
        .list_skills(repos, &app_state.db)
        .await
        .map_err(IpcError::from)
}

/// 获取指定应用的技能列表（兼容旧 API）
#[tauri::command]
pub async fn get_skills_for_app(
    app: String,
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<Vec<Skill>, IpcError> {
    // 新版本不再区分应用，统一返回所有技能
    let _ = parse_app_type(&app)?; // 验证 app 参数有效
    get_skills(service, app_state).await
}

/// 安装技能（兼容旧 API）
#[tauri::command]
pub async fn install_skill(
    directory: String,
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<bool, IpcError> {
    install_skill_for_app("claude".to_string(), directory, service, app_state).await
}

/// 安装指定应用的技能（兼容旧 API）
#[tauri::command]
pub async fn install_skill_for_app(
    app: String,
    directory: String,
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<bool, IpcError> {
    let app_type = parse_app_type(&app)?;

    // 先获取技能信息
    let repos = SkillService::list_db_repos(&app_state.db).map_err(IpcError::from)?;
    let skills = service
        .0
        .discover_available(repos)
        .await
        .map_err(IpcError::from)?;

    let skill = skills
        .into_iter()
        .find(|s| {
            let install_name = std::path::Path::new(&s.directory)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| s.directory.clone());
            install_name.eq_ignore_ascii_case(&directory)
                || s.directory.eq_ignore_ascii_case(&directory)
        })
        .ok_or_else(|| {
            IpcError::skill(
                ErrorCode::SkillNotFound,
                &[("directory", &directory)],
                Some("checkRepoUrl"),
            )
        })?;

    service
        .0
        .install(&app_state.db, &skill, &app_type)
        .await
        .map_err(IpcError::from)?;

    Ok(true)
}

/// 卸载技能（兼容旧 API）
#[tauri::command]
pub fn uninstall_skill(
    directory: String,
    app_state: State<'_, AppState>,
) -> Result<SkillUninstallResult, IpcError> {
    uninstall_skill_for_app("claude".to_string(), directory, app_state)
}

/// 卸载指定应用的技能（兼容旧 API）
#[tauri::command]
pub fn uninstall_skill_for_app(
    app: String,
    directory: String,
    app_state: State<'_, AppState>,
) -> Result<SkillUninstallResult, IpcError> {
    let _ = parse_app_type(&app)?; // 验证参数

    // 通过 directory 找到对应的 skill id
    let skills = SkillService::get_all_installed(&app_state.db).map_err(IpcError::from)?;

    let skill = skills
        .into_iter()
        .find(|s| s.directory.eq_ignore_ascii_case(&directory))
        .ok_or_else(|| format!("未找到已安装的 Skill: {directory}"))?;

    SkillService::uninstall(&app_state.db, &skill.id).map_err(IpcError::from)
}

// ========== 仓库管理命令 ==========

/// 获取技能仓库列表
#[tauri::command]
pub fn get_skill_repos(app_state: State<'_, AppState>) -> Result<Vec<SkillRepo>, IpcError> {
    SkillService::list_db_repos(&app_state.db).map_err(IpcError::from)
}

/// 添加技能仓库
#[tauri::command]
pub fn add_skill_repo(repo: SkillRepo, app_state: State<'_, AppState>) -> Result<bool, IpcError> {
    SkillService::save_db_repo(&app_state.db, &repo).map_err(IpcError::from)?;
    Ok(true)
}

/// 删除技能仓库
#[tauri::command]
pub fn remove_skill_repo(
    owner: String,
    name: String,
    app_state: State<'_, AppState>,
) -> Result<bool, IpcError> {
    SkillService::remove_db_repo(&app_state.db, &owner, &name).map_err(IpcError::from)?;
    Ok(true)
}

/// 从 ZIP 文件安装 Skills
#[tauri::command]
pub fn install_skills_from_zip(
    file_path: String,
    current_app: String,
    app_state: State<'_, AppState>,
) -> Result<Vec<InstalledSkill>, IpcError> {
    let app_type = parse_app_type(&current_app)?;
    install_skills_from_picked_zip(&app_state.db, &file_path, &app_type)
}

/// 只接受由 `open_zip_file_dialog` 选出的 ZIP 路径
fn install_skills_from_picked_zip(
    db: &Arc<crate::database::Database>,
    file_path: &str,
    app_type: &AppType,
) -> Result<Vec<InstalledSkill>, IpcError> {
    crate::picked_paths::require_picked(crate::picked_paths::PickPurpose::SkillZip, file_path)?;
    let path = std::path::Path::new(file_path);

    SkillService::install_from_zip(db, path, app_type).map_err(IpcError::from)
}

#[cfg(test)]
mod install_from_zip_path_tests {
    use super::*;
    use crate::database::Database;
    use crate::picked_paths::{self, PickPurpose};
    use std::io::Write;

    fn write_zip_without_skills(path: &std::path::Path) {
        let file = std::fs::File::create(path).expect("create zip");
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("README.txt", zip::write::SimpleFileOptions::default())
            .expect("start file");
        zip.write_all(b"no skills here").expect("write entry");
        zip.finish().expect("finish zip");
    }

    #[test]
    fn install_rejects_zip_path_not_chosen_by_dialog() {
        let temp = tempfile::TempDir::new().expect("tempdir");
        let zip_path = temp.path().join("attacker.zip");
        write_zip_without_skills(&zip_path);
        let zip_str = zip_path.to_string_lossy().to_string();
        let db = Arc::new(Database::memory().expect("memory db"));

        let err = install_skills_from_picked_zip(&db, &zip_str, &AppType::Claude).unwrap_err();
        assert!(err.message.contains("file dialog"), "{err}");

        // SQL 对话框选出的路径不能用于安装 Skills
        picked_paths::register(PickPurpose::SqlImport, &zip_str);
        let err = install_skills_from_picked_zip(&db, &zip_str, &AppType::Claude).unwrap_err();
        assert!(err.message.contains("file dialog"), "{err}");
    }

    #[test]
    fn install_accepts_zip_path_chosen_by_dialog() {
        let temp = tempfile::TempDir::new().expect("tempdir");
        let zip_path = temp.path().join("picked.zip");
        write_zip_without_skills(&zip_path);
        let zip_str = zip_path.to_string_lossy().to_string();
        let db = Arc::new(Database::memory().expect("memory db"));

        picked_paths::register(PickPurpose::SkillZip, &zip_str);
        // 校验通过后进入解压流程；该 ZIP 不含 SKILL.md，因此报告 NO_SKILLS_IN_ZIP
        let err = install_skills_from_picked_zip(&db, &zip_str, &AppType::Claude).unwrap_err();
        assert!(!err.message.contains("file dialog"), "{err}");
        assert_eq!(err.code, ErrorCode::NoSkillsInZip, "{err}");
    }
}
