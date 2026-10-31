//! MCP (Model Context Protocol) 服务器管理模块
//!
//! 本模块负责 MCP 服务器配置的验证、同步和导入导出。
//!
//! ## 模块结构
//!
//! - `validation` - 服务器配置验证
//! - `claude` - Claude MCP 同步和导入
//! - `codex` - Codex MCP 同步和导入（含 TOML 转换）
//! - `gemini` - Gemini MCP 同步和导入
//! - `opencode` - OpenCode MCP 同步和导入（含 local/remote 格式转换）
//! - `hermes` - Hermes MCP 同步和导入

mod claude;
mod codex;
mod gemini;
mod grokbuild;
mod hermes;
mod opencode;
mod validation;

use serde_json::Value;
use std::collections::HashMap;

use crate::app_config::McpConfig;
use validation::extract_server_spec;

/// 返回 legacy `McpConfig` 中已启用的 MCP 服务器（过滤 enabled==true），
/// 供各应用的 `sync_enabled_to_*` 兼容入口共用
pub(crate) fn collect_enabled_servers(cfg: &McpConfig) -> HashMap<String, Value> {
    let mut out = HashMap::new();
    for (id, entry) in cfg.servers.iter() {
        let enabled = entry
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !enabled {
            continue;
        }
        match extract_server_spec(entry) {
            Ok(spec) => {
                out.insert(id.clone(), spec);
            }
            Err(err) => {
                log::warn!("跳过无效的 MCP 条目 '{id}': {err}");
            }
        }
    }
    out
}

// 重新导出公共 API
pub use claude::{
    import_from_claude, project_to_claude, remove_server_from_claude, sync_enabled_to_claude,
    sync_single_server_to_claude,
};
pub use codex::{
    import_from_codex, remove_server_from_codex, sync_enabled_to_codex, sync_single_server_to_codex,
};
pub use gemini::{
    import_from_gemini, project_to_gemini, remove_server_from_gemini, sync_enabled_to_gemini,
    sync_single_server_to_gemini,
};
pub use grokbuild::{
    import_from_grokbuild, remove_server_from_grokbuild, sync_single_server_to_grokbuild,
};
pub use hermes::{import_from_hermes, remove_server_from_hermes, sync_single_server_to_hermes};
pub use opencode::{
    import_from_opencode, remove_server_from_opencode, sync_single_server_to_opencode,
};
pub use validation::{find_cmd_wrap_unsafe_arg, validate_server_spec};
