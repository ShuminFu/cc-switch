//! Generated from src/lib/api/hermes.ts by tools/gen_api.mjs.
//! Untyped payloads are `serde_json::Value` until the contract crate types them.
#![allow(dead_code, unused_imports, clippy::too_many_arguments)]

use std::collections::HashMap;

use cc_switch_contract::commands::hermes as cmd;
use cc_switch_contract::{AppId, AppSettings, IpcError, Provider, ProviderMeta};
use serde_json::json;

use crate::ipc;

pub async fn get_model_config() -> Result<Option<serde_json::Value>, IpcError> {
    ipc::invoke_no_args(cmd::GET_HERMES_MODEL_CONFIG).await
}

// TODO(port): openWebUI(path: string) -> void: invoke("open_hermes_web_ui", [["path","path ?? null"]])

pub async fn launch_dashboard() -> Result<(), IpcError> {
    ipc::invoke_no_args(cmd::LAUNCH_HERMES_DASHBOARD).await
}

pub async fn get_memory(kind: &serde_json::Value) -> Result<String, IpcError> {
    ipc::invoke(cmd::GET_HERMES_MEMORY, &json!({ "kind": kind })).await
}

pub async fn set_memory(kind: &serde_json::Value, content: &str) -> Result<(), IpcError> {
    ipc::invoke(
        cmd::SET_HERMES_MEMORY,
        &json!({ "kind": kind, "content": content }),
    )
    .await
}

pub async fn get_memory_limits() -> Result<serde_json::Value, IpcError> {
    ipc::invoke_no_args(cmd::GET_HERMES_MEMORY_LIMITS).await
}

pub async fn set_memory_enabled(kind: &serde_json::Value, enabled: bool) -> Result<(), IpcError> {
    ipc::invoke(
        cmd::SET_HERMES_MEMORY_ENABLED,
        &json!({ "kind": kind, "enabled": enabled }),
    )
    .await
}
