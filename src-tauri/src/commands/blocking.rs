//! Helpers for running blocking command bodies off the UI thread.
//!
//! Tauri executes plain (non-`async`) commands inline on the main/UI thread,
//! so any file I/O, SQLite work or process spawning there freezes the window
//! and tray (CMD-08 / PERF-12). Commands instead become `async` and hand the
//! blocking body to Tauri's blocking pool through these helpers, which keeps
//! tokio workers free as well.

use tauri::{AppHandle, Manager};

use crate::error::AppError;
use crate::store::AppState;

/// Error types a command can return when the blocking task itself fails
/// (panicked or was cancelled) rather than returning an error.
pub(crate) trait BlockingTaskError {
    fn from_task_failure(message: String) -> Self;
}

impl BlockingTaskError for String {
    fn from_task_failure(message: String) -> Self {
        message
    }
}

impl BlockingTaskError for AppError {
    fn from_task_failure(message: String) -> Self {
        AppError::Message(message)
    }
}

/// Run `f` on the blocking thread pool and wait for its result.
pub(crate) async fn run_blocking<T, E, F>(f: F) -> Result<T, E>
where
    F: FnOnce() -> Result<T, E> + Send + 'static,
    T: Send + 'static,
    E: BlockingTaskError + Send + 'static,
{
    match tauri::async_runtime::spawn_blocking(f).await {
        Ok(result) => result,
        Err(e) => Err(E::from_task_failure(format!("后台任务执行失败: {e}"))),
    }
}

/// Run `f` with the managed [`AppState`] on the blocking thread pool.
///
/// `State<'_, AppState>` borrows from the command invocation and cannot move
/// into `spawn_blocking`, so the closure re-acquires it from the `AppHandle`.
pub(crate) async fn run_blocking_with_state<T, E, F>(app: AppHandle, f: F) -> Result<T, E>
where
    F: FnOnce(&AppState) -> Result<T, E> + Send + 'static,
    T: Send + 'static,
    E: BlockingTaskError + Send + 'static,
{
    run_blocking(move || match app.try_state::<AppState>() {
        Some(state) => f(state.inner()),
        None => Err(E::from_task_failure("应用状态尚未初始化".to_string())),
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::run_blocking;
    use crate::error::AppError;

    #[tokio::test]
    async fn returns_closure_result() {
        let ok: Result<u32, String> = run_blocking(|| Ok(3)).await;
        assert_eq!(ok, Ok(3));
        let err: Result<u32, AppError> = run_blocking(|| Err(AppError::Message("x".into()))).await;
        assert!(matches!(err, Err(AppError::Message(m)) if m == "x"));
    }

    #[tokio::test]
    async fn panics_become_errors() {
        let result: Result<(), String> = run_blocking(|| panic!("boom")).await;
        assert!(result.unwrap_err().contains("后台任务执行失败"));
    }

    #[tokio::test]
    async fn runs_off_the_calling_thread() {
        let caller = std::thread::current().id();
        let worker: Result<std::thread::ThreadId, String> =
            run_blocking(|| Ok(std::thread::current().id())).await;
        assert_ne!(worker.unwrap(), caller);
    }
}
