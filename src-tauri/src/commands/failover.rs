//! 故障转移队列命令
//!
//! 管理代理模式下的故障转移队列（基于 providers 表的 in_failover_queue 字段）

use crate::app_config::AppType;
use crate::database::FailoverQueueItem;
use crate::provider::Provider;
use crate::store::AppState;
use std::str::FromStr;
use tauri::Emitter;

/// Validate a webview-supplied app id before it reaches the DAO (CMD-23) and
/// return its canonical form (`"grok"` → `"grokbuild"`).
pub(crate) fn canonical_app_type(app_type: &str) -> Result<AppType, String> {
    AppType::from_str(app_type).map_err(|e| e.to_string())
}

/// 获取故障转移队列
#[tauri::command]
pub async fn get_failover_queue(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<Vec<FailoverQueueItem>, String> {
    let app = canonical_app_type(&app_type)?;
    state
        .db
        .get_failover_queue(app.as_str())
        .map_err(|e| e.to_string())
}

/// 获取可添加到故障转移队列的供应商（不在队列中的）
#[tauri::command]
pub async fn get_available_providers_for_failover(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<Vec<Provider>, String> {
    let app = canonical_app_type(&app_type)?;
    state
        .db
        .get_available_providers_for_failover(app.as_str())
        .map_err(|e| e.to_string())
}

/// 添加供应商到故障转移队列
#[tauri::command]
#[specta::specta]
pub async fn add_to_failover_queue(
    state: tauri::State<'_, AppState>,
    app_type: String,
    provider_id: String,
) -> Result<(), String> {
    let app = canonical_app_type(&app_type)?;
    state
        .db
        .add_to_failover_queue(app.as_str(), &provider_id)
        .map_err(|e| e.to_string())
}

/// 从故障转移队列移除供应商
#[tauri::command]
#[specta::specta]
pub async fn remove_from_failover_queue(
    state: tauri::State<'_, AppState>,
    app_type: String,
    provider_id: String,
) -> Result<(), String> {
    let app = canonical_app_type(&app_type)?;
    state
        .db
        .remove_from_failover_queue(app.as_str(), &provider_id)
        .map_err(|e| e.to_string())
}

/// 获取指定应用的自动故障转移开关状态（从 proxy_config 表读取）
#[tauri::command]
#[specta::specta]
pub async fn get_auto_failover_enabled(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<bool, String> {
    let app = canonical_app_type(&app_type)?;
    state
        .db
        .get_proxy_config_for_app(app.as_str())
        .await
        .map(|config| config.auto_failover_enabled)
        .map_err(|e| e.to_string())
}

/// Turn automatic failover on for one app. Shared by the UI command and the
/// tray "Auto" item so both have the same failure semantics (CMD-44):
///
/// 1. refuse unless the app's proxy takeover is on;
/// 2. if the queue is empty, add the current provider as P1;
/// 3. switch to P1 — on failure, remove the auto-added queue entry and stop;
/// 4. only then persist `auto_failover_enabled = true`.
///
/// Returns the P1 provider id. Nothing is persisted when it fails.
pub(crate) async fn enable_auto_failover(
    state: &AppState,
    app_type: &AppType,
) -> Result<String, String> {
    let app_type_str = app_type.as_str();
    let mut config = state
        .db
        .get_proxy_config_for_app(app_type_str)
        .await
        .map_err(|e| e.to_string())?;

    if !config.enabled {
        return Err("需要先启用该应用的代理接管，再开启故障转移".to_string());
    }

    // 队列为空时把当前供应商自动加入作为 P1，避免用户陷入"必须先加队列才能开启"的死锁
    let mut auto_added_provider_id: Option<String> = None;
    let mut queue = state
        .db
        .get_failover_queue(app_type_str)
        .map_err(|e| e.to_string())?;

    if queue.is_empty() {
        let current_id = crate::settings::get_effective_current_provider(&state.db, app_type)
            .map_err(|e| e.to_string())?;

        let Some(current_id) = current_id else {
            return Err("故障转移队列为空，且未设置当前供应商，无法开启故障转移".to_string());
        };

        state
            .db
            .add_to_failover_queue(app_type_str, &current_id)
            .map_err(|e| e.to_string())?;
        auto_added_provider_id = Some(current_id);

        queue = state
            .db
            .get_failover_queue(app_type_str)
            .map_err(|e| e.to_string())?;
    }

    let rollback_queue = |state: &AppState| {
        if let Some(provider_id) = auto_added_provider_id.as_deref() {
            let _ = state
                .db
                .remove_from_failover_queue(app_type_str, provider_id);
        }
    };

    let Some(p1_provider_id) = queue.first().map(|item| item.provider_id.clone()) else {
        rollback_queue(state);
        return Err("故障转移队列为空，无法开启故障转移".to_string());
    };

    // 开启前先切到 P1。只有切换成功后才写入 auto_failover_enabled=true，
    // 避免 P1 不可切换（例如 official provider）时留下“开关已开但目标未切”的脏状态。
    if let Err(e) = state
        .proxy_service
        .switch_proxy_target(app_type_str, &p1_provider_id)
        .await
    {
        rollback_queue(state);
        return Err(e);
    }

    config.auto_failover_enabled = true;
    if let Err(e) = state.db.update_proxy_config_for_app(config).await {
        rollback_queue(state);
        return Err(e.to_string());
    }

    Ok(p1_provider_id)
}

/// 设置指定应用的自动故障转移开关状态（写入 proxy_config 表）
///
/// 注意：关闭故障转移时不会清除队列，队列内容会保留供下次开启时使用
#[tauri::command]
#[specta::specta]
pub async fn set_auto_failover_enabled(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    app_type: String,
    enabled: bool,
) -> Result<(), String> {
    log::info!(
        "[Failover] Setting auto_failover_enabled: app_type='{app_type}', enabled={enabled}"
    );
    let app_enum = canonical_app_type(&app_type)?;

    if enabled {
        let p1_provider_id = enable_auto_failover(&state, &app_enum).await?;
        // 发射 provider-switched 事件（让前端刷新当前供应商）
        let event_data = serde_json::json!({
            "appType": app_enum.as_str(),
            "providerId": p1_provider_id,
            "source": "failoverEnabled"
        });
        let _ = app.emit(crate::events::PROVIDER_SWITCHED, event_data);
    } else {
        let mut config = state
            .db
            .get_proxy_config_for_app(app_enum.as_str())
            .await
            .map_err(|e| e.to_string())?;
        config.auto_failover_enabled = false;
        state
            .db
            .update_proxy_config_for_app(config)
            .await
            .map_err(|e| e.to_string())?;
    }

    // 刷新托盘菜单（去抖），确保状态同步
    crate::tray::request_tray_rebuild(&app, false);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{canonical_app_type, enable_auto_failover};
    use crate::app_config::AppType;
    use crate::database::Database;
    use crate::store::AppState;
    use std::sync::Arc;

    #[test]
    fn app_type_is_validated_and_canonicalised() {
        assert_eq!(canonical_app_type("grok").unwrap(), AppType::GrokBuild);
        assert_eq!(canonical_app_type("Claude").unwrap(), AppType::Claude);
        assert!(canonical_app_type("../etc").is_err());
        assert!(canonical_app_type("").is_err());
    }

    /// CMD-44: when the takeover is off, enabling fails without touching the
    /// queue or the persisted flag (the tray used to persist flags first).
    #[tokio::test]
    async fn enable_auto_failover_refuses_without_takeover_and_persists_nothing() {
        let db = Arc::new(Database::memory().expect("db"));
        let state = AppState::new(db.clone());

        let err = enable_auto_failover(&state, &AppType::Claude)
            .await
            .expect_err("must refuse without takeover");
        assert!(err.contains("代理接管"));

        let config = db.get_proxy_config_for_app("claude").await.unwrap();
        assert!(!config.auto_failover_enabled);
        assert!(db.get_failover_queue("claude").unwrap().is_empty());
    }

    /// CMD-44: an empty queue with no current provider fails cleanly too.
    #[tokio::test]
    async fn enable_auto_failover_with_empty_queue_and_no_current_provider_fails_cleanly() {
        let db = Arc::new(Database::memory().expect("db"));
        let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
        config.enabled = true;
        db.update_proxy_config_for_app(config).await.unwrap();
        let state = AppState::new(db.clone());

        assert!(enable_auto_failover(&state, &AppType::Claude)
            .await
            .is_err());
        let config = db.get_proxy_config_for_app("claude").await.unwrap();
        assert!(!config.auto_failover_enabled);
        assert!(db.get_failover_queue("claude").unwrap().is_empty());
    }
}
