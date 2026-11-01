//! Claude Desktop 3P profiles. Writes need the database (gateway token, proxy
//! port), so they go through the provider switch flow, not this adapter.

use std::path::PathBuf;

use serde_json::Value;

use super::{AdapterCapabilities, AppAdapter, CommonConfigShape, WriteMode};
use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::Provider;

pub(super) struct ClaudeDesktopAdapter;

impl AppAdapter for ClaudeDesktopAdapter {
    fn app(&self) -> AppType {
        AppType::ClaudeDesktop
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            mode: WriteMode::Exclusive,
            generic_live_read: false,
            mcp: false,
            proxy_takeover: false,
            common_config: CommonConfigShape::None,
        }
    }

    fn live_paths(&self) -> Vec<PathBuf> {
        crate::claude_desktop_config::get_config_library_path()
            .map(|path| vec![path])
            .unwrap_or_default()
    }

    fn read_live(&self) -> Result<Value, AppError> {
        Err(AppError::localized(
            "claude_desktop.live.read_unsupported",
            "Claude Desktop 3P 配置不支持作为通用 live 配置导入，请使用“从 Claude 导入兼容供应商”。",
            "Claude Desktop 3P configuration cannot be imported as a generic live config. Use 'Import compatible providers from Claude' instead.",
        ))
    }

    fn write_live(&self, _provider: &Provider) -> Result<(), AppError> {
        Err(AppError::localized(
            "claude_desktop.live.requires_db_context",
            "Claude Desktop 配置写入需要通过供应商切换流程执行",
            "Claude Desktop configuration must be written through the provider switch flow",
        ))
    }

    fn import_live(&self) -> Result<Value, AppError> {
        Err(AppError::localized(
            "claude_desktop.import_unsupported",
            "Claude Desktop 3P 配置不能通过通用导入读取，请使用“从 Claude 导入兼容供应商”。",
            "Claude Desktop 3P config cannot be imported through the generic import flow. Use 'Import compatible providers from Claude' instead.",
        ))
    }
}
