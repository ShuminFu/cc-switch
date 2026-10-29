//! 单次转发尝试的路由计划
//!
//! forwarder 在每次尝试（每个 provider、每次整流重试）时解析一次 [`RoutePlan`]，
//! 请求体转换、URL 构造与认证都基于它；计划随 `ForwardResult` 交给 handler，
//! 响应解码只读计划，不再按最终 provider 重新推导格式——故障转移后两侧因此
//! 不会出现「按 A 的格式发请求、按 B 的格式解响应」的分歧。

use std::fmt;

use crate::proxy::protocol::{ClaudeApiFormat, WireFormat};
use crate::proxy::providers::AuthStrategy;

/// 供应商特有的行为开关
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VendorQuirks {
    /// GitHub Copilot（动态 endpoint、指纹头、按模型 vendor 选择上游格式）
    pub copilot: bool,
    /// Codex OAuth（ChatGPT 账号；openai_responses 非流请求会被上游升级为 SSE）
    pub codex_oauth: bool,
    /// base_url 是完整上游端点，不再拼接路径
    pub full_url: bool,
    /// Codex 官方供应商：认证头由客户端提供，代理只透传
    pub codex_official_auth_passthrough: bool,
    /// Codex → Anthropic 时模拟 Claude Code 指纹
    pub impersonate_claude_code: bool,
}

/// 本次尝试会读写的跨请求旁路存储
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SideStores {
    /// Codex Responses → Chat：请求侧回填历史 function call，响应侧记录新的
    pub codex_chat_history: bool,
    /// Claude → Gemini Native：thought signature 等 shadow 回放
    pub gemini_shadow: bool,
}

/// 响应解码方式（由计划的客户端 / 上游线协议决定）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseDecode {
    /// 上游与客户端同协议，直接透传
    Passthrough,
    /// Anthropic 客户端，上游为给定的非 Anthropic 格式
    AnthropicFrom(ClaudeApiFormat),
    /// Codex Responses 客户端，上游为 Anthropic Messages
    ResponsesFromAnthropic,
    /// Codex Responses 客户端，上游为 Chat Completions
    ResponsesFromChat,
}

/// 单次转发尝试的路由计划
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutePlan {
    /// 本次尝试的 provider ID
    pub provider_id: String,
    /// 客户端说的线协议
    pub client: WireFormat,
    /// 实际发往上游的线协议
    pub upstream: WireFormat,
    /// Claude 客户端解析出的上游 API 格式（非 Claude 客户端为 `None`）
    pub claude_api_format: Option<ClaudeApiFormat>,
    /// 最终上游 URL
    pub url: String,
    /// 认证策略（无凭据时为 `None`）
    pub auth: Option<AuthStrategy>,
    pub quirks: VendorQuirks,
    pub side_stores: SideStores,
}

impl RoutePlan {
    /// 请求与响应是否需要在两种线协议之间转换
    pub fn needs_transform(&self) -> bool {
        self.client != self.upstream
    }

    /// 响应解码方式
    pub fn response_decode(&self) -> ResponseDecode {
        if !self.needs_transform() {
            return ResponseDecode::Passthrough;
        }
        match (self.client, self.upstream) {
            (WireFormat::AnthropicMessages, upstream) => ResponseDecode::AnthropicFrom(
                self.claude_api_format
                    .filter(|format| format.wire_format() == upstream)
                    .unwrap_or_else(|| claude_api_format_for_wire(upstream)),
            ),
            (WireFormat::OpenAiResponses, WireFormat::AnthropicMessages) => {
                ResponseDecode::ResponsesFromAnthropic
            }
            (WireFormat::OpenAiResponses, WireFormat::OpenAiChat) => {
                ResponseDecode::ResponsesFromChat
            }
            // 其余组合没有请求侧转换，按透传处理
            _ => ResponseDecode::Passthrough,
        }
    }
}

fn claude_api_format_for_wire(wire: WireFormat) -> ClaudeApiFormat {
    match wire {
        WireFormat::AnthropicMessages => ClaudeApiFormat::Anthropic,
        WireFormat::OpenAiChat => ClaudeApiFormat::OpenAiChat,
        WireFormat::OpenAiResponses => ClaudeApiFormat::OpenAiResponses,
        WireFormat::GeminiNative => ClaudeApiFormat::GeminiNative,
    }
}

/// 日志用摘要：URL 去掉 query，避免把可能携带的凭据写进日志
impl fmt::Display for RoutePlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let url = self
            .url
            .split_once('?')
            .map_or(self.url.as_str(), |(path, _)| path);
        write!(
            f,
            "provider={} client={:?} upstream={:?} api_format={} decode={:?} url={} auth={:?} quirks={:?} side_stores={:?}",
            self.provider_id,
            self.client,
            self.upstream,
            self.claude_api_format
                .map_or("native", ClaudeApiFormat::as_str),
            self.response_decode(),
            url,
            self.auth,
            self.quirks,
            self.side_stores,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(
        client: WireFormat,
        upstream: WireFormat,
        claude_api_format: Option<ClaudeApiFormat>,
    ) -> RoutePlan {
        RoutePlan {
            provider_id: "p".to_string(),
            client,
            upstream,
            claude_api_format,
            url: "https://example.com/v1/messages?key=secret".to_string(),
            auth: Some(AuthStrategy::Bearer),
            quirks: VendorQuirks::default(),
            side_stores: SideStores::default(),
        }
    }

    #[test]
    fn same_wire_format_is_passthrough() {
        let anthropic = plan(
            WireFormat::AnthropicMessages,
            WireFormat::AnthropicMessages,
            Some(ClaudeApiFormat::Anthropic),
        );
        assert!(!anthropic.needs_transform());
        assert_eq!(anthropic.response_decode(), ResponseDecode::Passthrough);

        let codex_chat = plan(WireFormat::OpenAiChat, WireFormat::OpenAiChat, None);
        assert_eq!(codex_chat.response_decode(), ResponseDecode::Passthrough);
    }

    #[test]
    fn anthropic_client_decodes_with_the_resolved_api_format() {
        for format in [
            ClaudeApiFormat::OpenAiChat,
            ClaudeApiFormat::OpenAiResponses,
            ClaudeApiFormat::GeminiNative,
        ] {
            let plan = plan(
                WireFormat::AnthropicMessages,
                format.wire_format(),
                Some(format),
            );
            assert!(plan.needs_transform());
            assert_eq!(
                plan.response_decode(),
                ResponseDecode::AnthropicFrom(format)
            );
        }
    }

    #[test]
    fn anthropic_client_decode_follows_upstream_when_format_disagrees() {
        let plan = plan(
            WireFormat::AnthropicMessages,
            WireFormat::OpenAiResponses,
            Some(ClaudeApiFormat::OpenAiChat),
        );
        assert_eq!(
            plan.response_decode(),
            ResponseDecode::AnthropicFrom(ClaudeApiFormat::OpenAiResponses)
        );
    }

    #[test]
    fn codex_responses_client_decodes_by_upstream() {
        assert_eq!(
            plan(
                WireFormat::OpenAiResponses,
                WireFormat::AnthropicMessages,
                None
            )
            .response_decode(),
            ResponseDecode::ResponsesFromAnthropic
        );
        assert_eq!(
            plan(WireFormat::OpenAiResponses, WireFormat::OpenAiChat, None).response_decode(),
            ResponseDecode::ResponsesFromChat
        );
    }

    #[test]
    fn display_omits_url_query() {
        let rendered = plan(
            WireFormat::AnthropicMessages,
            WireFormat::AnthropicMessages,
            Some(ClaudeApiFormat::Anthropic),
        )
        .to_string();
        assert!(rendered.contains("url=https://example.com/v1/messages "));
        assert!(!rendered.contains("secret"));
        assert!(rendered.contains("api_format=anthropic"));
        assert!(rendered.contains("decode=Passthrough"));
    }
}
