//! Generated from src/lib/api/vscode.ts by tools/gen_api.mjs.
//! Untyped payloads are `serde_json::Value` until the contract crate types them.
#![allow(dead_code, unused_imports, clippy::too_many_arguments)]

use std::collections::HashMap;

use cc_switch_contract::commands::vscode as cmd;
use cc_switch_contract::{AppId, AppSettings, IpcError, Provider, ProviderMeta};
use serde_json::json;

use crate::ipc;

pub async fn get_live_provider_settings(app_id: AppId) -> Result<serde_json::Value, IpcError> {
    ipc::invoke(cmd::READ_LIVE_PROVIDER_SETTINGS, &json!({ "app": app_id })).await
}

// TODO(port): testApiEndpoints(urls: string[], options: { timeoutSecs?: number }) -> EndpointLatencyResult[]: invoke("test_api_endpoints", [["urls","urls"],["timeoutSecs","options?.timeoutSecs"]])

pub async fn get_custom_endpoints(
    app_id: AppId,
    provider_id: &str,
) -> Result<Vec<serde_json::Value>, IpcError> {
    ipc::invoke(
        cmd::GET_CUSTOM_ENDPOINTS,
        &json!({ "app": app_id, "providerId": provider_id }),
    )
    .await
}

pub async fn add_custom_endpoint(
    app_id: AppId,
    provider_id: &str,
    url: &str,
) -> Result<(), IpcError> {
    ipc::invoke(
        cmd::ADD_CUSTOM_ENDPOINT,
        &json!({ "app": app_id, "providerId": provider_id, "url": url }),
    )
    .await
}

pub async fn remove_custom_endpoint(
    app_id: AppId,
    provider_id: &str,
    url: &str,
) -> Result<(), IpcError> {
    ipc::invoke(
        cmd::REMOVE_CUSTOM_ENDPOINT,
        &json!({ "app": app_id, "providerId": provider_id, "url": url }),
    )
    .await
}

pub async fn update_endpoint_last_used(
    app_id: AppId,
    provider_id: &str,
    url: &str,
) -> Result<(), IpcError> {
    ipc::invoke(
        cmd::UPDATE_ENDPOINT_LAST_USED,
        &json!({ "app": app_id, "providerId": provider_id, "url": url }),
    )
    .await
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
