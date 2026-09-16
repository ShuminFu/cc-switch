//! Generated from src/lib/api/sessions.ts by tools/gen_api.mjs.
//! Untyped payloads are `serde_json::Value` until the contract crate types them.
#![allow(dead_code, unused_imports, clippy::too_many_arguments)]

use std::collections::HashMap;

use cc_switch_contract::commands::sessions as cmd;
use cc_switch_contract::{AppId, AppSettings, IpcError, Provider, ProviderMeta};
use serde_json::json;

use crate::ipc;

pub async fn list() -> Result<Vec<serde_json::Value>, IpcError> {
    ipc::invoke_no_args(cmd::LIST_SESSIONS).await
}

pub async fn get_messages(
    provider_id: &str,
    source_path: &str,
) -> Result<Vec<serde_json::Value>, IpcError> {
    ipc::invoke(
        cmd::GET_SESSION_MESSAGES,
        &json!({ "providerId": provider_id, "sourcePath": source_path }),
    )
    .await
}

// TODO(port): delete(options: DeleteSessionOptions) -> boolean: invoke("delete_session", [["providerId","providerId"],["sessionId","sessionId"],["sourcePath","sourcePath"]])

pub async fn delete_many(
    items: &Vec<serde_json::Value>,
) -> Result<Vec<serde_json::Value>, IpcError> {
    ipc::invoke(cmd::DELETE_SESSIONS, &json!({ "items": items })).await
}

// TODO(port): launchTerminal(options: { command: string; cwd?: string | null; customConfig?: string | null; }) -> boolean: invoke("launch_session_terminal", [["command","command"],["cwd","cwd"],["customConfig","customConfig"]])
