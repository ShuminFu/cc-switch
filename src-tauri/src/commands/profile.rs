//! 项目 Profile 管理命令

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use super::run_blocking_with_state;

use crate::database::Profile;
use crate::services::profile::{ProfilePayload, ProfileScope, ProfileService};
use crate::store::AppState;

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfileDto {
    pub id: String,
    pub name: String,
    // ProfilePayload lives in services/profile.rs; typed as JSON in the bindings.
    #[specta(type = serde_json::Value)]
    pub payload: ProfilePayload,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
}

impl From<Profile> for ProfileDto {
    fn from(profile: Profile) -> Self {
        // 单条 payload 损坏不应拖垮整个列表：降级为默认值并记日志
        let payload = serde_json::from_str(&profile.payload).unwrap_or_else(|e| {
            log::warn!(
                "解析 profile '{}' payload 失败，使用默认值: {e}",
                profile.id
            );
            ProfilePayload::default()
        });
        Self {
            id: profile.id,
            name: profile.name,
            payload,
            created_at: profile.created_at,
            updated_at: profile.updated_at,
        }
    }
}

/// 每个分组当前激活的项目 id（未使用项目时为 null）
#[derive(Debug, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CurrentProfileIds {
    pub claude: Option<String>,
    pub claude_desktop: Option<String>,
    pub codex: Option<String>,
    pub gemini: Option<String>,
    pub grokbuild: Option<String>,
    pub opencode: Option<String>,
    pub openclaw: Option<String>,
    pub hermes: Option<String>,
}

impl CurrentProfileIds {
    fn slot_mut(&mut self, scope: ProfileScope) -> &mut Option<String> {
        match scope {
            ProfileScope::Claude => &mut self.claude,
            ProfileScope::ClaudeDesktop => &mut self.claude_desktop,
            ProfileScope::Codex => &mut self.codex,
            ProfileScope::Gemini => &mut self.gemini,
            ProfileScope::GrokBuild => &mut self.grokbuild,
            ProfileScope::OpenCode => &mut self.opencode,
            ProfileScope::OpenClaw => &mut self.openclaw,
            ProfileScope::Hermes => &mut self.hermes,
        }
    }
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfilesResponse {
    pub profiles: Vec<ProfileDto>,
    pub current_ids: CurrentProfileIds,
}

/// Profile 应用完成后的统一收尾：发事件 + 重建托盘菜单
///
/// 只对项目所属分组内的应用发 provider-switched。UI 与托盘两个入口必须
/// 共用此函数，保证事件 payload 形状一致（前端 App.tsx 的
/// provider-switched 监听依赖该形状）。
pub fn emit_profile_apply_events(
    app: &tauri::AppHandle,
    state: &AppState,
    profile_id: &str,
    scope: ProfileScope,
) {
    emit_profile_events(app, state, Some(profile_id), scope);
}

/// 与 [`emit_profile_apply_events`] 相同；`profile_id = None` 表示该分组已切到
/// "不使用项目"（前端据 profileId=null 刷新）。
pub fn emit_profile_events(
    app: &tauri::AppHandle,
    state: &AppState,
    profile_id: Option<&str>,
    scope: ProfileScope,
) {
    for app_type in scope.apps().iter() {
        let app_str = app_type.as_str();
        let (proxy_enabled, auto_failover_enabled) = state.db.get_proxy_flags_sync(app_str);
        let provider_id = crate::settings::get_effective_current_provider(&state.db, app_type)
            .ok()
            .flatten()
            .unwrap_or_default();
        let event_data = serde_json::json!({
            "appType": app_str,
            "proxyEnabled": proxy_enabled,
            "autoFailoverEnabled": auto_failover_enabled,
            "providerId": provider_id,
        });
        if let Err(e) = app.emit(crate::events::PROVIDER_SWITCHED, event_data) {
            log::error!("发射 provider-switched 事件失败: {e}");
        }
    }
    if let Err(e) = app.emit(
        crate::events::PROFILE_APPLIED,
        serde_json::json!({ "profileId": profile_id, "scope": scope.as_str() }),
    ) {
        log::error!("发射 profile-applied 事件失败: {e}");
    }
    crate::tray::refresh_tray_menu(app);
}

fn list_profiles_inner(state: &AppState) -> Result<ProfilesResponse, String> {
    let profiles = ProfileService::list(state).map_err(|e| e.to_string())?;
    let mut current_ids = CurrentProfileIds::default();
    for scope in ProfileScope::ALL {
        *current_ids.slot_mut(scope) = state
            .db
            .get_current_profile_id(scope.as_str())
            .map_err(|e| e.to_string())?;
    }
    Ok(ProfilesResponse {
        profiles: profiles.into_iter().map(ProfileDto::from).collect(),
        current_ids,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn list_profiles(handle: AppHandle) -> Result<ProfilesResponse, String> {
    run_blocking_with_state(handle, list_profiles_inner).await
}

#[tauri::command]
#[specta::specta]
pub async fn create_profile(
    handle: AppHandle,
    name: String,
    scope: String,
) -> Result<ProfileDto, String> {
    let scope = ProfileScope::parse(&scope).map_err(|e| e.to_string())?;
    run_blocking_with_state(handle, move |state| {
        ProfileService::create(state, &name, scope)
            .map(ProfileDto::from)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn update_profile(
    handle: AppHandle,
    id: String,
    name: Option<String>,
    resnapshot: Option<bool>,
    scope: Option<String>,
) -> Result<ProfileDto, String> {
    let scope = scope
        .map(|s| ProfileScope::parse(&s))
        .transpose()
        .map_err(|e| e.to_string())?;
    run_blocking_with_state(handle, move |state| {
        ProfileService::update(state, &id, name, resnapshot.unwrap_or(false), scope)
            .map(ProfileDto::from)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn delete_profile(handle: AppHandle, id: String) -> Result<(), String> {
    run_blocking_with_state(handle, move |state| {
        ProfileService::delete(state, &id).map_err(|e| e.to_string())
    })
    .await
}

/// "不使用项目"：清除分组的当前项目；若首次应用项目前拍过基线，则恢复该基线
/// （PRODUCT-68），返回 warnings。
#[tauri::command]
#[specta::specta]
pub async fn clear_current_profile(
    app: tauri::AppHandle,
    scope: String,
) -> Result<Vec<String>, String> {
    let scope = ProfileScope::parse(&scope).map_err(|e| e.to_string())?;
    let (warnings, should_stop_proxy) = run_blocking_with_state(app.clone(), move |state| {
        ProfileService::clear_current(state, scope).map_err(|e| e.to_string())
    })
    .await?;
    finish_profile_change(app, None, vec![scope], should_stop_proxy).await;
    Ok(warnings)
}

/// 应用项目快照（只作用于发起页所属分组内的应用）。
///
/// 应用过程会写多份 live 文件并可能等待代理的按应用切换锁，整体在阻塞
/// 线程池执行（`ProviderService` 内部通过 `block_on_async` 安全地等待代理
/// future），不会占用 UI 线程或 tokio worker。
#[tauri::command]
#[specta::specta]
pub async fn apply_profile(
    app: tauri::AppHandle,
    id: String,
    scope: String,
) -> Result<Vec<String>, String> {
    let scope = ProfileScope::parse(&scope).map_err(|e| e.to_string())?;
    let profile_id = id.clone();
    let (warnings, should_stop_proxy) = run_blocking_with_state(app.clone(), move |state| {
        ProfileService::apply(state, &profile_id, scope).map_err(|e| e.to_string())
    })
    .await?;

    finish_profile_change(app, Some(id), vec![scope], should_stop_proxy).await;
    Ok(warnings)
}

/// 项目应用 / 清除后的统一收尾：必要时停止代理，然后为每个分组发事件并刷新托盘。
///
/// 停止代理可能等待服务端退出，放到后台执行，停止后再补发事件刷新 UI
/// （与托盘入口的行为一致）；否则直接在阻塞线程池发事件（事件发射与托盘
/// 重建本身也会读 DB / settings）。
pub(crate) async fn finish_profile_change(
    app: tauri::AppHandle,
    profile_id: Option<String>,
    scopes: Vec<ProfileScope>,
    should_stop_proxy: bool,
) {
    if scopes.is_empty() {
        return;
    }
    let emit = move |app_handle: tauri::AppHandle| async move {
        let handle_for_events = app_handle.clone();
        let _ = run_blocking_with_state(app_handle, move |state| -> Result<(), String> {
            for scope in scopes {
                emit_profile_events(&handle_for_events, state, profile_id.as_deref(), scope);
            }
            Ok(())
        })
        .await;
    };

    if should_stop_proxy {
        let app_handle = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Some(app_state) = app_handle.try_state::<AppState>() {
                if let Err(e) = app_state.proxy_service.stop().await {
                    log::warn!("切换项目后停止代理服务失败: {e}");
                }
            }
            emit(app_handle).await;
        });
    } else {
        emit(app).await;
    }
}
