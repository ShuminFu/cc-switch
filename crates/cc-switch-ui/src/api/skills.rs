//! Generated from src/lib/api/skills.ts by tools/gen_api.mjs.
//! Untyped payloads are `serde_json::Value` until the contract crate types them.
#![allow(dead_code, unused_imports, clippy::too_many_arguments)]

use std::collections::HashMap;

use cc_switch_contract::commands::skills as cmd;
use cc_switch_contract::{AppId, AppSettings, IpcError, Provider, ProviderMeta};
use serde_json::json;

use crate::ipc;

pub async fn get_installed() -> Result<Vec<serde_json::Value>, IpcError> {
    ipc::invoke_no_args(cmd::GET_INSTALLED_SKILLS).await
}

pub async fn get_backups() -> Result<Vec<serde_json::Value>, IpcError> {
    ipc::invoke_no_args(cmd::GET_SKILL_BACKUPS).await
}

pub async fn delete_backup(backup_id: &str) -> Result<bool, IpcError> {
    ipc::invoke(cmd::DELETE_SKILL_BACKUP, &json!({ "backupId": backup_id })).await
}

pub async fn install_unified(
    skill: &serde_json::Value,
    current_app: AppId,
) -> Result<serde_json::Value, IpcError> {
    ipc::invoke(
        cmd::INSTALL_SKILL_UNIFIED,
        &json!({ "skill": skill, "currentApp": current_app }),
    )
    .await
}

pub async fn uninstall_unified(id: &str) -> Result<serde_json::Value, IpcError> {
    ipc::invoke(cmd::UNINSTALL_SKILL_UNIFIED, &json!({ "id": id })).await
}

pub async fn restore_backup(
    backup_id: &str,
    current_app: AppId,
) -> Result<serde_json::Value, IpcError> {
    ipc::invoke(
        cmd::RESTORE_SKILL_BACKUP,
        &json!({ "backupId": backup_id, "currentApp": current_app }),
    )
    .await
}

pub async fn toggle_app(id: &str, app: AppId, enabled: bool) -> Result<bool, IpcError> {
    ipc::invoke(
        cmd::TOGGLE_SKILL_APP,
        &json!({ "id": id, "app": app, "enabled": enabled }),
    )
    .await
}

pub async fn scan_unmanaged() -> Result<Vec<serde_json::Value>, IpcError> {
    ipc::invoke_no_args(cmd::SCAN_UNMANAGED_SKILLS).await
}

pub async fn import_from_apps(
    imports: &Vec<serde_json::Value>,
) -> Result<Vec<serde_json::Value>, IpcError> {
    ipc::invoke(cmd::IMPORT_SKILLS_FROM_APPS, &json!({ "imports": imports })).await
}

pub async fn discover_available() -> Result<Vec<serde_json::Value>, IpcError> {
    ipc::invoke_no_args(cmd::DISCOVER_AVAILABLE_SKILLS).await
}

pub async fn check_updates() -> Result<Vec<serde_json::Value>, IpcError> {
    ipc::invoke_no_args(cmd::CHECK_SKILL_UPDATES).await
}

pub async fn update_skill(id: &str) -> Result<serde_json::Value, IpcError> {
    ipc::invoke(cmd::UPDATE_SKILL, &json!({ "id": id })).await
}

pub async fn migrate_storage(target: &str) -> Result<serde_json::Value, IpcError> {
    ipc::invoke(cmd::MIGRATE_SKILL_STORAGE, &json!({ "target": target })).await
}

pub async fn search_skills_sh(
    query: &str,
    limit: f64,
    offset: f64,
) -> Result<serde_json::Value, IpcError> {
    ipc::invoke(
        cmd::SEARCH_SKILLS_SH,
        &json!({ "query": query, "limit": limit, "offset": offset }),
    )
    .await
}

// TODO(port): getAll invokes several commands: get_skills, get_skills_for_app

// TODO(port): install invokes several commands: install_skill, install_skill_for_app

// TODO(port): uninstall invokes several commands: uninstall_skill, uninstall_skill_for_app

pub async fn get_repos() -> Result<Vec<serde_json::Value>, IpcError> {
    ipc::invoke_no_args(cmd::GET_SKILL_REPOS).await
}

pub async fn add_repo(repo: &serde_json::Value) -> Result<bool, IpcError> {
    ipc::invoke(cmd::ADD_SKILL_REPO, &json!({ "repo": repo })).await
}

pub async fn remove_repo(owner: &str, name: &str) -> Result<bool, IpcError> {
    ipc::invoke(
        cmd::REMOVE_SKILL_REPO,
        &json!({ "owner": owner, "name": name }),
    )
    .await
}

pub async fn open_zip_file_dialog() -> Result<Option<String>, IpcError> {
    ipc::invoke_no_args(cmd::OPEN_ZIP_FILE_DIALOG).await
}

pub async fn install_from_zip(
    file_path: &str,
    current_app: AppId,
) -> Result<Vec<serde_json::Value>, IpcError> {
    ipc::invoke(
        cmd::INSTALL_SKILLS_FROM_ZIP,
        &json!({ "filePath": file_path, "currentApp": current_app }),
    )
    .await
}
