//! Bridge for driving async proxy-service futures from synchronous provider code.
//!
//! `ProviderService` is synchronous (it mostly does file I/O and SQLite work),
//! but a few steps need `ProxyService` futures that await tokio locks (the
//! per-app switch lock, the server `RwLock`). Those used to be driven with
//! `futures::executor::block_on`, which parks the current thread without
//! telling tokio. When that thread is a tokio worker, every other task queued
//! on that worker (possibly the very task holding the lock) is starved, which
//! is a latent deadlock (CMD-07 / SVC-05).
//!
//! [`block_on_async`] picks a safe way to wait for the current context:
//! - outside any tokio runtime (tray handlers, `spawn_blocking` threads,
//!   plain `#[test]`s): drive the future on Tauri's shared runtime;
//! - on a multi-thread runtime worker: `block_in_place` first, so tokio hands
//!   the worker's other tasks to another thread before we block;
//! - on a current-thread runtime (`#[tokio::test]`): blocking would stall the
//!   only scheduler thread, so the future is driven on a scoped helper thread
//!   with Tauri's shared runtime instead.
//!
//! Commands should still call provider operations through `spawn_blocking`
//! so none of this work runs on the UI thread or on a tokio worker.

use std::future::Future;

use tokio::runtime::{Handle, RuntimeFlavor};

/// Wait for `fut` from synchronous code without stalling a tokio worker.
pub(crate) fn block_on_async<F>(fut: F) -> F::Output
where
    F: Future + Send,
    F::Output: Send,
{
    match Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == RuntimeFlavor::CurrentThread => {
            std::thread::scope(|scope| {
                scope
                    .spawn(move || tauri::async_runtime::block_on(fut))
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
        }
        // `block_in_place` is a no-op wrapper on threads that are not runtime
        // workers (e.g. `spawn_blocking` threads), where `Handle::block_on` is
        // allowed directly.
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fut)),
        Err(_) => tauri::async_runtime::block_on(fut),
    }
}

#[cfg(test)]
mod tests {
    use super::block_on_async;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::Mutex;

    #[test]
    fn works_outside_any_runtime() {
        assert_eq!(block_on_async(async { 7 }), 7);
    }

    #[tokio::test]
    async fn works_on_current_thread_runtime() {
        let value = block_on_async(async {
            tokio::time::sleep(Duration::from_millis(1)).await;
            11
        });
        assert_eq!(value, 11);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn does_not_starve_lock_holder_on_single_worker() {
        // The lock holder runs on the only worker. A plain executor block_on on
        // that worker would never let it release the lock; block_in_place
        // hands the worker's queue to another thread first.
        let lock = Arc::new(Mutex::new(()));
        let guard = lock.clone().lock_owned().await;
        let holder = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            drop(guard);
        });
        tokio::task::yield_now().await;

        let waiter = lock.clone();
        let acquired = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::task::spawn_blocking(move || {
                // Mirrors a sync provider operation that needs the switch lock.
                block_on_async(async move {
                    let _g = waiter.lock().await;
                    true
                })
            }),
        )
        .await
        .expect("must not deadlock")
        .expect("join");
        assert!(acquired);

        let direct = lock.clone();
        let acquired_on_worker = block_on_async(async move {
            let _g = direct.lock().await;
            true
        });
        assert!(acquired_on_worker);
        holder.await.unwrap();
    }
}
