//! 熔断器模块
//!
//! 实现熔断器模式，用于防止向不健康的供应商发送请求

use super::log_codes::cb as log_cb;
use super::types::AppProxyConfig;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// HalfOpen 探测名额的过期时间
///
/// 正常情况下名额由 `HalfOpenPermit` 在记录结果或 Drop 时释放；这里只是兜底：
/// 若某个名额被占用超过该时长（远超上游超时上限），视为已失效，允许新的探测。
const HALF_OPEN_PERMIT_STALE_AFTER: Duration = Duration::from_secs(30 * 60);

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

/// 熔断器失败阈值的统一默认值
///
/// 熔断器默认配置、DAO 回退配置以及 `record_result` 读取配置失败时的回退都使用此值，
/// 需与 schema.rs 中 `circuit_failure_threshold` 列的 SQL DEFAULT 保持一致。
pub const DEFAULT_CIRCUIT_FAILURE_THRESHOLD: u32 = 4;

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
            failure_threshold: DEFAULT_CIRCUIT_FAILURE_THRESHOLD,
            success_threshold: 2,
            timeout_seconds: 60,
            error_rate_threshold: 0.6,
            min_requests: 10,
        }
    }
}

/// 熔断器实例
pub struct CircuitBreaker {
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
    /// 半开状态当前占用的探测名额（用于限流，同一时间只放行一个探测）
    half_open_slot: Arc<Mutex<Option<HalfOpenSlot>>>,
    /// 探测名额 ID 生成器，用于区分新旧名额
    next_permit_id: AtomicU64,
}

/// 已被占用的 HalfOpen 探测名额
#[derive(Debug, Clone, Copy)]
struct HalfOpenSlot {
    id: u64,
    acquired_at: Instant,
}

fn lock_slot(slot: &Mutex<Option<HalfOpenSlot>>) -> MutexGuard<'_, Option<HalfOpenSlot>> {
    slot.lock().unwrap_or_else(|e| e.into_inner())
}

/// HalfOpen 探测名额（RAII 守卫）
///
/// 由 `allow_request` 在占用探测名额时返回。名额在以下任一时机释放：
/// - 调用方把它传给 `record_success` / `record_failure`（或调用 `release`）；
/// - 守卫被 Drop，例如客户端断开导致请求 future 在记录结果前被取消。
///
/// 释放是幂等的，且只会清除自己占用的名额：若熔断器已重新进入 HalfOpen
/// 或名额已被新的探测占用，旧守卫的释放不会影响新名额。
pub struct HalfOpenPermit {
    slot: Arc<Mutex<Option<HalfOpenSlot>>>,
    id: u64,
    released: AtomicBool,
}

impl HalfOpenPermit {
    /// 释放探测名额（幂等）
    pub fn release(&self) {
        if self.released.swap(true, Ordering::SeqCst) {
            return;
        }
        let mut slot = lock_slot(&self.slot);
        if slot.is_some_and(|current| current.id == self.id) {
            *slot = None;
        }
    }
}

impl Drop for HalfOpenPermit {
    fn drop(&mut self) {
        self.release();
    }
}

impl std::fmt::Debug for HalfOpenPermit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HalfOpenPermit")
            .field("id", &self.id)
            .field("released", &self.released.load(Ordering::SeqCst))
            .finish()
    }
}

/// 熔断器放行结果
///
/// `permit` 为 `Some` 表示本次放行占用了 HalfOpen 探测名额。
/// 调用方应持有该守卫直到请求结束，并把它传给 `record_success` / `record_failure`；
/// 若请求在记录结果前被取消，守卫 Drop 时会自动释放名额。
#[derive(Debug)]
pub struct AllowResult {
    pub allowed: bool,
    pub permit: Option<HalfOpenPermit>,
}

impl AllowResult {
    fn denied() -> Self {
        Self {
            allowed: false,
            permit: None,
        }
    }

    fn allowed() -> Self {
        Self {
            allowed: true,
            permit: None,
        }
    }

    /// 本次放行是否占用了 HalfOpen 探测名额
    #[allow(dead_code)]
    pub fn used_half_open_permit(&self) -> bool {
        self.permit.is_some()
    }
}

impl CircuitBreaker {
    /// 创建新的熔断器
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            state: Arc::new(RwLock::new(CircuitState::Closed)),
            consecutive_failures: Arc::new(AtomicU32::new(0)),
            consecutive_successes: Arc::new(AtomicU32::new(0)),
            total_requests: Arc::new(AtomicU32::new(0)),
            failed_requests: Arc::new(AtomicU32::new(0)),
            last_opened_at: Arc::new(RwLock::new(None)),
            config: Arc::new(RwLock::new(config)),
            half_open_slot: Arc::new(Mutex::new(None)),
            next_permit_id: AtomicU64::new(0),
        }
    }

    /// 更新熔断器配置（热更新，不重置状态）
    pub async fn update_config(&self, new_config: CircuitBreakerConfig) {
        *self.config.write().await = new_config;
    }

    /// 获取当前配置的失败阈值
    pub async fn failure_threshold(&self) -> u32 {
        self.config.read().await.failure_threshold
    }

    /// 判断当前 Provider 是否“可被纳入候选链路”
    ///
    /// 这个方法不会占用 HalfOpen 探测名额，仅用于路由选择阶段的“可用性判断”：
    /// - Closed / HalfOpen：可用（返回 true）
    /// - Open：若超时到达则切到 HalfOpen 并返回 true，否则返回 false
    ///
    /// 注意：真正发起请求前仍需调用 `allow_request()` 来获取 HalfOpen 探测名额，
    /// 并在请求结束后通过 `record_success()` / `record_failure()` 释放
    /// （或由 `HalfOpenPermit` Drop 时释放）。
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
            CircuitState::Closed => AllowResult::allowed(),
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
                            CircuitState::Closed => AllowResult::allowed(),
                            CircuitState::HalfOpen => self.allow_half_open_probe(),
                            CircuitState::Open => AllowResult::denied(),
                        };
                    }
                }

                AllowResult::denied()
            }
            CircuitState::HalfOpen => self.allow_half_open_probe(),
        }
    }

    /// 记录成功
    pub async fn record_success(&self, permit: Option<&HalfOpenPermit>) {
        let state = *self.state.read().await;
        let config = self.config.read().await;

        if let Some(permit) = permit {
            permit.release();
        }

        // 重置失败计数
        self.consecutive_failures.store(0, Ordering::SeqCst);
        self.total_requests.fetch_add(1, Ordering::SeqCst);

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
    pub async fn record_failure(&self, permit: Option<&HalfOpenPermit>) {
        let state = *self.state.read().await;
        let config = self.config.read().await;

        if let Some(permit) = permit {
            permit.release();
        }

        // 更新计数器
        let failures = self.consecutive_failures.fetch_add(1, Ordering::SeqCst) + 1;
        self.total_requests.fetch_add(1, Ordering::SeqCst);
        self.failed_requests.fetch_add(1, Ordering::SeqCst);

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
                    // 检查错误率
                    let total = self.total_requests.load(Ordering::SeqCst);
                    let failed = self.failed_requests.load(Ordering::SeqCst);

                    if total >= config.min_requests {
                        let error_rate = failed as f64 / total as f64;

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
        // 半开状态限流：同一时间只允许一个请求通过进行探测
        let mut slot = lock_slot(&self.half_open_slot);

        if let Some(current) = *slot {
            if current.acquired_at.elapsed() < HALF_OPEN_PERMIT_STALE_AFTER {
                return AllowResult::denied();
            }
            log::warn!(
                "[{}] HalfOpen 探测名额占用超过 {}s 未释放，视为失效",
                log_cb::HALF_OPEN_PERMIT_STALE,
                HALF_OPEN_PERMIT_STALE_AFTER.as_secs()
            );
        }

        let id = self.next_permit_id.fetch_add(1, Ordering::SeqCst);
        *slot = Some(HalfOpenSlot {
            id,
            acquired_at: Instant::now(),
        });

        AllowResult {
            allowed: true,
            permit: Some(HalfOpenPermit {
                slot: self.half_open_slot.clone(),
                id,
                released: AtomicBool::new(false),
            }),
        }
    }

    /// 清空 HalfOpen 探测名额（旧守卫随后的释放不会影响新名额）
    fn clear_half_open_slot(&self) {
        *lock_slot(&self.half_open_slot) = None;
    }

    /// 转换到打开状态
    async fn transition_to_open(&self) {
        *self.state.write().await = CircuitState::Open;
        *self.last_opened_at.write().await = Some(Instant::now());
        self.consecutive_failures.store(0, Ordering::SeqCst);
        self.consecutive_successes.store(0, Ordering::SeqCst);
    }

    /// 转换到半开状态
    async fn transition_to_half_open(&self) {
        let mut state = self.state.write().await;
        if *state != CircuitState::Open {
            return;
        }

        *state = CircuitState::HalfOpen;
        self.consecutive_successes.store(0, Ordering::SeqCst);
        // 重置半开状态的探测名额
        self.clear_half_open_slot();
    }

    /// 转换到关闭状态
    async fn transition_to_closed(&self) {
        *self.state.write().await = CircuitState::Closed;
        self.consecutive_failures.store(0, Ordering::SeqCst);
        self.consecutive_successes.store(0, Ordering::SeqCst);
        // 重置计数器
        self.total_requests.store(0, Ordering::SeqCst);
        self.failed_requests.store(0, Ordering::SeqCst);
        // Closed 状态不需要探测名额，清空避免遗留占用
        self.clear_half_open_slot();
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
            breaker.record_failure(None).await;
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
        breaker.record_failure(None).await;
        breaker.record_failure(None).await;
        assert_eq!(breaker.get_state().await, CircuitState::Open);

        // 手动转换到半开状态
        breaker.transition_to_half_open().await;
        assert_eq!(breaker.get_state().await, CircuitState::HalfOpen);

        // 记录 2 次成功
        breaker.record_success(None).await;
        breaker.record_success(None).await;

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
        assert!(first.used_half_open_permit());
        assert_eq!(breaker.get_state().await, CircuitState::HalfOpen);

        // 模拟并发下的“重复 HalfOpen 转换调用”，不应重置 in-flight 计数
        breaker.transition_to_half_open().await;

        // 由于名额仍被占用，第二次请求应被拒绝
        let second = breaker.allow_request().await;
        assert!(!second.allowed);
        assert!(!second.used_half_open_permit());
    }

    async fn half_open_breaker() -> CircuitBreaker {
        let breaker = CircuitBreaker::new(CircuitBreakerConfig {
            timeout_seconds: 0,
            ..Default::default()
        });
        breaker.transition_to_open().await;
        breaker
    }

    #[tokio::test]
    async fn test_dropped_half_open_permit_frees_the_probe_slot() {
        let breaker = half_open_breaker().await;

        let first = breaker.allow_request().await;
        assert!(first.allowed);
        assert!(first.used_half_open_permit());
        assert!(!breaker.allow_request().await.allowed);

        // 模拟请求 future 在 record_* 之前被取消：守卫直接被 drop
        drop(first);

        let next = breaker.allow_request().await;
        assert!(next.allowed, "dropping the guard must free the probe slot");
        assert!(next.used_half_open_permit());
        assert_eq!(breaker.get_state().await, CircuitState::HalfOpen);
    }

    #[tokio::test]
    async fn test_recorded_permit_drop_does_not_release_newer_probe() {
        let breaker = CircuitBreaker::new(CircuitBreakerConfig {
            timeout_seconds: 0,
            success_threshold: 2,
            ..Default::default()
        });
        breaker.transition_to_open().await;

        let first = breaker.allow_request().await;
        breaker.record_success(first.permit.as_ref()).await;
        assert_eq!(breaker.get_state().await, CircuitState::HalfOpen);

        // 名额已由 record_success 释放，新的探测可以占用
        let second = breaker.allow_request().await;
        assert!(second.allowed);

        // 旧守卫 Drop 不得释放新探测的名额
        drop(first);
        assert!(!breaker.allow_request().await.allowed);

        drop(second);
        assert!(breaker.allow_request().await.allowed);
    }

    #[tokio::test]
    async fn test_permit_from_previous_half_open_round_does_not_free_new_slot() {
        let breaker = half_open_breaker().await;

        let stale = breaker.allow_request().await;
        assert!(stale.allowed);

        // 探测失败重新熔断，再次超时进入新一轮 HalfOpen
        breaker.record_failure(None).await;
        assert_eq!(breaker.get_state().await, CircuitState::Open);
        let fresh = breaker.allow_request().await;
        assert!(fresh.allowed);
        assert!(fresh.used_half_open_permit());

        stale.permit.as_ref().unwrap().release();
        assert!(
            !breaker.allow_request().await.allowed,
            "an old round's permit must not free the current probe"
        );
    }

    #[tokio::test]
    async fn test_stale_half_open_permit_is_treated_as_free() {
        let breaker = half_open_breaker().await;

        let first = breaker.allow_request().await;
        assert!(first.allowed);
        assert!(!breaker.allow_request().await.allowed);

        let Some(long_ago) =
            Instant::now().checked_sub(HALF_OPEN_PERMIT_STALE_AFTER + Duration::from_secs(1))
        else {
            // 系统运行时间过短，无法构造过期时间点
            return;
        };
        lock_slot(&breaker.half_open_slot)
            .as_mut()
            .expect("probe slot is held")
            .acquired_at = long_ago;

        let second = breaker.allow_request().await;
        assert!(second.allowed, "a stale permit must not block new probes");
        assert!(second.used_half_open_permit());

        // 过期守卫随后释放，不影响新探测的名额
        drop(first);
        assert!(!breaker.allow_request().await.allowed);
    }

    #[tokio::test]
    async fn test_transition_to_closed_clears_half_open_slot() {
        let breaker = half_open_breaker().await;

        let permit = breaker.allow_request().await;
        assert!(permit.used_half_open_permit());
        assert!(lock_slot(&breaker.half_open_slot).is_some());

        breaker.reset().await;
        assert_eq!(breaker.get_state().await, CircuitState::Closed);
        assert!(lock_slot(&breaker.half_open_slot).is_none());

        // 旧守卫在 Closed 后释放是无副作用的
        drop(permit);
        assert!(breaker.allow_request().await.allowed);
    }

    #[tokio::test]
    async fn test_circuit_breaker_reset() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            ..Default::default()
        };
        let breaker = CircuitBreaker::new(config);

        // 打开熔断器
        breaker.record_failure(None).await;
        breaker.record_failure(None).await;
        assert_eq!(breaker.get_state().await, CircuitState::Open);

        // 重置
        breaker.reset().await;
        assert_eq!(breaker.get_state().await, CircuitState::Closed);
        assert!(breaker.allow_request().await.allowed);
    }
}
