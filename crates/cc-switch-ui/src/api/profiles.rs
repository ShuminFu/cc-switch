//! Generated from src/lib/api/profiles.ts by tools/gen_api.mjs.
//! Untyped payloads are `serde_json::Value` until the contract crate types them.
#![allow(dead_code, unused_imports, clippy::too_many_arguments)]

use std::collections::HashMap;

use cc_switch_contract::commands::profiles as cmd;
use cc_switch_contract::{AppId, AppSettings, IpcError, Provider, ProviderMeta};
use serde_json::json;

use crate::ipc;

pub async fn list() -> Result<serde_json::Value, IpcError> {
    ipc::invoke_no_args(cmd::LIST_PROFILES).await
}

pub async fn create(name: &str, scope: &serde_json::Value) -> Result<serde_json::Value, IpcError> {
    ipc::invoke(
        cmd::CREATE_PROFILE,
        &json!({ "name": name, "scope": scope }),
    )
    .await
}

// TODO(port): update(id: string, options: { name?: string; resnapshot?: boolean; scope?: ProfileScope }) -> Profile: invoke("update_profile", [["id","id"],["name","options.name"],["resnapshot","options.resnapshot"],["scope","options.scope"]])

pub async fn delete(id: &str) -> Result<(), IpcError> {
    ipc::invoke(cmd::DELETE_PROFILE, &json!({ "id": id })).await
}

pub async fn apply(id: &str, scope: &serde_json::Value) -> Result<Vec<String>, IpcError> {
    ipc::invoke(cmd::APPLY_PROFILE, &json!({ "id": id, "scope": scope })).await
}

pub async fn clear_current(scope: &serde_json::Value) -> Result<(), IpcError> {
    ipc::invoke(cmd::CLEAR_CURRENT_PROFILE, &json!({ "scope": scope })).await
}
