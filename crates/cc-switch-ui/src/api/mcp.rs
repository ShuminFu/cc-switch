//! Generated from src/lib/api/mcp.ts by tools/gen_api.mjs.
//! Untyped payloads are `serde_json::Value` until the contract crate types them.
#![allow(dead_code, unused_imports, clippy::too_many_arguments)]

use std::collections::HashMap;

use cc_switch_contract::commands::mcp as cmd;
use cc_switch_contract::{AppId, AppSettings, IpcError, Provider, ProviderMeta};
use serde_json::json;

use crate::ipc;

pub async fn get_status() -> Result<serde_json::Value, IpcError> {
    ipc::invoke_no_args(cmd::GET_CLAUDE_MCP_STATUS).await
}

pub async fn read_config() -> Result<Option<String>, IpcError> {
    ipc::invoke_no_args(cmd::READ_CLAUDE_MCP_CONFIG).await
}

pub async fn upsert_server(id: &str, spec: &serde_json::Value) -> Result<bool, IpcError> {
    ipc::invoke(
        cmd::UPSERT_CLAUDE_MCP_SERVER,
        &json!({ "id": id, "spec": spec }),
    )
    .await
}

pub async fn delete_server(id: &str) -> Result<bool, IpcError> {
    ipc::invoke(cmd::DELETE_CLAUDE_MCP_SERVER, &json!({ "id": id })).await
}

pub async fn validate_command(cmd: &str) -> Result<bool, IpcError> {
    ipc::invoke(cmd::VALIDATE_MCP_COMMAND, &json!({ "cmd": cmd })).await
}

pub async fn get_config(app: Option<AppId>) -> Result<serde_json::Value, IpcError> {
    ipc::invoke(cmd::GET_MCP_CONFIG, &json!({ "app": app })).await
}

// TODO(port): upsertServerInConfig(app: AppId, id: string, spec: McpServer, options: { syncOtherSide?: boolean }) -> boolean: invoke("upsert_mcp_server_in_config", [["__raw","payload"]])

// TODO(port): deleteServerInConfig(app: AppId, id: string, options: { syncOtherSide?: boolean }) -> boolean: invoke("delete_mcp_server_in_config", [["__raw","payload"]])

pub async fn set_enabled(app: AppId, id: &str, enabled: bool) -> Result<bool, IpcError> {
    ipc::invoke(
        cmd::SET_MCP_ENABLED,
        &json!({ "app": app, "id": id, "enabled": enabled }),
    )
    .await
}

pub async fn get_all_servers() -> Result<serde_json::Value, IpcError> {
    ipc::invoke_no_args(cmd::GET_MCP_SERVERS).await
}

pub async fn upsert_unified_server(server: &serde_json::Value) -> Result<(), IpcError> {
    ipc::invoke(cmd::UPSERT_MCP_SERVER, &json!({ "server": server })).await
}

pub async fn delete_unified_server(id: &str) -> Result<bool, IpcError> {
    ipc::invoke(cmd::DELETE_MCP_SERVER, &json!({ "id": id })).await
}

pub async fn toggle_app(server_id: &str, app: AppId, enabled: bool) -> Result<(), IpcError> {
    ipc::invoke(
        cmd::TOGGLE_MCP_APP,
        &json!({ "serverId": server_id, "app": app, "enabled": enabled }),
    )
    .await
}

pub async fn import_from_apps() -> Result<f64, IpcError> {
    ipc::invoke_no_args(cmd::IMPORT_MCP_FROM_APPS).await
}
