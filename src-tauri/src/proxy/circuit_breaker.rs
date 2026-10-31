//! 熔断器模块
//!
//! 实现熔断器模式，用于防止向不健康的供应商发送请求

use super::log_codes::cb as log_cb;
use super::types::AppProxyConfig;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// 熔断器状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CircuitState {
    /// 关闭状态 - 正常工作
    Closed,
    /// 打开状态 - 熔断激活，拒绝请求
    Open,
    /// 半开状态 - 尝试恢复，允许部分请求通过
    HalfOpen,
}

impl std::fmt::Display for CircuitState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CircuitState::Closed => write!(f, "closed"),
            CircuitState::Open => write!(f, "open"),
            CircuitState::HalfOpen => write!(f, "half_open"),
        }
    }
}

/// Default consecutive-failure threshold. Single source of truth for the
/// breaker default and for the router's fallback when the per-app config
/// cannot be read (matches the DB schema default).
pub const DEFAULT_FAILURE_THRESHOLD: u32 = 4;

/// Outcomes older than this no longer count towards the error rate, so a long
/// healthy history cannot mask a burst of failures and a stale blip cannot
/// open the breaker much later.
pub const ERROR_RATE_WINDOW: Duration = Duration::from_secs(60);

/// 熔断器配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CircuitBreakerConfig {
    /// 失败阈值 - 连续失败多少次后打开熔断器
    pub failure_threshold: u32,
    /// 成功阈值 - 半开状态下成功多少次后关闭熔断器
    pub success_threshold: u32,
    /// 超时时间 - 熔断器打开后多久尝试半开（秒）
    pub timeout_seconds: u64,
    /// 错误率阈值 - 错误率超过此值时打开熔断器 (0.0-1.0)
    pub error_rate_threshold: f64,
    /// 最小请求数 - 计算错误率前的最小请求数
    ///
    /// Also the size of the sliding window: the error rate is computed over the
    /// most recent `min_requests` outcomes within [`ERROR_RATE_WINDOW`].
    pub min_requests: u32,
}

impl From<&AppProxyConfig> for CircuitBreakerConfig {
    fn from(config: &AppProxyConfig) -> Self {
        Self {
            failure_threshold: config.circuit_failure_threshold,
            success_threshold: config.circuit_success_threshold,
            timeout_seconds: config.circuit_timeout_seconds as u64,
            error_rate_threshold: config.circuit_error_rate_threshold,
            min_requests: config.circuit_min_requests,
        }
    }
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: DEFAULT_FAILURE_THRESHOLD,
            success_threshold: 2,
            timeout_seconds: 60,
            error_rate_threshold: 0.6,
            min_requests: 10,
        }
    }
}

/// A breaker state transition, reported to every listener registered with
/// [`add_state_listener`]. Only breakers created with a key (see
/// [`CircuitBreaker::new_registered`]) report transitions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CircuitStateChange {
    /// Breaker key, `"app_type:provider_id"`.
    pub key: String,
    pub from: CircuitState,
    pub to: CircuitState,
}

impl CircuitStateChange {
    /// Split the key into `(app_type, provider_id)`.
    pub fn app_and_provider(&self) -> Option<(&str, &str)> {
        split_breaker_key(&self.key)
    }
}

/// Split a breaker key (`"app_type:provider_id"`) into its parts.
pub fn split_breaker_key(key: &str) -> Option<(&str, &str)> {
    key.split_once(':')
        .filter(|(app, id)| !app.is_empty() && !id.is_empty())
}

/// Callback invoked synchronously on every keyed state transition. Listeners
/// must not block; spawn a task for anything slow.
pub type CircuitStateListener = Arc<dyn Fn(&CircuitStateChange) + Send + Sync>;

static STATE_LISTENERS: once_cell::sync::Lazy<std::sync::RwLock<Vec<(u64, CircuitStateListener)>>> =
    once_cell::sync::Lazy::new(|| std::sync::RwLock::new(Vec::new()));
static NEXT_LISTENER_ID: AtomicU64 = AtomicU64::new(1);

/// Registry of live keyed breakers, used by the background health prober.
/// Holds weak references so stopping the proxy (dropping its router) drops
/// the breakers too.
static BREAKER_REGISTRY: once_cell::sync::Lazy<
    std::sync::Mutex<HashMap<String, Weak<CircuitBreaker>>>,
> = once_cell::sync::Lazy::new(|| std::sync::Mutex::new(HashMap::new()));

/// Register a state-change listener; returns an id for [`remove_state_listener`].
pub fn add_state_listener(listener: CircuitStateListener) -> u64 {
    let id = NEXT_LISTENER_ID.fetch_add(1, Ordering::SeqCst);
    if let Ok(mut listeners) = STATE_LISTENERS.write() {
        listeners.push((id, listener));
    }
    id
}

/// Remove a listener registered with [`add_state_listener`].
#[allow(dead_code)]
pub fn remove_state_listener(id: u64) {
    if let Ok(mut listeners) = STATE_LISTENERS.write() {
        listeners.retain(|(existing, _)| *existing != id);
    }
}

fn notify_state_listeners(change: &CircuitStateChange) {
    // Clone the list first so a listener can (un)register without deadlocking.
    let listeners: Vec<CircuitStateListener> = match STATE_LISTENERS.read() {
        Ok(listeners) => listeners.iter().map(|(_, l)| l.clone()).collect(),
        Err(_) => return,
    };
    for listener in listeners {
        listener(change);
    }
}

/// Every live breaker created with [`CircuitBreaker::new_registered`], with its
/// key. Dead entries are pruned as a side effect.
pub fn registered_breakers() -> Vec<(String, Arc<CircuitBreaker>)> {
    let Ok(mut registry) = BREAKER_REGISTRY.lock() else {
        return Vec::new();
    };
    registry.retain(|_, weak| weak.strong_count() > 0);
    registry
        .iter()
        .filter_map(|(key, weak)| weak.upgrade().map(|b| (key.clone(), b)))
        .collect()
}

/// 熔断器实例
pub struct CircuitBreaker {
    /// `"app_type:provider_id"` for breakers owned by the router; `None` for
    /// anonymous breakers, which do not report state changes.
    key: Option<String>,
    /// 当前状态
    state: Arc<RwLock<CircuitState>>,
    /// 连续失败计数
    consecutive_failures: Arc<AtomicU32>,
    /// 连续成功计数（半开状态）
    consecutive_successes: Arc<AtomicU32>,
    /// 总请求计数
    total_requests: Arc<AtomicU32>,
    /// 失败请求计数
    failed_requests: Arc<AtomicU32>,
    /// 上次打开时间
    last_opened_at: Arc<RwLock<Option<Instant>>>,
    /// 配置（支持热更新）
    config: Arc<RwLock<CircuitBreakerConfig>>,
    /// 半开状态已放行的请求数（用于限流）
    half_open_requests: Arc<AtomicU32>,
    /// 每次进入 HalfOpen 递增；用于让过期的 `HalfOpenPermit` 在 Drop 时
    /// 不去释放新一轮 HalfOpen 的探测名额
    half_open_generation: Arc<AtomicU64>,
    /// Sliding window of recent outcomes (`true` = failure) used for the error
    /// rate. Bounded by `min_requests` entries and [`ERROR_RATE_WINDOW`] age.
    recent_outcomes: std::sync::Mutex<VecDeque<(Instant, bool)>>,
}

/// 熔断器放行结果
///
/// `used_half_open_permit` 表示本次放行是否占用了 HalfOpen 探测名额。
/// 调用方应在请求结束后把该值传回 `record_success` / `record_failure` 用于正确释放名额。
#[derive(Debug, Clone, Copy)]
pub struct AllowResult {
    pub allowed: bool,
    pub used_half_open_permit: bool,
}

/// RAII guard for a HalfOpen probe permit.
///
/// If the request future is dropped before the result is recorded (client
/// disconnect, Ctrl+C, cancellation), `Drop` releases the permit so the breaker
/// cannot get stuck in HalfOpen with its only probe slot taken.
///
/// Callers that record the result explicitly call [`HalfOpenPermit::take`]
/// to obtain the `used_half_open_permit` flag for `record_success` /
/// `record_failure` / `release_half_open_permit`; that hands release
/// responsibility over and disarms the guard, so the permit is released once.
#[derive(Debug)]
#[must_use = "dropping the guard releases the HalfOpen permit immediately"]
pub struct HalfOpenPermit {
    slot: Option<(Arc<AtomicU32>, Arc<AtomicU64>, u64)>,
}

impl HalfOpenPermit {
    /// A guard that holds no permit (Closed state, or breaker bypassed).
    pub fn none() -> Self {
        Self { slot: None }
    }

    /// Whether the guard still holds an unreleased permit.
    #[allow(dead_code)]
    pub fn is_held(&self) -> bool {
        self.slot.is_some()
    }

    /// Hand the permit over to an explicit record/release call.
    ///
    /// Returns `true` the first time when a permit is held, `false` afterwards.
    pub fn take(&mut self) -> bool {
        self.slot.take().is_some()
    }
}

impl Drop for HalfOpenPermit {
    fn drop(&mut self) {
        if let Some((counter, generation, held_generation)) = self.slot.take() {
            // A newer HalfOpen round already reset the counter; releasing now would
            // free someone else's probe slot.
            if generation.load(Ordering::SeqCst) != held_generation {
                return;
            }
            log::debug!(
                "[{}] HalfOpen probe dropped before completion, releasing permit",
                log_cb::HALF_OPEN_PROBE_FAILED
            );
            release_counter(&counter);
        }
    }
}

/// Saturating decrement used for every HalfOpen permit release.
fn release_counter(counter: &AtomicU32) {
    let mut current = counter.load(Ordering::SeqCst);
    loop {
        if current == 0 {
            return;
        }
        match counter.compare_exchange(current, current - 1, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return,
            Err(actual) => current = actual,
        }
    }
}

impl CircuitBreaker {
    /// 创建新的熔断器（匿名：不注册、不上报状态变化；生产路径用 `new_registered`）
    #[allow(dead_code)]
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self::with_key(None, config)
    }

    /// Create a keyed breaker, register it for the background health prober
    /// and report its state transitions to the registered listeners.
    pub fn new_registered(key: &str, config: CircuitBreakerConfig) -> Arc<Self> {
        let breaker = Arc::new(Self::with_key(Some(key.to_string()), config));
        if let Ok(mut registry) = BREAKER_REGISTRY.lock() {
            registry.insert(key.to_string(), Arc::downgrade(&breaker));
        }
        breaker
    }

    fn with_key(key: Option<String>, config: CircuitBreakerConfig) -> Self {
        Self {
            key,
            state: Arc::new(RwLock::new(CircuitState::Closed)),
            consecutive_failures: Arc::new(AtomicU32::new(0)),
            consecutive_successes: Arc::new(AtomicU32::new(0)),
            total_requests: Arc::new(AtomicU32::new(0)),
            failed_requests: Arc::new(AtomicU32::new(0)),
            last_opened_at: Arc::new(RwLock::new(None)),
            config: Arc::new(RwLock::new(config)),
            half_open_requests: Arc::new(AtomicU32::new(0)),
            half_open_generation: Arc::new(AtomicU64::new(0)),
            recent_outcomes: std::sync::Mutex::new(VecDeque::new()),
        }
    }

    /// 更新熔断器配置（热更新，不重置熔断状态）
    ///
    /// The error-rate window is cleared so a new `min_requests` /
    /// `error_rate_threshold` is never compared against samples collected
    /// under the old settings.
    pub async fn update_config(&self, new_config: CircuitBreakerConfig) {
        *self.config.write().await = new_config;
        self.clear_outcomes();
    }

    /// Current configuration snapshot.
    pub async fn config(&self) -> CircuitBreakerConfig {
        self.config.read().await.clone()
    }

    /// The breaker key (`"app_type:provider_id"`), if it has one.
    #[allow(dead_code)]
    pub fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }

    fn report_transition(&self, from: CircuitState, to: CircuitState) {
        if from == to {
            return;
        }
        if let Some(key) = &self.key {
            notify_state_listeners(&CircuitStateChange {
                key: key.clone(),
                from,
                to,
            });
        }
    }

    fn clear_outcomes(&self) {
        if let Ok(mut window) = self.recent_outcomes.lock() {
            window.clear();
        }
    }

    /// Push an outcome into the sliding window and return `(samples, failures)`
    /// currently inside it.
    fn push_outcome_at(&self, at: Instant, failed: bool, capacity: u32) -> (u32, u32) {
        let Ok(mut window) = self.recent_outcomes.lock() else {
            return (0, 0);
        };
        window.push_back((at, failed));
        let capacity = capacity.max(1) as usize;
        while window.len() > capacity {
            window.pop_front();
        }
        while let Some((ts, _)) = window.front() {
            if at.saturating_duration_since(*ts) > ERROR_RATE_WINDOW {
                window.pop_front();
            } else {
                break;
            }
        }
        let failures = window.iter().filter(|(_, f)| *f).count() as u32;
        (window.len() as u32, failures)
    }

    /// 判断当前 Provider 是否“可被纳入候选链路”
    ///
    /// 这个方法不会占用 HalfOpen 探测名额，仅用于路由选择阶段的“可用性判断”：
    /// - Closed / HalfOpen：可用（返回 true）
    /// - Open：若超时到达则切到 HalfOpen 并返回 true，否则返回 false
    ///
    /// 注意：真正发起请求前仍需调用 `allow_request()` 来获取 HalfOpen 探测名额，
    /// 并在请求结束后通过 `record_success()` / `record_failure()` 释放。
    pub async fn is_available(&self) -> bool {
        let state = *self.state.read().await;
        let config = self.config.read().await;

        match state {
            CircuitState::Closed | CircuitState::HalfOpen => true,
            CircuitState::Open => {
                if let Some(opened_at) = *self.last_opened_at.read().await {
                    if opened_at.elapsed().as_secs() >= config.timeout_seconds {
                        drop(config); // 释放读锁再转换状态
                        log::info!(
                            "[{}] 熔断器 Open → HalfOpen (超时恢复)",
                            log_cb::OPEN_TO_HALF_OPEN
                        );
                        self.transition_to_half_open().await;
                        return true;
                    }
                }
                false
            }
        }
    }

    /// 检查是否允许请求通过
    pub async fn allow_request(&self) -> AllowResult {
        let state = *self.state.read().await;

        match state {
            CircuitState::Closed => AllowResult {
                allowed: true,
                used_half_open_permit: false,
            },
            CircuitState::Open => {
                let config = self.config.read().await;
                // 检查是否应该尝试半开
                if let Some(opened_at) = *self.last_opened_at.read().await {
                    if opened_at.elapsed().as_secs() >= config.timeout_seconds {
                        drop(config); // 释放读锁再转换状态
                        log::info!(
                            "[{}] 熔断器 Open → HalfOpen (超时恢复)",
                            log_cb::OPEN_TO_HALF_OPEN
                        );
                        self.transition_to_half_open().await;

                        // 转换后按当前状态决定是否需要获取 HalfOpen 探测名额
                        let current_state = *self.state.read().await;
                        return match current_state {
                            CircuitState::Closed => AllowResult {
                                allowed: true,
                                used_half_open_permit: false,
                            },
                            CircuitState::HalfOpen => self.allow_half_open_probe(),
                            CircuitState::Open => AllowResult {
                                allowed: false,
                                used_half_open_permit: false,
                            },
                        };
                    }
                }

                AllowResult {
                    allowed: false,
                    used_half_open_permit: false,
                }
            }
            CircuitState::HalfOpen => self.allow_half_open_probe(),
        }
    }

    /// Like [`allow_request`](Self::allow_request), but also returns an RAII
    /// guard that releases the HalfOpen permit if it is dropped unrecorded.
    pub async fn allow_request_guarded(&self) -> (AllowResult, HalfOpenPermit) {
        let result = self.allow_request().await;
        let permit = if result.used_half_open_permit {
            HalfOpenPermit {
                slot: Some((
                    self.half_open_requests.clone(),
                    self.half_open_generation.clone(),
                    self.half_open_generation.load(Ordering::SeqCst),
                )),
            }
        } else {
            HalfOpenPermit::none()
        };
        (result, permit)
    }

    /// 记录成功
    pub async fn record_success(&self, used_half_open_permit: bool) {
        let state = *self.state.read().await;
        let config = self.config.read().await;

        if used_half_open_permit {
            self.release_half_open_permit();
        }

        // 重置失败计数
        self.consecutive_failures.store(0, Ordering::SeqCst);
        self.total_requests.fetch_add(1, Ordering::SeqCst);
        self.push_outcome_at(Instant::now(), false, config.min_requests);

        if state == CircuitState::HalfOpen {
            let successes = self.consecutive_successes.fetch_add(1, Ordering::SeqCst) + 1;

            if successes >= config.success_threshold {
                drop(config); // 释放读锁再转换状态
                log::info!(
                    "[{}] 熔断器 HalfOpen → Closed (恢复正常)",
                    log_cb::HALF_OPEN_TO_CLOSED
                );
                self.transition_to_closed().await;
            }
        }
    }

    /// 记录失败
    pub async fn record_failure(&self, used_half_open_permit: bool) {
        let state = *self.state.read().await;
        let config = self.config.read().await;

        if used_half_open_permit {
            self.release_half_open_permit();
        }

        // 更新计数器
        let failures = self.consecutive_failures.fetch_add(1, Ordering::SeqCst) + 1;
        self.total_requests.fetch_add(1, Ordering::SeqCst);
        self.failed_requests.fetch_add(1, Ordering::SeqCst);
        let (window_total, window_failed) =
            self.push_outcome_at(Instant::now(), true, config.min_requests);

        // 重置成功计数
        self.consecutive_successes.store(0, Ordering::SeqCst);

        // 检查是否应该打开熔断器
        match state {
            CircuitState::HalfOpen => {
                // HalfOpen 状态下失败，立即转为 Open
                log::warn!(
                    "[{}] 熔断器 HalfOpen 探测失败 → Open",
                    log_cb::HALF_OPEN_PROBE_FAILED
                );
                drop(config);
                self.transition_to_open().await;
            }
            CircuitState::Closed => {
                // 检查连续失败次数
                if failures >= config.failure_threshold {
                    log::warn!(
                        "[{}] 熔断器触发: 连续失败 {failures} 次 → Open",
                        log_cb::TRIGGERED_FAILURES
                    );
                    drop(config); // 释放读锁再转换状态
                    self.transition_to_open().await;
                } else {
                    // 检查错误率（滑动窗口：最近 min_requests 次且不超过 ERROR_RATE_WINDOW）
                    if window_total >= config.min_requests.max(1) {
                        let error_rate = window_failed as f64 / window_total as f64;

                        if error_rate >= config.error_rate_threshold {
                            log::warn!(
                                "[{}] 熔断器触发: 错误率 {:.1}% → Open",
                                log_cb::TRIGGERED_ERROR_RATE,
                                error_rate * 100.0
                            );
                            drop(config); // 释放读锁再转换状态
                            self.transition_to_open().await;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// 获取当前状态
    #[allow(dead_code)]
    pub async fn get_state(&self) -> CircuitState {
        *self.state.read().await
    }

    /// 获取统计信息
    #[allow(dead_code)]
    pub async fn get_stats(&self) -> CircuitBreakerStats {
        CircuitBreakerStats {
            state: *self.state.read().await,
            consecutive_failures: self.consecutive_failures.load(Ordering::SeqCst),
            consecutive_successes: self.consecutive_successes.load(Ordering::SeqCst),
            total_requests: self.total_requests.load(Ordering::SeqCst),
            failed_requests: self.failed_requests.load(Ordering::SeqCst),
        }
    }

    /// 重置熔断器（手动恢复）
    #[allow(dead_code)]
    pub async fn reset(&self) {
        log::info!("[{}] 熔断器手动重置 → Closed", log_cb::MANUAL_RESET);
        self.transition_to_closed().await;
    }

    fn allow_half_open_probe(&self) -> AllowResult {
        // 半开状态限流：只允许有限请求通过进行探测
        let max_half_open_requests = 1u32;
        let current = self.half_open_requests.fetch_add(1, Ordering::SeqCst);

        if current < max_half_open_requests {
            AllowResult {
                allowed: true,
                used_half_open_permit: true,
            }
        } else {
            // 超过限额，回退计数，拒绝请求
            self.half_open_requests.fetch_sub(1, Ordering::SeqCst);
            AllowResult {
                allowed: false,
                used_half_open_permit: false,
            }
        }
    }

    /// 仅释放 HalfOpen permit，不影响健康统计
    ///
    /// 用于整流器等场景：请求结果不应计入 Provider 健康度，
    /// 但仍需释放占用的探测名额，避免 HalfOpen 状态卡死
    pub fn release_half_open_permit(&self) {
        release_counter(&self.half_open_requests);
    }

    /// 转换到打开状态
    async fn transition_to_open(&self) {
        let previous = std::mem::replace(&mut *self.state.write().await, CircuitState::Open);
        self.clear_outcomes();
        *self.last_opened_at.write().await = Some(Instant::now());
        self.consecutive_failures.store(0, Ordering::SeqCst);
        self.consecutive_successes.store(0, Ordering::SeqCst);
        self.report_transition(previous, CircuitState::Open);
    }

    /// 转换到半开状态
    async fn transition_to_half_open(&self) {
        let mut state = self.state.write().await;
        if *state != CircuitState::Open {
            return;
        }

        *state = CircuitState::HalfOpen;
        self.half_open_generation.fetch_add(1, Ordering::SeqCst);
        self.consecutive_successes.store(0, Ordering::SeqCst);
        // 重置半开状态的请求限流计数
        self.half_open_requests.store(0, Ordering::SeqCst);
        drop(state);
        self.report_transition(CircuitState::Open, CircuitState::HalfOpen);
    }

    /// 转换到关闭状态
    async fn transition_to_closed(&self) {
        let previous = std::mem::replace(&mut *self.state.write().await, CircuitState::Closed);
        self.consecutive_failures.store(0, Ordering::SeqCst);
        self.consecutive_successes.store(0, Ordering::SeqCst);
        // 重置计数器
        self.total_requests.store(0, Ordering::SeqCst);
        self.failed_requests.store(0, Ordering::SeqCst);
        self.clear_outcomes();
        self.report_transition(previous, CircuitState::Closed);
    }
}

/// 熔断器统计信息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CircuitBreakerStats {
    pub state: CircuitState,
    pub consecutive_failures: u32,
    pub consecutive_successes: u32,
    pub total_requests: u32,
    pub failed_requests: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PROXY-36: concurrent failures from many tasks keep counters consistent.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_record_failure_keeps_counters_consistent() {
        let breaker = Arc::new(CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1_000,
            error_rate_threshold: 1.1,
            ..Default::default()
        }));
        let mut tasks = Vec::new();
        for _ in 0..100 {
            let b = breaker.clone();
            tasks.push(tokio::spawn(async move { b.record_failure(false).await }));
        }
        for t in tasks {
            t.await.unwrap();
        }
        let stats = breaker.get_stats().await;
        assert_eq!(stats.consecutive_failures, 100);
        assert_eq!(stats.failed_requests, 100);
        assert_eq!(stats.total_requests, 100);
        assert_eq!(stats.state, CircuitState::Closed);
    }

    /// PROXY-36: hot-updating `timeout_seconds` applies to an already open breaker.
    #[tokio::test]
    async fn hot_update_of_timeout_applies_to_open_breaker() {
        let breaker = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            timeout_seconds: 3_600,
            ..Default::default()
        });
        breaker.record_failure(false).await;
        assert_eq!(breaker.get_state().await, CircuitState::Open);
        assert!(!breaker.is_available().await);

        breaker
            .update_config(CircuitBreakerConfig {
                failure_threshold: 1,
                timeout_seconds: 0,
                ..Default::default()
            })
            .await;
        assert!(breaker.is_available().await);
        assert_eq!(breaker.get_state().await, CircuitState::HalfOpen);
    }

    #[tokio::test]
    async fn test_circuit_breaker_closed_to_open() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            ..Default::default()
        };
        let breaker = CircuitBreaker::new(config);

        // 初始状态应该是关闭
        assert_eq!(breaker.get_state().await, CircuitState::Closed);
        assert!(breaker.allow_request().await.allowed);

        // 记录 3 次失败
        for _ in 0..3 {
            breaker.record_failure(false).await;
        }

        // 应该转换到打开状态
        assert_eq!(breaker.get_state().await, CircuitState::Open);
        assert!(!breaker.allow_request().await.allowed);
    }

    #[tokio::test]
    async fn test_circuit_breaker_half_open_to_closed() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            success_threshold: 2,
            ..Default::default()
        };
        let breaker = CircuitBreaker::new(config);

        // 打开熔断器
        breaker.record_failure(false).await;
        breaker.record_failure(false).await;
        assert_eq!(breaker.get_state().await, CircuitState::Open);

        // 手动转换到半开状态
        breaker.transition_to_half_open().await;
        assert_eq!(breaker.get_state().await, CircuitState::HalfOpen);

        // 记录 2 次成功
        breaker.record_success(false).await;
        breaker.record_success(false).await;

        // 应该转换到关闭状态
        assert_eq!(breaker.get_state().await, CircuitState::Closed);
    }

    #[tokio::test]
    async fn test_half_open_transition_does_not_reset_inflight_permit() {
        let config = CircuitBreakerConfig {
            timeout_seconds: 0,
            ..Default::default()
        };
        let breaker = CircuitBreaker::new(config);

        // 进入 Open，然后由于 timeout_seconds=0，allow_request 会立即切换到 HalfOpen 并占用探测名额
        breaker.transition_to_open().await;
        let first = breaker.allow_request().await;
        assert!(first.allowed);
        assert!(first.used_half_open_permit);
        assert_eq!(breaker.get_state().await, CircuitState::HalfOpen);

        // 模拟并发下的“重复 HalfOpen 转换调用”，不应重置 in-flight 计数
        breaker.transition_to_half_open().await;

        // 由于名额仍被占用，第二次请求应被拒绝
        let second = breaker.allow_request().await;
        assert!(!second.allowed);
        assert!(!second.used_half_open_permit);
    }

    #[tokio::test]
    async fn dropped_half_open_probe_releases_permit() {
        let breaker = Arc::new(CircuitBreaker::new(CircuitBreakerConfig {
            timeout_seconds: 0,
            ..Default::default()
        }));
        breaker.transition_to_open().await;

        // Simulate a request handler that takes the probe permit and then awaits a
        // pending upstream; the client disconnects and the future is dropped.
        let b = breaker.clone();
        let (acquired_tx, acquired_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let (allow, mut permit) = b.allow_request_guarded().await;
            assert!(allow.allowed && permit.is_held());
            let _ = acquired_tx.send(());
            futures::future::pending::<()>().await;
            b.record_success(permit.take()).await;
        });
        acquired_rx.await.unwrap();
        assert_eq!(breaker.get_state().await, CircuitState::HalfOpen);
        assert!(!breaker.allow_request().await.allowed, "slot is taken");

        task.abort();
        let _ = task.await;

        let next = breaker.allow_request().await;
        assert!(
            next.allowed,
            "permit must be released when the future is dropped"
        );
        assert!(next.used_half_open_permit);
    }

    #[tokio::test]
    async fn taken_permit_is_released_once() {
        let breaker = CircuitBreaker::new(CircuitBreakerConfig {
            timeout_seconds: 0,
            success_threshold: 5,
            ..Default::default()
        });
        breaker.transition_to_open().await;

        let (_, mut permit) = breaker.allow_request_guarded().await;
        assert!(permit.take());
        assert!(!permit.take(), "second take must not release again");
        breaker.record_success(true).await;

        // Another probe takes the slot; dropping the already-taken guard must not free it.
        let (allow, _second) = breaker.allow_request_guarded().await;
        assert!(allow.allowed);
        drop(permit);
        assert!(!breaker.allow_request().await.allowed);
    }

    #[tokio::test]
    async fn stale_permit_does_not_release_newer_half_open_round() {
        let breaker = CircuitBreaker::new(CircuitBreakerConfig {
            timeout_seconds: 0,
            ..Default::default()
        });
        breaker.transition_to_open().await;
        let (_, stale) = breaker.allow_request_guarded().await;
        assert!(stale.is_held());

        // A new Open → HalfOpen round starts while the stale probe is still around.
        breaker.transition_to_open().await;
        let (allow, _fresh) = breaker.allow_request_guarded().await;
        assert!(allow.allowed);

        drop(stale);
        assert!(
            !breaker.allow_request().await.allowed,
            "stale guard must not free the fresh probe's slot"
        );
    }

    /// Config where only the error-rate rule can open the breaker.
    fn rate_only_config() -> CircuitBreakerConfig {
        CircuitBreakerConfig {
            failure_threshold: u32::MAX,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn error_rate_uses_recent_window_not_lifetime_totals() {
        let breaker = CircuitBreaker::new(rate_only_config());
        for _ in 0..1000 {
            breaker.record_success(false).await;
        }
        // Last 10 requests: 6 failures interleaved with 4 successes.
        for i in 0..10 {
            if i % 5 < 3 {
                breaker.record_failure(false).await;
            } else {
                breaker.record_success(false).await;
            }
        }
        assert_eq!(
            breaker.get_state().await,
            CircuitState::Open,
            "6/10 recent failures must open despite 1000 earlier successes"
        );
    }

    #[tokio::test]
    async fn old_outcomes_expire_from_the_window() {
        let breaker = CircuitBreaker::new(rate_only_config());
        let long_ago = Instant::now()
            .checked_sub(ERROR_RATE_WINDOW + Duration::from_secs(1))
            .expect("monotonic clock far enough from boot");
        for _ in 0..100 {
            breaker.push_outcome_at(long_ago, false, 10);
        }
        breaker.record_failure(false).await;
        breaker.record_failure(false).await;
        assert_eq!(breaker.get_state().await, CircuitState::Closed);

        let (samples, failures) = breaker.push_outcome_at(Instant::now(), false, 10);
        assert_eq!((samples, failures), (3, 2), "expired samples are dropped");
    }

    #[tokio::test]
    async fn early_blip_below_min_requests_does_not_open() {
        let breaker = CircuitBreaker::new(rate_only_config());
        for _ in 0..9 {
            breaker.record_failure(false).await;
        }
        assert_eq!(breaker.get_state().await, CircuitState::Closed);
    }

    #[tokio::test]
    async fn update_config_resets_error_rate_window() {
        let breaker = CircuitBreaker::new(rate_only_config());
        for _ in 0..5 {
            breaker.record_failure(false).await;
        }
        breaker
            .update_config(CircuitBreakerConfig {
                min_requests: 6,
                ..rate_only_config()
            })
            .await;
        // Without the reset this failure would be the 6th sample at 100% → Open.
        breaker.record_failure(false).await;
        assert_eq!(breaker.get_state().await, CircuitState::Closed);
    }

    #[test]
    fn default_failure_threshold_is_single_source() {
        assert_eq!(
            CircuitBreakerConfig::default().failure_threshold,
            DEFAULT_FAILURE_THRESHOLD
        );
        assert_eq!(
            DEFAULT_FAILURE_THRESHOLD, 4,
            "must match the DB schema default"
        );
    }

    #[tokio::test]
    async fn test_circuit_breaker_reset() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            ..Default::default()
        };
        let breaker = CircuitBreaker::new(config);

        // 打开熔断器
        breaker.record_failure(false).await;
        breaker.record_failure(false).await;
        assert_eq!(breaker.get_state().await, CircuitState::Open);

        // 重置
        breaker.reset().await;
        assert_eq!(breaker.get_state().await, CircuitState::Closed);
        assert!(breaker.allow_request().await.allowed);
    }

    #[tokio::test]
    async fn keyed_breaker_reports_transitions_and_registers() {
        let key = format!("claude:listener-test-{}", uuid::Uuid::new_v4());
        let seen: Arc<std::sync::Mutex<Vec<CircuitStateChange>>> = Arc::default();
        let sink = seen.clone();
        let watched = key.clone();
        let id = add_state_listener(Arc::new(move |change: &CircuitStateChange| {
            if change.key == watched {
                sink.lock().unwrap().push(change.clone());
            }
        }));

        let breaker = CircuitBreaker::new_registered(
            &key,
            CircuitBreakerConfig {
                failure_threshold: 1,
                success_threshold: 1,
                timeout_seconds: 0,
                ..Default::default()
            },
        );
        assert!(registered_breakers().iter().any(|(k, _)| k == &key));

        breaker.record_failure(false).await; // Closed -> Open
        breaker.record_failure(false).await; // Open stays Open: no event
        let allow = breaker.allow_request().await; // Open -> HalfOpen
        assert!(allow.used_half_open_permit);
        breaker.record_success(true).await; // HalfOpen -> Closed
        breaker.reset().await; // Closed -> Closed: no event

        let states: Vec<(CircuitState, CircuitState)> = seen
            .lock()
            .unwrap()
            .iter()
            .map(|c| (c.from, c.to))
            .collect();
        assert_eq!(
            states,
            vec![
                (CircuitState::Closed, CircuitState::Open),
                (CircuitState::Open, CircuitState::HalfOpen),
                (CircuitState::HalfOpen, CircuitState::Closed),
            ]
        );
        remove_state_listener(id);

        drop(breaker);
        assert!(!registered_breakers().iter().any(|(k, _)| k == &key));
    }

    #[test]
    fn split_breaker_key_parses_app_and_provider() {
        assert_eq!(split_breaker_key("claude:abc"), Some(("claude", "abc")));
        assert_eq!(split_breaker_key("codex:a:b"), Some(("codex", "a:b")));
        assert_eq!(split_breaker_key("claude:"), None);
        assert_eq!(split_breaker_key("nokey"), None);
    }

    // ---- TEST-19: deterministic state-machine transitions ------------------
    // The Open → HalfOpen timeout is driven by backdating `last_opened_at`
    // instead of sleeping, so these run in microseconds and never flake.

    async fn backdate_open(breaker: &CircuitBreaker, secs: u64) {
        let at = Instant::now()
            .checked_sub(Duration::from_secs(secs))
            .expect("monotonic clock far enough from boot");
        *breaker.last_opened_at.write().await = Some(at);
    }

    fn threshold_config(failures: u32, timeout_seconds: u64) -> CircuitBreakerConfig {
        CircuitBreakerConfig {
            failure_threshold: failures,
            success_threshold: 1,
            timeout_seconds,
            error_rate_threshold: 1.1, // rate never trips; consecutive count does
            min_requests: 1000,
        }
    }

    #[tokio::test]
    async fn open_breaker_rejects_until_timeout_elapses() {
        let breaker = CircuitBreaker::new(threshold_config(1, 60));
        breaker.record_failure(false).await;
        assert_eq!(breaker.get_state().await, CircuitState::Open);

        backdate_open(&breaker, 59).await;
        assert!(!breaker.allow_request().await.allowed);
        assert!(!breaker.is_available().await);
        assert_eq!(breaker.get_state().await, CircuitState::Open);
    }

    #[tokio::test]
    async fn open_breaker_half_opens_after_timeout_and_hands_out_one_probe() {
        let breaker = CircuitBreaker::new(threshold_config(1, 60));
        breaker.record_failure(false).await;
        backdate_open(&breaker, 61).await;

        let first = breaker.allow_request().await;
        assert!(first.allowed && first.used_half_open_permit, "{first:?}");
        assert_eq!(breaker.get_state().await, CircuitState::HalfOpen);
        let second = breaker.allow_request().await;
        assert!(!second.allowed, "only one probe in flight: {second:?}");
    }

    #[tokio::test]
    async fn is_available_also_moves_open_to_half_open_after_timeout() {
        let breaker = CircuitBreaker::new(threshold_config(1, 30));
        breaker.record_failure(false).await;
        backdate_open(&breaker, 31).await;
        assert!(breaker.is_available().await);
        assert_eq!(breaker.get_state().await, CircuitState::HalfOpen);
    }

    #[tokio::test]
    async fn zero_timeout_half_opens_immediately() {
        let breaker = CircuitBreaker::new(threshold_config(1, 0));
        breaker.record_failure(false).await;
        assert_eq!(breaker.get_state().await, CircuitState::Open);
        assert!(breaker.allow_request().await.allowed);
        assert_eq!(breaker.get_state().await, CircuitState::HalfOpen);
    }

    #[tokio::test]
    async fn failed_probe_reopens_with_a_fresh_timeout() {
        let breaker = CircuitBreaker::new(threshold_config(1, 60));
        breaker.record_failure(false).await;
        backdate_open(&breaker, 120).await;
        let probe = breaker.allow_request().await;
        assert!(probe.used_half_open_permit);

        breaker.record_failure(probe.used_half_open_permit).await;
        assert_eq!(breaker.get_state().await, CircuitState::Open);
        let reopened_at = breaker.last_opened_at.read().await.expect("reopened");
        assert!(reopened_at.elapsed() < Duration::from_secs(5));
        assert!(!breaker.allow_request().await.allowed, "timeout restarted");
    }

    #[tokio::test]
    async fn successful_probe_closes_and_clears_failure_streak() {
        let breaker = CircuitBreaker::new(threshold_config(2, 60));
        breaker.record_failure(false).await;
        breaker.record_failure(false).await;
        backdate_open(&breaker, 61).await;
        let probe = breaker.allow_request().await;
        breaker.record_success(probe.used_half_open_permit).await;
        assert_eq!(breaker.get_state().await, CircuitState::Closed);

        // One failure after closing does not re-open a threshold-2 breaker.
        breaker.record_failure(false).await;
        assert_eq!(breaker.get_state().await, CircuitState::Closed);
    }

    #[tokio::test]
    async fn success_resets_the_consecutive_failure_count() {
        let breaker = CircuitBreaker::new(threshold_config(3, 60));
        for outcome in [false, false, true, false, false] {
            if outcome {
                breaker.record_success(false).await;
            } else {
                breaker.record_failure(false).await;
            }
        }
        assert_eq!(breaker.get_state().await, CircuitState::Closed);
        breaker.record_failure(false).await;
        assert_eq!(breaker.get_state().await, CircuitState::Open);
    }

    #[tokio::test]
    async fn error_rate_trips_once_min_requests_reached() {
        // Table: (outcomes, expected state). true = success.
        let cases: &[(&[bool], CircuitState)] = &[
            (&[true, false, true], CircuitState::Closed), // below min_requests
            (&[true, false, true, false], CircuitState::Open), // 50% >= 50%
            (&[true, true, true, false], CircuitState::Closed), // 25%
        ];
        for (outcomes, expected) in cases {
            let breaker = CircuitBreaker::new(CircuitBreakerConfig {
                failure_threshold: 100,
                success_threshold: 1,
                timeout_seconds: 60,
                error_rate_threshold: 0.5,
                min_requests: 4,
            });
            for ok in outcomes.iter() {
                if *ok {
                    breaker.record_success(false).await;
                } else {
                    breaker.record_failure(false).await;
                }
            }
            assert_eq!(breaker.get_state().await, *expected, "{outcomes:?}");
        }
    }

    #[tokio::test]
    async fn reset_returns_to_closed_and_accepts_requests() {
        let breaker = CircuitBreaker::new(threshold_config(1, 3600));
        breaker.record_failure(false).await;
        assert!(!breaker.allow_request().await.allowed);
        breaker.reset().await;
        assert_eq!(breaker.get_state().await, CircuitState::Closed);
        assert!(breaker.allow_request().await.allowed);
        assert_eq!(breaker.get_stats().await.consecutive_failures, 0);
    }
}
