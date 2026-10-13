//! Helpers for running blocking command bodies off the UI thread.
//!
//! In Tauri 2, a `#[tauri::command]` that is not `async` executes on the main
//! (UI) thread, so any file IO, SQLite work or process spawning in it freezes
//! the window until it returns. Commands that do real work should be `async`
//! and hand that work to Tauri's blocking pool through these helpers. The
//! frontend `invoke` signature is unaffected by the change.
//!
//! The blocking pool is a better home than a tokio worker too: service code
//! such as `ProviderService::switch` uses `futures::executor::block_on`
//! internally, which must not run on an async runtime worker thread.

use std::cell::Cell;
use std::sync::{Mutex, MutexGuard};

use tauri::{AppHandle, Manager};

use crate::error::AppError;
use crate::store::AppState;

/// Process-wide lock for operations that mutate provider / MCP / skill state,
/// the current-provider pointer, or live client config files.
///
/// While such commands were synchronous they were implicitly serialized by
/// the single main thread. Now that they run on a thread pool, every entry
/// point that performs these writes (pooled commands, tray clicks, profile
/// apply, deep-link import, config import / cloud-sync restore) takes this
/// lock so their read-modify-write sequences cannot interleave.
///
/// Rules:
/// - Take it only at entry points, never inside services: it is not
///   re-entrant, and a nested acquisition deadlocks (debug builds panic
///   instead, see [`lock_mutations`]).
/// - Take it only on a blocking thread (blocking pool or a dedicated
///   thread), never on the UI thread or a tokio worker.
/// - Lock order: this lock first, then the proxy's per-app `switch_locks`
///   (taken inside `ProviderService::switch` / hot-switch). Proxy failover
///   only takes `switch_locks`, so the order never inverts.
static MUTATION_LOCK: Mutex<()> = Mutex::new(());

thread_local! {
    static HOLDS_MUTATION_LOCK: Cell<bool> = const { Cell::new(false) };
}

/// RAII guard returned by [`lock_mutations`].
pub(crate) struct MutationGuard {
    _inner: MutexGuard<'static, ()>,
}

impl Drop for MutationGuard {
    fn drop(&mut self) {
        HOLDS_MUTATION_LOCK.with(|held| held.set(false));
    }
}

/// Acquire [`MUTATION_LOCK`] on the current (blocking) thread.
///
/// A poisoned lock is recovered so one panicking job cannot wedge every later
/// mutation. Re-entrant acquisition on the same thread would deadlock; debug
/// builds turn that into a panic so it surfaces in tests.
pub(crate) fn lock_mutations() -> MutationGuard {
    debug_assert!(
        !HOLDS_MUTATION_LOCK.with(Cell::get),
        "MUTATION_LOCK is not re-entrant: take it only at entry points"
    );
    let inner = MUTATION_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    HOLDS_MUTATION_LOCK.with(|held| held.set(true));
    MutationGuard { _inner: inner }
}

/// Run a blocking job on Tauri's blocking pool and await its result.
pub(crate) async fn run_blocking<T, E, F>(job: F) -> Result<T, E>
where
    T: Send + 'static,
    E: From<AppError> + Send + 'static,
    F: FnOnce() -> Result<T, E> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(job)
        .await
        .map_err(|e| E::from(AppError::Message(format!("后台任务执行失败: {e}"))))?
}

/// Like [`run_blocking`], but holds [`MUTATION_LOCK`] for the job's duration.
pub(crate) async fn run_blocking_serialized<T, E, F>(job: F) -> Result<T, E>
where
    T: Send + 'static,
    E: From<AppError> + Send + 'static,
    F: FnOnce() -> Result<T, E> + Send + 'static,
{
    run_blocking(move || {
        let _guard = lock_mutations();
        job()
    })
    .await
}

/// Run a mutating job that needs [`AppState`], serialized with other
/// mutating commands (see [`MUTATION_LOCK`]).
///
/// `State<'_, AppState>` borrows from the command invocation and cannot move
/// into a `'static` closure; an `AppHandle` is cheap to clone and resolves the
/// managed state again on the worker thread.
pub(crate) async fn with_state_serialized<T, E, F>(handle: AppHandle, job: F) -> Result<T, E>
where
    T: Send + 'static,
    E: From<AppError> + Send + 'static,
    F: FnOnce(&AppState) -> Result<T, E> + Send + 'static,
{
    run_blocking_serialized(move || job(managed_state(&handle)?)).await
}

fn managed_state(handle: &AppHandle) -> Result<&AppState, AppError> {
    handle
        .try_state::<AppState>()
        .map(|state| state.inner())
        .ok_or_else(|| AppError::Message("AppState 未初始化".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn run_blocking_propagates_ok_and_err() {
        let ok: Result<u32, String> = run_blocking(|| Ok(7)).await;
        assert_eq!(ok, Ok(7));

        let err: Result<u32, AppError> =
            run_blocking(|| Err(AppError::Message("nope".to_string()))).await;
        assert!(matches!(err, Err(AppError::Message(m)) if m == "nope"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn run_blocking_reports_panics_as_errors() {
        let result: Result<(), String> = run_blocking(|| panic!("boom")).await;
        let message = result.expect_err("panic must surface as an error");
        assert!(message.contains("后台任务执行失败"), "{message}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn blocking_pool_allows_futures_block_on_with_tokio_primitives() {
        // ProviderService::switch drives async DB / proxy calls with
        // futures::executor::block_on; that must work from the blocking pool.
        let lock = Arc::new(tokio::sync::RwLock::new(1u32));
        let value: Result<u32, String> = run_blocking(move || {
            let handle = tokio::runtime::Handle::current();
            let spawned = handle.spawn(async { 41u32 });
            let inner = futures::executor::block_on(async {
                let guard = lock.read().await;
                *guard + spawned.await.unwrap_or_default()
            });
            Ok(inner)
        })
        .await;
        assert_eq!(value, Ok(42));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn serialized_jobs_never_overlap() {
        let active = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));

        let jobs = (0..6).map(|_| {
            let active = active.clone();
            let max_seen = max_seen.clone();
            run_blocking_serialized(move || {
                let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                max_seen.fetch_max(now, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(15));
                active.fetch_sub(1, Ordering::SeqCst);
                Ok::<_, String>(())
            })
        });
        for result in futures::future::join_all(jobs).await {
            result.expect("serialized job");
        }

        assert_eq!(max_seen.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn serialized_lock_survives_a_panicking_job() {
        let panicked: Result<(), String> = run_blocking_serialized(|| panic!("boom")).await;
        assert!(panicked.is_err());

        let next: Result<u8, String> = run_blocking_serialized(|| Ok(1)).await;
        assert_eq!(next, Ok(1));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn direct_lock_excludes_serialized_jobs() {
        // Entry points outside the command layer (tray, deep link, ...) call
        // lock_mutations() on their own blocking thread; that must exclude
        // pooled serialized commands too.
        let active = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));

        let direct = {
            let active = active.clone();
            let max_seen = max_seen.clone();
            tauri::async_runtime::spawn_blocking(move || {
                for _ in 0..3 {
                    let _guard = lock_mutations();
                    let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                    max_seen.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(10));
                    active.fetch_sub(1, Ordering::SeqCst);
                }
            })
        };
        let pooled = (0..3).map(|_| {
            let active = active.clone();
            let max_seen = max_seen.clone();
            run_blocking_serialized(move || {
                let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                max_seen.fetch_max(now, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(10));
                active.fetch_sub(1, Ordering::SeqCst);
                Ok::<_, String>(())
            })
        });
        for result in futures::future::join_all(pooled).await {
            result.expect("serialized job");
        }
        direct.await.expect("direct locker");

        assert_eq!(max_seen.load(Ordering::SeqCst), 1);
    }

    #[cfg(debug_assertions)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn nested_acquisition_panics_instead_of_deadlocking() {
        let nested: Result<(), String> = run_blocking_serialized(|| {
            let _again = lock_mutations();
            Ok(())
        })
        .await;
        let message = nested.expect_err("nested lock must fail loudly");
        assert!(message.contains("后台任务执行失败"), "{message}");

        // The thread-local flag and the lock are both released afterwards.
        let next: Result<u8, String> = run_blocking_serialized(|| Ok(2)).await;
        assert_eq!(next, Ok(2));
    }
}
