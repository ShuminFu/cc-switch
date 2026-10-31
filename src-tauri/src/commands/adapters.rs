//! Live-config adapter diagnostics and previews (CFG-12, CFG-22, CFG-23,
//! CFG-44, CFG-46).
//!
//! These commands only read, except `purge_codex_history_backups`.

use serde::Serialize;
use tauri::AppHandle;

use crate::adapters::plan::{describe_changes, LiveWritePlan};
use crate::adapters::{adapter_for, ImportCandidate, McpSyncSkip};
use crate::app_config::AppType;
use crate::codex_history_migration::CodexHistoryBackupUsage;
use crate::config::ConfigDirSource;
use crate::error::AppError;

use super::{timed_blocking, timed_blocking_with_state};

/// One tool's resolved config directory.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AppConfigDirInfo {
    pub app: AppType,
    pub path: String,
    pub exists: bool,
    pub source: ConfigDirSource,
}

/// Every tool's config directory and whether it comes from a cc-switch
/// override, the tool's environment variable or the default (CFG-12).
#[tauri::command]
#[specta::specta]
pub async fn get_app_config_dirs() -> Result<Vec<AppConfigDirInfo>, AppError> {
    timed_blocking("get_app_config_dirs", || {
        Ok(AppType::all()
            .map(|app| {
                let (path, source) = crate::config::resolve_app_config_dir(&app);
                AppConfigDirInfo {
                    exists: path.exists(),
                    path: path.to_string_lossy().into_owned(),
                    app,
                    source,
                }
            })
            .collect())
    })
    .await
}

/// MCP-capable apps whose config directory is missing; MCP sync writes
/// nothing for them (CFG-46).
#[tauri::command]
#[specta::specta]
pub async fn get_mcp_sync_skips() -> Result<Vec<McpSyncSkip>, AppError> {
    timed_blocking("get_mcp_sync_skips", || {
        Ok(crate::adapters::mcp_sync_skips(AppType::all()))
    })
    .await
}

/// Providers found in `app`'s live config that can be imported (CFG-23).
/// Candidates carry the provider settings (including credentials) so the
/// UI can import them; nothing is logged.
#[tauri::command]
#[specta::specta]
pub async fn get_import_candidates(app: AppType) -> Result<Vec<ImportCandidate>, AppError> {
    timed_blocking("get_import_candidates", move || {
        adapter_for(&app).import_candidates()
    })
    .await
}

/// What switching `app` to `provider_id` would write to its primary live
/// files, without writing (CFG-22). Common config is applied the same way
/// as the switch. MCP projection and side files (Codex model catalog, OMO)
/// are not part of the plan yet; adapters without a planner return
/// `supported: false`.
#[tauri::command]
#[specta::specta]
pub async fn preview_live_write(
    app_handle: AppHandle,
    app: AppType,
    provider_id: String,
) -> Result<LiveWritePlan, AppError> {
    timed_blocking_with_state("preview_live_write", app_handle, move |state| {
        let provider = state
            .db
            .get_provider_by_id(&provider_id, app.as_str())?
            .ok_or_else(|| {
                AppError::localized(
                    "provider.not_found",
                    format!("供应商不存在: {provider_id}"),
                    format!("Provider not found: {provider_id}"),
                )
            })?;
        build_live_write_plan(&state.db, &app, &provider)
    })
    .await
}

pub(crate) fn build_live_write_plan(
    db: &crate::database::Database,
    app: &AppType,
    provider: &crate::provider::Provider,
) -> Result<LiveWritePlan, AppError> {
    let mut effective = provider.clone();
    effective.settings_config =
        crate::services::provider::build_effective_settings_with_common_config(db, app, provider)?;
    let planned = adapter_for(app).plan_live(&effective)?;
    Ok(LiveWritePlan {
        app: app.clone(),
        provider_id: provider.id.clone(),
        supported: planned.is_some(),
        files: planned.map(|w| describe_changes(&w)).unwrap_or_default(),
    })
}

/// Field-level issues in a provider's settings for `app`, with a dotted
/// path, an i18n key and params for inline editor markers (CFG-24).
/// Advisory: saving is not blocked.
#[tauri::command]
#[specta::specta]
pub async fn validate_provider_settings(
    app: AppType,
    settings: serde_json::Value,
) -> Result<Vec<crate::adapters::validate::ValidationIssue>, AppError> {
    Ok(crate::adapters::validate::validate_provider_settings(
        &app, &settings,
    ))
}

/// Disk used by the Codex history migration backups (CFG-44).
#[tauri::command]
#[specta::specta]
pub async fn get_codex_history_backup_usage() -> Result<CodexHistoryBackupUsage, AppError> {
    timed_blocking("get_codex_history_backup_usage", || {
        Ok(crate::codex_history_migration::codex_history_backup_usage())
    })
    .await
}

/// Delete the Codex history migration backups. The official-history
/// restore ledger is kept unless `include_restore_ledger` is true.
#[tauri::command]
#[specta::specta]
pub async fn purge_codex_history_backups(
    include_restore_ledger: bool,
) -> Result<CodexHistoryBackupUsage, AppError> {
    timed_blocking("purge_codex_history_backups", move || {
        crate::codex_history_migration::purge_codex_history_backups(include_restore_ledger)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::provider::{Provider, ProviderMeta};
    use serde_json::json;

    #[test]
    fn preview_applies_common_config_and_leaves_files_untouched() {
        let _home = crate::test_support::isolated_home();
        let db = Database::memory().unwrap();
        db.set_config_snippet(
            "claude",
            Some(r#"{"includeCoAuthoredBy":false}"#.to_string()),
        )
        .unwrap();
        let mut provider = Provider::with_id(
            "p1".into(),
            "P1".into(),
            json!({ "env": { "ANTHROPIC_BASE_URL": "https://example.test" } }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            common_config_enabled: Some(true),
            ..Default::default()
        });

        let plan = build_live_write_plan(&db, &AppType::Claude, &provider).unwrap();
        assert!(plan.supported);
        assert_eq!(plan.files.len(), 1);
        let file = &plan.files[0];
        assert!(file.changed);
        assert_eq!(file.before, None);
        assert!(file.after.contains("includeCoAuthoredBy"), "{}", file.after);
        assert!(!std::path::Path::new(&file.path).exists(), "preview wrote");
    }

    #[test]
    fn preview_reports_unsupported_adapters() {
        let _home = crate::test_support::isolated_home();
        let db = Database::memory().unwrap();
        let provider = Provider::with_id("p".into(), "P".into(), json!({}), None);
        let plan = build_live_write_plan(&db, &AppType::Hermes, &provider).unwrap();
        assert!(!plan.supported);
        assert!(plan.files.is_empty());
    }

    #[test]
    fn config_dirs_cover_every_app() {
        let _home = crate::test_support::isolated_home();
        for app in AppType::all() {
            let (path, source) = crate::config::resolve_app_config_dir(&app);
            if app != AppType::ClaudeDesktop {
                assert!(!path.as_os_str().is_empty(), "{app:?}");
                assert_eq!(source, ConfigDirSource::Default, "{app:?}");
            }
        }
    }
}
