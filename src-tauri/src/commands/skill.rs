//! Skills 命令层
//!
//! v3.10.0+ 统一管理架构：
//! - 支持三应用开关（Claude/Codex/Gemini）
//! - SSOT 存储在 ~/.cc-switch/skills/

use crate::app_config::{AppType, InstalledSkill, UnmanagedSkill};
use crate::error::format_skill_error;
use crate::services::skill::{
    DiscoverableSkill, ImportSkillSelection, MigrationResult, Skill, SkillBackupEntry, SkillRepo,
    SkillService, SkillStorageLocation, SkillUninstallResult, SkillUpdateInfo,
    SkillsShSearchResult,
};
use crate::store::AppState;
use std::str::FromStr;
use std::sync::Arc;
use tauri::{AppHandle, State};

use super::{run_blocking, run_blocking_with_state};

/// SkillService 状态包装
pub struct SkillServiceState(pub Arc<SkillService>);

/// 解析 app 参数为 AppType
fn parse_app_type(app: &str) -> Result<AppType, String> {
    AppType::from_str(app).map_err(|e| e.to_string())
}

// ========== 统一管理命令 ==========

/// 获取所有已安装的 Skills
#[tauri::command]
pub async fn get_installed_skills(handle: AppHandle) -> Result<Vec<InstalledSkill>, String> {
    run_blocking_with_state(handle, |state| {
        SkillService::get_all_installed(&state.db).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn get_skill_backups() -> Result<Vec<SkillBackupEntry>, String> {
    run_blocking(|| SkillService::list_backups().map_err(|e| e.to_string())).await
}

#[tauri::command]
#[specta::specta]
pub async fn delete_skill_backup(backup_id: String) -> Result<bool, String> {
    run_blocking(move || {
        SkillService::delete_backup(&backup_id).map_err(|e| e.to_string())?;
        Ok(true)
    })
    .await
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
) -> Result<InstalledSkill, String> {
    let app_type = parse_app_type(&current_app)?;

    service
        .0
        .install(&app_state.db, &skill, &app_type)
        .await
        .map_err(|e| e.to_string())
}

/// 卸载 Skill（新版统一卸载）
#[tauri::command]
pub async fn uninstall_skill_unified(
    id: String,
    handle: AppHandle,
) -> Result<SkillUninstallResult, String> {
    run_blocking_with_state(handle, move |state| {
        SkillService::uninstall(&state.db, &id).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn restore_skill_backup(
    backup_id: String,
    current_app: String,
    handle: AppHandle,
) -> Result<InstalledSkill, String> {
    let app_type = parse_app_type(&current_app)?;
    run_blocking_with_state(handle, move |state| {
        SkillService::restore_from_backup(&state.db, &backup_id, &app_type)
            .map_err(|e| e.to_string())
    })
    .await
}

/// 切换 Skill 的应用启用状态
#[tauri::command]
#[specta::specta]
pub async fn toggle_skill_app(
    id: String,
    app: String,
    enabled: bool,
    handle: AppHandle,
) -> Result<bool, String> {
    let app_type = parse_app_type(&app)?;
    run_blocking_with_state(handle, move |state| {
        SkillService::toggle_app(&state.db, &id, &app_type, enabled).map_err(|e| e.to_string())?;
        Ok(true)
    })
    .await
}

/// 扫描未管理的 Skills（遍历所有应用的 skills 目录）
#[tauri::command]
pub async fn scan_unmanaged_skills(handle: AppHandle) -> Result<Vec<UnmanagedSkill>, String> {
    run_blocking_with_state(handle, |state| {
        SkillService::scan_unmanaged(&state.db).map_err(|e| e.to_string())
    })
    .await
}

/// 从应用目录导入 Skills
#[tauri::command]
pub async fn import_skills_from_apps(
    imports: Vec<ImportSkillSelection>,
    handle: AppHandle,
) -> Result<Vec<InstalledSkill>, String> {
    run_blocking_with_state(handle, move |state| {
        SkillService::import_from_apps(&state.db, imports).map_err(|e| e.to_string())
    })
    .await
}

// ========== 发现功能命令 ==========

/// 发现可安装的 Skills（从仓库获取）
#[tauri::command]
pub async fn discover_available_skills(
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<Vec<DiscoverableSkill>, String> {
    let repos = app_state.db.get_skill_repos().map_err(|e| e.to_string())?;
    service
        .0
        .discover_available(repos)
        .await
        .map_err(|e| e.to_string())
}

/// 检查 Skills 更新
#[tauri::command]
pub async fn check_skill_updates(
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<Vec<SkillUpdateInfo>, String> {
    service
        .0
        .check_updates(&app_state.db)
        .await
        .map_err(|e| e.to_string())
}

/// 更新单个 Skill
#[tauri::command]
pub async fn update_skill(
    id: String,
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<InstalledSkill, String> {
    service
        .0
        .update_skill(&app_state.db, &id)
        .await
        .map_err(|e| e.to_string())
}

/// 迁移 Skill 存储位置
#[tauri::command]
pub async fn migrate_skill_storage(
    target: SkillStorageLocation,
    handle: AppHandle,
) -> Result<MigrationResult, String> {
    // 迁移会整目录复制/移动文件，放到阻塞线程池执行。
    run_blocking_with_state(handle, move |state| {
        SkillService::migrate_storage(&state.db, target).map_err(|e| e.to_string())
    })
    .await
}

/// 搜索 skills.sh 公共目录
#[tauri::command]
pub async fn search_skills_sh(
    query: String,
    limit: usize,
    offset: usize,
) -> Result<SkillsShSearchResult, String> {
    SkillService::search_skills_sh(&query, limit, offset)
        .await
        .map_err(|e| e.to_string())
}

// ========== 兼容旧 API 的命令 ==========

/// 获取技能列表（兼容旧 API）
#[tauri::command]
pub async fn get_skills(
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<Vec<Skill>, String> {
    let repos = app_state.db.get_skill_repos().map_err(|e| e.to_string())?;
    service
        .0
        .list_skills(repos, &app_state.db)
        .await
        .map_err(|e| e.to_string())
}

/// 获取指定应用的技能列表（兼容旧 API）
#[tauri::command]
pub async fn get_skills_for_app(
    app: String,
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<Vec<Skill>, String> {
    // 新版本不再区分应用，统一返回所有技能
    let _ = parse_app_type(&app)?; // 验证 app 参数有效
    get_skills(service, app_state).await
}

/// 安装技能（兼容旧 API）
#[tauri::command]
#[specta::specta]
pub async fn install_skill(
    directory: String,
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    install_skill_for_app("claude".to_string(), directory, service, app_state).await
}

/// 安装指定应用的技能（兼容旧 API）
#[tauri::command]
#[specta::specta]
pub async fn install_skill_for_app(
    app: String,
    directory: String,
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    let app_type = parse_app_type(&app)?;

    // 先获取技能信息
    let repos = app_state.db.get_skill_repos().map_err(|e| e.to_string())?;
    let skills = service
        .0
        .discover_available(repos)
        .await
        .map_err(|e| e.to_string())?;

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
            format_skill_error(
                "SKILL_NOT_FOUND",
                &[("directory", &directory)],
                Some("checkRepoUrl"),
            )
        })?;

    service
        .0
        .install(&app_state.db, &skill, &app_type)
        .await
        .map_err(|e| e.to_string())?;

    Ok(true)
}

/// 卸载技能（兼容旧 API）
#[tauri::command]
pub async fn uninstall_skill(
    directory: String,
    handle: AppHandle,
) -> Result<SkillUninstallResult, String> {
    run_blocking_with_state(handle, move |state| {
        uninstall_skill_by_directory(state, &directory)
    })
    .await
}

/// 卸载指定应用的技能（兼容旧 API）
#[tauri::command]
pub async fn uninstall_skill_for_app(
    app: String,
    directory: String,
    handle: AppHandle,
) -> Result<SkillUninstallResult, String> {
    let _ = parse_app_type(&app)?; // 验证参数
    run_blocking_with_state(handle, move |state| {
        uninstall_skill_by_directory(state, &directory)
    })
    .await
}

fn uninstall_skill_by_directory(
    state: &AppState,
    directory: &str,
) -> Result<SkillUninstallResult, String> {
    // 通过 directory 找到对应的 skill id
    let skills = SkillService::get_all_installed(&state.db).map_err(|e| e.to_string())?;

    let skill = skills
        .into_iter()
        .find(|s| s.directory.eq_ignore_ascii_case(directory))
        .ok_or_else(|| format!("未找到已安装的 Skill: {directory}"))?;

    SkillService::uninstall(&state.db, &skill.id).map_err(|e| e.to_string())
}

// ========== 仓库管理命令 ==========

/// 获取技能仓库列表
#[tauri::command]
pub async fn get_skill_repos(handle: AppHandle) -> Result<Vec<SkillRepo>, String> {
    run_blocking_with_state(handle, |state| {
        state.db.get_skill_repos().map_err(|e| e.to_string())
    })
    .await
}

/// 添加技能仓库
#[tauri::command]
pub async fn add_skill_repo(repo: SkillRepo, handle: AppHandle) -> Result<bool, String> {
    run_blocking_with_state(handle, move |state| {
        state.db.save_skill_repo(&repo).map_err(|e| e.to_string())?;
        Ok(true)
    })
    .await
}

/// 删除技能仓库
#[tauri::command]
#[specta::specta]
pub async fn remove_skill_repo(
    owner: String,
    name: String,
    handle: AppHandle,
) -> Result<bool, String> {
    run_blocking_with_state(handle, move |state| {
        state
            .db
            .delete_skill_repo(&owner, &name)
            .map_err(|e| e.to_string())?;
        Ok(true)
    })
    .await
}

/// 从 ZIP 文件安装 Skills（解压归档并复制文件）
#[tauri::command]
pub async fn install_skills_from_zip(
    file_path: String,
    current_app: String,
    handle: AppHandle,
) -> Result<Vec<InstalledSkill>, String> {
    let app_type = parse_app_type(&current_app)?;
    run_blocking_with_state(handle, move |state| {
        let path = std::path::Path::new(&file_path);
        SkillService::install_from_zip(&state.db, path, &app_type).map_err(|e| e.to_string())
    })
    .await
}
