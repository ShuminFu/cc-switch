//! 熔断器模块
//!
//! 实现熔断器模式，用于防止向不健康的供应商发送请求

use super::log_codes::cb as log_cb;
use super::types::AppProxyConfig;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// HalfOpen 探测名额的过期时间
///
/// 正常情况下名额由 `HalfOpenPermit` 在记录结果或 Drop 时释放；这里只是兜底：
/// 若某个名额被占用超过该时长（远超上游超时上限），视为已失效，允许新的探测。
const HALF_OPEN_PERMIT_STALE_AFTER: Duration = Duration::from_secs(30 * 60);

/// 错误率滑动窗口大小：只统计最近这么多次请求的结果
///
/// 错误率若按 Closed 以来的全部请求计算，长时间的成功历史会掩盖间歇性故障；
/// 改为最近 N 次结果后，旧的成功/失败会随新请求移出窗口。
/// `min_requests` 也作用于窗口内的请求数（超过窗口大小时按窗口大小截断）。
const ERROR_RATE_WINDOW_SIZE: usize = 100;

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
    /// 最小请求数 - 计算错误率前滑动窗口内的最小请求数
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
    /// 最近请求结果的滑动窗口（用于计算错误率）
    outcomes: Arc<Mutex<OutcomeWindow>>,
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

/// 最近 `ERROR_RATE_WINDOW_SIZE` 次请求结果的环形缓冲区
#[derive(Debug)]
struct OutcomeWindow {
    /// `true` 表示失败
    outcomes: VecDeque<bool>,
    failures: usize,
}

impl OutcomeWindow {
    fn new() -> Self {
        Self {
            outcomes: VecDeque::with_capacity(ERROR_RATE_WINDOW_SIZE),
            failures: 0,
        }
    }

    /// 记录一次结果，窗口已满时淘汰最旧的结果；返回 (窗口内请求数, 窗口内失败数)
    fn record(&mut self, failed: bool) -> (usize, usize) {
        if self.outcomes.len() == ERROR_RATE_WINDOW_SIZE && self.outcomes.pop_front() == Some(true)
        {
            self.failures -= 1;
        }
        self.outcomes.push_back(failed);
        if failed {
            self.failures += 1;
        }
        (self.outcomes.len(), self.failures)
    }

    fn clear(&mut self) {
        self.outcomes.clear();
        self.failures = 0;
    }

    fn len(&self) -> usize {
        self.outcomes.len()
    }
}

fn lock_outcomes(outcomes: &Mutex<OutcomeWindow>) -> MutexGuard<'_, OutcomeWindow> {
    outcomes.lock().unwrap_or_else(|e| e.into_inner())
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
    #[cfg(test)]
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
            outcomes: Arc::new(Mutex::new(OutcomeWindow::new())),
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
        lock_outcomes(&self.outcomes).record(false);

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
        let (total, failed) = lock_outcomes(&self.outcomes).record(true);

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
                    // 检查滑动窗口内的错误率
                    let min_requests = (config.min_requests as usize).min(ERROR_RATE_WINDOW_SIZE);

                    if total >= min_requests {
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
    pub async fn get_state(&self) -> CircuitState {
        *self.state.read().await
    }

    /// 获取统计信息
    ///
    /// `total_requests` / `failed_requests` 为错误率滑动窗口内的计数。
    pub async fn get_stats(&self) -> CircuitBreakerStats {
        let (total_requests, failed_requests) = {
            let outcomes = lock_outcomes(&self.outcomes);
            (outcomes.len() as u32, outcomes.failures as u32)
        };
        let state = *self.state.read().await;
        let open_remaining_secs = if state == CircuitState::Open {
            let timeout_seconds = self.config.read().await.timeout_seconds;
            let elapsed = self
                .last_opened_at
                .read()
                .await
                .map_or(0, |opened_at| opened_at.elapsed().as_secs());
            Some(timeout_seconds.saturating_sub(elapsed))
        } else {
            None
        };
        CircuitBreakerStats {
            state,
            consecutive_failures: self.consecutive_failures.load(Ordering::SeqCst),
            consecutive_successes: self.consecutive_successes.load(Ordering::SeqCst),
            total_requests,
            failed_requests,
            open_remaining_secs,
        }
    }

    /// 手动探测（"立即探测"）：获取 HalfOpen 探测名额
    ///
    /// 与 `allow_request` 的区别：Open 状态下不等待超时，直接切到 HalfOpen 并占用探测名额。
    /// - Closed：无需探测，返回不放行
    /// - Open / HalfOpen：按 HalfOpen 限流规则占用名额（已有探测在途时不放行）
    ///
    /// 放行时调用方必须把返回的名额传给 `record_manual_probe_result`。
    pub async fn begin_manual_probe(&self) -> AllowResult {
        let state = self.get_state().await;
        if state == CircuitState::Closed {
            return AllowResult::denied();
        }
        if state == CircuitState::Open {
            log::info!(
                "[{}] 熔断器 Open → HalfOpen (手动探测)",
                log_cb::OPEN_TO_HALF_OPEN
            );
            self.transition_to_half_open().await;
        }

        match self.get_state().await {
            CircuitState::HalfOpen => self.allow_half_open_probe(),
            CircuitState::Closed | CircuitState::Open => AllowResult::denied(),
        }
    }

    /// 记录手动探测结果
    ///
    /// 成功：用户主动验证供应商已恢复，直接 HalfOpen → Closed（不再等待 `success_threshold`）；
    /// 失败：与 HalfOpen 探测失败一致，重新 Open 并重新计时。
    pub async fn record_manual_probe_result(&self, permit: Option<&HalfOpenPermit>, success: bool) {
        if !success {
            self.record_failure(permit).await;
            return;
        }

        self.record_success(permit).await;
        if self.get_state().await == CircuitState::HalfOpen {
            log::info!(
                "[{}] 熔断器 HalfOpen → Closed (手动探测成功)",
                log_cb::HALF_OPEN_TO_CLOSED
            );
            self.transition_to_closed().await;
        }
    }

    /// 重置熔断器（手动恢复）
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
        // 重置错误率窗口
        lock_outcomes(&self.outcomes).clear();
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
    /// Open 状态下距离允许 HalfOpen 探测的剩余秒数（0 表示下一个请求即会探测）；其余状态为 `None`
    pub open_remaining_secs: Option<u64>,
}

/// 手动探测（"立即探测"）结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CircuitProbeResult {
    /// 是否真正发出了探测请求（熔断器为 Closed、或已有探测在途时为 `false`）
    pub probed: bool,
    /// 探测请求是否成功
    pub success: bool,
    /// 探测失败原因或未探测原因
    pub message: Option<String>,
    /// 探测后的熔断器状态
    pub stats: CircuitBreakerStats,
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
    async fn test_intermittent_failures_after_long_clean_history_open_breaker() {
        let breaker = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1000,
            error_rate_threshold: 0.3,
            ..Default::default()
        });

        for _ in 0..1000 {
            breaker.record_success(None).await;
        }

        // 30 次失败/成功交替：窗口内错误率达到 30%，而全量错误率仅约 3%
        for _ in 0..30 {
            breaker.record_failure(None).await;
            if breaker.get_state().await == CircuitState::Open {
                break;
            }
            breaker.record_success(None).await;
        }

        assert_eq!(
            breaker.get_state().await,
            CircuitState::Open,
            "a recent burst of failures must not be hidden by a long clean history"
        );
    }

    #[tokio::test]
    async fn test_failures_that_left_the_window_do_not_trip_breaker() {
        let breaker = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1000,
            error_rate_threshold: 0.5,
            ..Default::default()
        });

        // 窗口内错误率 49%，未触发熔断
        for _ in 0..51 {
            breaker.record_success(None).await;
        }
        for _ in 0..49 {
            breaker.record_failure(None).await;
        }
        assert_eq!(breaker.get_state().await, CircuitState::Closed);

        // 一整个窗口的成功把旧失败全部挤出窗口
        for _ in 0..ERROR_RATE_WINDOW_SIZE {
            breaker.record_success(None).await;
        }
        let stats = breaker.get_stats().await;
        assert_eq!(stats.total_requests, ERROR_RATE_WINDOW_SIZE as u32);
        assert_eq!(stats.failed_requests, 0);

        // 新的少量失败只按窗口计算（2/100），不会叠加已移出窗口的 49 次失败
        breaker.record_failure(None).await;
        breaker.record_failure(None).await;
        assert_eq!(breaker.get_state().await, CircuitState::Closed);
        assert_eq!(breaker.get_stats().await.failed_requests, 2);
    }

    #[tokio::test]
    async fn test_min_requests_applies_to_window_and_is_capped_at_window_size() {
        let breaker = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1000,
            error_rate_threshold: 0.6,
            min_requests: 10_000,
            ..Default::default()
        });

        // min_requests 超过窗口大小时按窗口大小截断，否则错误率永远不会被检查
        for _ in 0..ERROR_RATE_WINDOW_SIZE {
            breaker.record_failure(None).await;
            if breaker.get_state().await == CircuitState::Open {
                break;
            }
        }
        assert_eq!(breaker.get_state().await, CircuitState::Open);
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

    #[tokio::test]
    async fn test_circuit_breaker_stats_report_open_remaining_secs() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            timeout_seconds: 600,
            ..Default::default()
        };
        let breaker = CircuitBreaker::new(config);

        assert_eq!(breaker.get_stats().await.open_remaining_secs, None);

        breaker.record_failure(None).await;
        let stats = breaker.get_stats().await;
        assert_eq!(stats.state, CircuitState::Open);
        let remaining = stats
            .open_remaining_secs
            .expect("open breaker reports time left");
        assert!((599..=600).contains(&remaining), "remaining={remaining}");
    }

    #[tokio::test]
    async fn test_circuit_breaker_manual_probe_success_closes_without_waiting() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            success_threshold: 3,
            timeout_seconds: 600,
            ..Default::default()
        };
        let breaker = CircuitBreaker::new(config);
        breaker.record_failure(None).await;
        assert_eq!(breaker.get_state().await, CircuitState::Open);
        assert!(
            !breaker.allow_request().await.allowed,
            "timeout not reached"
        );

        let probe = breaker.begin_manual_probe().await;
        assert!(probe.allowed);
        assert!(probe.used_half_open_permit());
        assert_eq!(breaker.get_state().await, CircuitState::HalfOpen);

        // 探测在途时，第二次手动探测与普通请求都不放行
        assert!(!breaker.begin_manual_probe().await.allowed);
        assert!(!breaker.allow_request().await.allowed);

        breaker
            .record_manual_probe_result(probe.permit.as_ref(), true)
            .await;
        let stats = breaker.get_stats().await;
        assert_eq!(stats.state, CircuitState::Closed);
        assert_eq!(stats.open_remaining_secs, None);
        assert!(breaker.allow_request().await.allowed);
    }

    #[tokio::test]
    async fn test_circuit_breaker_manual_probe_failure_reopens_and_restarts_timer() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            timeout_seconds: 600,
            ..Default::default()
        };
        let breaker = CircuitBreaker::new(config);
        breaker.record_failure(None).await;

        let probe = breaker.begin_manual_probe().await;
        assert!(probe.allowed);
        breaker
            .record_manual_probe_result(probe.permit.as_ref(), false)
            .await;

        let stats = breaker.get_stats().await;
        assert_eq!(stats.state, CircuitState::Open);
        assert!(stats.open_remaining_secs.unwrap() >= 599);
        // 名额已释放：可再次手动探测
        let again = breaker.begin_manual_probe().await;
        assert!(again.allowed);
    }

    #[tokio::test]
    async fn test_circuit_breaker_manual_probe_is_noop_when_closed() {
        let breaker = CircuitBreaker::new(CircuitBreakerConfig::default());
        let probe = breaker.begin_manual_probe().await;
        assert!(!probe.allowed);
        assert!(probe.permit.is_none());
        assert_eq!(breaker.get_state().await, CircuitState::Closed);
    }
}
