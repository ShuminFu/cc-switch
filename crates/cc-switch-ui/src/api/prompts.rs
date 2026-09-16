//! Generated from src/lib/api/prompts.ts by tools/gen_api.mjs.
//! Untyped payloads are `serde_json::Value` until the contract crate types them.
#![allow(dead_code, unused_imports, clippy::too_many_arguments)]

use std::collections::HashMap;

use cc_switch_contract::commands::prompts as cmd;
use cc_switch_contract::{AppId, AppSettings, IpcError, Provider, ProviderMeta};
use serde_json::json;

use crate::ipc;

pub async fn get_prompts(app: AppId) -> Result<HashMap<String, serde_json::Value>, IpcError> {
    ipc::invoke(cmd::GET_PROMPTS, &json!({ "app": app })).await
}

pub async fn upsert_prompt(
    app: AppId,
    id: &str,
    prompt: &serde_json::Value,
) -> Result<(), IpcError> {
    ipc::invoke(
        cmd::UPSERT_PROMPT,
        &json!({ "app": app, "id": id, "prompt": prompt }),
    )
    .await
}

pub async fn delete_prompt(app: AppId, id: &str) -> Result<(), IpcError> {
    ipc::invoke(cmd::DELETE_PROMPT, &json!({ "app": app, "id": id })).await
}

pub async fn enable_prompt(app: AppId, id: &str) -> Result<(), IpcError> {
    ipc::invoke(cmd::ENABLE_PROMPT, &json!({ "app": app, "id": id })).await
}

pub async fn import_from_file(app: AppId) -> Result<String, IpcError> {
    ipc::invoke(cmd::IMPORT_PROMPT_FROM_FILE, &json!({ "app": app })).await
}

pub async fn get_current_file_content(app: AppId) -> Result<Option<String>, IpcError> {
    ipc::invoke(cmd::GET_CURRENT_PROMPT_FILE_CONTENT, &json!({ "app": app })).await
}
