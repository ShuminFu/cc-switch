use tauri::State;

use crate::ctx::{Ctx, TauriEventSink};
use crate::error::IpcError;
use crate::services::subscription::SubscriptionQuota;
use crate::store::AppState;

/// 查询官方订阅额度（缓存 / 托盘 / 前端通知语义见
/// `services::usage_query::refresh_subscription_quota`）
#[tauri::command]
pub async fn get_subscription_quota(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    tool: String,
) -> Result<SubscriptionQuota, IpcError> {
    let sink = TauriEventSink::new(app);
    crate::services::usage_query::refresh_subscription_quota(&Ctx::new(&state, &sink), &tool)
        .await
        .map_err(IpcError::from)
}
