//! Generated from src/lib/api/failover.ts by tools/gen_api.mjs.
//! Untyped payloads are `serde_json::Value` until the contract crate types them.
#![allow(dead_code, unused_imports, clippy::too_many_arguments)]

use std::collections::HashMap;

use cc_switch_contract::commands::failover as cmd;
use cc_switch_contract::{AppId, AppSettings, IpcError, Provider, ProviderMeta};
use serde_json::json;

use crate::ipc;

pub async fn get_provider_health(
    provider_id: &str,
    app_type: &str,
) -> Result<serde_json::Value, IpcError> {
    ipc::invoke(
        cmd::GET_PROVIDER_HEALTH,
        &json!({ "providerId": provider_id, "appType": app_type }),
    )
    .await
}

pub async fn reset_circuit_breaker(provider_id: &str, app_type: &str) -> Result<(), IpcError> {
    ipc::invoke(
        cmd::RESET_CIRCUIT_BREAKER,
        &json!({ "providerId": provider_id, "appType": app_type }),
    )
    .await
}

pub async fn get_circuit_breaker_config() -> Result<serde_json::Value, IpcError> {
    ipc::invoke_no_args(cmd::GET_CIRCUIT_BREAKER_CONFIG).await
}

pub async fn update_circuit_breaker_config(config: &serde_json::Value) -> Result<(), IpcError> {
    ipc::invoke(
        cmd::UPDATE_CIRCUIT_BREAKER_CONFIG,
        &json!({ "config": config }),
    )
    .await
}

pub async fn get_circuit_breaker_stats(
    provider_id: &str,
    app_type: &str,
) -> Result<Option<serde_json::Value>, IpcError> {
    ipc::invoke(
        cmd::GET_CIRCUIT_BREAKER_STATS,
        &json!({ "providerId": provider_id, "appType": app_type }),
    )
    .await
}

pub async fn get_failover_queue(app_type: &str) -> Result<Vec<serde_json::Value>, IpcError> {
    ipc::invoke(cmd::GET_FAILOVER_QUEUE, &json!({ "appType": app_type })).await
}

pub async fn get_available_providers_for_failover(
    app_type: &str,
) -> Result<Vec<Provider>, IpcError> {
    ipc::invoke(
        cmd::GET_AVAILABLE_PROVIDERS_FOR_FAILOVER,
        &json!({ "appType": app_type }),
    )
    .await
}

pub async fn add_to_failover_queue(app_type: &str, provider_id: &str) -> Result<(), IpcError> {
    ipc::invoke(
        cmd::ADD_TO_FAILOVER_QUEUE,
        &json!({ "appType": app_type, "providerId": provider_id }),
    )
    .await
}

pub async fn remove_from_failover_queue(app_type: &str, provider_id: &str) -> Result<(), IpcError> {
    ipc::invoke(
        cmd::REMOVE_FROM_FAILOVER_QUEUE,
        &json!({ "appType": app_type, "providerId": provider_id }),
    )
    .await
}

pub async fn get_auto_failover_enabled(app_type: &str) -> Result<bool, IpcError> {
    ipc::invoke(
        cmd::GET_AUTO_FAILOVER_ENABLED,
        &json!({ "appType": app_type }),
    )
    .await
}

pub async fn set_auto_failover_enabled(app_type: &str, enabled: bool) -> Result<(), IpcError> {
    ipc::invoke(
        cmd::SET_AUTO_FAILOVER_ENABLED,
        &json!({ "appType": app_type, "enabled": enabled }),
    )
    .await
}
