//! 故障转移队列命令
//!
//! 管理代理模式下的故障转移队列（基于 providers 表的 in_failover_queue 字段），
//! 策略见 `services::failover`

use crate::ctx::{Ctx, TauriEventSink};
use crate::database::FailoverQueueItem;
use crate::error::IpcError;
use crate::provider::Provider;
use crate::services::failover::FailoverService;
use crate::store::AppState;

/// 获取故障转移队列
#[tauri::command]
pub async fn get_failover_queue(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<Vec<FailoverQueueItem>, IpcError> {
    FailoverService::queue(&state, &app_type).map_err(IpcError::from)
}

/// 获取可添加到故障转移队列的供应商（不在队列中的）
#[tauri::command]
pub async fn get_available_providers_for_failover(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<Vec<Provider>, IpcError> {
    FailoverService::available_providers(&state, &app_type).map_err(IpcError::from)
}

/// 添加供应商到故障转移队列
#[tauri::command]
pub async fn add_to_failover_queue(
    state: tauri::State<'_, AppState>,
    app_type: String,
    provider_id: String,
) -> Result<(), IpcError> {
    FailoverService::add_to_queue(&state, &app_type, &provider_id).map_err(IpcError::from)
}

/// 从故障转移队列移除供应商
#[tauri::command]
pub async fn remove_from_failover_queue(
    state: tauri::State<'_, AppState>,
    app_type: String,
    provider_id: String,
) -> Result<(), IpcError> {
    FailoverService::remove_from_queue(&state, &app_type, &provider_id).map_err(IpcError::from)
}

/// 获取指定应用的自动故障转移开关状态（从 proxy_config 表读取）
#[tauri::command]
pub async fn get_auto_failover_enabled(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<bool, IpcError> {
    FailoverService::auto_failover_enabled(&state, &app_type)
        .await
        .map_err(IpcError::from)
}

/// 设置指定应用的自动故障转移开关状态（写入 proxy_config 表）
///
/// 注意：关闭故障转移时不会清除队列，队列内容会保留供下次开启时使用
#[tauri::command]
pub async fn set_auto_failover_enabled(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    app_type: String,
    enabled: bool,
) -> Result<(), IpcError> {
    let sink = TauriEventSink::new(app);
    FailoverService::set_auto_failover_enabled(&Ctx::new(&state, &sink), &app_type, enabled)
        .await
        .map_err(IpcError::from)
}
