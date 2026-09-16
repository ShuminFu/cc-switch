//! Generated from src/lib/api/proxy.ts by tools/gen_api.mjs.
//! Untyped payloads are `serde_json::Value` until the contract crate types them.
#![allow(dead_code, unused_imports, clippy::too_many_arguments)]

use std::collections::HashMap;

use cc_switch_contract::commands::proxy as cmd;
use cc_switch_contract::{AppId, AppSettings, IpcError, Provider, ProviderMeta};
use serde_json::json;

use crate::ipc;

pub async fn start_proxy_server() -> Result<serde_json::Value, IpcError> {
    ipc::invoke_no_args(cmd::START_PROXY_SERVER).await
}

pub async fn stop_proxy_with_restore() -> Result<(), IpcError> {
    ipc::invoke_no_args(cmd::STOP_PROXY_WITH_RESTORE).await
}

pub async fn get_proxy_status() -> Result<serde_json::Value, IpcError> {
    ipc::invoke_no_args(cmd::GET_PROXY_STATUS).await
}

pub async fn is_proxy_running() -> Result<bool, IpcError> {
    ipc::invoke_no_args(cmd::IS_PROXY_RUNNING).await
}

pub async fn is_live_takeover_active() -> Result<bool, IpcError> {
    ipc::invoke_no_args(cmd::IS_LIVE_TAKEOVER_ACTIVE).await
}

pub async fn switch_proxy_provider(app_type: &str, provider_id: &str) -> Result<(), IpcError> {
    ipc::invoke(
        cmd::SWITCH_PROXY_PROVIDER,
        &json!({ "appType": app_type, "providerId": provider_id }),
    )
    .await
}

pub async fn get_proxy_takeover_status() -> Result<serde_json::Value, IpcError> {
    ipc::invoke_no_args(cmd::GET_PROXY_TAKEOVER_STATUS).await
}

pub async fn set_proxy_takeover_for_app(app_type: &str, enabled: bool) -> Result<(), IpcError> {
    ipc::invoke(
        cmd::SET_PROXY_TAKEOVER_FOR_APP,
        &json!({ "appType": app_type, "enabled": enabled }),
    )
    .await
}

pub async fn get_proxy_config() -> Result<serde_json::Value, IpcError> {
    ipc::invoke_no_args(cmd::GET_PROXY_CONFIG).await
}

pub async fn update_proxy_config(config: &serde_json::Value) -> Result<(), IpcError> {
    ipc::invoke(cmd::UPDATE_PROXY_CONFIG, &json!({ "config": config })).await
}

pub async fn get_global_proxy_config() -> Result<serde_json::Value, IpcError> {
    ipc::invoke_no_args(cmd::GET_GLOBAL_PROXY_CONFIG).await
}

pub async fn update_global_proxy_config(config: &serde_json::Value) -> Result<(), IpcError> {
    ipc::invoke(
        cmd::UPDATE_GLOBAL_PROXY_CONFIG,
        &json!({ "config": config }),
    )
    .await
}

pub async fn get_proxy_config_for_app(app_type: &str) -> Result<serde_json::Value, IpcError> {
    ipc::invoke(
        cmd::GET_PROXY_CONFIG_FOR_APP,
        &json!({ "appType": app_type }),
    )
    .await
}

pub async fn update_proxy_config_for_app(config: &serde_json::Value) -> Result<(), IpcError> {
    ipc::invoke(
        cmd::UPDATE_PROXY_CONFIG_FOR_APP,
        &json!({ "config": config }),
    )
    .await
}

pub async fn get_default_cost_multiplier(app_type: &str) -> Result<String, IpcError> {
    ipc::invoke(
        cmd::GET_DEFAULT_COST_MULTIPLIER,
        &json!({ "appType": app_type }),
    )
    .await
}

pub async fn set_default_cost_multiplier(app_type: &str, value: &str) -> Result<(), IpcError> {
    ipc::invoke(
        cmd::SET_DEFAULT_COST_MULTIPLIER,
        &json!({ "appType": app_type, "value": value }),
    )
    .await
}

pub async fn get_pricing_model_source(app_type: &str) -> Result<String, IpcError> {
    ipc::invoke(
        cmd::GET_PRICING_MODEL_SOURCE,
        &json!({ "appType": app_type }),
    )
    .await
}

pub async fn set_pricing_model_source(app_type: &str, value: &str) -> Result<(), IpcError> {
    ipc::invoke(
        cmd::SET_PRICING_MODEL_SOURCE,
        &json!({ "appType": app_type, "value": value }),
    )
    .await
}
