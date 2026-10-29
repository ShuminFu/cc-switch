//! Handler 配置模块
//!
//! 定义各 API 处理器的配置结构和使用量解析器

use crate::proxy::protocol::WireFormat;
use crate::proxy::stream::StreamUsageFormat;
use crate::proxy::usage::parser::TokenUsage;
use serde_json::Value;

/// 使用量解析器类型别名
pub type ResponseUsageParser = fn(&Value) -> Option<TokenUsage>;

/// 流式 usage 事件预过滤器类型别名。
///
/// 参数是 SSE `data:` 原始字符串。返回 false 时跳过 JSON parse，避免在
/// token/chunk 高频路径上解析与 usage 无关的事件。
pub type StreamUsageEventFilter = fn(&str) -> bool;

/// 各 API 的使用量解析配置
#[derive(Clone, Copy)]
pub struct UsageParserConfig {
    /// 流式 usage 的事件格式（由 `UsageTap` 逐事件累计 usage 与归因模型）
    pub stream_usage: StreamUsageFormat,
    /// 非流式响应解析器
    pub response_parser: ResponseUsageParser,
    /// 流式 usage 事件预过滤器
    pub stream_event_filter: Option<StreamUsageEventFilter>,
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
// 预定义配置
// ============================================================================

/// Claude API 解析配置
pub const CLAUDE_PARSER_CONFIG: UsageParserConfig = UsageParserConfig {
    stream_usage: StreamUsageFormat::Anthropic,
    response_parser: TokenUsage::from_claude_response,
    stream_event_filter: Some(claude_stream_usage_event_filter),
    app_type_str: "claude",
};

/// OpenAI Chat Completions API 解析配置（用于 Codex /v1/chat/completions）
pub const OPENAI_PARSER_CONFIG: UsageParserConfig = UsageParserConfig {
    stream_usage: StreamUsageFormat::OpenAiChat,
    response_parser: TokenUsage::from_openai_response,
    stream_event_filter: Some(openai_stream_usage_event_filter),
    app_type_str: "codex",
};

/// Codex 智能解析配置（自动检测 OpenAI 或 Codex 格式）
pub const CODEX_PARSER_CONFIG: UsageParserConfig = UsageParserConfig {
    stream_usage: StreamUsageFormat::CodexAuto,
    response_parser: TokenUsage::from_codex_response_auto,
    stream_event_filter: Some(codex_stream_usage_event_filter),
    app_type_str: "codex",
};

/// Gemini API 解析配置
pub const GEMINI_PARSER_CONFIG: UsageParserConfig = UsageParserConfig {
    stream_usage: StreamUsageFormat::Gemini,
    response_parser: TokenUsage::from_gemini_response,
    stream_event_filter: Some(gemini_stream_usage_event_filter),
    app_type_str: "gemini",
};

/// 按客户端线协议选择 usage 解析配置（透传响应与转换后的流式响应共用）
pub fn parser_config_for(client_format: WireFormat) -> &'static UsageParserConfig {
    match client_format {
        WireFormat::AnthropicMessages => &CLAUDE_PARSER_CONFIG,
        WireFormat::OpenAiChat => &OPENAI_PARSER_CONFIG,
        WireFormat::OpenAiResponses => &CODEX_PARSER_CONFIG,
        WireFormat::GeminiNative => &GEMINI_PARSER_CONFIG,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::stream::UsageTap;
    use serde_json::json;

    /// 按配置的流式 usage 格式折叠事件并取归因模型
    fn extract_model(config: UsageParserConfig, events: &[Value], fallback: &str) -> String {
        UsageTap::from_events(config.stream_usage, events).model(fallback)
    }

    #[test]
    fn parser_configs_carry_expected_app_type_and_filters() {
        assert_eq!(CLAUDE_PARSER_CONFIG.app_type_str, "claude");
        assert_eq!(OPENAI_PARSER_CONFIG.app_type_str, "codex");
        assert_eq!(CODEX_PARSER_CONFIG.app_type_str, "codex");
        assert_eq!(GEMINI_PARSER_CONFIG.app_type_str, "gemini");

        for config in [
            CLAUDE_PARSER_CONFIG,
            OPENAI_PARSER_CONFIG,
            CODEX_PARSER_CONFIG,
            GEMINI_PARSER_CONFIG,
        ] {
            assert!(config.stream_event_filter.is_some());
        }
    }

    #[test]
    fn codex_parser_configs_use_distinct_stream_filters() {
        // Codex 两个入口分别使用 OpenAI Chat 与自动检测解析器
        let chat_filter = OPENAI_PARSER_CONFIG.stream_event_filter.unwrap();
        let responses_filter = CODEX_PARSER_CONFIG.stream_event_filter.unwrap();
        let completed = r#"{"type":"response.completed","response":{}}"#;
        assert!(!chat_filter(completed));
        assert!(responses_filter(completed));
    }

    #[test]
    fn stream_usage_event_filters_match_only_usage_events() {
        assert!(claude_stream_usage_event_filter(
            r#"{"type":"message_start","message":{}}"#
        ));
        assert!(claude_stream_usage_event_filter(
            r#"{"type":"message_delta","usage":{}}"#
        ));
        assert!(!claude_stream_usage_event_filter(
            r#"{"type":"content_block_delta","delta":{"text":"hi"}}"#
        ));

        assert!(openai_stream_usage_event_filter(
            r#"{"choices":[],"usage":{"prompt_tokens":1}}"#
        ));
        assert!(!openai_stream_usage_event_filter(
            r#"{"choices":[{"delta":{"content":"hi"}}]}"#
        ));

        assert!(codex_stream_usage_event_filter(
            r#"{"type":"response.completed"}"#
        ));
        assert!(codex_stream_usage_event_filter(r#"{"usage":{}}"#));
        assert!(!codex_stream_usage_event_filter(
            r#"{"type":"response.output_text.delta","delta":"hi"}"#
        ));

        assert!(gemini_stream_usage_event_filter(
            r#"{"usageMetadata":{"promptTokenCount":1}}"#
        ));
        assert!(!gemini_stream_usage_event_filter(r#"{"candidates":[]}"#));
    }

    #[test]
    fn claude_model_extractor_prefers_usage_model_and_ignores_empty() {
        let events = vec![json!({
            "type": "message_start",
            "message": {
                "id": "msg_1",
                "model": "claude-upstream",
                "usage": { "input_tokens": 10 }
            }
        })];
        assert_eq!(
            extract_model(CLAUDE_PARSER_CONFIG, &events, "fallback"),
            "claude-upstream"
        );

        let empty_model = vec![json!({
            "type": "message_start",
            "message": { "model": "", "usage": { "input_tokens": 10 } }
        })];
        assert_eq!(
            extract_model(CLAUDE_PARSER_CONFIG, &empty_model, "fallback"),
            "fallback"
        );
        assert_eq!(
            extract_model(CLAUDE_PARSER_CONFIG, &[], "fallback"),
            "fallback"
        );
    }

    #[test]
    fn openai_model_extractor_falls_back_to_event_model() {
        let events = vec![
            json!({ "model": "", "choices": [] }),
            json!({ "model": "gpt-chunk", "choices": [] }),
        ];
        assert_eq!(
            extract_model(OPENAI_PARSER_CONFIG, &events, "fallback"),
            "gpt-chunk"
        );
        assert_eq!(
            extract_model(
                OPENAI_PARSER_CONFIG,
                &[json!({ "choices": [] })],
                "fallback"
            ),
            "fallback"
        );
    }

    #[test]
    fn codex_model_extractor_reads_completed_response_then_chunks() {
        let completed = vec![json!({
            "type": "response.completed",
            "response": { "model": "gpt-responses" }
        })];
        assert_eq!(
            extract_model(CODEX_PARSER_CONFIG, &completed, "fallback"),
            "gpt-responses"
        );

        let chunks = vec![json!({ "model": "gpt-chat", "choices": [] })];
        assert_eq!(
            extract_model(CODEX_PARSER_CONFIG, &chunks, "fallback"),
            "gpt-chat"
        );

        let empty = vec![json!({
            "type": "response.completed",
            "response": { "model": "" }
        })];
        assert_eq!(
            extract_model(CODEX_PARSER_CONFIG, &empty, "fallback"),
            "fallback"
        );
    }

    #[test]
    fn gemini_model_extractor_uses_fallback_without_usage() {
        assert_eq!(
            extract_model(
                GEMINI_PARSER_CONFIG,
                &[json!({ "candidates": [] })],
                "gemini-req"
            ),
            "gemini-req"
        );
    }
}
