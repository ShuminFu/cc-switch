//! 使用统计实时刷新事件模块
//!
//! 当 `proxy_request_logs` 表写入新数据时（代理日志、会话同步、归档等），
//! 通过本模块向前端 emit `usage-log-recorded` 事件，让 UsageDashboard
//! 立刻 invalidate 查询缓存而无需等待轮询周期。
//!
//! 设计要点：
//! - 全局单例 AppHandle：写日志路径上不持有 AppHandle，用 OnceCell 共享。
//! - 200ms 防抖合并：流式响应等场景在短时间内可能写入多条日志，
//!   合并成一次事件可避免前端连续 invalidate。
//! - 批量合并：会话日志同步/回填期间用 [`begin_batch`] 抑制逐行通知，
//!   批次结束时最多发出一次事件（SESS-08），避免初次导入时的事件风暴。
//! - 不阻塞写入：通知失败仅记录 warn 日志，不向上传播错误。

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

use tauri::{AppHandle, Emitter};

/// 前端监听的事件名
pub const EVENT_USAGE_LOG_RECORDED: &str = "usage-log-recorded";

/// 防抖窗口：合并 200ms 内的多次通知。
const DEBOUNCE_WINDOW: Duration = Duration::from_millis(200);

static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

/// 全局通知合并器。
static COALESCER: Coalescer = Coalescer::new();

/// 通知合并状态机（与 AppHandle/定时器解耦，便于单元测试）。
///
/// - `batch_depth > 0` 时通知只记为 dirty，不调度 emit；
/// - 最外层批次结束且期间有通知时，调度一次 emit；
/// - 批次外的通知走 200ms 防抖：已有待发 emit 时直接合并。
struct Coalescer {
    batch_depth: AtomicUsize,
    batch_dirty: AtomicBool,
    emit_scheduled: AtomicBool,
}

impl Coalescer {
    const fn new() -> Self {
        Self {
            batch_depth: AtomicUsize::new(0),
            batch_dirty: AtomicBool::new(false),
            emit_scheduled: AtomicBool::new(false),
        }
    }

    /// 记录一次通知；返回 true 表示调用方需要调度一次 emit。
    fn notify(&self) -> bool {
        if self.batch_depth.load(Ordering::Acquire) > 0 {
            self.batch_dirty.store(true, Ordering::Release);
            // 与 end_batch 竞争：若批次恰好在此期间结束，由这里补发。
            if self.batch_depth.load(Ordering::Acquire) > 0 {
                return false;
            }
            if !self.batch_dirty.swap(false, Ordering::AcqRel) {
                return false;
            }
        }
        self.claim_schedule()
    }

    fn begin_batch(&self) {
        self.batch_depth.fetch_add(1, Ordering::AcqRel);
    }

    /// 结束一个批次；返回 true 表示调用方需要调度一次 emit。
    fn end_batch(&self) -> bool {
        if self.batch_depth.fetch_sub(1, Ordering::AcqRel) != 1 {
            return false;
        }
        if !self.batch_dirty.swap(false, Ordering::AcqRel) {
            return false;
        }
        self.claim_schedule()
    }

    /// 已有待发 emit 时返回 false（合并到既有任务）。
    fn claim_schedule(&self) -> bool {
        !self.emit_scheduled.swap(true, Ordering::AcqRel)
    }

    /// 定时器到期、即将 emit 前调用：先清标志，emit 期间的新通知会重新调度。
    fn emit_started(&self) {
        self.emit_scheduled.store(false, Ordering::Release);
    }
}

/// 在应用 setup 阶段调用一次，注入 AppHandle。
///
/// 重复调用是无害的（OnceLock 仅首次写入生效），但应用启动期只该被
/// `lib.rs::run` 调一次。
pub fn init(handle: AppHandle) {
    if APP_HANDLE.set(handle).is_err() {
        log::debug!("usage_events::init 重复调用，已忽略");
    } else {
        log::info!("[usage-event] AppHandle 已注入，事件推送启用");
    }
}

/// 通知前端有新的使用日志写入。
///
/// 调用方**不**需要持有 AppHandle，可以从任意线程/任意写入路径调用。
/// 内部 200ms 防抖合并（批次内则推迟到批次结束），绝不阻塞调用线程。
pub fn notify_log_recorded() {
    // AppHandle 未注入（典型出现在单元测试或 setup 之前）：直接放弃。
    if APP_HANDLE.get().is_none() {
        return;
    }
    if COALESCER.notify() {
        schedule_emit();
    }
}

/// RAII 批次守卫：存活期间的通知被合并，drop 时最多 emit 一次。
#[must_use = "通知只在守卫存活期间被合并"]
pub struct BatchGuard {
    _private: (),
}

impl Drop for BatchGuard {
    fn drop(&mut self) {
        if COALESCER.end_batch() {
            schedule_emit();
        }
    }
}

/// 开始一个通知批次（可嵌套），用于会话同步、回填等批量写入。
pub fn begin_batch() -> BatchGuard {
    COALESCER.begin_batch();
    BatchGuard { _private: () }
}

fn schedule_emit() {
    let Some(handle) = APP_HANDLE.get() else {
        COALESCER.emit_started();
        return;
    };
    let handle = handle.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(DEBOUNCE_WINDOW).await;
        // 必须先清标志再 emit：万一 emit 期间又有新通知进来，
        // 下一轮防抖窗口会重新调度，不会丢失。
        COALESCER.emit_started();

        if let Err(e) = handle.emit(EVENT_USAGE_LOG_RECORDED, ()) {
            log::warn!("emit {EVENT_USAGE_LOG_RECORDED} 失败: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notifications_outside_batch_are_debounced() {
        let c = Coalescer::new();
        assert!(c.notify(), "first notification schedules an emit");
        assert!(!c.notify(), "merged into the pending emit");
        c.emit_started();
        assert!(c.notify(), "a new window schedules again");
    }

    #[test]
    fn batch_coalesces_many_rows_into_one_emit() {
        let c = Coalescer::new();
        c.begin_batch();
        let scheduled = (0..10_000).filter(|_| c.notify()).count();
        assert_eq!(scheduled, 0, "no emit while the batch is open");
        assert!(c.end_batch(), "one emit when the batch ends");
    }

    #[test]
    fn nested_batches_emit_once_at_outermost_end() {
        let c = Coalescer::new();
        c.begin_batch();
        c.begin_batch();
        assert!(!c.notify());
        assert!(!c.end_batch(), "inner end does not emit");
        assert!(c.end_batch(), "outer end emits");
    }

    #[test]
    fn empty_batch_does_not_emit() {
        let c = Coalescer::new();
        c.begin_batch();
        assert!(!c.end_batch());
    }
}
