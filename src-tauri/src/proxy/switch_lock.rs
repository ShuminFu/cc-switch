//! Per-app switch lock
//!
//! 确保同一应用同时只有一个供应商切换操作在执行，
//! 防止并发切换导致 is_current 与 Live 备份不一致。

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, OwnedMutexGuard, RwLock};

/// 每个应用类型一把互斥锁，保证同一应用的切换操作串行执行。
///
/// 不同应用之间（如 Claude 和 Codex）可以并行切换。
#[derive(Clone, Default)]
pub struct SwitchLockManager {
    locks: Arc<RwLock<HashMap<String, Arc<Mutex<()>>>>>,
}

impl SwitchLockManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// 获取指定应用的切换锁。
    ///
    /// 返回 `OwnedMutexGuard`，持有期间同一 `app_type` 的其他切换会排队等待。
    pub async fn lock_for_app(&self, app_type: &str) -> OwnedMutexGuard<()> {
        let lock = {
            let locks = self.locks.read().await;
            if let Some(lock) = locks.get(app_type) {
                lock.clone()
            } else {
                drop(locks);
                let mut locks = self.locks.write().await;
                locks
                    .entry(app_type.to_string())
                    .or_insert_with(|| Arc::new(Mutex::new(())))
                    .clone()
            }
        };
        lock.lock_owned().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::FutureExt;

    // TEST-19: contention is checked by polling once (`now_or_never`), not by
    // racing timers, so the tests are deterministic.

    #[tokio::test]
    async fn same_app_switches_are_serialised() {
        let locks = SwitchLockManager::new();
        let held = locks.lock_for_app("claude").await;
        assert!(
            locks.lock_for_app("claude").now_or_never().is_none(),
            "a second switch for the same app must wait"
        );
        drop(held);
        assert!(locks.lock_for_app("claude").now_or_never().is_some());
    }

    #[tokio::test]
    async fn different_apps_switch_in_parallel() {
        let locks = SwitchLockManager::new();
        let _claude = locks.lock_for_app("claude").await;
        assert!(locks.lock_for_app("codex").now_or_never().is_some());
    }

    #[tokio::test]
    async fn clones_share_the_same_locks() {
        let locks = SwitchLockManager::new();
        let clone = locks.clone();
        let _held = locks.lock_for_app("gemini").await;
        assert!(clone.lock_for_app("gemini").now_or_never().is_none());
    }

    #[tokio::test]
    async fn waiter_acquires_after_release() {
        let locks = SwitchLockManager::new();
        let held = locks.lock_for_app("claude").await;
        let waiter = {
            let locks = locks.clone();
            tokio::spawn(async move {
                let _g = locks.lock_for_app("claude").await;
            })
        };
        drop(held);
        waiter
            .await
            .expect("waiter finished once the lock was released");
    }
}
