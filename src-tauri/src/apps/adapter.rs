//! 按应用划分的配置适配器
//!
//! 每个受管应用实现一个 [`AppAdapter`]，把"这个应用的配置放在哪、长什么样、
//! 怎么读写"收拢到 `apps/<app>/` 一个模块里。调用方通过
//! [`super::adapter`] 取得适配器后统一分派，不再各自 `match AppType`。
//!
//! 可选能力（MCP 投影、代理接管、会话扫描）以子 trait 表达，由
//! [`AppAdapter::mcp`] / [`AppAdapter::takeover`] / [`AppAdapter::session_source`]
//! 返回；不支持的应用返回 `None`。

use std::fmt::Debug;
use std::path::PathBuf;

use serde_json::Value;

use crate::error::AppError;
use crate::provider::Provider;

/// 供应商直连凭据
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credentials {
    pub api_key: String,
    pub base_url: String,
}

/// 单个应用的 live 配置适配器
pub trait AppAdapter: Debug + Sync {
    /// 应用配置目录（已考虑 settings.json 中的目录覆盖）
    fn config_dir(&self) -> PathBuf;

    /// 提示词文件路径
    fn prompt_file(&self) -> PathBuf;

    /// Skills 目录
    fn skills_dir(&self) -> PathBuf;

    /// 读取当前 live 配置，转换为供应商 `settings_config` 结构
    fn read_live(&self) -> Result<Value, AppError>;

    /// 首次导入时读取 live 配置作为 `default` 供应商的 `settings_config`
    fn import_live(&self) -> Result<Value, AppError>;

    /// 把供应商配置写入 live 文件
    fn write_provider(&self, provider: &Provider) -> Result<(), AppError>;

    /// 从 live 配置中移除供应商。
    ///
    /// 只有累加模式应用需要实现；切换模式应用的 live 只承载当前供应商，默认 no-op。
    fn remove_provider(&self, _provider_id: &str) -> Result<(), AppError> {
        Ok(())
    }

    /// 校验供应商 `settings_config` 的结构
    fn validate_provider(&self, provider: &Provider) -> Result<(), AppError>;

    /// 切走时把 live 配置回填到供应商之前，剥掉只属于 live 的投影内容
    fn backfill_live_settings(&self, _provider: &Provider, live_settings: Value) -> Value {
        live_settings
    }

    /// 从供应商配置中提取直连凭据
    fn credentials(&self, provider: &Provider) -> Result<Credentials, AppError>;

    /// 通用配置片段支持；不支持的应用返回 `None`
    fn common_snippet(&self) -> Option<&dyn CommonSnippet> {
        None
    }

    /// CC Switch 管理的 MCP 服务器投影；不支持的应用返回 `None`
    fn mcp(&self) -> Option<&dyn McpProjector> {
        None
    }

    /// 本地代理接管；不支持的应用返回 `None`
    fn takeover(&self) -> Option<&dyn ProxyTakeover> {
        None
    }

    /// 会话历史来源；不支持的应用返回 `None`
    fn session_source(&self) -> Option<&dyn SessionSource> {
        None
    }
}

/// 通用配置片段的提取 / 判定 / 合并 / 剥离。
///
/// `snippet` 参数均为调用方已去除首尾空白且非空的片段文本。
pub trait CommonSnippet {
    /// 从供应商配置中提取可共享的片段（剥掉凭据与端点）
    fn extract(&self, settings: &Value) -> Result<String, AppError>;

    /// `settings` 是否已包含整个片段
    fn contains(&self, settings: &Value, snippet: &str) -> bool;

    /// 把片段合并进 `settings`
    fn apply(&self, settings: &Value, snippet: &str) -> Result<Value, AppError>;

    /// 从 `settings` 中剥掉片段
    fn remove(&self, settings: &Value, snippet: &str) -> Result<Value, AppError>;
}

/// 把 CC Switch 的 MCP 服务器写入 / 移出应用自己的配置
pub trait McpProjector {
    fn upsert(&self, id: &str, spec: &Value) -> Result<(), AppError>;

    fn remove(&self, id: &str) -> Result<(), AppError>;
}

/// 本地代理接管 live 配置的应用侧约定
pub trait ProxyTakeover {
    /// live / 备份配置是否带有代理接管占位符
    fn has_placeholder(&self, settings: &Value) -> bool;
}

/// 会话历史扫描入口
pub trait SessionSource {
    /// 存放会话文件的根目录（删除会话时据此做路径校验）
    fn session_roots(&self) -> Vec<PathBuf>;
}
