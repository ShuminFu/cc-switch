//! Generated from src/lib/api/settings.ts by tools/gen_api.mjs.
//! Untyped payloads are `serde_json::Value` until the contract crate types them.
#![allow(dead_code, unused_imports, clippy::too_many_arguments)]

use std::collections::HashMap;

use cc_switch_contract::commands::settings as cmd;
use cc_switch_contract::{AppId, AppSettings, IpcError, Provider, ProviderMeta};
use serde_json::json;

use crate::ipc;

pub async fn get() -> Result<AppSettings, IpcError> {
    ipc::invoke_no_args(cmd::GET_SETTINGS).await
}

pub async fn save(settings: &AppSettings) -> Result<bool, IpcError> {
    ipc::invoke(cmd::SAVE_SETTINGS, &json!({ "settings": settings })).await
}

pub async fn has_codex_unify_history_backup() -> Result<bool, IpcError> {
    ipc::invoke_no_args(cmd::HAS_CODEX_UNIFY_HISTORY_BACKUP).await
}

pub async fn restore_codex_unified_history() -> Result<serde_json::Value, IpcError> {
    ipc::invoke_no_args(cmd::RESTORE_CODEX_UNIFIED_HISTORY).await
}

pub async fn restart() -> Result<bool, IpcError> {
    ipc::invoke_no_args(cmd::RESTART_APP).await
}

pub async fn install_update_and_restart() -> Result<bool, IpcError> {
    ipc::invoke_no_args(cmd::INSTALL_UPDATE_AND_RESTART).await
}

pub async fn check_updates() -> Result<(), IpcError> {
    ipc::invoke_no_args(cmd::CHECK_FOR_UPDATES).await
}

pub async fn is_portable() -> Result<bool, IpcError> {
    ipc::invoke_no_args(cmd::IS_PORTABLE_MODE).await
}

pub async fn get_config_dir(app_id: AppId) -> Result<String, IpcError> {
    ipc::invoke(cmd::GET_CONFIG_DIR, &json!({ "app": app_id })).await
}

pub async fn open_config_folder(app_id: AppId) -> Result<(), IpcError> {
    ipc::invoke(cmd::OPEN_CONFIG_FOLDER, &json!({ "app": app_id })).await
}

pub async fn pick_directory(default_path: Option<String>) -> Result<Option<String>, IpcError> {
    ipc::invoke(cmd::PICK_DIRECTORY, &json!({ "defaultPath": default_path })).await
}

pub async fn select_config_directory(
    default_path: Option<String>,
) -> Result<Option<String>, IpcError> {
    ipc::invoke(cmd::PICK_DIRECTORY, &json!({ "defaultPath": default_path })).await
}

pub async fn get_claude_code_config_path() -> Result<String, IpcError> {
    ipc::invoke_no_args(cmd::GET_CLAUDE_CODE_CONFIG_PATH).await
}

pub async fn get_app_config_path() -> Result<String, IpcError> {
    ipc::invoke_no_args(cmd::GET_APP_CONFIG_PATH).await
}

pub async fn open_app_config_folder() -> Result<(), IpcError> {
    ipc::invoke_no_args(cmd::OPEN_APP_CONFIG_FOLDER).await
}

pub async fn get_app_config_dir_override() -> Result<Option<String>, IpcError> {
    ipc::invoke_no_args(cmd::GET_APP_CONFIG_DIR_OVERRIDE).await
}

pub async fn set_app_config_dir_override(path: Option<String>) -> Result<bool, IpcError> {
    ipc::invoke(cmd::SET_APP_CONFIG_DIR_OVERRIDE, &json!({ "path": path })).await
}

// TODO(port): applyClaudePluginConfig(options: { official: boolean; }) -> boolean: invoke("apply_claude_plugin_config", [["official","official"]])

pub async fn apply_claude_onboarding_skip() -> Result<bool, IpcError> {
    ipc::invoke_no_args(cmd::APPLY_CLAUDE_ONBOARDING_SKIP).await
}

pub async fn clear_claude_onboarding_skip() -> Result<bool, IpcError> {
    ipc::invoke_no_args(cmd::CLEAR_CLAUDE_ONBOARDING_SKIP).await
}

pub async fn save_file_dialog(default_name: &str) -> Result<Option<String>, IpcError> {
    ipc::invoke(
        cmd::SAVE_FILE_DIALOG,
        &json!({ "defaultName": default_name }),
    )
    .await
}

pub async fn open_file_dialog() -> Result<Option<String>, IpcError> {
    ipc::invoke_no_args(cmd::OPEN_FILE_DIALOG).await
}

pub async fn export_config_to_file(file_path: &str) -> Result<serde_json::Value, IpcError> {
    ipc::invoke(
        cmd::EXPORT_CONFIG_TO_FILE,
        &json!({ "filePath": file_path }),
    )
    .await
}

pub async fn import_config_from_file(file_path: &str) -> Result<serde_json::Value, IpcError> {
    ipc::invoke(
        cmd::IMPORT_CONFIG_FROM_FILE,
        &json!({ "filePath": file_path }),
    )
    .await
}

pub async fn webdav_test_connection(
    settings: &serde_json::Value,
    preserve_empty_password: Option<serde_json::Value>,
) -> Result<serde_json::Value, IpcError> {
    ipc::invoke(
        cmd::WEBDAV_TEST_CONNECTION,
        &json!({ "settings": settings, "preserveEmptyPassword": preserve_empty_password }),
    )
    .await
}

pub async fn webdav_sync_upload() -> Result<serde_json::Value, IpcError> {
    ipc::invoke_no_args(cmd::WEBDAV_SYNC_UPLOAD).await
}

pub async fn webdav_sync_download() -> Result<serde_json::Value, IpcError> {
    ipc::invoke_no_args(cmd::WEBDAV_SYNC_DOWNLOAD).await
}

pub async fn webdav_sync_save_settings(
    settings: &serde_json::Value,
    password_touched: Option<serde_json::Value>,
) -> Result<serde_json::Value, IpcError> {
    ipc::invoke(
        cmd::WEBDAV_SYNC_SAVE_SETTINGS,
        &json!({ "settings": settings, "passwordTouched": password_touched }),
    )
    .await
}

pub async fn webdav_sync_fetch_remote_info() -> Result<serde_json::Value, IpcError> {
    ipc::invoke_no_args(cmd::WEBDAV_SYNC_FETCH_REMOTE_INFO).await
}

pub async fn s3_test_connection(
    settings: &serde_json::Value,
    preserve_empty_password: Option<serde_json::Value>,
) -> Result<serde_json::Value, IpcError> {
    ipc::invoke(
        cmd::S3_TEST_CONNECTION,
        &json!({ "settings": settings, "preserveEmptyPassword": preserve_empty_password }),
    )
    .await
}

pub async fn s3_sync_upload() -> Result<serde_json::Value, IpcError> {
    ipc::invoke_no_args(cmd::S3_SYNC_UPLOAD).await
}

pub async fn s3_sync_download() -> Result<serde_json::Value, IpcError> {
    ipc::invoke_no_args(cmd::S3_SYNC_DOWNLOAD).await
}

pub async fn s3_sync_save_settings(
    settings: &serde_json::Value,
    password_touched: bool,
) -> Result<serde_json::Value, IpcError> {
    ipc::invoke(
        cmd::S3_SYNC_SAVE_SETTINGS,
        &json!({ "settings": settings, "passwordTouched": password_touched }),
    )
    .await
}

pub async fn s3_sync_fetch_remote_info() -> Result<serde_json::Value, IpcError> {
    ipc::invoke_no_args(cmd::S3_SYNC_FETCH_REMOTE_INFO).await
}

pub async fn sync_current_providers_live() -> Result<(), IpcError> {
    ipc::invoke_no_args(cmd::SYNC_CURRENT_PROVIDERS_LIVE).await
}

pub async fn open_external(url: &str) -> Result<(), IpcError> {
    ipc::invoke(cmd::OPEN_EXTERNAL, &json!({ "url": url })).await
}

pub async fn set_auto_launch(enabled: bool) -> Result<bool, IpcError> {
    ipc::invoke(cmd::SET_AUTO_LAUNCH, &json!({ "enabled": enabled })).await
}

pub async fn get_auto_launch_status() -> Result<bool, IpcError> {
    ipc::invoke_no_args(cmd::GET_AUTO_LAUNCH_STATUS).await
}

pub async fn get_tool_versions(
    tools: Option<Vec<String>>,
    wsl_shell_by_tool: Option<serde_json::Value>,
) -> Result<serde_json::Value, IpcError> {
    ipc::invoke(
        cmd::GET_TOOL_VERSIONS,
        &json!({ "tools": tools, "wslShellByTool": wsl_shell_by_tool }),
    )
    .await
}

pub async fn run_tool_lifecycle_action(
    tools: &Vec<String>,
    action: &str,
    wsl_shell_by_tool: Option<serde_json::Value>,
) -> Result<(), IpcError> {
    ipc::invoke(
        cmd::RUN_TOOL_LIFECYCLE_ACTION,
        &json!({ "tools": tools, "action": action, "wslShellByTool": wsl_shell_by_tool }),
    )
    .await
}

pub async fn probe_tool_installations(
    tools: &Vec<String>,
) -> Result<Vec<serde_json::Value>, IpcError> {
    ipc::invoke(cmd::PROBE_TOOL_INSTALLATIONS, &json!({ "tools": tools })).await
}

pub async fn get_rectifier_config() -> Result<serde_json::Value, IpcError> {
    ipc::invoke_no_args(cmd::GET_RECTIFIER_CONFIG).await
}

pub async fn set_rectifier_config(config: &serde_json::Value) -> Result<bool, IpcError> {
    ipc::invoke(cmd::SET_RECTIFIER_CONFIG, &json!({ "config": config })).await
}

pub async fn get_optimizer_config() -> Result<serde_json::Value, IpcError> {
    ipc::invoke_no_args(cmd::GET_OPTIMIZER_CONFIG).await
}

pub async fn set_optimizer_config(config: &serde_json::Value) -> Result<bool, IpcError> {
    ipc::invoke(cmd::SET_OPTIMIZER_CONFIG, &json!({ "config": config })).await
}

pub async fn get_log_config() -> Result<serde_json::Value, IpcError> {
    ipc::invoke_no_args(cmd::GET_LOG_CONFIG).await
}

pub async fn set_log_config(config: &serde_json::Value) -> Result<bool, IpcError> {
    ipc::invoke(cmd::SET_LOG_CONFIG, &json!({ "config": config })).await
}

pub async fn create_db_backup() -> Result<String, IpcError> {
    ipc::invoke_no_args(cmd::CREATE_DB_BACKUP).await
}

pub async fn list_db_backups() -> Result<Vec<serde_json::Value>, IpcError> {
    ipc::invoke_no_args(cmd::LIST_DB_BACKUPS).await
}

pub async fn restore_db_backup(filename: &str) -> Result<String, IpcError> {
    ipc::invoke(cmd::RESTORE_DB_BACKUP, &json!({ "filename": filename })).await
}

pub async fn rename_db_backup(old_filename: &str, new_name: &str) -> Result<String, IpcError> {
    ipc::invoke(
        cmd::RENAME_DB_BACKUP,
        &json!({ "oldFilename": old_filename, "newName": new_name }),
    )
    .await
}

pub async fn delete_db_backup(filename: &str) -> Result<(), IpcError> {
    ipc::invoke(cmd::DELETE_DB_BACKUP, &json!({ "filename": filename })).await
}
