//! 故障转移服务（CCS-181）
//!
//! 承接原先写在命令里的故障转移策略：队列维护、自动故障转移开关、熔断器重置后
//! 切回高优先级供应商、立即探测熔断器。命令层只负责解析参数并转换错误。

use std::str::FromStr;

use serde_json::json;

use crate::app_config::AppType;
use crate::commands::CopilotAuthState;
use crate::ctx::Ctx;
use crate::database::FailoverQueueItem;
use crate::error::AppError;
use crate::provider::Provider;
use crate::proxy::failover_switch::FailoverSwitchManager;
use crate::proxy::types::ProviderHealth;
use crate::proxy::CircuitProbeResult;
use crate::services::stream_check::StreamCheckService;
use crate::store::AppState;

pub struct FailoverService;

impl FailoverService {
    /// 故障转移队列（按 sort_index 排序）
    pub fn queue(state: &AppState, app_type: &str) -> Result<Vec<FailoverQueueItem>, AppError> {
        state.db.get_failover_queue(app_type)
    }

    /// 可加入故障转移队列的供应商（尚未在队列中的）
    pub fn available_providers(
        state: &AppState,
        app_type: &str,
    ) -> Result<Vec<Provider>, AppError> {
        state.db.get_available_providers_for_failover(app_type)
    }

    pub fn add_to_queue(
        state: &AppState,
        app_type: &str,
        provider_id: &str,
    ) -> Result<(), AppError> {
        state.db.add_to_failover_queue(app_type, provider_id)
    }

    pub fn remove_from_queue(
        state: &AppState,
        app_type: &str,
        provider_id: &str,
    ) -> Result<(), AppError> {
        state.db.remove_from_failover_queue(app_type, provider_id)
    }

    /// 指定应用的自动故障转移开关（proxy_config 表）
    pub async fn auto_failover_enabled(state: &AppState, app_type: &str) -> Result<bool, AppError> {
        state
            .db
            .get_proxy_config_for_app(app_type)
            .await
            .map(|config| config.auto_failover_enabled)
    }

    /// 设置自动故障转移开关
    ///
    /// 开启时先切到队列 P1，切换成功后才写入开关；队列为空时把当前供应商自动
    /// 加入作为 P1（切换失败则撤回）。关闭时不清空队列，保留给下次开启使用。
    pub async fn set_auto_failover_enabled(
        ctx: &Ctx<'_>,
        app_type: &str,
        enabled: bool,
    ) -> Result<(), AppError> {
        let state = ctx.state;
        log::info!(
            "[Failover] Setting auto_failover_enabled: app_type='{app_type}', enabled={enabled}"
        );

        let mut config = state.db.get_proxy_config_for_app(app_type).await?;
        if enabled && !config.enabled {
            return Err(AppError::Message(
                "需要先启用该应用的代理接管，再开启故障转移".to_string(),
            ));
        }

        let mut p1_provider_id = String::new();
        if enabled {
            let (p1, auto_added) = Self::ensure_queue_head(state, app_type)?;
            // 只有切换成功后才写入 auto_failover_enabled=true，避免 P1 不可切换
            // （例如 official provider）时留下“开关已开但目标未切”的脏状态。
            if let Err(e) = state.proxy_service.switch_proxy_target(app_type, &p1).await {
                if let Some(provider_id) = auto_added {
                    let _ = state.db.remove_from_failover_queue(app_type, &provider_id);
                }
                return Err(AppError::Message(e));
            }
            p1_provider_id = p1;
        }

        config.auto_failover_enabled = enabled;
        state.db.update_proxy_config_for_app(config).await?;

        if enabled {
            // 让前端刷新当前供应商
            ctx.events.emit(
                "provider-switched",
                json!({
                    "appType": app_type,
                    "providerId": p1_provider_id,
                    "source": "failoverEnabled"
                }),
            );
        }
        ctx.events.refresh_tray();
        Ok(())
    }

    /// 返回队列 P1；队列为空时把当前供应商加入队列，并在第二项返回其 ID，
    /// 避免用户陷入"必须先加队列才能开启"的死锁
    fn ensure_queue_head(
        state: &AppState,
        app_type: &str,
    ) -> Result<(String, Option<String>), AppError> {
        let mut queue = state.db.get_failover_queue(app_type)?;
        let mut auto_added = None;
        if queue.is_empty() {
            let app = AppType::from_str(app_type)
                .map_err(|_| AppError::Message(format!("无效的应用类型: {app_type}")))?;
            let Some(current_id) =
                crate::settings::get_effective_current_provider(&state.db, &app)?
            else {
                return Err(AppError::Message(
                    "故障转移队列为空，且未设置当前供应商，无法开启故障转移".to_string(),
                ));
            };
            state.db.add_to_failover_queue(app_type, &current_id)?;
            auto_added = Some(current_id);
            queue = state.db.get_failover_queue(app_type)?;
        }
        let head = queue
            .first()
            .map(|item| item.provider_id.clone())
            .ok_or_else(|| AppError::Message("故障转移队列为空，无法开启故障转移".to_string()))?;
        Ok((head, auto_added))
    }

    /// 供应商健康状态
    pub async fn provider_health(
        state: &AppState,
        provider_id: &str,
        app_type: &str,
    ) -> Result<ProviderHealth, AppError> {
        state.db.get_provider_health(provider_id, app_type).await
    }

    /// 重置熔断器
    ///
    /// 重置数据库健康状态与内存熔断器后，若该应用已被接管、开启了自动故障转移
    /// 且代理在运行，而恢复的供应商在队列中的优先级高于当前供应商，则自动切回。
    /// 返回尝试切回的供应商 ID（未触发切回时为 None）；切回失败只记日志。
    pub async fn reset_circuit_breaker(
        ctx: &Ctx<'_>,
        provider_id: &str,
        app_type: &str,
    ) -> Result<Option<String>, AppError> {
        let state = ctx.state;
        state
            .db
            .update_provider_health(provider_id, app_type, true, None)
            .await?;
        state
            .proxy_service
            .reset_provider_circuit_breaker(provider_id, app_type)
            .await
            .map_err(AppError::Message)?;

        let (app_enabled, auto_failover_enabled) =
            match state.db.get_proxy_config_for_app(app_type).await {
                Ok(config) => (config.enabled, config.auto_failover_enabled),
                Err(e) => {
                    log::error!(
                        "[{app_type}] Failed to read proxy_config: {e}, defaulting to disabled"
                    );
                    (false, false)
                }
            };
        if !(app_enabled && auto_failover_enabled && state.proxy_service.is_running().await) {
            return Ok(None);
        }

        let Some(current_id) = state.db.get_current_provider(app_type)? else {
            return Ok(None);
        };
        let queue = state.db.get_failover_queue(app_type)?;
        let Some((restored, current)) = recovery_priorities(&queue, provider_id, &current_id)
        else {
            return Ok(None);
        };
        log::info!(
            "[Recovery] 供应商 {provider_id} 已恢复且优先级更高 (P{restored} vs P{current})，自动切换"
        );

        let provider_name = state
            .db
            .get_all_providers(app_type)
            .ok()
            .and_then(|providers| providers.get(provider_id).map(|p| p.name.clone()))
            .unwrap_or_else(|| provider_id.to_string());
        let switch_manager = FailoverSwitchManager::new(state.db.clone());
        if let Err(e) = switch_manager
            .try_switch(
                ctx.events.app_handle(),
                app_type,
                provider_id,
                &provider_name,
            )
            .await
        {
            log::error!("[Recovery] 自动切换失败: {e}");
        }
        Ok(Some(provider_id.to_string()))
    }

    /// 立即探测熔断器（"probe now"）
    ///
    /// 经 HalfOpen 路径发送一次最小探测请求（见 `StreamCheckService::probe_circuit_target`）：
    /// Open 状态下不等待超时；探测成功则熔断器直接 Closed，失败则重新 Open。
    /// 代理未运行时返回 None，不发请求。
    pub async fn probe_circuit_breaker(
        state: &AppState,
        copilot_state: &CopilotAuthState,
        provider_id: &str,
        app_type: &str,
    ) -> Result<Option<CircuitProbeResult>, AppError> {
        let app = AppType::from_str(app_type)?;
        let providers = state.db.get_all_providers(app_type)?;
        let provider = providers
            .get(provider_id)
            .ok_or_else(|| AppError::Message(format!("供应商 {provider_id} 不存在")))?;
        let config = state.db.get_stream_check_config()?;
        let base_url_override =
            StreamCheckService::resolve_copilot_base_url_override(provider, copilot_state).await?;

        state
            .proxy_service
            .probe_provider_circuit_breaker(provider_id, app_type, || {
                StreamCheckService::probe_circuit_target(&app, provider, &config, base_url_override)
            })
            .await
    }
}

/// 恢复的供应商在队列中的优先级（sort_index 越小越高）严格高于当前供应商时，
/// 返回 `(恢复者, 当前)` 的 sort_index；任一方不在队列或无序号时返回 None
fn recovery_priorities(
    queue: &[FailoverQueueItem],
    restored_id: &str,
    current_id: &str,
) -> Option<(usize, usize)> {
    let order = |id: &str| {
        queue
            .iter()
            .find(|item| item.provider_id == id)
            .and_then(|item| item.sort_index)
    };
    let (restored, current) = (order(restored_id)?, order(current_id)?);
    (restored < current).then_some((restored, current))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::RecordingEventSink;
    use std::sync::Arc;

    fn item(id: &str, sort_index: Option<usize>) -> FailoverQueueItem {
        FailoverQueueItem {
            provider_id: id.to_string(),
            provider_name: id.to_string(),
            sort_index,
            provider_notes: None,
        }
    }

    #[test]
    fn recovery_switches_back_only_to_a_strictly_higher_priority_provider() {
        let queue = vec![item("p1", Some(1)), item("p2", Some(2)), item("p3", None)];
        assert_eq!(recovery_priorities(&queue, "p1", "p2"), Some((1, 2)));
        // 优先级更低或相同：不切回
        assert_eq!(recovery_priorities(&queue, "p2", "p1"), None);
        assert_eq!(recovery_priorities(&queue, "p1", "p1"), None);
        // 不在队列或无序号：不切回
        assert_eq!(recovery_priorities(&queue, "p3", "p2"), None);
        assert_eq!(recovery_priorities(&queue, "missing", "p2"), None);
        assert_eq!(recovery_priorities(&queue, "p1", "missing"), None);
    }

    fn state() -> AppState {
        AppState::new(Arc::new(
            crate::database::Database::memory().expect("memory db"),
        ))
    }

    #[tokio::test]
    async fn reset_circuit_breaker_marks_provider_healthy_without_switching_when_proxy_is_stopped()
    {
        let state = state();
        let provider = Provider::with_id(
            "p1".to_string(),
            "P1".to_string(),
            serde_json::json!({}),
            None,
        );
        state
            .db
            .save_provider("claude", &provider)
            .expect("save provider");
        state
            .db
            .update_provider_health("p1", "claude", false, Some("boom".to_string()))
            .await
            .expect("mark unhealthy");
        let sink = RecordingEventSink::new();
        let ctx = Ctx::new(&state, &sink);

        let switched = FailoverService::reset_circuit_breaker(&ctx, "p1", "claude")
            .await
            .expect("reset");

        assert_eq!(switched, None);
        let health = FailoverService::provider_health(&state, "p1", "claude")
            .await
            .expect("health");
        assert!(health.is_healthy);
        assert!(sink.events().is_empty());
    }

    #[tokio::test]
    async fn enabling_auto_failover_requires_proxy_takeover_and_leaves_no_events() {
        let state = state();
        let sink = RecordingEventSink::new();
        let ctx = Ctx::new(&state, &sink);

        let err = FailoverService::set_auto_failover_enabled(&ctx, "claude", true)
            .await
            .expect_err("takeover is off");
        assert!(err.to_string().contains("代理接管"), "{err}");
        assert!(!FailoverService::auto_failover_enabled(&state, "claude")
            .await
            .expect("read flag"));
        assert!(sink.events().is_empty());
        assert_eq!(sink.tray_refreshes(), 0);
    }

    #[tokio::test]
    async fn disabling_auto_failover_refreshes_the_tray_without_a_switch_event() {
        let state = state();
        let sink = RecordingEventSink::new();
        let ctx = Ctx::new(&state, &sink);

        FailoverService::set_auto_failover_enabled(&ctx, "claude", false)
            .await
            .expect("disable");

        assert!(sink.events().is_empty());
        assert_eq!(sink.tray_refreshes(), 1);
    }
}
