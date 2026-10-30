use indexmap::IndexMap;
use tauri::{Emitter, Manager, State};

use crate::app_config::AppType;
use crate::commands::copilot::CopilotAuthState;
use crate::ctx::{Ctx, TauriEventSink};
use crate::error::{AppError, IpcError};
use crate::provider::Provider;
use crate::services::provider::{LiveWritePreview, RevertSwitchResult};
use crate::services::{
    EndpointLatency, ProviderService, ProviderSortUpdate, SpeedtestService, SwitchResult,
};
use crate::store::AppState;
use std::str::FromStr;

/// 获取所有供应商
#[tauri::command]
pub fn get_providers(
    state: State<'_, AppState>,
    app: String,
) -> Result<IndexMap<String, Provider>, IpcError> {
    let app_type = AppType::from_str(&app).map_err(IpcError::from)?;
    ProviderService::list(state.inner(), app_type).map_err(IpcError::from)
}

#[tauri::command]
pub fn get_current_provider(state: State<'_, AppState>, app: String) -> Result<String, IpcError> {
    let app_type = AppType::from_str(&app).map_err(IpcError::from)?;
    ProviderService::current(state.inner(), app_type).map_err(IpcError::from)
}

#[tauri::command]
pub fn add_provider(
    state: State<'_, AppState>,
    app: String,
    provider: Provider,
    add_to_live: Option<bool>,
) -> Result<bool, IpcError> {
    let app_type = AppType::from_str(&app).map_err(IpcError::from)?;
    ProviderService::add(
        state.inner(),
        app_type,
        provider,
        add_to_live.unwrap_or(true),
    )
    .map_err(IpcError::from)
}

#[tauri::command]
pub fn update_provider(
    state: State<'_, AppState>,
    app: String,
    provider: Provider,
    original_id: Option<String>,
) -> Result<bool, IpcError> {
    let app_type = AppType::from_str(&app).map_err(IpcError::from)?;
    ProviderService::update(state.inner(), app_type, original_id.as_deref(), provider)
        .map_err(IpcError::from)
}

/// 预览保存供应商时将写入 live 配置的内容（不写盘）
#[tauri::command]
pub fn preview_live_write(
    state: State<'_, AppState>,
    app: String,
    provider: Provider,
    original_id: Option<String>,
) -> Result<LiveWritePreview, IpcError> {
    let app_type = AppType::from_str(&app).map_err(IpcError::from)?;
    ProviderService::preview_live_write(state.inner(), app_type, original_id.as_deref(), provider)
        .map_err(IpcError::from)
}

#[tauri::command]
pub fn delete_provider(
    state: State<'_, AppState>,
    app: String,
    id: String,
) -> Result<bool, IpcError> {
    let app_type = AppType::from_str(&app).map_err(IpcError::from)?;
    ProviderService::delete(state.inner(), app_type, &id)
        .map(|_| true)
        .map_err(IpcError::from)
}

#[tauri::command]
pub fn remove_provider_from_live_config(
    state: tauri::State<'_, AppState>,
    app: String,
    id: String,
) -> Result<bool, IpcError> {
    let app_type = AppType::from_str(&app).map_err(IpcError::from)?;
    ProviderService::remove_from_live_config(state.inner(), app_type, &id)
        .map(|_| true)
        .map_err(IpcError::from)
}

/// 在阻塞线程池上执行供应商切换。
///
/// `ProviderService::switch` 会 block_on 每个应用的切换锁并做文件 I/O；
/// 若在主线程同步执行，并发的接管/热切换持锁时会卡住 UI。
pub(crate) async fn run_switch_off_main<F>(switch: F) -> Result<SwitchResult, AppError>
where
    F: FnOnce() -> Result<SwitchResult, AppError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(switch)
        .await
        .map_err(|e| AppError::Message(format!("切换供应商任务失败: {e}")))?
}

#[tauri::command]
pub async fn switch_provider(
    app_handle: tauri::AppHandle,
    app: String,
    id: String,
) -> Result<SwitchResult, IpcError> {
    let app_type = AppType::from_str(&app).map_err(IpcError::from)?;
    run_switch_off_main(move || {
        let state = app_handle.state::<AppState>();
        ProviderService::switch(state.inner(), app_type, &id)
    })
    .await
    .map_err(IpcError::from)
}

/// 撤销该应用最近一次供应商切换：live 文件还原为切换前内容并恢复当前供应商
#[tauri::command]
pub async fn revert_last_switch(
    app_handle: tauri::AppHandle,
    app: String,
) -> Result<RevertSwitchResult, IpcError> {
    let app_type = AppType::from_str(&app).map_err(IpcError::from)?;
    // 与切换相同：等待切换锁并做文件 I/O，放到阻塞线程池执行
    tauri::async_runtime::spawn_blocking(move || {
        let state = app_handle.state::<AppState>();
        ProviderService::revert_last_switch(state.inner(), app_type)
    })
    .await
    .map_err(|e| AppError::Message(format!("撤销切换任务失败: {e}")))
    .and_then(|result| result)
    .map_err(IpcError::from)
}

#[tauri::command]
pub fn import_default_config(state: State<'_, AppState>, app: String) -> Result<bool, IpcError> {
    let app_type = AppType::from_str(&app).map_err(IpcError::from)?;
    ProviderService::import_default_config_with_common_snippet(state.inner(), app_type)
        .map_err(IpcError::from)
}

#[tauri::command]
pub async fn get_claude_desktop_status(
    state: State<'_, AppState>,
) -> Result<crate::claude_desktop_config::ClaudeDesktopStatus, IpcError> {
    let proxy_running = state.proxy_service.is_running().await;
    crate::claude_desktop_config::get_status(state.db.as_ref(), proxy_running)
        .map_err(IpcError::from)
}

#[tauri::command]
pub fn get_claude_desktop_default_routes(
) -> Result<Vec<crate::claude_desktop_config::ClaudeDesktopDefaultRoute>, IpcError> {
    Ok(crate::claude_desktop_config::default_proxy_routes())
}

#[tauri::command]
pub fn import_claude_desktop_providers_from_claude(
    state: State<'_, AppState>,
) -> Result<usize, IpcError> {
    crate::services::claude_desktop_import::import_from_claude(state.inner())
        .map_err(IpcError::from)
}

#[tauri::command]
pub fn ensure_claude_desktop_official_provider(
    state: State<'_, AppState>,
) -> Result<bool, IpcError> {
    ProviderService::ensure_official_provider(
        state.inner(),
        crate::database::CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID,
        AppType::ClaudeDesktop,
    )
    .map_err(IpcError::from)
}

#[tauri::command]
pub fn ensure_codex_official_provider(state: State<'_, AppState>) -> Result<bool, IpcError> {
    ProviderService::ensure_official_provider(
        state.inner(),
        crate::database::CODEX_OFFICIAL_PROVIDER_ID,
        AppType::Codex,
    )
    .map_err(IpcError::from)
}

/// 查询供应商用量；成功拿到快照时写入用量缓存、通知前端并刷新托盘
/// （见 `services::usage_query::refresh_provider_usage`）
#[tauri::command]
pub async fn query_provider_usage(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    copilot_state: State<'_, CopilotAuthState>,
    provider_id: String, // 前端键为 providerId，由 Tauri 映射
    app: String,
) -> Result<crate::provider::UsageResult, IpcError> {
    let app_type = AppType::from_str(&app).map_err(IpcError::from)?;
    let sink = TauriEventSink::new(app_handle);
    crate::services::usage_query::refresh_provider_usage(
        &Ctx::new(&state, &sink),
        &copilot_state,
        app_type,
        &provider_id,
    )
    .await
    .map_err(IpcError::from)
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn test_usage_script(
    state: State<'_, AppState>,
    provider_id: String,
    app: String,
    script_code: String,
    timeout: Option<u64>,
    api_key: Option<String>,
    base_url: Option<String>,
    access_token: Option<String>,
    user_id: Option<String>,
    template_type: Option<String>,
) -> Result<crate::provider::UsageResult, IpcError> {
    let app_type = AppType::from_str(&app).map_err(IpcError::from)?;
    ProviderService::test_usage_script(
        state.inner(),
        app_type,
        &provider_id,
        &script_code,
        timeout.unwrap_or(10),
        api_key.as_deref(),
        base_url.as_deref(),
        access_token.as_deref(),
        user_id.as_deref(),
        template_type.as_deref(),
    )
    .await
    .map_err(IpcError::from)
}

#[tauri::command]
pub fn read_live_provider_settings(app: String) -> Result<serde_json::Value, IpcError> {
    let app_type = AppType::from_str(&app).map_err(IpcError::from)?;
    ProviderService::read_live_settings(app_type).map_err(IpcError::from)
}

#[tauri::command]
pub async fn test_api_endpoints(
    urls: Vec<String>,
    timeout_secs: Option<u64>,
) -> Result<Vec<EndpointLatency>, IpcError> {
    SpeedtestService::test_endpoints(urls, timeout_secs)
        .await
        .map_err(IpcError::from)
}

#[tauri::command]
pub fn get_custom_endpoints(
    state: State<'_, AppState>,
    app: String,
    provider_id: String,
) -> Result<Vec<crate::settings::CustomEndpoint>, IpcError> {
    let app_type = AppType::from_str(&app).map_err(IpcError::from)?;
    ProviderService::get_custom_endpoints(state.inner(), app_type, &provider_id)
        .map_err(IpcError::from)
}

#[tauri::command]
pub fn add_custom_endpoint(
    state: State<'_, AppState>,
    app: String,
    provider_id: String,
    url: String,
) -> Result<(), IpcError> {
    let app_type = AppType::from_str(&app).map_err(IpcError::from)?;
    ProviderService::add_custom_endpoint(state.inner(), app_type, &provider_id, url)
        .map_err(IpcError::from)
}

#[tauri::command]
pub fn remove_custom_endpoint(
    state: State<'_, AppState>,
    app: String,
    provider_id: String,
    url: String,
) -> Result<(), IpcError> {
    let app_type = AppType::from_str(&app).map_err(IpcError::from)?;
    ProviderService::remove_custom_endpoint(state.inner(), app_type, &provider_id, url)
        .map_err(IpcError::from)
}

#[tauri::command]
pub fn update_endpoint_last_used(
    state: State<'_, AppState>,
    app: String,
    provider_id: String,
    url: String,
) -> Result<(), IpcError> {
    let app_type = AppType::from_str(&app).map_err(IpcError::from)?;
    ProviderService::update_endpoint_last_used(state.inner(), app_type, &provider_id, url)
        .map_err(IpcError::from)
}

#[tauri::command]
pub fn update_providers_sort_order(
    state: State<'_, AppState>,
    app: String,
    updates: Vec<ProviderSortUpdate>,
) -> Result<bool, IpcError> {
    let app_type = AppType::from_str(&app).map_err(IpcError::from)?;
    ProviderService::update_sort_order(state.inner(), app_type, updates).map_err(IpcError::from)
}

use crate::provider::UniversalProvider;
use std::collections::HashMap;
use tauri::AppHandle;

#[derive(Clone, serde::Serialize)]
pub struct UniversalProviderSyncedEvent {
    pub action: String,
    pub id: String,
}

fn emit_universal_provider_synced(app: &AppHandle, action: &str, id: &str) {
    let _ = app.emit(
        "universal-provider-synced",
        UniversalProviderSyncedEvent {
            action: action.to_string(),
            id: id.to_string(),
        },
    );
}

#[tauri::command]
pub fn get_universal_providers(
    state: State<'_, AppState>,
) -> Result<HashMap<String, UniversalProvider>, IpcError> {
    ProviderService::list_universal(state.inner()).map_err(IpcError::from)
}

#[tauri::command]
pub fn get_universal_provider(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<UniversalProvider>, IpcError> {
    ProviderService::get_universal(state.inner(), &id).map_err(IpcError::from)
}

#[tauri::command]
pub fn upsert_universal_provider(
    app: AppHandle,
    state: State<'_, AppState>,
    provider: UniversalProvider,
) -> Result<bool, IpcError> {
    let id = provider.id.clone();
    let result =
        ProviderService::upsert_universal(state.inner(), provider).map_err(IpcError::from)?;

    emit_universal_provider_synced(&app, "upsert", &id);

    Ok(result)
}

#[tauri::command]
pub fn delete_universal_provider(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<bool, IpcError> {
    let result = ProviderService::delete_universal(state.inner(), &id).map_err(IpcError::from)?;

    emit_universal_provider_synced(&app, "delete", &id);

    Ok(result)
}

#[tauri::command]
pub fn sync_universal_provider(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<bool, IpcError> {
    let result =
        ProviderService::sync_universal_to_apps(state.inner(), &id).map_err(IpcError::from)?;

    emit_universal_provider_synced(&app, "sync", &id);

    Ok(result)
}

#[tauri::command]
pub fn import_opencode_providers_from_live(state: State<'_, AppState>) -> Result<usize, IpcError> {
    crate::services::provider::import_opencode_providers_from_live(state.inner())
        .map_err(IpcError::from)
}

#[tauri::command]
pub fn get_opencode_live_provider_ids() -> Result<Vec<String>, IpcError> {
    crate::opencode_config::get_providers()
        .map(|providers| providers.keys().cloned().collect())
        .map_err(IpcError::from)
}

// ============================================================================
// OpenClaw 专属命令 → 已迁移至 commands/openclaw.rs
// ============================================================================
