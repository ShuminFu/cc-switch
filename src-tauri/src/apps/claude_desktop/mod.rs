//! Claude Desktop 适配器（3P 配置库中的 CC Switch profile）

pub mod config;

use std::path::PathBuf;

use serde_json::Value;

use super::adapter::{AppAdapter, Credentials, UpstreamProtocol};
use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;

#[derive(Debug)]
pub struct ClaudeDesktopAdapter;

impl AppAdapter for ClaudeDesktopAdapter {
    fn config_dir(&self) -> Result<PathBuf, AppError> {
        config::get_config_library_path()
    }

    /// Claude Desktop 没有提示词文件
    fn prompt_file(&self) -> Option<PathBuf> {
        None
    }

    /// Claude Desktop 没有 skills 目录
    fn skills_dir(&self) -> Option<PathBuf> {
        None
    }

    fn live_files(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    fn read_live(&self) -> Result<Value, AppError> {
        Err(AppError::localized(
            "claude_desktop.live.read_unsupported",
            "Claude Desktop 3P 配置不支持作为通用 live 配置导入，请使用“从 Claude 导入兼容供应商”。",
            "Claude Desktop 3P configuration cannot be imported as a generic live config. Use 'Import compatible providers from Claude' instead.",
        ))
    }

    fn import_live(&self) -> Result<Value, AppError> {
        Err(AppError::localized(
            "claude_desktop.import_unsupported",
            "Claude Desktop 3P 配置不能通过通用导入读取，请使用“从 Claude 导入兼容供应商”。",
            "Claude Desktop 3P config cannot be imported through the generic import flow. Use 'Import compatible providers from Claude' instead.",
        ))
    }

    /// 3P profile 需要数据库中的网关令牌，只能经 [`Self::write_effective_provider`] 写入
    fn write_provider(&self, _provider: &Provider) -> Result<(), AppError> {
        Err(AppError::localized(
            "claude_desktop.live.requires_db_context",
            "Claude Desktop 配置写入需要通过供应商切换流程执行",
            "Claude Desktop configuration must be written through the provider switch flow",
        ))
    }

    fn write_effective_provider(&self, db: &Database, provider: &Provider) -> Result<(), AppError> {
        config::apply_provider(db, provider)?;
        log::info!(
            "Claude Desktop 3P profile '{}' written for provider '{}'",
            config::PROFILE_ID,
            provider.id
        );
        Ok(())
    }

    fn validate_provider(&self, provider: &Provider) -> Result<(), AppError> {
        config::validate_provider(provider)
    }

    /// 3P profile 不经过代理接管备份，始终直接写入
    fn takeover_uses_live_backup(&self) -> bool {
        false
    }

    /// Anthropic 风格的 env，保留 JS 脚本路径依赖的 OpenRouter / Google key 兜底
    fn usage_credentials(&self, settings: &Value) -> (String, String) {
        super::claude::anthropic_usage_credentials(settings)
    }

    fn upstream_protocol(&self) -> UpstreamProtocol {
        UpstreamProtocol::Anthropic
    }

    fn env_base_url_key(&self) -> Option<&'static str> {
        Some("ANTHROPIC_BASE_URL")
    }

    fn credentials(&self, provider: &Provider) -> Result<Credentials, AppError> {
        let credentials = config::direct_gateway_credentials(provider)?;
        Ok(Credentials {
            api_key: credentials.api_key,
            base_url: credentials.base_url,
        })
    }
}
