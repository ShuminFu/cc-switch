//! Generated from src/lib/api/model-fetch.ts by tools/gen_api.mjs.
//! Untyped payloads are `serde_json::Value` until the contract crate types them.
#![allow(dead_code, unused_imports, clippy::too_many_arguments)]

use std::collections::HashMap;

use cc_switch_contract::commands::model_fetch as cmd;
use cc_switch_contract::{AppId, AppSettings, IpcError, Provider, ProviderMeta};
use serde_json::json;

use crate::ipc;
