//! Generated from src/lib/api/providers.ts by tools/gen_api.mjs.
//! Untyped payloads are `serde_json::Value` until the contract crate types them.
#![allow(dead_code, unused_imports, clippy::too_many_arguments)]

use std::collections::HashMap;

use cc_switch_contract::commands::providers as cmd;
use cc_switch_contract::{AppId, AppSettings, IpcError, Provider, ProviderMeta};
use serde_json::json;

use crate::ipc;

pub async fn get_all(app_id: AppId) -> Result<HashMap<String, Provider>, IpcError> {
    ipc::invoke(cmd::GET_PROVIDERS, &json!({ "app": app_id })).await
}

pub async fn get_current(app_id: AppId) -> Result<String, IpcError> {
    ipc::invoke(cmd::GET_CURRENT_PROVIDER, &json!({ "app": app_id })).await
}

pub async fn add(
    provider: &Provider,
    app_id: AppId,
    add_to_live: Option<bool>,
) -> Result<bool, IpcError> {
    ipc::invoke(
        cmd::ADD_PROVIDER,
        &json!({ "provider": provider, "app": app_id, "addToLive": add_to_live }),
    )
    .await
}

pub async fn update(
    provider: &Provider,
    app_id: AppId,
    original_id: Option<String>,
) -> Result<bool, IpcError> {
    ipc::invoke(
        cmd::UPDATE_PROVIDER,
        &json!({ "provider": provider, "app": app_id, "originalId": original_id }),
    )
    .await
}

pub async fn delete(id: &str, app_id: AppId) -> Result<bool, IpcError> {
    ipc::invoke(cmd::DELETE_PROVIDER, &json!({ "id": id, "app": app_id })).await
}

pub async fn remove_from_live_config(id: &str, app_id: AppId) -> Result<bool, IpcError> {
    ipc::invoke(
        cmd::REMOVE_PROVIDER_FROM_LIVE_CONFIG,
        &json!({ "id": id, "app": app_id }),
    )
    .await
}

pub async fn switch(id: &str, app_id: AppId) -> Result<serde_json::Value, IpcError> {
    ipc::invoke(cmd::SWITCH_PROVIDER, &json!({ "id": id, "app": app_id })).await
}

pub async fn import_default(app_id: AppId) -> Result<bool, IpcError> {
    ipc::invoke(cmd::IMPORT_DEFAULT_CONFIG, &json!({ "app": app_id })).await
}

pub async fn import_claude_desktop_from_claude() -> Result<f64, IpcError> {
    ipc::invoke_no_args(cmd::IMPORT_CLAUDE_DESKTOP_PROVIDERS_FROM_CLAUDE).await
}

pub async fn ensure_claude_desktop_official_provider() -> Result<bool, IpcError> {
    ipc::invoke_no_args(cmd::ENSURE_CLAUDE_DESKTOP_OFFICIAL_PROVIDER).await
}

pub async fn ensure_codex_official_provider() -> Result<bool, IpcError> {
    ipc::invoke_no_args(cmd::ENSURE_CODEX_OFFICIAL_PROVIDER).await
}

pub async fn get_claude_desktop_status() -> Result<serde_json::Value, IpcError> {
    ipc::invoke_no_args(cmd::GET_CLAUDE_DESKTOP_STATUS).await
}

pub async fn get_claude_desktop_default_routes() -> Result<Vec<serde_json::Value>, IpcError> {
    ipc::invoke_no_args(cmd::GET_CLAUDE_DESKTOP_DEFAULT_ROUTES).await
}

pub async fn update_tray_menu() -> Result<bool, IpcError> {
    ipc::invoke_no_args(cmd::UPDATE_TRAY_MENU).await
}

pub async fn update_sort_order(
    updates: &Vec<serde_json::Value>,
    app_id: AppId,
) -> Result<bool, IpcError> {
    ipc::invoke(
        cmd::UPDATE_PROVIDERS_SORT_ORDER,
        &json!({ "updates": updates, "app": app_id }),
    )
    .await
}

// TODO(port): openTerminal(providerId: string, appId: AppId, options: OpenTerminalOptions) -> boolean: invoke("open_provider_terminal", [["providerId","providerId"],["app","appId"],["cwd","cwd"]])

pub async fn import_open_code_from_live() -> Result<f64, IpcError> {
    ipc::invoke_no_args(cmd::IMPORT_OPENCODE_PROVIDERS_FROM_LIVE).await
}

pub async fn get_open_code_live_provider_ids() -> Result<Vec<String>, IpcError> {
    ipc::invoke_no_args(cmd::GET_OPENCODE_LIVE_PROVIDER_IDS).await
}

pub async fn get_open_claw_live_provider_ids() -> Result<Vec<String>, IpcError> {
    ipc::invoke_no_args(cmd::GET_OPENCLAW_LIVE_PROVIDER_IDS).await
}

pub async fn get_hermes_live_provider_ids() -> Result<Vec<String>, IpcError> {
    ipc::invoke_no_args(cmd::GET_HERMES_LIVE_PROVIDER_IDS).await
}

pub async fn import_open_claw_from_live() -> Result<f64, IpcError> {
    ipc::invoke_no_args(cmd::IMPORT_OPENCLAW_PROVIDERS_FROM_LIVE).await
}

pub async fn import_hermes_from_live() -> Result<f64, IpcError> {
    ipc::invoke_no_args(cmd::IMPORT_HERMES_PROVIDERS_FROM_LIVE).await
}

pub async fn get_all_() -> Result<serde_json::Value, IpcError> {
    ipc::invoke_no_args(cmd::GET_UNIVERSAL_PROVIDERS).await
}

pub async fn get(id: &str) -> Result<Option<serde_json::Value>, IpcError> {
    ipc::invoke(cmd::GET_UNIVERSAL_PROVIDER, &json!({ "id": id })).await
}

pub async fn upsert(provider: &serde_json::Value) -> Result<bool, IpcError> {
    ipc::invoke(
        cmd::UPSERT_UNIVERSAL_PROVIDER,
        &json!({ "provider": provider }),
    )
    .await
}

pub async fn delete_(id: &str) -> Result<bool, IpcError> {
    ipc::invoke(cmd::DELETE_UNIVERSAL_PROVIDER, &json!({ "id": id })).await
}

pub async fn sync(id: &str) -> Result<bool, IpcError> {
    ipc::invoke(cmd::SYNC_UNIVERSAL_PROVIDER, &json!({ "id": id })).await
}
