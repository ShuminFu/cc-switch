//! Generated from src/lib/api/workspace.ts by tools/gen_api.mjs.
//! Untyped payloads are `serde_json::Value` until the contract crate types them.
#![allow(dead_code, unused_imports, clippy::too_many_arguments)]

use std::collections::HashMap;

use cc_switch_contract::commands::workspace as cmd;
use cc_switch_contract::{AppId, AppSettings, IpcError, Provider, ProviderMeta};
use serde_json::json;

use crate::ipc;

pub async fn read_file(filename: &str) -> Result<Option<String>, IpcError> {
    ipc::invoke(cmd::READ_WORKSPACE_FILE, &json!({ "filename": filename })).await
}

pub async fn write_file(filename: &str, content: &str) -> Result<(), IpcError> {
    ipc::invoke(
        cmd::WRITE_WORKSPACE_FILE,
        &json!({ "filename": filename, "content": content }),
    )
    .await
}

pub async fn list_daily_memory_files() -> Result<Vec<serde_json::Value>, IpcError> {
    ipc::invoke_no_args(cmd::LIST_DAILY_MEMORY_FILES).await
}

pub async fn read_daily_memory_file(filename: &str) -> Result<Option<String>, IpcError> {
    ipc::invoke(
        cmd::READ_DAILY_MEMORY_FILE,
        &json!({ "filename": filename }),
    )
    .await
}

pub async fn write_daily_memory_file(filename: &str, content: &str) -> Result<(), IpcError> {
    ipc::invoke(
        cmd::WRITE_DAILY_MEMORY_FILE,
        &json!({ "filename": filename, "content": content }),
    )
    .await
}

pub async fn delete_daily_memory_file(filename: &str) -> Result<(), IpcError> {
    ipc::invoke(
        cmd::DELETE_DAILY_MEMORY_FILE,
        &json!({ "filename": filename }),
    )
    .await
}

pub async fn search_daily_memory_files(query: &str) -> Result<Vec<serde_json::Value>, IpcError> {
    ipc::invoke(cmd::SEARCH_DAILY_MEMORY_FILES, &json!({ "query": query })).await
}

pub async fn open_directory(subdir: &str) -> Result<(), IpcError> {
    ipc::invoke(cmd::OPEN_WORKSPACE_DIRECTORY, &json!({ "subdir": subdir })).await
}
