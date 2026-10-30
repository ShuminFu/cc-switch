//! 供应商路由器模块
//!
//! 负责选择和管理代理目标供应商，实现智能故障转移

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use crate::proxy::circuit_breaker::{
    AllowResult, CircuitBreaker, CircuitBreakerConfig, CircuitBreakerStats, CircuitProbeResult,
    CircuitState, HalfOpenPermit,
};
use crate::proxy::protocol::{AppRef, BreakerKey};
use crate::proxy::runtime_config::RuntimeConfigStore;
use indexmap::IndexMap;
use std::collections::HashMap;
use std::future::Future;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// 会话粘性路由表的容量上限：超出后淘汰最久未使用的会话
pub const STICKY_SESSION_CAPACITY: usize = 1024;

/// 会话粘性路由表：有界 LRU，(应用, session_id) → 上次成功服务该会话的 provider_id
///
/// 让故障转移后的会话继续留在接手的 provider 上，避免回到队首时丢失上游 prompt 缓存。
struct StickySessions {
    capacity: usize,
    /// 按最近使用排序：队首最久未使用，队尾最近使用
    entries: IndexMap<(AppRef, String), String>,
}

impl StickySessions {
    fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: IndexMap::new(),
        }
    }

    /// 查询并把命中的会话标记为最近使用
    fn get(&mut self, key: &(AppRef, String)) -> Option<String> {
        let index = self.entries.get_index_of(key)?;
        let last = self.entries.len() - 1;
        self.entries.move_index(index, last);
        self.entries.get_index(last).map(|(_, id)| id.clone())
    }

    /// 写入（或刷新）会话映射；超出容量时淘汰最久未使用的会话
    fn insert(&mut self, key: (AppRef, String), provider_id: String) {
        self.entries.shift_remove(&key);
        self.entries.insert(key, provider_id);
        while self.entries.len() > self.capacity {
            self.entries.shift_remove_index(0);
        }
    }
}

/// 供应商路由器
pub struct ProviderRouter {
    /// 数据库连接
    db: Arc<Database>,
    /// 运行时配置快照（请求路径上的配置/供应商读取不再直连数据库）
    runtime_config: Arc<RuntimeConfigStore>,
    /// 熔断器管理器 - 每个（应用, 供应商）一个熔断器
    circuit_breakers: Arc<RwLock<HashMap<BreakerKey, Arc<CircuitBreaker>>>>,
    /// 限流冷却截止时间 - 上游 429/529 带 Retry-After 时写入，与熔断器相互独立
    cooldowns: Arc<Mutex<HashMap<BreakerKey, Instant>>>,
    /// 会话粘性路由表（仅在开启「会话粘性路由」时读写）
    sticky_sessions: Mutex<StickySessions>,
}

impl ProviderRouter {
    /// 创建新的供应商路由器（独立的运行时配置快照，测试用）
    #[cfg(test)]
    pub fn new(db: Arc<Database>) -> Self {
        let runtime_config = Arc::new(RuntimeConfigStore::new(db.clone()));
        Self::with_runtime_config(db, runtime_config)
    }

    /// 使用共享的运行时配置快照创建路由器（代理服务器内与 `ProxyState` 共用同一份）
    pub fn with_runtime_config(db: Arc<Database>, runtime_config: Arc<RuntimeConfigStore>) -> Self {
        Self {
            db,
            runtime_config,
            circuit_breakers: Arc::new(RwLock::new(HashMap::new())),
            cooldowns: Arc::new(Mutex::new(HashMap::new())),
            sticky_sessions: Mutex::new(StickySessions::new(STICKY_SESSION_CAPACITY)),
        }
    }

    /// 选择可用的供应商（支持故障转移）
    ///
    /// 返回按优先级排序的可用供应商列表：
    /// - 故障转移关闭时：仅返回当前供应商
    /// - 故障转移开启时：仅使用故障转移队列，按队列顺序依次尝试（P1 → P2 → ...）；
    ///   处于限流冷却中的 provider 被跳过，只有其余 provider 都不可用时才按队列顺序兜底返回
    pub async fn select_providers(&self, app_type: &str) -> Result<Vec<Provider>, AppError> {
        self.select_providers_for_session(app_type, None).await
    }

    /// 与 [`Self::select_providers`] 相同，但带会话粘性：
    ///
    /// `sticky_session` 为 `Some` 且故障转移开启时，若该会话上次成功使用的 provider
    /// 仍可用（熔断器放行且不在限流冷却中），把它移到队首，其余 provider 保持队列顺序。
    /// 调用方只在用户开启「会话粘性路由」且 session_id 由客户端提供时传入 `Some`。
    pub async fn select_providers_for_session(
        &self,
        app_type: &str,
        sticky_session: Option<&str>,
    ) -> Result<Vec<Provider>, AppError> {
        let mut result = Vec::new();
        let mut total_providers = 0usize;
        let mut circuit_open_count = 0usize;
        let mut cooling_down = Vec::new();

        // 检查该应用的自动故障转移开关是否开启（proxy_config，经运行时配置快照）
        let auto_failover_enabled = match self.runtime_config.app_proxy_config(app_type).await {
            Ok(config) => config.auto_failover_enabled,
            Err(e) => {
                log::error!("[{app_type}] 读取 proxy_config 失败: {e}，默认禁用故障转移");
                false
            }
        };

        let app_providers = self.runtime_config.app_providers(app_type)?;

        if auto_failover_enabled {
            // 故障转移开启：仅按队列顺序依次尝试（P1 → P2 → ...）
            // 使用 DAO 返回的队列排序，确保和前端展示一致
            total_providers = app_providers.failover_queue.len();

            for provider_id in &app_providers.failover_queue {
                let Some(provider) = app_providers.all.get(provider_id).cloned() else {
                    continue;
                };

                let circuit_key = BreakerKey::new(app_type, provider.id.as_str());
                let breaker = self.get_or_create_circuit_breaker(&circuit_key).await;

                if !breaker.is_available().await {
                    circuit_open_count += 1;
                } else if self.cooldown_remaining(&provider.id, app_type).is_some() {
                    cooling_down.push(provider);
                } else {
                    result.push(provider);
                }
            }

            if result.is_empty() && !cooling_down.is_empty() {
                log::info!(
                    "[{app_type}] 可用供应商均处于限流冷却中，按队列顺序继续尝试 ({} 个)",
                    cooling_down.len()
                );
                result = cooling_down;
            } else if let Some(session_id) = sticky_session {
                self.prefer_sticky_provider(app_type, session_id, &mut result);
            }
        } else {
            // 故障转移关闭：仅使用当前供应商，跳过熔断器检查
            let current_id = AppType::from_str(app_type)
                .ok()
                .and_then(|app_enum| app_providers.effective_current_provider(&app_enum))
                .or_else(|| app_providers.db_current.clone());

            if let Some(current_id) = current_id {
                if let Some(current) = app_providers.all.get(&current_id).cloned() {
                    total_providers = 1;
                    result.push(current);
                }
            }
        }

        if result.is_empty() {
            if total_providers > 0 && circuit_open_count == total_providers {
                log::warn!("[{app_type}] [FO-004] 所有供应商均已熔断");
                return Err(AppError::AllProvidersCircuitOpen);
            } else {
                log::warn!("[{app_type}] [FO-005] 未配置供应商");
                return Err(AppError::NoProvidersConfigured);
            }
        }

        Ok(result)
    }

    /// 把会话的粘性 provider（若在可用列表中）移到队首
    fn prefer_sticky_provider(&self, app_type: &str, session_id: &str, result: &mut Vec<Provider>) {
        let Some(sticky_id) = self.sticky_session_provider(app_type, session_id) else {
            return;
        };
        if let Some(index) = result.iter().position(|p| p.id == sticky_id) {
            if index > 0 {
                log::debug!(
                    "[{app_type}] 会话粘性路由: session={session_id} 优先使用 provider_id={sticky_id}"
                );
                let sticky = result.remove(index);
                result.insert(0, sticky);
            }
        }
    }

    /// 记录成功服务该会话的 provider（会话粘性路由）
    pub fn remember_session_provider(&self, app_type: &str, session_id: &str, provider_id: &str) {
        if session_id.is_empty() {
            return;
        }
        let mut sticky = self
            .sticky_sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        sticky.insert(
            (AppRef::from(app_type), session_id.to_string()),
            provider_id.to_string(),
        );
    }

    /// 该会话当前粘住的 provider（并标记为最近使用）
    pub fn sticky_session_provider(&self, app_type: &str, session_id: &str) -> Option<String> {
        let mut sticky = self
            .sticky_sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        sticky.get(&(AppRef::from(app_type), session_id.to_string()))
    }

    /// 请求执行前获取熔断器“放行许可”
    ///
    /// - Closed：直接放行
    /// - Open：超时到达后切到 HalfOpen 并放行一次探测
    /// - HalfOpen：按限流规则放行探测
    ///
    /// 注意：调用方需持有返回的 `HalfOpenPermit` 直到请求结束，并通过 `record_result()`
    /// 或 `release_permit_neutral()` 释放；请求被取消时守卫 Drop 也会释放名额。
    pub async fn allow_provider_request(&self, provider_id: &str, app_type: &str) -> AllowResult {
        let circuit_key = BreakerKey::new(app_type, provider_id);
        let breaker = self.get_or_create_circuit_breaker(&circuit_key).await;
        breaker.allow_request().await
    }

    /// 记录供应商请求结果
    pub async fn record_result(
        &self,
        provider_id: &str,
        app_type: &str,
        half_open_permit: Option<&HalfOpenPermit>,
        success: bool,
        error_msg: Option<String>,
    ) -> Result<(), AppError> {
        let circuit_key = BreakerKey::new(app_type, provider_id);
        let breaker = self.get_or_create_circuit_breaker(&circuit_key).await;

        // 1. 按应用独立获取熔断器配置；读取失败时回退到熔断器自身的阈值，
        //    保证数据库健康状态与熔断器使用同一个阈值
        let failure_threshold = self.failure_threshold_for(app_type, &breaker).await;

        // 2. 更新熔断器状态

        if success {
            breaker.record_success(half_open_permit).await;
        } else {
            breaker.record_failure(half_open_permit).await;
        }

        // 3. 更新数据库健康状态（使用配置的阈值）
        self.db
            .update_provider_health_with_threshold(
                provider_id,
                app_type,
                success,
                error_msg.clone(),
                failure_threshold,
            )
            .await?;

        Ok(())
    }

    /// 手动探测（"立即探测"）：经 HalfOpen 路径发送一次探测请求并记录结果
    ///
    /// - Closed：不发请求，直接返回当前状态（`probed = false`）
    /// - Open：不等待超时，立即切到 HalfOpen 并占用探测名额
    /// - HalfOpen：已有探测在途时不发请求（`probed = false`）
    ///
    /// `probe` 发出实际请求，`Ok(())` 表示供应商已恢复。成功后熔断器直接 Closed，
    /// 失败则重新 Open；结果同步写入数据库健康状态。
    pub async fn probe_provider<F, Fut>(
        &self,
        provider_id: &str,
        app_type: &str,
        probe: F,
    ) -> Result<CircuitProbeResult, AppError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<(), String>>,
    {
        let circuit_key = BreakerKey::new(app_type, provider_id);
        let breaker = self.get_or_create_circuit_breaker(&circuit_key).await;

        let allow = breaker.begin_manual_probe().await;
        if !allow.allowed {
            let stats = breaker.get_stats().await;
            let reason = match stats.state {
                CircuitState::Closed => "circuit breaker is closed",
                _ => "a half-open probe is already in flight",
            };
            return Ok(CircuitProbeResult {
                probed: false,
                success: false,
                message: Some(reason.to_string()),
                stats,
            });
        }

        let failure_threshold = self.failure_threshold_for(app_type, &breaker).await;
        let outcome = probe().await;
        let success = outcome.is_ok();
        let error_msg = outcome.err();
        log::info!(
            "[{app_type}] 手动探测 provider_id={provider_id}: {}",
            if success { "成功" } else { "失败" }
        );

        breaker
            .record_manual_probe_result(allow.permit.as_ref(), success)
            .await;
        self.db
            .update_provider_health_with_threshold(
                provider_id,
                app_type,
                success,
                error_msg.clone(),
                failure_threshold,
            )
            .await?;

        Ok(CircuitProbeResult {
            probed: true,
            success,
            message: error_msg,
            stats: breaker.get_stats().await,
        })
    }

    /// 健康状态使用的失败阈值：按应用读取配置，失败时回退到熔断器自身的阈值
    async fn failure_threshold_for(&self, app_type: &str, breaker: &CircuitBreaker) -> u32 {
        match self.runtime_config.app_proxy_config(app_type).await {
            Ok(app_config) => app_config.circuit_failure_threshold,
            Err(e) => {
                let fallback = breaker.failure_threshold().await;
                log::warn!("[{app_type}] 读取 proxy_config 失败: {e}，使用熔断器阈值 {fallback}");
                fallback
            }
        }
    }

    /// 让 provider 进入限流冷却（上游 429/529 的 Retry-After）
    ///
    /// 冷却只影响 [`Self::select_providers`] 的选择顺序，不改变熔断器状态；
    /// 已有更晚的冷却截止时间时保留较晚者。`duration` 为零时不做任何事。
    pub fn start_cooldown(&self, provider_id: &str, app_type: &str, duration: Duration) {
        if duration.is_zero() {
            return;
        }
        let until = Instant::now() + duration;
        let mut cooldowns = self.cooldowns.lock().unwrap_or_else(|e| e.into_inner());
        let entry = cooldowns
            .entry(BreakerKey::new(app_type, provider_id))
            .or_insert(until);
        if *entry < until {
            *entry = until;
        }
    }

    /// provider 剩余的限流冷却时长（未在冷却中时返回 `None`，并清理已过期的记录）
    pub fn cooldown_remaining(&self, provider_id: &str, app_type: &str) -> Option<Duration> {
        let key = BreakerKey::new(app_type, provider_id);
        let mut cooldowns = self.cooldowns.lock().unwrap_or_else(|e| e.into_inner());
        let until = *cooldowns.get(&key)?;
        let remaining = until.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            cooldowns.remove(&key);
            return None;
        }
        Some(remaining)
    }

    /// 重置熔断器（手动恢复）
    pub async fn reset_circuit_breaker(&self, circuit_key: &BreakerKey) {
        let breakers = self.circuit_breakers.read().await;
        if let Some(breaker) = breakers.get(circuit_key) {
            breaker.reset().await;
        }
    }

    /// 重置指定供应商的熔断器
    pub async fn reset_provider_breaker(&self, provider_id: &str, app_type: &str) {
        let circuit_key = BreakerKey::new(app_type, provider_id);
        self.reset_circuit_breaker(&circuit_key).await;
    }

    /// 仅释放 HalfOpen permit，不影响健康统计（neutral 接口）
    ///
    /// 用于整流器等场景：请求结果不应计入 Provider 健康度，
    /// 但仍需释放占用的探测名额，避免 HalfOpen 状态卡死
    pub async fn release_permit_neutral(
        &self,
        provider_id: &str,
        app_type: &str,
        half_open_permit: Option<&HalfOpenPermit>,
    ) {
        if let Some(permit) = half_open_permit {
            log::debug!("[{app_type}] 中性释放 HalfOpen 探测名额: provider_id={provider_id}");
            permit.release();
        }
    }

    /// 更新所有熔断器的配置（热更新）
    pub async fn update_all_configs(&self, config: CircuitBreakerConfig) {
        let breakers = self.circuit_breakers.read().await;
        for breaker in breakers.values() {
            breaker.update_config(config.clone()).await;
        }
    }

    /// 更新指定应用已创建熔断器的配置（热更新）
    pub async fn update_app_configs(&self, app_type: &str, config: CircuitBreakerConfig) {
        let app = AppRef::from(app_type);
        let breakers = self.circuit_breakers.read().await;
        for (key, breaker) in breakers.iter() {
            if key.app() == &app {
                breaker.update_config(config.clone()).await;
            }
        }
    }

    /// 获取熔断器状态（该供应商尚无熔断器实例时返回 `None`）
    pub async fn get_circuit_breaker_stats(
        &self,
        provider_id: &str,
        app_type: &str,
    ) -> Option<CircuitBreakerStats> {
        let circuit_key = BreakerKey::new(app_type, provider_id);
        let breakers = self.circuit_breakers.read().await;

        if let Some(breaker) = breakers.get(&circuit_key) {
            Some(breaker.get_stats().await)
        } else {
            None
        }
    }

    /// 获取或创建熔断器
    async fn get_or_create_circuit_breaker(&self, key: &BreakerKey) -> Arc<CircuitBreaker> {
        // 先尝试读锁获取
        {
            let breakers = self.circuit_breakers.read().await;
            if let Some(breaker) = breakers.get(key) {
                return breaker.clone();
            }
        }

        // 按应用独立读取熔断器配置；必须在拿写锁之前完成，
        // 否则快照未命中时的数据库读取会阻塞所有熔断器查询
        let config = match self
            .runtime_config
            .app_proxy_config(key.app().as_str())
            .await
        {
            Ok(app_config) => CircuitBreakerConfig::from(&app_config),
            Err(_) => CircuitBreakerConfig::default(),
        };

        // 如果不存在，获取写锁创建（持锁期间不再有 await）
        let mut breakers = self.circuit_breakers.write().await;

        // 双重检查，防止竞争条件（并发创建时以先插入者为准）
        if let Some(breaker) = breakers.get(key) {
            return breaker.clone();
        }

        log::debug!(
            "[{}] 创建熔断器: provider_id={}",
            key.app(),
            key.provider_id()
        );
        let breaker = Arc::new(CircuitBreaker::new(config));
        breakers.insert(key.clone(), breaker.clone());

        breaker
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use serde_json::json;
    use serial_test::serial;
    use std::env;
    use tempfile::TempDir;

    struct TempHome {
        /// Held only so the temp directory lives as long as the guard.
        _dir: TempDir,
        original_home: Option<String>,
        original_userprofile: Option<String>,
        original_test_home: Option<String>,
    }

    impl TempHome {
        fn new() -> Self {
            let dir = TempDir::new().expect("failed to create temp home");
            let original_home = env::var("HOME").ok();
            let original_userprofile = env::var("USERPROFILE").ok();
            let original_test_home = env::var("CC_SWITCH_TEST_HOME").ok();

            env::set_var("HOME", dir.path());
            env::set_var("USERPROFILE", dir.path());
            env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            crate::settings::reload_settings().expect("reload settings");

            Self {
                _dir: dir,
                original_home,
                original_userprofile,
                original_test_home,
            }
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            match &self.original_home {
                Some(value) => env::set_var("HOME", value),
                None => env::remove_var("HOME"),
            }

            match &self.original_userprofile {
                Some(value) => env::set_var("USERPROFILE", value),
                None => env::remove_var("USERPROFILE"),
            }

            match &self.original_test_home {
                Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
                None => env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_provider_router_creation() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());
        let router = ProviderRouter::new(db);

        let breaker = router
            .get_or_create_circuit_breaker(&BreakerKey::new("claude", "test"))
            .await;
        assert!(breaker.allow_request().await.allowed);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[serial]
    async fn test_concurrent_breaker_creation_yields_single_entry() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());
        let router = Arc::new(ProviderRouter::new(db));
        let key = BreakerKey::new("claude", "concurrent");

        let barrier = Arc::new(tokio::sync::Barrier::new(16));
        let handles: Vec<_> = (0..16)
            .map(|_| {
                let router = router.clone();
                let key = key.clone();
                let barrier = barrier.clone();
                tokio::spawn(async move {
                    barrier.wait().await;
                    router.get_or_create_circuit_breaker(&key).await
                })
            })
            .collect();

        let mut breakers = Vec::new();
        for handle in handles {
            breakers.push(handle.await.unwrap());
        }

        let first = &breakers[0];
        assert!(breakers.iter().all(|b| Arc::ptr_eq(first, b)));
        let map = router.circuit_breakers.read().await;
        assert_eq!(map.len(), 1);
        assert!(Arc::ptr_eq(first, map.get(&key).unwrap()));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[serial]
    async fn test_breaker_creation_does_not_hold_map_lock_during_db_read() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());
        let router = Arc::new(ProviderRouter::new(db.clone()));
        let key = BreakerKey::new("claude", "slow-db");

        let handle = {
            // 持有数据库连接锁，让创建熔断器时的配置读取阻塞在数据库上
            let conn = db.conn.lock().unwrap();
            let handle = {
                let router = router.clone();
                let key = key.clone();
                tokio::spawn(async move { router.get_or_create_circuit_breaker(&key).await })
            };
            std::thread::sleep(std::time::Duration::from_millis(200));

            // 配置读取阻塞期间，熔断器表仍可被其他请求读取
            assert!(
                router.circuit_breakers.try_read().is_ok(),
                "breaker map lock is held while reading the config from the database"
            );
            drop(conn);
            handle
        };

        let breaker = handle.await.unwrap();
        let map = router.circuit_breakers.read().await;
        assert!(Arc::ptr_eq(&breaker, map.get(&key).unwrap()));
    }

    #[tokio::test]
    #[serial]
    async fn test_failover_disabled_uses_current_provider() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        let provider_a =
            Provider::with_id("a".to_string(), "Provider A".to_string(), json!({}), None);
        let provider_b =
            Provider::with_id("b".to_string(), "Provider B".to_string(), json!({}), None);

        db.save_provider("claude", &provider_a).unwrap();
        db.save_provider("claude", &provider_b).unwrap();
        db.set_current_provider("claude", "a").unwrap();
        db.add_to_failover_queue("claude", "b").unwrap();

        let router = ProviderRouter::new(db.clone());
        let providers = router.select_providers("claude").await.unwrap();

        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id, "a");
    }

    #[tokio::test]
    #[serial]
    async fn test_failover_enabled_uses_queue_order_ignoring_current() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        // 设置 sort_index 来控制顺序：b=1, a=2
        let mut provider_a =
            Provider::with_id("a".to_string(), "Provider A".to_string(), json!({}), None);
        provider_a.sort_index = Some(2);
        let mut provider_b =
            Provider::with_id("b".to_string(), "Provider B".to_string(), json!({}), None);
        provider_b.sort_index = Some(1);

        db.save_provider("claude", &provider_a).unwrap();
        db.save_provider("claude", &provider_b).unwrap();
        db.set_current_provider("claude", "a").unwrap();

        db.add_to_failover_queue("claude", "b").unwrap();
        db.add_to_failover_queue("claude", "a").unwrap();

        // 启用自动故障转移（使用新的 proxy_config API）
        let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
        config.auto_failover_enabled = true;
        db.update_proxy_config_for_app(config).await.unwrap();

        let router = ProviderRouter::new(db.clone());
        let providers = router.select_providers("claude").await.unwrap();

        assert_eq!(providers.len(), 2);
        // 故障转移开启时：仅按队列顺序选择（忽略当前供应商）
        assert_eq!(providers[0].id, "b");
        assert_eq!(providers[1].id, "a");
    }

    #[tokio::test]
    #[serial]
    async fn test_cooldown_skips_provider_until_it_expires() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        for id in ["a", "b"] {
            let provider = Provider::with_id(id.to_string(), id.to_string(), json!({}), None);
            db.save_provider("claude", &provider).unwrap();
            db.add_to_failover_queue("claude", id).unwrap();
        }
        let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
        config.auto_failover_enabled = true;
        db.update_proxy_config_for_app(config).await.unwrap();

        let router = ProviderRouter::new(db.clone());
        let ids = |providers: Vec<Provider>| -> Vec<String> {
            providers.into_iter().map(|p| p.id).collect()
        };

        // 零时长不进入冷却；更短的冷却不会覆盖更长的
        router.start_cooldown("a", "claude", Duration::ZERO);
        assert!(router.cooldown_remaining("a", "claude").is_none());
        router.start_cooldown("a", "claude", Duration::from_millis(200));
        router.start_cooldown("a", "claude", Duration::from_millis(1));
        assert!(router.cooldown_remaining("a", "claude").unwrap() > Duration::from_millis(100));
        assert!(
            router.cooldown_remaining("a", "codex").is_none(),
            "cooldowns are per app"
        );

        assert_eq!(ids(router.select_providers("claude").await.unwrap()), ["b"]);

        // 其余 provider 也都在冷却中：按队列顺序兜底，而不是报「未配置供应商」
        router.start_cooldown("b", "claude", Duration::from_millis(200));
        assert_eq!(
            ids(router.select_providers("claude").await.unwrap()),
            ["a", "b"]
        );

        tokio::time::sleep(Duration::from_millis(250)).await;
        assert!(router.cooldown_remaining("a", "claude").is_none());
        assert_eq!(
            ids(router.select_providers("claude").await.unwrap()),
            ["a", "b"]
        );
        let breaker = router
            .get_circuit_breaker_stats("a", "claude")
            .await
            .unwrap();
        assert_eq!(breaker.state, CircuitState::Closed);
    }

    #[test]
    fn test_sticky_sessions_evicts_least_recently_used() {
        let key = |s: &str| (AppRef::from("claude"), s.to_string());
        let mut sticky = StickySessions::new(2);
        sticky.insert(key("s1"), "a".to_string());
        sticky.insert(key("s2"), "b".to_string());
        // 读取 s1 让它变成最近使用，下一次写入淘汰 s2
        assert_eq!(sticky.get(&key("s1")).as_deref(), Some("a"));
        sticky.insert(key("s3"), "c".to_string());
        assert_eq!(sticky.entries.len(), 2);
        assert!(sticky.get(&key("s2")).is_none());
        assert_eq!(sticky.get(&key("s1")).as_deref(), Some("a"));
        assert_eq!(sticky.get(&key("s3")).as_deref(), Some("c"));
        // 覆盖写入不增加条目
        sticky.insert(key("s1"), "b".to_string());
        assert_eq!(sticky.entries.len(), 2);
        assert_eq!(sticky.get(&key("s1")).as_deref(), Some("b"));
    }

    #[tokio::test]
    #[serial]
    async fn test_sticky_session_table_is_bounded() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());
        let router = ProviderRouter::new(db);

        for i in 0..=STICKY_SESSION_CAPACITY {
            router.remember_session_provider("claude", &format!("s{i}"), "a");
        }
        let len = router.sticky_sessions.lock().unwrap().entries.len();
        assert_eq!(len, STICKY_SESSION_CAPACITY);
        assert!(router.sticky_session_provider("claude", "s0").is_none());
        assert_eq!(
            router
                .sticky_session_provider("claude", &format!("s{STICKY_SESSION_CAPACITY}"))
                .as_deref(),
            Some("a")
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_sticky_session_prefers_its_provider_while_available() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        db.update_circuit_breaker_config(&CircuitBreakerConfig {
            failure_threshold: 1,
            timeout_seconds: 600,
            ..Default::default()
        })
        .await
        .unwrap();
        for id in ["a", "b", "c"] {
            let provider = Provider::with_id(id.to_string(), id.to_string(), json!({}), None);
            db.save_provider("claude", &provider).unwrap();
            db.add_to_failover_queue("claude", id).unwrap();
        }
        let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
        config.auto_failover_enabled = true;
        db.update_proxy_config_for_app(config).await.unwrap();

        let router = ProviderRouter::new(db.clone());
        let ids = |providers: Vec<Provider>| -> Vec<String> {
            providers.into_iter().map(|p| p.id).collect()
        };
        let select = |session: Option<&'static str>| {
            let router = &router;
            async move {
                ids(router
                    .select_providers_for_session("claude", session)
                    .await
                    .unwrap())
            }
        };

        router.remember_session_provider("claude", "s1", "b");
        assert_eq!(select(Some("s1")).await, ["b", "a", "c"]);
        // 其他会话、未开启粘性（None）与其他应用均不受影响
        assert_eq!(select(Some("s2")).await, ["a", "b", "c"]);
        assert_eq!(select(None).await, ["a", "b", "c"]);
        assert!(router.sticky_session_provider("codex", "s1").is_none());

        // 粘性 provider 处于限流冷却中：回到队列顺序
        router.start_cooldown("b", "claude", Duration::from_secs(60));
        assert_eq!(select(Some("s1")).await, ["a", "c"]);

        // 粘性 provider 熔断：回到队列顺序
        let router = ProviderRouter::new(db.clone());
        router.remember_session_provider("claude", "s1", "b");
        router
            .record_result("b", "claude", None, false, Some("fail".to_string()))
            .await
            .unwrap();
        assert_eq!(
            ids(router
                .select_providers_for_session("claude", Some("s1"))
                .await
                .unwrap()),
            ["a", "c"]
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_failover_enabled_uses_queue_only_even_if_current_not_in_queue() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        let provider_a =
            Provider::with_id("a".to_string(), "Provider A".to_string(), json!({}), None);
        let mut provider_b =
            Provider::with_id("b".to_string(), "Provider B".to_string(), json!({}), None);
        provider_b.sort_index = Some(1);

        db.save_provider("claude", &provider_a).unwrap();
        db.save_provider("claude", &provider_b).unwrap();
        db.set_current_provider("claude", "a").unwrap();

        // 只把 b 加入故障转移队列（模拟“当前供应商不在队列里”的常见配置）
        db.add_to_failover_queue("claude", "b").unwrap();

        let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
        config.auto_failover_enabled = true;
        db.update_proxy_config_for_app(config).await.unwrap();

        let router = ProviderRouter::new(db.clone());
        let providers = router.select_providers("claude").await.unwrap();

        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id, "b");
    }

    #[tokio::test]
    #[serial]
    async fn test_select_providers_does_not_consume_half_open_permit() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        db.update_circuit_breaker_config(&CircuitBreakerConfig {
            failure_threshold: 1,
            timeout_seconds: 0,
            ..Default::default()
        })
        .await
        .unwrap();

        let provider_a =
            Provider::with_id("a".to_string(), "Provider A".to_string(), json!({}), None);
        let provider_b =
            Provider::with_id("b".to_string(), "Provider B".to_string(), json!({}), None);

        db.save_provider("claude", &provider_a).unwrap();
        db.save_provider("claude", &provider_b).unwrap();

        db.add_to_failover_queue("claude", "a").unwrap();
        db.add_to_failover_queue("claude", "b").unwrap();

        // 启用自动故障转移（使用新的 proxy_config API）
        let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
        config.auto_failover_enabled = true;
        db.update_proxy_config_for_app(config).await.unwrap();

        let router = ProviderRouter::new(db.clone());

        router
            .record_result("b", "claude", None, false, Some("fail".to_string()))
            .await
            .unwrap();

        let providers = router.select_providers("claude").await.unwrap();
        assert_eq!(providers.len(), 2);

        assert!(router.allow_provider_request("b", "claude").await.allowed);
    }

    #[tokio::test]
    #[serial]
    async fn test_release_permit_neutral_frees_half_open_slot() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        // 配置熔断器：1 次失败即熔断，0 秒超时立即进入 HalfOpen
        db.update_circuit_breaker_config(&CircuitBreakerConfig {
            failure_threshold: 1,
            timeout_seconds: 0,
            ..Default::default()
        })
        .await
        .unwrap();

        let provider_a =
            Provider::with_id("a".to_string(), "Provider A".to_string(), json!({}), None);
        db.save_provider("claude", &provider_a).unwrap();
        db.add_to_failover_queue("claude", "a").unwrap();

        // 启用自动故障转移
        let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
        config.auto_failover_enabled = true;
        db.update_proxy_config_for_app(config).await.unwrap();

        let router = ProviderRouter::new(db.clone());

        // 触发熔断：1 次失败
        router
            .record_result("a", "claude", None, false, Some("fail".to_string()))
            .await
            .unwrap();

        // 第一次请求：获取 HalfOpen 探测名额
        let first = router.allow_provider_request("a", "claude").await;
        assert!(first.allowed);
        assert!(first.used_half_open_permit());

        // 第二次请求应被拒绝（名额已被占用）
        let second = router.allow_provider_request("a", "claude").await;
        assert!(!second.allowed);

        // 使用 release_permit_neutral 释放名额（不影响健康统计）
        router
            .release_permit_neutral("a", "claude", first.permit.as_ref())
            .await;

        // 第三次请求应被允许（名额已释放）
        let third = router.allow_provider_request("a", "claude").await;
        assert!(third.allowed);
        assert!(third.used_half_open_permit());
    }

    #[tokio::test]
    #[serial]
    async fn test_record_result_config_read_failure_uses_default_threshold() {
        use crate::proxy::circuit_breaker::{CircuitState, DEFAULT_CIRCUIT_FAILURE_THRESHOLD};

        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        let provider_a =
            Provider::with_id("a".to_string(), "Provider A".to_string(), json!({}), None);
        db.save_provider("claude", &provider_a).unwrap();

        // 让 proxy_config 读取失败（模拟配置读取错误）
        {
            let conn = db.conn.lock().unwrap();
            conn.execute("DROP TABLE proxy_config", []).unwrap();
        }
        assert!(db.get_proxy_config_for_app("claude").await.is_err());

        let router = ProviderRouter::new(db.clone());

        for _ in 0..DEFAULT_CIRCUIT_FAILURE_THRESHOLD - 1 {
            router
                .record_result("a", "claude", None, false, Some("fail".to_string()))
                .await
                .unwrap();
        }
        let health = db.get_provider_health("a", "claude").await.unwrap();
        assert!(health.is_healthy);

        router
            .record_result("a", "claude", None, false, Some("fail".to_string()))
            .await
            .unwrap();

        let health = db.get_provider_health("a", "claude").await.unwrap();
        assert_eq!(
            health.consecutive_failures,
            DEFAULT_CIRCUIT_FAILURE_THRESHOLD
        );
        assert!(!health.is_healthy);

        // 数据库健康状态与熔断器在同一次失败时翻转
        let stats = router
            .get_circuit_breaker_stats("a", "claude")
            .await
            .unwrap();
        assert_eq!(stats.state, CircuitState::Open);
    }

    #[tokio::test]
    #[serial]
    async fn test_update_app_configs_only_touches_that_app() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());
        let router = ProviderRouter::new(db);

        let claude = router
            .get_or_create_circuit_breaker(&BreakerKey::new("claude", "shared:id"))
            .await;
        let codex = router
            .get_or_create_circuit_breaker(&BreakerKey::new("codex", "shared:id"))
            .await;
        let claude_threshold = claude.failure_threshold().await;

        router
            .update_app_configs(
                "codex",
                CircuitBreakerConfig {
                    failure_threshold: claude_threshold + 7,
                    ..CircuitBreakerConfig::default()
                },
            )
            .await;

        assert_eq!(codex.failure_threshold().await, claude_threshold + 7);
        assert_eq!(claude.failure_threshold().await, claude_threshold);
    }

    #[tokio::test]
    #[serial]
    async fn test_failure_threshold_defaults_are_consistent() {
        use crate::proxy::circuit_breaker::DEFAULT_CIRCUIT_FAILURE_THRESHOLD;

        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        assert_eq!(
            CircuitBreakerConfig::default().failure_threshold,
            DEFAULT_CIRCUIT_FAILURE_THRESHOLD
        );
        // schema.rs 的 SQL DEFAULT 需与常量一致（claude 行的 seed 值是有意不同的，
        // 这里插入仅含 app_type 的行来验证列默认值）
        {
            let conn = db.conn.lock().unwrap();
            conn.execute("DELETE FROM proxy_config WHERE app_type = 'codex'", [])
                .unwrap();
            conn.execute("INSERT INTO proxy_config (app_type) VALUES ('codex')", [])
                .unwrap();
        }
        let config = db.get_proxy_config_for_app("codex").await.unwrap();
        assert_eq!(
            config.circuit_failure_threshold,
            DEFAULT_CIRCUIT_FAILURE_THRESHOLD
        );
    }
}
