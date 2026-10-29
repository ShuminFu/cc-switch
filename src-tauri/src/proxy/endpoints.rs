//! 客户端端点表
//!
//! 每个 [`ClientEndpoint`] 描述一类客户端入口：挂载路径、客户端线协议、所属应用、
//! 上游端点的推导方式以及请求体解码方式。`server.rs` 依据 [`CLIENT_ENDPOINTS`]
//! 生成路由，所有条目共用同一个 `handlers::handle_proxied`。

use super::protocol::WireFormat;
use crate::app_config::AppType;

/// 路由挂载的 HTTP 方法
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RouteMethod {
    Post,
    /// 覆盖所有方法（Gemini 的 GET `/models` 等只读端点也需要经过代理）
    Any,
}

/// 转发给上游的端点如何从入站 URI 推导
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UpstreamEndpoint {
    /// 使用入站 path + query，可选去掉一个本地命名空间前缀
    Inbound { strip_prefix: Option<&'static str> },
    /// 固定上游路径，保留入站 query
    Fixed(&'static str),
}

/// 入站请求体解码方式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BodyDecoding {
    /// 原样解析 JSON
    Plain,
    /// Codex 客户端可能压缩请求体（zstd / gzip 等），解析前先解压
    ContentEncoded,
}

/// 客户端端点描述
#[derive(Debug)]
pub(crate) struct ClientEndpoint {
    /// 挂载的全部路径（带前缀 / 不带前缀等别名）
    pub paths: &'static [&'static str],
    pub method: RouteMethod,
    /// 客户端说的线协议，决定响应处理分支与错误渲染器
    pub client_format: WireFormat,
    pub app_type: AppType,
    /// 日志标签
    pub tag: &'static str,
    /// proxy_config / provider 表中的 app_type 字符串
    pub app_type_str: &'static str,
    pub upstream: UpstreamEndpoint,
    pub body_decoding: BodyDecoding,
    /// 允许空请求体（GET 类只读端点），空体按 `null` 处理
    pub allow_empty_body: bool,
    /// 模型名在 URI 中（Gemini）
    pub model_from_uri: bool,
    /// 需要校验 Claude Desktop gateway token
    pub claude_desktop_gateway_auth: bool,
}

impl ClientEndpoint {
    const fn new(
        paths: &'static [&'static str],
        client_format: WireFormat,
        app_type: AppType,
        tag: &'static str,
        app_type_str: &'static str,
        upstream: UpstreamEndpoint,
    ) -> Self {
        Self {
            paths,
            method: RouteMethod::Post,
            client_format,
            app_type,
            tag,
            app_type_str,
            upstream,
            body_decoding: BodyDecoding::Plain,
            allow_empty_body: false,
            model_from_uri: false,
            claude_desktop_gateway_auth: false,
        }
    }

    const fn content_encoded(mut self) -> Self {
        self.body_decoding = BodyDecoding::ContentEncoded;
        self
    }
}

const INBOUND: UpstreamEndpoint = UpstreamEndpoint::Inbound { strip_prefix: None };

/// 全部代理端点
pub(crate) static CLIENT_ENDPOINTS: &[ClientEndpoint] = &[
    // Claude API (支持带前缀和不带前缀两种格式)
    ClientEndpoint::new(
        &["/v1/messages", "/claude/v1/messages"],
        WireFormat::AnthropicMessages,
        AppType::Claude,
        "Claude",
        "claude",
        INBOUND,
    ),
    // Claude Desktop 3P 本地 gateway（独立 provider namespace）
    ClientEndpoint {
        claude_desktop_gateway_auth: true,
        ..ClientEndpoint::new(
            &["/claude-desktop/v1/messages"],
            WireFormat::AnthropicMessages,
            AppType::ClaudeDesktop,
            "Claude Desktop",
            "claude-desktop",
            UpstreamEndpoint::Inbound {
                strip_prefix: Some("/claude-desktop"),
            },
        )
    },
    // OpenAI Chat Completions API (Codex CLI，支持带前缀和不带前缀)
    ClientEndpoint::new(
        &[
            "/chat/completions",
            "/v1/chat/completions",
            "/v1/v1/chat/completions",
            "/codex/v1/chat/completions",
        ],
        WireFormat::OpenAiChat,
        AppType::Codex,
        "Codex",
        "codex",
        UpstreamEndpoint::Fixed("/chat/completions"),
    )
    .content_encoded(),
    // OpenAI Responses API (Codex CLI，支持带前缀和不带前缀)
    ClientEndpoint::new(
        &[
            "/responses",
            "/v1/responses",
            "/v1/v1/responses",
            "/codex/v1/responses",
        ],
        WireFormat::OpenAiResponses,
        AppType::Codex,
        "Codex",
        "codex",
        UpstreamEndpoint::Fixed("/responses"),
    )
    .content_encoded(),
    // OpenAI Responses Compact API (Codex CLI 远程压缩)
    ClientEndpoint::new(
        &[
            "/responses/compact",
            "/v1/responses/compact",
            "/v1/v1/responses/compact",
            "/codex/v1/responses/compact",
        ],
        WireFormat::OpenAiResponses,
        AppType::Codex,
        "Codex",
        "codex",
        UpstreamEndpoint::Fixed("/responses/compact"),
    )
    .content_encoded(),
    // Grok Build uses the Responses protocol but has an independent
    // provider namespace and failover queue.
    ClientEndpoint::new(
        &["/grokbuild/v1/responses"],
        WireFormat::OpenAiResponses,
        AppType::GrokBuild,
        "Grok Build",
        "grokbuild",
        UpstreamEndpoint::Fixed("/responses"),
    )
    .content_encoded(),
    ClientEndpoint::new(
        &["/grokbuild/v1/responses/compact"],
        WireFormat::OpenAiResponses,
        AppType::GrokBuild,
        "Grok Build",
        "grokbuild",
        UpstreamEndpoint::Fixed("/responses/compact"),
    )
    .content_encoded(),
    // Gemini API (支持带前缀和不带前缀；GA 版本 /v1 给原 SDK 留一条出口)
    //
    // 用 `any(..)` 覆盖所有 HTTP 方法：除了 POST `:generateContent` /
    // `:streamGenerateContent` / `:countTokens` 之外，Gemini SDK / CLI 还会发
    // GET `/models`、GET `/models/<id>` 等只读端点。如果只挂 POST，这些 GET
    // 请求会在路由层 404，绕过本地代理的统计、整流和故障转移。
    ClientEndpoint {
        method: RouteMethod::Any,
        allow_empty_body: true,
        model_from_uri: true,
        ..ClientEndpoint::new(
            &["/v1beta/*path", "/gemini/v1beta/*path", "/gemini/v1/*path"],
            WireFormat::GeminiNative,
            AppType::Gemini,
            "Gemini",
            "gemini",
            INBOUND,
        )
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn endpoint_paths_are_unique() {
        let mut seen = HashSet::new();
        for endpoint in CLIENT_ENDPOINTS {
            for path in endpoint.paths {
                assert!(seen.insert(*path), "duplicate route path {path}");
            }
        }
    }

    #[test]
    fn table_covers_every_proxied_route() {
        let expected = [
            ("/v1/messages", "claude", WireFormat::AnthropicMessages),
            (
                "/claude/v1/messages",
                "claude",
                WireFormat::AnthropicMessages,
            ),
            (
                "/claude-desktop/v1/messages",
                "claude-desktop",
                WireFormat::AnthropicMessages,
            ),
            ("/chat/completions", "codex", WireFormat::OpenAiChat),
            ("/v1/chat/completions", "codex", WireFormat::OpenAiChat),
            ("/v1/v1/chat/completions", "codex", WireFormat::OpenAiChat),
            (
                "/codex/v1/chat/completions",
                "codex",
                WireFormat::OpenAiChat,
            ),
            ("/responses", "codex", WireFormat::OpenAiResponses),
            ("/v1/responses", "codex", WireFormat::OpenAiResponses),
            ("/v1/v1/responses", "codex", WireFormat::OpenAiResponses),
            ("/codex/v1/responses", "codex", WireFormat::OpenAiResponses),
            ("/responses/compact", "codex", WireFormat::OpenAiResponses),
            (
                "/v1/responses/compact",
                "codex",
                WireFormat::OpenAiResponses,
            ),
            (
                "/v1/v1/responses/compact",
                "codex",
                WireFormat::OpenAiResponses,
            ),
            (
                "/codex/v1/responses/compact",
                "codex",
                WireFormat::OpenAiResponses,
            ),
            (
                "/grokbuild/v1/responses",
                "grokbuild",
                WireFormat::OpenAiResponses,
            ),
            (
                "/grokbuild/v1/responses/compact",
                "grokbuild",
                WireFormat::OpenAiResponses,
            ),
            ("/v1beta/*path", "gemini", WireFormat::GeminiNative),
            ("/gemini/v1beta/*path", "gemini", WireFormat::GeminiNative),
            ("/gemini/v1/*path", "gemini", WireFormat::GeminiNative),
        ];
        let total: usize = CLIENT_ENDPOINTS.iter().map(|e| e.paths.len()).sum();
        assert_eq!(total, expected.len());
        for (path, app, format) in expected {
            let endpoint = CLIENT_ENDPOINTS
                .iter()
                .find(|e| e.paths.contains(&path))
                .unwrap_or_else(|| panic!("missing route {path}"));
            assert_eq!(endpoint.app_type_str, app, "{path}");
            assert_eq!(endpoint.app_type.as_str(), app, "{path}");
            assert_eq!(endpoint.client_format, format, "{path}");
        }
    }

    #[test]
    fn compact_endpoints_forward_to_compact_upstream() {
        for endpoint in CLIENT_ENDPOINTS {
            let compact = endpoint.paths.iter().any(|p| p.ends_with("/compact"));
            if compact {
                assert_eq!(
                    endpoint.upstream,
                    UpstreamEndpoint::Fixed("/responses/compact")
                );
            }
        }
    }

    #[test]
    fn codex_endpoints_decode_compressed_bodies() {
        for endpoint in CLIENT_ENDPOINTS {
            let expected = matches!(
                endpoint.client_format,
                WireFormat::OpenAiChat | WireFormat::OpenAiResponses
            );
            assert_eq!(
                endpoint.body_decoding == BodyDecoding::ContentEncoded,
                expected,
                "{:?}",
                endpoint.paths
            );
        }
    }
}
