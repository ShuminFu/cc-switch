//! live 配置漂移检测命令（CCS-166）

use std::str::FromStr;

use crate::app_config::AppType;
use crate::error::IpcError;
use crate::services::drift::{DriftReport, DriftResolution, DriftService};
use crate::store::AppState;

/// 检测单个应用的 live 配置漂移
#[tauri::command]
pub async fn get_live_config_drift(
    state: tauri::State<'_, AppState>,
    app: String,
) -> Result<DriftReport, IpcError> {
    let app_type = AppType::from_str(&app).map_err(IpcError::from)?;
    DriftService::detect(state.inner(), &app_type).map_err(IpcError::from)
}

/// 检测全部支持漂移检测的应用
#[tauri::command]
pub async fn get_all_live_config_drift(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<DriftReport>, IpcError> {
    Ok(DriftService::detect_all(state.inner()))
}

/// 处理漂移：修复占位符 / 并入通用配置 / 忽略；返回处理后的报告
#[tauri::command]
pub async fn resolve_live_config_drift(
    state: tauri::State<'_, AppState>,
    app: String,
    resolution: DriftResolution,
) -> Result<DriftReport, IpcError> {
    let app_type = AppType::from_str(&app).map_err(IpcError::from)?;
    DriftService::resolve(state.inner(), &app_type, resolution)
        .await
        .map_err(IpcError::from)
}
