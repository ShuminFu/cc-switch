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
use std::future::Future;
use std::time::Duration;

use tauri::{AppHandle, Manager};
use tokio::sync::{Mutex, MutexGuard};

use crate::error::AppError;
use crate::store::AppState;

/// Process-wide lock for operations that mutate provider / MCP / skill state,
/// the current-provider pointer, or live client config files.
///
/// While such commands were synchronous they were implicitly serialized by
/// the single main thread. Now that they run on a thread pool, every entry
/// point that performs these writes takes this lock so their
/// read-modify-write sequences cannot interleave:
/// - blocking side ([`lock_mutations`]): pooled commands, tray clicks,
///   profile apply, deep-link import, config import / cloud-sync restore;
/// - async side ([`with_mutations_locked`] / [`try_with_mutations_locked`]):
///   proxy takeover toggles, proxy-mode target switches, auto-failover
///   enable, failover switches after a successful retry, startup takeover
///   restore and exit-time restore.
///
/// It is a `tokio::sync::Mutex` so async entry points can wait for it
/// without blocking a runtime worker, while blocking-pool code uses
/// `blocking_lock`. Unlike `std::sync::Mutex` it does not poison, so a
/// panicking job cannot wedge later mutations.
///
/// Rules:
/// - Take it only at entry points, never inside services: it is not
///   re-entrant, and a nested acquisition deadlocks (debug builds panic
///   instead, for nesting on the same thread or within the same task).
///   In particular `ProxyService::{set_takeover_for_app, switch_proxy_target,
///   hot_switch_provider}` must stay lock-free: the tray's Auto click calls
///   them while already holding the lock.
/// - An async holder must not wait on a blocking job that takes the lock
///   (e.g. a `run_blocking_serialized` call) — that deadlocks across threads
///   and cannot be detected.
/// - Blocking side: only on a blocking thread (blocking pool or a dedicated
///   thread), never on the UI thread or a tokio worker.
/// - Lock order: this lock first, then the proxy's per-app `switch_locks`
///   (taken inside `ProviderService::switch` / hot-switch). Nothing takes
///   `switch_locks` and then this lock, so the order never inverts.
static MUTATION_LOCK: Mutex<()> = Mutex::const_new(());

thread_local! {
    static HOLDS_MUTATION_LOCK: Cell<bool> = const { Cell::new(false) };
}

tokio::task_local! {
    static TASK_HOLDS_MUTATION_LOCK: ();
}

const REENTRANT_LOCK_MESSAGE: &str =
    "MUTATION_LOCK is not re-entrant: take it only at entry points";

fn debug_assert_not_holding() {
    debug_assert!(
        !HOLDS_MUTATION_LOCK.with(Cell::get) && TASK_HOLDS_MUTATION_LOCK.try_with(|_| ()).is_err(),
        "{REENTRANT_LOCK_MESSAGE}"
    );
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
/// Must not be called on a tokio worker or inside an async context (tokio's
/// `blocking_lock` panics there). Re-entrant acquisition would deadlock;
/// debug builds turn that into a panic so it surfaces in tests.
pub(crate) fn lock_mutations() -> MutationGuard {
    debug_assert_not_holding();
    let inner = MUTATION_LOCK.blocking_lock();
    HOLDS_MUTATION_LOCK.with(|held| held.set(true));
    MutationGuard { _inner: inner }
}

/// Run `fut` while holding [`MUTATION_LOCK`], waiting for it asynchronously.
///
/// Use for async entry points driven by the user (commands, startup) where
/// waiting behind an in-flight mutation is the correct behaviour.
pub(crate) async fn with_mutations_locked<F: Future>(fut: F) -> F::Output {
    debug_assert_not_holding();
    let _guard = MUTATION_LOCK.lock().await;
    TASK_HOLDS_MUTATION_LOCK.scope((), fut).await
}

/// Like [`with_mutations_locked`], but gives up after `timeout`.
///
/// Returns `None` (without running `fut`) if the lock could not be acquired
/// in time. For background paths, e.g. failover triggered by live proxy
/// traffic, which must not queue for an unbounded time behind a long UI
/// operation; callers log and decide whether to skip or proceed.
pub(crate) async fn try_with_mutations_locked<F: Future>(
    timeout: Duration,
    fut: F,
) -> Option<F::Output> {
    debug_assert_not_holding();
    let guard = tokio::time::timeout(timeout, MUTATION_LOCK.lock())
        .await
        .ok()?;
    let output = TASK_HOLDS_MUTATION_LOCK.scope((), fut).await;
    drop(guard);
    Some(output)
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

    /// Records the maximum number of critical sections observed at once.
    #[derive(Clone, Default)]
    struct Overlap {
        active: Arc<AtomicUsize>,
        max_seen: Arc<AtomicUsize>,
    }

    impl Overlap {
        fn enter(&self) {
            let now = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_seen.fetch_max(now, Ordering::SeqCst);
        }
        fn exit(&self) {
            self.active.fetch_sub(1, Ordering::SeqCst);
        }
        fn max(&self) -> usize {
            self.max_seen.load(Ordering::SeqCst)
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn async_and_blocking_holders_exclude_each_other() {
        // Proxy-side writers (async) and pooled commands / tray (blocking)
        // must never be inside their critical sections at the same time.
        let overlap = Overlap::default();

        let mut tasks = Vec::new();
        for i in 0..4 {
            let overlap = overlap.clone();
            tasks.push(tokio::spawn(async move {
                if i % 2 == 0 {
                    with_mutations_locked(async {
                        overlap.enter();
                        tokio::time::sleep(Duration::from_millis(10)).await;
                        overlap.exit();
                    })
                    .await;
                } else {
                    let acquired = try_with_mutations_locked(Duration::from_secs(5), async {
                        overlap.enter();
                        tokio::time::sleep(Duration::from_millis(10)).await;
                        overlap.exit();
                    })
                    .await;
                    assert!(acquired.is_some(), "lock should free up within 5s");
                }
            }));
        }
        let blocking = (0..4).map(|_| {
            let overlap = overlap.clone();
            run_blocking_serialized(move || {
                overlap.enter();
                std::thread::sleep(Duration::from_millis(10));
                overlap.exit();
                Ok::<_, String>(())
            })
        });
        for result in futures::future::join_all(blocking).await {
            result.expect("blocking holder");
        }
        for task in tasks {
            task.await.expect("async holder");
        }

        assert_eq!(overlap.max(), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn try_lock_times_out_behind_a_long_blocking_mutation() {
        // A failover switch must not queue indefinitely behind a slow UI job.
        let (held_tx, held_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let long_job = run_blocking_serialized(move || {
            let _ = held_tx.send(());
            let _ = release_rx.recv_timeout(Duration::from_secs(5));
            Ok::<_, String>(())
        });
        let long_job = tokio::spawn(long_job);
        held_rx.await.expect("long job holds the lock");

        let ran = Arc::new(AtomicUsize::new(0));
        let ran_in_fut = ran.clone();
        let outcome = try_with_mutations_locked(Duration::from_millis(50), async move {
            ran_in_fut.fetch_add(1, Ordering::SeqCst);
        })
        .await;
        assert!(outcome.is_none(), "must give up while the lock is held");
        assert_eq!(ran.load(Ordering::SeqCst), 0, "fut must not run on timeout");

        release_tx.send(()).expect("release long job");
        long_job.await.expect("join").expect("long job");

        let outcome = try_with_mutations_locked(Duration::from_secs(1), async { 7 }).await;
        assert_eq!(outcome, Some(7), "lock is usable again afterwards");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn async_holder_waits_for_in_flight_blocking_mutation() {
        // Ordering: an async writer arriving while a blocking mutation is in
        // progress observes its completed effects, never a half-written state.
        let log = Arc::new(std::sync::Mutex::new(Vec::<&'static str>::new()));
        let (held_tx, held_rx) = tokio::sync::oneshot::channel();

        let blocking_log = log.clone();
        let blocking = tokio::spawn(run_blocking_serialized(move || {
            let _ = held_tx.send(());
            blocking_log.lock().unwrap().push("blocking:start");
            std::thread::sleep(Duration::from_millis(30));
            blocking_log.lock().unwrap().push("blocking:end");
            Ok::<_, String>(())
        }));
        held_rx.await.expect("blocking job holds the lock");

        let async_log = log.clone();
        with_mutations_locked(async move {
            async_log.lock().unwrap().push("async");
        })
        .await;
        blocking.await.expect("join").expect("blocking job");

        assert_eq!(
            *log.lock().unwrap(),
            vec!["blocking:start", "blocking:end", "async"]
        );
    }

    #[cfg(debug_assertions)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn nested_async_acquisition_panics_instead_of_deadlocking() {
        let nested = tokio::spawn(with_mutations_locked(async {
            with_mutations_locked(async {}).await;
        }))
        .await;
        assert!(nested.is_err(), "nested async lock must panic in debug");

        // The guard was released while unwinding.
        let next = try_with_mutations_locked(Duration::from_secs(1), async { 3 }).await;
        assert_eq!(next, Some(3));
    }
}
