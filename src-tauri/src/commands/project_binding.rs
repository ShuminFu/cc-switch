//! 目录 → 项目绑定命令（PRODUCT-12）

use std::path::PathBuf;

use serde::Serialize;
use tauri::AppHandle;

use super::profile::finish_profile_change;
use super::run_blocking_with_state;

use crate::services::profile::ProfileScope;
use crate::services::project_binding::{self, DetectedProject, ProjectApplyReport, ProjectBinding};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectBindingsResponse {
    pub bindings: Vec<ProjectBinding>,
    /// 支持"项目级应用"（写项目内配置文件）的分组
    pub project_level_scopes: Vec<ProfileScope>,
}

fn parse_scopes(scopes: Option<Vec<String>>) -> Result<Vec<ProfileScope>, String> {
    scopes
        .unwrap_or_default()
        .iter()
        .map(|s| ProfileScope::parse(s).map_err(|e| e.to_string()))
        .collect()
}

/// 未指定目录时使用进程当前目录（供 CLI 场景复用；GUI 中前端总会传入目录）
fn resolve_dir(path: Option<String>) -> Result<PathBuf, String> {
    match path.filter(|p| !p.trim().is_empty()) {
        Some(p) => Ok(PathBuf::from(p)),
        None => std::env::current_dir().map_err(|e| e.to_string()),
    }
}

#[tauri::command]
pub async fn list_project_bindings(handle: AppHandle) -> Result<ProjectBindingsResponse, String> {
    run_blocking_with_state(handle, |state| {
        Ok(ProjectBindingsResponse {
            bindings: project_binding::load_bindings(&state.db).map_err(|e| e.to_string())?,
            project_level_scopes: project_binding::project_level_scopes(),
        })
    })
    .await
}

#[tauri::command]
pub async fn bind_project_directory(
    handle: AppHandle,
    path: String,
    profile_id: String,
    scopes: Option<Vec<String>>,
    project_level: Option<bool>,
) -> Result<ProjectBinding, String> {
    let scopes = parse_scopes(scopes)?;
    run_blocking_with_state(handle, move |state| {
        project_binding::bind(
            &state.db,
            &PathBuf::from(path),
            &profile_id,
            scopes,
            project_level.unwrap_or(false),
        )
        .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn unbind_project_directory(handle: AppHandle, path: String) -> Result<bool, String> {
    run_blocking_with_state(handle, move |state| {
        project_binding::unbind(&state.db, &path).map_err(|e| e.to_string())
    })
    .await
}

/// 检测目录所属的项目绑定（不改动任何配置）
#[tauri::command]
pub async fn detect_project(
    handle: AppHandle,
    path: Option<String>,
) -> Result<DetectedProject, String> {
    let dir = resolve_dir(path)?;
    run_blocking_with_state(handle, move |state| {
        project_binding::detect(&state.db, &dir).map_err(|e| e.to_string())
    })
    .await
}

/// 检测并应用目录绑定；目录没有绑定时返回 null
#[tauri::command]
pub async fn apply_project_binding(
    app: AppHandle,
    path: Option<String>,
) -> Result<Option<ProjectApplyReport>, String> {
    let dir = resolve_dir(path)?;
    let report = run_blocking_with_state(app.clone(), move |state| {
        project_binding::apply_for_directory(state, &dir).map_err(|e| e.to_string())
    })
    .await?;
    if let Some(report) = &report {
        finish_profile_change(
            app,
            Some(report.profile_id.clone()),
            report.global_scopes.clone(),
            report.should_stop_proxy,
        )
        .await;
    }
    Ok(report)
}
