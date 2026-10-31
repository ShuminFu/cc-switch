//! Handler 配置模块
//!
//! 定义各 API 处理器的配置结构和使用量解析器

use crate::proxy::usage::parser::TokenUsage;
use serde_json::Value;

/// 使用量解析器类型别名
pub type StreamUsageParser = fn(&[Value]) -> Option<TokenUsage>;
pub type ResponseUsageParser = fn(&Value) -> Option<TokenUsage>;

/// 模型提取器类型别名
/// 参数: (流式事件列表, 请求中的模型名称) -> 最终使用的模型名称
pub type StreamModelExtractor = fn(&[Value], &str) -> String;

/// 流式 usage 事件预过滤器类型别名。
///
/// 参数是 SSE `data:` 原始字符串。返回 false 时跳过 JSON parse，避免在
/// token/chunk 高频路径上解析与 usage 无关的事件。
pub type StreamUsageEventFilter = fn(&str) -> bool;

/// 流式 usage 收集器保留哪些已解析事件（PERF-07）。
///
/// 解析器只需要「最后一个 usage 事件」与「首个带模型名的事件」时，没有必要把
/// 整条流的每个 chunk 都以 `serde_json::Value` 形式留到流结束。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamEventRetention {
    /// 保留全部被过滤器选中的事件（事件少或解析器依赖顺序语义时使用）。
    All,
    /// 只保留首个带 `model_key` 的事件与最后一个 usage 事件（至多两个）。
    LastUsage {
        /// usage 所在字段（OpenAI: `usage`，Gemini: `usageMetadata`）
        usage_key: &'static str,
        /// 为 true 时 `usage: null` 不算 usage 事件（OpenAI 每个 chunk 都带 null）
        ignore_null_usage: bool,
        /// 模型名字段（OpenAI: `model`，Gemini: `modelVersion`）
        model_key: &'static str,
    },
}

/// 流式响应在中途失败时，向客户端补发的协议终止错误事件格式（PROXY-09）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamErrorFormat {
    /// Anthropic Messages：`event: error`
    Anthropic,
    /// OpenAI Chat Completions / Gemini：`data: {"error": {...}}`
    OpenAiChat,
    /// OpenAI Responses：`event: response.failed`
    Responses,
}

/// 各 API 的使用量解析配置
#[derive(Clone, Copy)]
pub struct UsageParserConfig {
    /// 流式响应解析器
    pub stream_parser: StreamUsageParser,
    /// 非流式响应解析器
    pub response_parser: ResponseUsageParser,
    /// 流式响应中的模型提取器
    pub model_extractor: StreamModelExtractor,
    /// 流式 usage 事件预过滤器
    pub stream_event_filter: Option<StreamUsageEventFilter>,
    /// 流式 usage 事件保留策略
    pub stream_event_retention: StreamEventRetention,
    /// 流中途失败时补发的终止错误事件格式
    pub stream_error_format: StreamErrorFormat,
    /// 应用类型字符串（用于日志记录）
    pub app_type_str: &'static str,
}

// ============================================================================
// 流式 usage 事件预过滤
// ============================================================================

pub fn claude_stream_usage_event_filter(data: &str) -> bool {
    data.contains("\"message_start\"") || data.contains("\"message_delta\"")
}

fn openai_stream_usage_event_filter(data: &str) -> bool {
    data.contains("\"usage\"")
}

pub fn codex_stream_usage_event_filter(data: &str) -> bool {
    data.contains("\"response.completed\"") || data.contains("\"usage\"")
}

fn gemini_stream_usage_event_filter(data: &str) -> bool {
    data.contains("\"usageMetadata\"")
}

// ============================================================================
// 模型提取器实现
// ============================================================================

/// Claude 流式响应模型提取（优先使用 usage.model）
///
/// 空字符串模型名视为缺失（转换层对无回显上游会合成 model:""），
/// 落到 fallback_model（映射后的出站模型或客户端请求模型）。
fn claude_model_extractor(events: &[Value], fallback_model: &str) -> String {
    // 首先尝试从解析的 usage 中获取模型
    if let Some(usage) = TokenUsage::from_claude_stream_events(events) {
        if let Some(model) = usage.model.filter(|m| !m.is_empty()) {
            return model;
        }
    }
    fallback_model.to_string()
}

/// OpenAI Chat Completions 流式响应模型提取（优先使用 usage.model）
fn openai_model_extractor(events: &[Value], fallback_model: &str) -> String {
    // 首先尝试从解析的 usage 中获取模型
    if let Some(usage) = TokenUsage::from_openai_stream_events(events) {
        if let Some(model) = usage.model.filter(|m| !m.is_empty()) {
            return model;
        }
    }
    // 回退：从事件中直接提取
    events
        .iter()
        .find_map(|e| e.get("model")?.as_str().filter(|m| !m.is_empty()))
        .unwrap_or(fallback_model)
        .to_string()
}

/// Codex 智能流式响应模型提取（自动检测格式）
fn codex_auto_model_extractor(events: &[Value], fallback_model: &str) -> String {
    // 首先尝试从解析的 usage 中获取模型
    if let Some(usage) = TokenUsage::from_codex_stream_events_auto(events) {
        if let Some(model) = usage.model.filter(|m| !m.is_empty()) {
            return model;
        }
    }
    // 回退：从 response.completed 事件中提取
    events
        .iter()
        .find_map(|e| {
            if e.get("type")?.as_str()? == "response.completed" {
                e.get("response")?
                    .get("model")?
                    .as_str()
                    .filter(|m| !m.is_empty())
            } else {
                None
            }
        })
        .or_else(|| {
            // 再回退：从 OpenAI 格式事件中提取
            events
                .iter()
                .find_map(|e| e.get("model")?.as_str().filter(|m| !m.is_empty()))
        })
        .unwrap_or(fallback_model)
        .to_string()
}

/// Gemini 流式响应模型提取（优先使用 usage.model）
fn gemini_model_extractor(events: &[Value], fallback_model: &str) -> String {
    // 首先尝试从解析的 usage 中获取模型
    if let Some(usage) = TokenUsage::from_gemini_stream_chunks(events) {
        if let Some(model) = usage.model.filter(|m| !m.is_empty()) {
            return model;
        }
    }
    fallback_model.to_string()
}

// ============================================================================
// 预定义配置
// ============================================================================

/// Claude API 解析配置
pub const CLAUDE_PARSER_CONFIG: UsageParserConfig = UsageParserConfig {
    stream_parser: TokenUsage::from_claude_stream_events,
    response_parser: TokenUsage::from_claude_response,
    model_extractor: claude_model_extractor,
    stream_event_filter: Some(claude_stream_usage_event_filter),
    stream_event_retention: StreamEventRetention::All,
    stream_error_format: StreamErrorFormat::Anthropic,
    app_type_str: "claude",
};

/// OpenAI Chat Completions API 解析配置（用于 Codex /v1/chat/completions）
pub const OPENAI_PARSER_CONFIG: UsageParserConfig = UsageParserConfig {
    stream_parser: TokenUsage::from_openai_stream_events,
    response_parser: TokenUsage::from_openai_response,
    model_extractor: openai_model_extractor,
    stream_event_filter: Some(openai_stream_usage_event_filter),
    stream_event_retention: StreamEventRetention::LastUsage {
        usage_key: "usage",
        ignore_null_usage: true,
        model_key: "model",
    },
    stream_error_format: StreamErrorFormat::OpenAiChat,
    app_type_str: "codex",
};

/// Codex 智能解析配置（自动检测 OpenAI 或 Codex 格式）
pub const CODEX_PARSER_CONFIG: UsageParserConfig = UsageParserConfig {
    stream_parser: TokenUsage::from_codex_stream_events_auto,
    response_parser: TokenUsage::from_codex_response_auto,
    model_extractor: codex_auto_model_extractor,
    stream_event_filter: Some(codex_stream_usage_event_filter),
    stream_event_retention: StreamEventRetention::All,
    stream_error_format: StreamErrorFormat::Responses,
    app_type_str: "codex",
};

/// Gemini API 解析配置
pub const GEMINI_PARSER_CONFIG: UsageParserConfig = UsageParserConfig {
    stream_parser: TokenUsage::from_gemini_stream_chunks,
    response_parser: TokenUsage::from_gemini_response,
    model_extractor: gemini_model_extractor,
    stream_event_filter: Some(gemini_stream_usage_event_filter),
    stream_event_retention: StreamEventRetention::LastUsage {
        usage_key: "usageMetadata",
        ignore_null_usage: false,
        model_key: "modelVersion",
    },
    stream_error_format: StreamErrorFormat::OpenAiChat,
    app_type_str: "gemini",
};
