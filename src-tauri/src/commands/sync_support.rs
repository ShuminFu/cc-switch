use serde_json::{json, Value};
use tauri::Manager;

use crate::error::AppError;
use crate::services::provider::ProviderService;
use crate::settings;
use crate::store::AppState;

/// Rewrite live files from the (just replaced) database and reload settings.
///
/// Takes the managed [`AppState`] rather than building a throwaway one: the
/// live sync consults `state.proxy_service` for takeover detection, and a fresh
/// `ProxyService` always reports "not running / not taken over" (CMD-19).
pub(crate) fn run_post_import_sync(state: &AppState) -> Result<(), AppError> {
    ProviderService::sync_current_to_live(state)?;
    settings::reload_settings()?;
    Ok(())
}

/// [`run_post_import_sync`] on the blocking pool, using the app's managed state.
/// The outer `Err` is a task failure (panic / missing state).
pub(crate) async fn run_post_import_sync_blocking(
    app: tauri::AppHandle,
) -> Result<Result<(), AppError>, String> {
    tauri::async_runtime::spawn_blocking(move || match app.try_state::<AppState>() {
        Some(state) => Ok(run_post_import_sync(state.inner())),
        None => Err("应用状态尚未初始化".to_string()),
    })
    .await
    .map_err(|e| e.to_string())?
}

fn post_sync_warning<E: std::fmt::Display>(err: E) -> String {
    AppError::localized(
        "sync.post_operation_sync_failed",
        format!("后置同步状态失败: {err}"),
        format!("Post-operation synchronization failed: {err}"),
    )
    .to_string()
}

pub(crate) fn post_sync_warning_from_result(
    result: Result<Result<(), AppError>, String>,
) -> Option<String> {
    match result {
        Ok(Ok(())) => None,
        Ok(Err(err)) => Some(post_sync_warning(err)),
        Err(err) => Some(post_sync_warning(err)),
    }
}

pub(crate) fn attach_warning(mut value: Value, warning: Option<String>) -> Value {
    if let Some(message) = warning {
        if let Some(obj) = value.as_object_mut() {
            obj.insert("warning".to_string(), Value::String(message));
        }
    }
    value
}

pub(crate) fn success_payload_with_warning(backup_id: String, warning: Option<String>) -> Value {
    attach_warning(
        json!({
            "success": true,
            "message": "SQL imported successfully",
            "backupId": backup_id
        }),
        warning,
    )
}

#[cfg(test)]
mod tests {
    use super::{attach_warning, post_sync_warning_from_result};
    use serde_json::json;

    /// CMD-19: commands must reuse the managed AppState. A second
    /// `AppState::new` builds a second ProxyService that never sees the running
    /// proxy, so a post-import live sync could overwrite takeover-owned files.
    #[test]
    fn commands_never_construct_a_second_app_state() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/commands");
        let needle = ["AppState", "::new("].concat();
        for entry in std::fs::read_dir(&dir).expect("read commands dir") {
            let path = entry.expect("dir entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("read source");
            // Unit tests may build an isolated AppState over an in-memory DB.
            let source = source.split("#[cfg(test)]").next().unwrap_or_default();
            assert!(
                !source.contains(&needle),
                "{} constructs a throwaway AppState",
                path.display()
            );
        }
    }

    #[test]
    fn post_sync_warning_from_result_returns_none_on_success() {
        let warning = post_sync_warning_from_result(Ok(Ok(())));
        assert!(warning.is_none());
    }

    #[test]
    fn post_sync_warning_from_result_returns_some_on_sync_error() {
        let warning =
            post_sync_warning_from_result(Ok(Err(crate::error::AppError::Config("boom".into()))));
        assert!(warning.is_some());
    }

    #[tokio::test]
    async fn post_sync_warning_from_result_returns_some_on_join_error() {
        let handle = tokio::spawn(async move {
            panic!("forced join error");
        });
        let join_err = handle.await.expect_err("task should panic");
        let warning = post_sync_warning_from_result(Err(join_err.to_string()));
        assert!(warning.is_some());
    }

    #[test]
    fn attach_warning_adds_warning_without_dropping_existing_fields() {
        let payload = json!({ "status": "downloaded" });
        let updated = attach_warning(payload, Some("post sync warning".to_string()));
        assert_eq!(
            updated.get("status").and_then(|v| v.as_str()),
            Some("downloaded")
        );
        assert_eq!(
            updated.get("warning").and_then(|v| v.as_str()),
            Some("post sync warning")
        );
    }
}
