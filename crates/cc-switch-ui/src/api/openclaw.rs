//! Generated from src/lib/api/openclaw.ts by tools/gen_api.mjs.
//! Untyped payloads are `serde_json::Value` until the contract crate types them.
#![allow(dead_code, unused_imports, clippy::too_many_arguments)]

use std::collections::HashMap;

use cc_switch_contract::commands::openclaw as cmd;
use cc_switch_contract::{AppId, AppSettings, IpcError, Provider, ProviderMeta};
use serde_json::json;

use crate::ipc;

pub async fn get_default_model() -> Result<Option<serde_json::Value>, IpcError> {
    ipc::invoke_no_args(cmd::GET_OPENCLAW_DEFAULT_MODEL).await
}

pub async fn set_default_model(model: &serde_json::Value) -> Result<serde_json::Value, IpcError> {
    ipc::invoke(cmd::SET_OPENCLAW_DEFAULT_MODEL, &json!({ "model": model })).await
}

pub async fn get_model_catalog() -> Result<Option<serde_json::Value>, IpcError> {
    ipc::invoke_no_args(cmd::GET_OPENCLAW_MODEL_CATALOG).await
}

pub async fn set_model_catalog(
    catalog: &HashMap<String, serde_json::Value>,
) -> Result<serde_json::Value, IpcError> {
    ipc::invoke(
        cmd::SET_OPENCLAW_MODEL_CATALOG,
        &json!({ "catalog": catalog }),
    )
    .await
}

pub async fn get_agents_defaults() -> Result<Option<serde_json::Value>, IpcError> {
    ipc::invoke_no_args(cmd::GET_OPENCLAW_AGENTS_DEFAULTS).await
}

pub async fn set_agents_defaults(
    defaults: &serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    ipc::invoke(
        cmd::SET_OPENCLAW_AGENTS_DEFAULTS,
        &json!({ "defaults": defaults }),
    )
    .await
}

pub async fn get_env() -> Result<serde_json::Value, IpcError> {
    ipc::invoke_no_args(cmd::GET_OPENCLAW_ENV).await
}

pub async fn set_env(env: &serde_json::Value) -> Result<serde_json::Value, IpcError> {
    ipc::invoke(cmd::SET_OPENCLAW_ENV, &json!({ "env": env })).await
}

pub async fn get_tools() -> Result<serde_json::Value, IpcError> {
    ipc::invoke_no_args(cmd::GET_OPENCLAW_TOOLS).await
}

pub async fn set_tools(tools: &serde_json::Value) -> Result<serde_json::Value, IpcError> {
    ipc::invoke(cmd::SET_OPENCLAW_TOOLS, &json!({ "tools": tools })).await
}

pub async fn scan_health() -> Result<Vec<serde_json::Value>, IpcError> {
    ipc::invoke_no_args(cmd::SCAN_OPENCLAW_CONFIG_HEALTH).await
}

pub async fn get_live_provider(
    provider_id: &str,
) -> Result<Option<HashMap<String, serde_json::Value>>, IpcError> {
    ipc::invoke(
        cmd::GET_OPENCLAW_LIVE_PROVIDER,
        &json!({ "providerId": provider_id }),
    )
    .await
}
