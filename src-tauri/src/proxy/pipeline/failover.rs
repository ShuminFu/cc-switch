//! 故障转移记账
//!
//! `AttemptRecorder` 是故障转移循环里唯一更新 `ProxyStatus`、熔断器、数据库健康度
//! 并触发 `try_switch` 的地方：
//!
//! - 请求维度：[`AttemptRecorder::fail_request`] 记一次请求失败（失败计数 + 成功率）；
//! - 尝试维度：[`AttemptRecorder::begin`] 申请熔断器放行许可，返回的 [`Attempt`] 持有
//!   `HalfOpenPermit` 守卫，并通过消耗 `self` 的方法只记录一次结果。若请求在记录结果前
//!   被取消（future 被 drop），守卫随 `Attempt` 一起 Drop，释放 HalfOpen 探测名额。
//!
//! 切换计数：只有本次请求里排在前面的 provider 失败（或被熔断器拒绝）后由后面的
//! provider 接手，才计一次 `failover_count`；首个尝试就成功、但与请求开始时的当前供应商
//! 不同（回到更高优先级的 provider）计一次 `recovery_count`。两者都以
//! `current_providers` 中记录的「最近一次实际使用的 provider」去重，
//! 并发请求在异步切换落地前各自成功时只计一次。

use crate::provider::Provider;
use crate::proxy::circuit_breaker::HalfOpenPermit;
use crate::proxy::forwarder::ForwarderDeps;
use crate::proxy::types::ProxyStatus;
use std::sync::atomic::{AtomicBool, Ordering};

/// 按当前计数重新计算成功率
fn refresh_success_rate(status: &mut ProxyStatus) {
    if status.total_requests > 0 {
        status.success_rate =
            (status.success_requests as f32 / status.total_requests as f32) * 100.0;
    }
}

/// 单个客户端请求的故障转移记账器
pub(crate) struct AttemptRecorder<'a> {
    deps: &'a ForwarderDeps,
    app_type: &'a str,
    /// 请求开始时的"当前供应商 ID"；成功的 provider 与之不同即视为发生了故障转移
    current_provider_id_at_start: &'a str,
    /// 本次请求中是否已有排在前面的 provider 失败或被熔断器拒绝
    earlier_provider_failed: AtomicBool,
}

impl<'a> AttemptRecorder<'a> {
    pub(crate) fn new(
        deps: &'a ForwarderDeps,
        app_type: &'a str,
        current_provider_id_at_start: &'a str,
    ) -> Self {
        Self {
            deps,
            app_type,
            current_provider_id_at_start,
            earlier_provider_failed: AtomicBool::new(false),
        }
    }

    fn mark_provider_failed(&self) {
        self.earlier_provider_failed.store(true, Ordering::Relaxed);
    }

    /// 为 `provider` 开始一次尝试
    ///
    /// 先获取熔断器放行许可（HalfOpen 会占用探测名额）；`bypass_breaker` 为 true 时
    /// （单 Provider 场景）跳过检查，避免熔断器阻塞所有请求。被熔断器拒绝时返回 `None`。
    /// 放行后刷新「正在尝试哪个 provider」的展示字段。
    pub(crate) async fn begin<'p>(
        &self,
        provider: &'p Provider,
        bypass_breaker: bool,
    ) -> Option<Attempt<'_, 'p>> {
        let permit = if bypass_breaker {
            None
        } else {
            let allow = self
                .deps
                .router
                .allow_provider_request(&provider.id, self.app_type)
                .await;
            if !allow.allowed {
                self.mark_provider_failed();
                return None;
            }
            allow.permit
        };

        {
            let mut status = self.deps.status.write().await;
            status.current_provider = Some(provider.name.clone());
            status.current_provider_id = Some(provider.id.clone());
        }

        Some(Attempt {
            recorder: self,
            provider,
            permit,
        })
    }

    /// 记录一次客户端请求的最终失败（失败计数、最近错误与成功率）
    pub(crate) async fn fail_request(&self, message: String) {
        let mut status = self.deps.status.write().await;
        status.failed_requests += 1;
        status.last_error = Some(message);
        refresh_success_rate(&mut status);
    }
}

/// 对单个 provider 的一次尝试（含同一 provider 上的整流重试）
///
/// 持有熔断器 HalfOpen 探测许可；以下方法都消耗 `self`，保证每次尝试只记录一次结果。
pub(crate) struct Attempt<'r, 'p> {
    recorder: &'r AttemptRecorder<'r>,
    provider: &'p Provider,
    permit: Option<HalfOpenPermit>,
}

impl Attempt<'_, '_> {
    /// 尝试成功：记录熔断器/健康度，更新当前 provider 与成功统计；
    /// 实际使用的 provider 不是请求开始时的当前供应商时触发切换，
    /// 并按是否有前序 provider 失败分别计一次故障转移或恢复（见模块文档）
    pub(crate) async fn succeed(self) {
        let recorder = self.recorder;
        let deps = recorder.deps;
        let app_type = recorder.app_type;
        let provider = self.provider;

        // 普通闭合熔断状态异步记录，避免阻塞流式首包返回；
        // HalfOpen 探测仍同步等待，保证 permit 与熔断状态及时释放。
        if self.permit.is_some() {
            if let Err(e) = deps
                .router
                .record_result(&provider.id, app_type, self.permit.as_ref(), true, None)
                .await
            {
                log::warn!(
                    "[{app_type}] 记录 Provider 成功结果失败: provider_id={}, error={e}",
                    provider.id
                );
            }
        } else {
            let router = deps.router.clone();
            let provider_id = provider.id.clone();
            let app_type = app_type.to_string();
            tokio::spawn(async move {
                if let Err(e) = router
                    .record_result(&provider_id, &app_type, None, true, None)
                    .await
                {
                    log::warn!(
                        "[{app_type}] 异步记录 Provider 成功结果失败: provider_id={provider_id}, error={e}"
                    );
                }
            });
        }

        // 更新当前应用类型使用的 provider；写锁内取回上一次实际使用的 provider，
        // 已被并发请求切到同一 provider 时不再重复计数
        let previous_provider_id = {
            let mut current_providers = deps.current_providers.write().await;
            current_providers
                .insert(
                    app_type.to_string(),
                    (provider.id.clone(), provider.name.clone()),
                )
                .map(|(id, _)| id)
        };

        let switched_from_start = recorder.current_provider_id_at_start != provider.id.as_str();
        let newly_switched =
            switched_from_start && previous_provider_id.as_deref() != Some(provider.id.as_str());

        let mut status = deps.status.write().await;
        status.success_requests += 1;
        status.last_error = None;
        if newly_switched {
            if recorder.earlier_provider_failed.load(Ordering::Relaxed) {
                status.failover_count += 1;
            } else {
                status.recovery_count += 1;
                log::info!(
                    "[{app_type}] 已恢复到 Provider {} (id={})",
                    provider.name,
                    provider.id
                );
            }
        }
        if switched_from_start {
            // 异步触发供应商切换，更新 UI/托盘，并把"当前供应商"同步为实际使用的 provider
            let fm = deps.failover_manager.clone();
            let ah = deps.app_handle.clone();
            let pid = provider.id.clone();
            let pname = provider.name.clone();
            let at = app_type.to_string();
            tokio::spawn(async move {
                let _ = fm.try_switch(ah.as_ref(), &at, &pid, &pname).await;
            });
        }
        refresh_success_rate(&mut status);
    }

    /// Provider 故障：记录熔断器失败与数据库健康度，并刷新 `last_error`。
    ///
    /// 不计入 `failed_requests`——调用方会继续尝试下一家 provider，
    /// 全部失败时再由 [`AttemptRecorder::fail_request`] 记一次请求失败。
    pub(crate) async fn fail_provider(self, error: String, status_message: String) {
        let recorder = self.recorder;
        recorder.mark_provider_failed();
        let _ = recorder
            .deps
            .router
            .record_result(
                &self.provider.id,
                recorder.app_type,
                self.permit.as_ref(),
                false,
                Some(error),
            )
            .await;
        let mut status = recorder.deps.status.write().await;
        status.last_error = Some(status_message);
    }

    /// 客户端层错误：换哪家 provider 都会被拒绝，不污染熔断器和健康度，
    /// 仅中性释放 HalfOpen 许可，并记一次请求失败
    pub(crate) async fn fail_request(self, message: String) {
        let recorder = self.recorder;
        recorder
            .deps
            .router
            .release_permit_neutral(&self.provider.id, recorder.app_type, self.permit.as_ref())
            .await;
        recorder.fail_request(message).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::proxy::circuit_breaker::CircuitBreakerConfig;
    use crate::proxy::failover_switch::FailoverSwitchManager;
    use crate::proxy::provider_router::ProviderRouter;
    use crate::proxy::providers::{
        codex_chat_history::CodexChatHistoryStore, gemini_shadow::GeminiShadowStore,
    };
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    fn provider(id: &str) -> Provider {
        Provider::with_id(id.to_string(), format!("Provider {id}"), json!({}), None)
    }

    async fn deps_with_breaker(timeout_seconds: u64) -> ForwarderDeps {
        let db = Arc::new(Database::memory().expect("memory db"));
        db.get_proxy_config_for_app("claude")
            .await
            .expect("create proxy config");
        db.update_circuit_breaker_config(&CircuitBreakerConfig {
            failure_threshold: 1,
            timeout_seconds,
            ..Default::default()
        })
        .await
        .expect("write circuit config");
        for id in ["a", "b"] {
            db.save_provider("claude", &provider(id))
                .expect("save provider");
        }

        ForwarderDeps {
            router: Arc::new(ProviderRouter::new(db.clone())),
            status: Arc::new(RwLock::new(ProxyStatus::default())),
            current_providers: Arc::new(RwLock::new(HashMap::new())),
            gemini_shadow: Arc::new(GeminiShadowStore::new()),
            codex_chat_history: Arc::new(CodexChatHistoryStore::default()),
            failover_manager: Arc::new(FailoverSwitchManager::new(db)),
            app_handle: None,
        }
    }

    /// 让 provider 进入 Open（timeout=0 时下一次 allow 即进入 HalfOpen 探测）
    async fn trip(deps: &ForwarderDeps, provider_id: &str) {
        deps.router
            .record_result(provider_id, "claude", None, false, Some("boom".into()))
            .await
            .expect("record failure");
    }

    #[tokio::test]
    async fn success_on_start_provider_updates_stats_without_failover() {
        let deps = deps_with_breaker(60).await;
        deps.status.write().await.total_requests = 2;
        let recorder = AttemptRecorder::new(&deps, "claude", "a");
        let a = provider("a");

        let attempt = recorder.begin(&a, false).await.expect("closed breaker");
        attempt.succeed().await;

        let status = deps.status.read().await;
        assert_eq!(status.success_requests, 1);
        assert_eq!(status.failover_count, 0);
        assert_eq!(status.success_rate, 50.0);
        assert_eq!(status.current_provider_id.as_deref(), Some("a"));
        drop(status);
        assert_eq!(
            deps.current_providers.read().await.get("claude"),
            Some(&("a".to_string(), "Provider a".to_string()))
        );
    }

    #[tokio::test]
    async fn success_after_earlier_failure_counts_one_failover() {
        let deps = deps_with_breaker(60).await;
        deps.status.write().await.total_requests = 1;
        let recorder = AttemptRecorder::new(&deps, "claude", "a");
        let a = provider("a");
        let b = provider("b");

        recorder
            .begin(&a, false)
            .await
            .expect("closed breaker")
            .fail_provider("boom".into(), "Provider a 失败: boom".into())
            .await;
        recorder
            .begin(&b, false)
            .await
            .expect("closed breaker")
            .succeed()
            .await;

        let status = deps.status.read().await;
        assert_eq!(status.success_requests, 1);
        assert_eq!(status.failover_count, 1);
        assert_eq!(status.recovery_count, 0);
        assert_eq!(status.success_rate, 100.0);
        assert!(status.last_error.is_none());
    }

    #[tokio::test]
    async fn breaker_rejected_provider_counts_as_earlier_failure() {
        let deps = deps_with_breaker(60).await;
        trip(&deps, "a").await;
        let recorder = AttemptRecorder::new(&deps, "claude", "a");
        let (a, b) = (provider("a"), provider("b"));

        assert!(recorder.begin(&a, false).await.is_none());
        recorder
            .begin(&b, false)
            .await
            .expect("closed")
            .succeed()
            .await;

        let status = deps.status.read().await;
        assert_eq!(status.failover_count, 1);
        assert_eq!(status.recovery_count, 0);
    }

    #[tokio::test]
    async fn first_attempt_on_other_provider_counts_recovery_not_failover() {
        let deps = deps_with_breaker(60).await;
        // 上一次实际使用的是 b（此前故障转移到 b），请求开始时当前供应商也是 b
        deps.current_providers
            .write()
            .await
            .insert("claude".into(), ("b".into(), "Provider b".into()));
        let recorder = AttemptRecorder::new(&deps, "claude", "b");
        let a = provider("a");

        recorder
            .begin(&a, false)
            .await
            .expect("closed")
            .succeed()
            .await;

        let status = deps.status.read().await;
        assert_eq!(status.failover_count, 0);
        assert_eq!(status.recovery_count, 1);
    }

    #[tokio::test]
    async fn concurrent_switches_to_the_same_provider_count_once() {
        let deps = deps_with_breaker(60).await;
        let (a, b) = (provider("a"), provider("b"));
        // 两个请求都在切换落地前开始（当前供应商仍是 a），且 a 都失败
        let first = AttemptRecorder::new(&deps, "claude", "a");
        let second = AttemptRecorder::new(&deps, "claude", "a");
        for recorder in [&first, &second] {
            recorder
                .begin(&a, true)
                .await
                .expect("bypass")
                .fail_provider("boom".into(), "boom".into())
                .await;
        }

        first.begin(&b, true).await.expect("bypass").succeed().await;
        second
            .begin(&b, true)
            .await
            .expect("bypass")
            .succeed()
            .await;

        let status = deps.status.read().await;
        assert_eq!(status.success_requests, 2);
        assert_eq!(status.failover_count, 1);
    }

    #[tokio::test]
    async fn provider_failure_trips_breaker_without_counting_request_failure() {
        let deps = deps_with_breaker(60).await;
        let recorder = AttemptRecorder::new(&deps, "claude", "a");
        let a = provider("a");

        recorder
            .begin(&a, false)
            .await
            .expect("closed breaker")
            .fail_provider("boom".into(), "Provider a 失败: boom".into())
            .await;

        assert!(
            recorder.begin(&a, false).await.is_none(),
            "a provider failure must reach the breaker"
        );
        assert!(
            recorder.begin(&a, true).await.is_some(),
            "single-provider bypass must skip the breaker"
        );
        let status = deps.status.read().await;
        assert_eq!(status.failed_requests, 0);
        assert_eq!(status.last_error.as_deref(), Some("Provider a 失败: boom"));
    }

    #[tokio::test]
    async fn client_error_releases_half_open_permit_and_counts_request_failure() {
        let deps = deps_with_breaker(0).await;
        deps.status.write().await.total_requests = 4;
        deps.status.write().await.success_requests = 1;
        trip(&deps, "a").await;
        let recorder = AttemptRecorder::new(&deps, "claude", "a");
        let a = provider("a");

        let probe = recorder.begin(&a, false).await.expect("half-open probe");
        assert!(
            recorder.begin(&a, false).await.is_none(),
            "the probe slot is held while the attempt is in flight"
        );
        probe.fail_request("bad request".into()).await;

        let next = deps.router.allow_provider_request("a", "claude").await;
        assert!(next.allowed, "a client error must free the probe slot");
        assert!(
            next.used_half_open_permit(),
            "and leave the breaker half-open"
        );
        let status = deps.status.read().await;
        assert_eq!(status.failed_requests, 1);
        assert_eq!(status.success_rate, 25.0);
        assert_eq!(status.last_error.as_deref(), Some("bad request"));
    }

    #[tokio::test]
    async fn dropped_attempt_releases_half_open_permit() {
        let deps = deps_with_breaker(0).await;
        trip(&deps, "a").await;
        let recorder = AttemptRecorder::new(&deps, "claude", "a");
        let a = provider("a");

        let probe = recorder.begin(&a, false).await.expect("half-open probe");
        drop(probe);

        let next = deps.router.allow_provider_request("a", "claude").await;
        assert!(
            next.allowed,
            "cancelling an attempt must free the probe slot"
        );
        assert!(next.used_half_open_permit());
    }

    #[tokio::test]
    async fn request_failure_refreshes_success_rate() {
        let deps = deps_with_breaker(60).await;
        {
            let mut status = deps.status.write().await;
            status.total_requests = 2;
            status.success_requests = 1;
        }
        let recorder = AttemptRecorder::new(&deps, "claude", "a");

        recorder.fail_request("所有供应商都失败".into()).await;

        let status = deps.status.read().await;
        assert_eq!(status.failed_requests, 1);
        assert_eq!(status.success_rate, 50.0);
        assert_eq!(status.last_error.as_deref(), Some("所有供应商都失败"));
    }
}
