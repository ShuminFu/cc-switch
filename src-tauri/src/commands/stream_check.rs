//! 供应商连通性检查命令
//!
//! 注意：本检查只探测 base_url 是否可达，不发真实大模型请求，也不触碰故障转移
//! 熔断器（熔断器由真实转发流量驱动）。详见 `services::stream_check`。

use super::timed_blocking_with_state;
use crate::app_config::AppType;
use crate::commands::copilot::CopilotAuthState;
use crate::error::AppError;
use crate::services::stream_check::{
    HealthStatus, StreamCheckConfig, StreamCheckResult, StreamCheckService,
};
use crate::store::AppState;
use std::collections::HashSet;
use tauri::State;

/// 连通性检查（单个供应商）
#[tauri::command]
pub async fn stream_check_provider(
    state: State<'_, AppState>,
    copilot_state: State<'_, CopilotAuthState>,
    app_type: AppType,
    provider_id: String,
) -> Result<StreamCheckResult, AppError> {
    let config = state.db.get_stream_check_config()?;

    // One row by id instead of loading the whole provider table (CMD-21).
    let provider = state
        .db
        .get_provider_by_id(&provider_id, app_type.as_str())?
        .ok_or_else(|| AppError::Message(format!("供应商 {provider_id} 不存在")))?;

    // Copilot 端点是动态的（随 OAuth token 解析），需预先取出 host 再探测；
    // 其余供应商传 None，由服务层从 settings_config 提取 base_url。无需鉴权。
    let base_url_override = resolve_copilot_base_url_override(&provider, &copilot_state).await?;
    let result =
        StreamCheckService::check_with_retry(&app_type, &provider, &config, base_url_override)
            .await?;

    // 记录日志
    let _ =
        state
            .db
            .save_stream_check_log(&provider_id, &provider.name, app_type.as_str(), &result);

    Ok(result)
}

/// 批量连通性检查
#[tauri::command]
pub async fn stream_check_all_providers(
    state: State<'_, AppState>,
    copilot_state: State<'_, CopilotAuthState>,
    app_type: AppType,
    proxy_targets_only: bool,
) -> Result<Vec<(String, StreamCheckResult)>, AppError> {
    let config = state.db.get_stream_check_config()?;
    let providers = state.db.get_all_providers(app_type.as_str())?;

    let allowed_ids: Option<HashSet<String>> = if proxy_targets_only {
        let mut ids = HashSet::new();
        if let Ok(Some(current_id)) = state.db.get_current_provider(app_type.as_str()) {
            ids.insert(current_id);
        }
        if let Ok(queue) = state.db.get_failover_queue(app_type.as_str()) {
            for item in queue {
                ids.insert(item.provider_id);
            }
        }
        Some(ids)
    } else {
        None
    };

    let targets: Vec<_> = providers
        .into_iter()
        // Official OAuth providers intentionally have no user-configured probe
        // target. Never turn their runtime adapter defaults into unauthenticated
        // network probes against first-party endpoints.
        .filter(|(_, provider)| provider.category.as_deref() != Some("official"))
        .filter(|(id, _)| allowed_ids.as_ref().is_none_or(|ids| ids.contains(id)))
        .collect();

    // Probe a few providers at a time instead of one after another: with N
    // providers the wall time is about ceil(N / 4) single checks (CMD-26).
    // `buffered` keeps the result order of the provider list.
    use futures::stream::{self, StreamExt, TryStreamExt};
    let app_type_ref = &app_type;
    let config_ref = &config;
    let copilot_ref = &copilot_state;
    let results: Vec<(String, StreamCheckResult)> = stream::iter(targets)
        .map(|(id, provider)| async move {
            let base_url_override =
                resolve_copilot_base_url_override(&provider, copilot_ref).await?;
            let result = StreamCheckService::check_with_retry(
                app_type_ref,
                &provider,
                config_ref,
                base_url_override,
            )
            .await
            .unwrap_or_else(|e| failed_check_result(e.to_string()));
            Ok::<_, AppError>((id, provider.name, result))
        })
        .buffered(STREAM_CHECK_CONCURRENCY)
        .map_ok(|(id, name, result)| {
            let _ = state
                .db
                .save_stream_check_log(&id, &name, app_type.as_str(), &result);
            (id, result)
        })
        .try_collect()
        .await?;

    Ok(results)
}

/// How many providers `stream_check_all_providers` probes at once.
const STREAM_CHECK_CONCURRENCY: usize = 4;

fn failed_check_result(message: String) -> StreamCheckResult {
    StreamCheckResult {
        status: HealthStatus::Failed,
        success: false,
        message,
        response_time_ms: None,
        http_status: None,
        model_used: String::new(),
        tested_at: chrono::Utc::now().timestamp(),
        retry_count: 0,
        error_category: None,
    }
}

/// 获取连通性检查配置
#[tauri::command]
pub async fn get_stream_check_config(
    handle: tauri::AppHandle,
) -> Result<StreamCheckConfig, AppError> {
    timed_blocking_with_state("get_stream_check_config", handle, move |state| {
        state.db.get_stream_check_config()
    })
    .await
}

/// 保存连通性检查配置
#[tauri::command]
pub async fn save_stream_check_config(
    handle: tauri::AppHandle,
    config: StreamCheckConfig,
) -> Result<(), AppError> {
    timed_blocking_with_state("save_stream_check_config", handle, move |state| {
        state.db.save_stream_check_config(&config)
    })
    .await
}

/// Copilot 供应商的 base_url 需要从 OAuth 管理器动态解析（按账号或默认端点）。
/// `is_full_url` 的供应商已是完整地址，无需解析。
async fn resolve_copilot_base_url_override(
    provider: &crate::provider::Provider,
    copilot_state: &State<'_, CopilotAuthState>,
) -> Result<Option<String>, AppError> {
    let is_copilot = is_copilot_provider(provider);
    let is_full_url = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.is_full_url)
        .unwrap_or(false);

    if !is_copilot || is_full_url {
        return Ok(None);
    }

    let auth_manager = copilot_state.0.read().await;
    let account_id = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.managed_account_id_for("github_copilot"));

    let endpoint = match account_id.as_deref() {
        Some(id) => auth_manager.get_api_endpoint(id).await,
        None => auth_manager.get_default_api_endpoint().await,
    };

    Ok(Some(endpoint))
}

fn is_copilot_provider(provider: &crate::provider::Provider) -> bool {
    provider
        .meta
        .as_ref()
        .and_then(|meta| meta.provider_type.as_deref())
        == Some("github_copilot")
        || provider
            .settings_config
            .pointer("/env/ANTHROPIC_BASE_URL")
            .and_then(|value| value.as_str())
            .map(|url| url.contains("githubcopilot.com"))
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::is_copilot_provider;
    use crate::provider::{Provider, ProviderMeta};
    use serde_json::json;

    #[test]
    fn copilot_provider_detection_accepts_provider_type_or_base_url() {
        let typed_provider = Provider {
            id: "p1".to_string(),
            name: "typed".to_string(),
            settings_config: json!({}),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: Some(ProviderMeta {
                provider_type: Some("github_copilot".to_string()),
                ..Default::default()
            }),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };
        assert!(is_copilot_provider(&typed_provider));

        let url_provider = Provider {
            id: "p2".to_string(),
            name: "url".to_string(),
            settings_config: json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.githubcopilot.com"
                }
            }),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };
        assert!(is_copilot_provider(&url_provider));
    }

    #[test]
    fn copilot_full_url_metadata_is_available_for_override_guard() {
        let provider = Provider {
            id: "p3".to_string(),
            name: "relay".to_string(),
            settings_config: json!({}),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: Some(ProviderMeta {
                provider_type: Some("github_copilot".to_string()),
                is_full_url: Some(true),
                ..Default::default()
            }),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };

        assert!(is_copilot_provider(&provider));
        assert_eq!(
            provider.meta.as_ref().and_then(|meta| meta.is_full_url),
            Some(true)
        );
    }
}
