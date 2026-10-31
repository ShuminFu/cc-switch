//! OpenCode typed provider config (CFG-11 split from `provider.rs`).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

// ============================================================================
// OpenCode 供应商配置结构
// ============================================================================

/// OpenCode 供应商的 settings_config 结构
///
/// OpenCode 使用 AI SDK 包名来指定供应商类型，与其他应用的配置格式不同。
/// 配置示例：
/// ```json
/// {
///   "npm": "@ai-sdk/openai-compatible",
///   "options": { "baseURL": "https://api.example.com/v1", "apiKey": "sk-xxx" },
///   "models": { "gpt-4o": { "name": "GPT-4o" } }
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenCodeProviderConfig {
    /// AI SDK 包名，如 "@ai-sdk/openai-compatible", "@ai-sdk/anthropic"
    pub npm: String,

    /// 供应商名称（可选，用于显示）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// 供应商选项（API 密钥、基础 URL 等）
    #[serde(default)]
    pub options: OpenCodeProviderOptions,

    /// 模型定义映射
    #[serde(default)]
    pub models: HashMap<String, OpenCodeModel>,
}

impl Default for OpenCodeProviderConfig {
    fn default() -> Self {
        Self {
            npm: "@ai-sdk/openai-compatible".to_string(),
            name: None,
            options: OpenCodeProviderOptions::default(),
            models: HashMap::new(),
        }
    }
}

/// OpenCode 供应商选项
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpenCodeProviderOptions {
    /// API 基础 URL
    #[serde(rename = "baseURL", skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,

    /// API 密钥（支持环境变量引用，如 "{env:API_KEY}"）
    #[serde(rename = "apiKey", skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// 自定义请求头
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,

    /// 额外选项（timeout, setCacheKey 等）
    /// 使用 flatten 捕获所有未明确定义的字段
    #[serde(flatten, default, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, Value>,
}

/// OpenCode 模型定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenCodeModel {
    /// 模型显示名称
    pub name: String,

    /// 模型限制（上下文和输出 token 数）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<OpenCodeModelLimit>,

    /// 模型额外选项（provider 路由等）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<HashMap<String, Value>>,

    /// 额外字段（cost、modalities、thinking、variants 等）
    /// 使用 flatten 捕获所有未明确定义的字段
    #[serde(flatten, default, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, Value>,
}

/// OpenCode 模型限制
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpenCodeModelLimit {
    /// 上下文 token 限制
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<u64>,

    /// 输出 token 限制
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<u64>,
}
