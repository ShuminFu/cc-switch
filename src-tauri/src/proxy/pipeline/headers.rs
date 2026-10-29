//! 出站请求头策略
//!
//! [`HeaderPolicy`] 是一张按路由构建的声明式规则表：每条规则把一组客户端头名映射到
//! 一个 [`HeaderAction`]（丢弃 / 透传 / 原位改写 / 写入槽位），按客户端原始顺序逐个
//! 应用，第一条匹配的规则生效。需要「只在首次出现的位置替换、缺失时末尾补齐」的头
//! （认证、accept-encoding、user-agent、anthropic-beta 等）由 [`Slot`] 描述，槽位的
//! 补齐顺序即 [`HeaderPolicy::slots`] 的声明顺序。

use super::copilot::CopilotOptimization;
use crate::provider::{LocalProxyRequestOverrides, Provider};
use crate::proxy::error::ProxyError;
use crate::proxy::types::CopilotOptimizerConfig;
use http::{HeaderMap, HeaderName, HeaderValue};

/// Claude Code client fingerprint (used for Codex→Anthropic emulation to pass a
/// gateway's "Claude Code only" check).
const CLAUDE_CODE_USER_AGENT: &str = "claude-cli/1.0.119 (external, cli)";
const CLAUDE_CODE_BETA: &str = "claude-code-20250219";
const CONTEXT_1M_BETA: &str = "context-1m-2025-08-07";
const DEFAULT_ANTHROPIC_VERSION: &str = "2023-06-01";

/// 连接 / 追踪 / CDN 类头：无条件跳过
const HOP_AND_TRACE_HEADERS: &[&str] = &[
    "content-length",
    "transfer-encoding",
    "x-forwarded-host",
    "x-forwarded-port",
    "x-forwarded-proto",
    "forwarded",
    "cf-connecting-ip",
    "cf-ipcountry",
    "cf-ray",
    "cf-visitor",
    "true-client-ip",
    "fastly-client-ip",
    "x-azure-clientip",
    "x-azure-fdid",
    "x-azure-ref",
    "akamai-origin-hop",
    "x-akamai-config-log-detail",
    "x-request-id",
    "x-correlation-id",
    "x-trace-id",
    "x-amzn-trace-id",
    "x-b3-traceid",
    "x-b3-spanid",
    "x-b3-parentspanid",
    "x-b3-sampled",
    "traceparent",
    "tracestate",
];

/// 客户端凭据头：用 adapter 提供的认证头替换（在首个凭据头的位置）
const CREDENTIAL_HEADERS: &[&str] = &["authorization", "x-api-key", "x-goog-api-key"];

/// Copilot 指纹头名（由 get_auth_headers 注入，需在原始头中去重）
const COPILOT_FINGERPRINT_HEADERS: &[&str] = &[
    "user-agent",
    "editor-version",
    "editor-plugin-version",
    "copilot-integration-id",
    "x-github-api-version",
    "openai-intent",
    "x-initiator",
    "x-interaction-type",
    "x-interaction-id",
    "x-vscode-user-agent-library-version",
    "x-request-id",
    "x-agent-task-id",
];

type HeaderPair = (HeaderName, HeaderValue);

/// 由策略统一写入、每个请求最多出现一次的头
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Slot {
    Auth,
    AcceptEncoding,
    Accept,
    XApp,
    UserAgent,
    AnthropicBeta,
    AnthropicVersion,
}

/// 槽位写入的内容
#[derive(Debug, Clone)]
enum SlotFill {
    /// 保留客户端首次出现的原值
    Original,
    /// 写入给定的头（可为空：仅吞掉客户端的值）
    Headers(Vec<HeaderPair>),
}

#[derive(Debug, Clone)]
struct SlotRule {
    slot: Slot,
    fill: SlotFill,
    /// 客户端没有发送该头时是否在末尾补齐
    append_if_missing: bool,
}

/// 头名匹配方式（头名已由 http crate 小写化）
#[derive(Clone, Copy)]
enum Matcher {
    Exact(&'static str),
    AnyOf(&'static [&'static str]),
    Predicate(fn(&str) -> bool),
}

impl Matcher {
    fn matches(&self, name: &str) -> bool {
        match self {
            Matcher::Exact(expected) => name == *expected,
            Matcher::AnyOf(names) => names.contains(&name),
            Matcher::Predicate(predicate) => predicate(name),
        }
    }
}

/// 对一个客户端头的处理
#[derive(Debug, Clone)]
pub(crate) enum HeaderAction {
    /// 丢弃
    Drop,
    /// 原样透传（每次出现都保留）
    Pass,
    /// 每次出现都原位改写为给定值（`None` 时丢弃）
    Rewrite(Option<HeaderValue>),
    /// 首次出现的位置写入槽位内容，同槽位后续出现丢弃
    Fill(Slot),
    /// 原样透传，并把槽位标记为已出现（不再补齐）
    PassAndClaim(Slot),
}

/// 构建 [`HeaderPolicy`] 所需的路由判定与预先算好的头值
pub(crate) struct HeaderPolicyInput {
    /// 上游 authority（原位替换 host）
    pub upstream_host: Option<String>,
    /// adapter 生成的认证头（含 Copilot 指纹 / ChatGPT-Account-Id）
    pub auth_headers: Vec<HeaderPair>,
    /// Codex 官方供应商：客户端 authorization 原样透传
    pub codex_official_auth_passthrough: bool,
    pub codex_responses_to_anthropic: bool,
    pub codex_impersonate_claude_code: bool,
    pub is_copilot: bool,
    /// transform / SSE 路径强制 `accept-encoding: identity`
    pub force_identity_encoding: bool,
    pub custom_user_agent: Option<HeaderValue>,
    /// 重建后的 anthropic-beta（`None` 表示不发送）
    pub anthropic_beta: Option<String>,
    /// 原生 Anthropic 上游：透传客户端 anthropic-version
    pub send_anthropic_headers: bool,
}

/// 声明式出站头策略
pub(crate) struct HeaderPolicy {
    rules: Vec<(Matcher, HeaderAction)>,
    /// 槽位定义；缺失补齐按此顺序追加
    slots: Vec<SlotRule>,
}

impl HeaderPolicy {
    pub(crate) fn for_route(input: HeaderPolicyInput) -> Self {
        let mut rules: Vec<(Matcher, HeaderAction)> = Vec::new();

        // host — 原位替换为上游 host（保持客户端原始位置）
        let host = input
            .upstream_host
            .as_deref()
            .and_then(|host| HeaderValue::from_str(host).ok());
        rules.push((Matcher::Exact("host"), HeaderAction::Rewrite(host)));
        rules.push((Matcher::AnyOf(HOP_AND_TRACE_HEADERS), HeaderAction::Drop));

        // The built-in Codex official provider deliberately has no credential in
        // CC Switch. `requires_openai_auth = true` makes Codex send its native
        // ChatGPT authorization, which must reach the fixed official upstream
        // unchanged. Other credential headers are still discarded.
        if input.codex_official_auth_passthrough {
            rules.push((
                Matcher::Exact("authorization"),
                HeaderAction::PassAndClaim(Slot::Auth),
            ));
        }
        rules.push((
            Matcher::AnyOf(CREDENTIAL_HEADERS),
            HeaderAction::Fill(Slot::Auth),
        ));

        // x-app — during Codex→Anthropic emulation, `cli` is injected uniformly
        if input.codex_impersonate_claude_code {
            rules.push((Matcher::Exact("x-app"), HeaderAction::Drop));
        }

        if input.codex_responses_to_anthropic {
            // Codex/OpenAI fingerprint headers never leak to an Anthropic upstream:
            // they leak client identity and can defeat strict gateway fingerprint checks.
            rules.push((
                Matcher::Predicate(is_codex_client_fingerprint_header),
                HeaderAction::Drop,
            ));
            // The Codex CLI sends `Accept: text/event-stream`, whereas a native
            // Anthropic client sends `application/json` (streaming is driven by
            // the body's stream:true). Strict Anthropic gateways return 406 Not
            // Acceptable for an event-stream Accept, so normalize it here.
            rules.push((Matcher::Exact("accept"), HeaderAction::Fill(Slot::Accept)));
        }

        rules.push((
            Matcher::Exact("accept-encoding"),
            HeaderAction::Fill(Slot::AcceptEncoding),
        ));
        // Copilot 指纹 UA 不可覆盖（由下方 Copilot 指纹规则丢弃）
        if !input.is_copilot {
            rules.push((
                Matcher::Exact("user-agent"),
                HeaderAction::Fill(Slot::UserAgent),
            ));
        }
        rules.push((
            Matcher::Exact("anthropic-beta"),
            HeaderAction::Fill(Slot::AnthropicBeta),
        ));
        rules.push((
            Matcher::Exact("anthropic-version"),
            if input.send_anthropic_headers {
                HeaderAction::PassAndClaim(Slot::AnthropicVersion)
            } else {
                HeaderAction::Drop
            },
        ));
        if input.is_copilot {
            rules.push((
                Matcher::AnyOf(COPILOT_FINGERPRINT_HEADERS),
                HeaderAction::Drop,
            ));
        }

        let slots = Self::slot_rules(input);
        Self { rules, slots }
    }

    fn slot_rules(input: HeaderPolicyInput) -> Vec<SlotRule> {
        let fixed = |name: HeaderName, value: &'static str| {
            SlotFill::Headers(vec![(name, HeaderValue::from_static(value))])
        };
        let anthropic_beta = input
            .anthropic_beta
            .as_deref()
            .and_then(|value| HeaderValue::from_str(value).ok())
            .map(|value| (HeaderName::from_static("anthropic-beta"), value));

        vec![
            SlotRule {
                slot: Slot::Auth,
                fill: SlotFill::Headers(input.auth_headers),
                append_if_missing: true,
            },
            SlotRule {
                slot: Slot::AcceptEncoding,
                fill: if input.force_identity_encoding {
                    fixed(http::header::ACCEPT_ENCODING, "identity")
                } else {
                    SlotFill::Original
                },
                append_if_missing: input.force_identity_encoding,
            },
            SlotRule {
                slot: Slot::Accept,
                fill: fixed(http::header::ACCEPT, "application/json"),
                append_if_missing: input.codex_responses_to_anthropic,
            },
            SlotRule {
                slot: Slot::XApp,
                fill: fixed(HeaderName::from_static("x-app"), "cli"),
                append_if_missing: input.codex_impersonate_claude_code,
            },
            SlotRule {
                slot: Slot::UserAgent,
                append_if_missing: input.custom_user_agent.is_some(),
                fill: match input.custom_user_agent {
                    Some(ua) => SlotFill::Headers(vec![(http::header::USER_AGENT, ua)]),
                    None => SlotFill::Original,
                },
            },
            SlotRule {
                slot: Slot::AnthropicBeta,
                append_if_missing: input.anthropic_beta.is_some(),
                fill: SlotFill::Headers(anthropic_beta.into_iter().collect()),
            },
            // anthropic-version: add the default only when it is missing. The
            // Codex→Anthropic path also needs it, independent of anthropic-beta.
            SlotRule {
                slot: Slot::AnthropicVersion,
                fill: fixed(
                    HeaderName::from_static("anthropic-version"),
                    DEFAULT_ANTHROPIC_VERSION,
                ),
                append_if_missing: input.send_anthropic_headers
                    || input.codex_responses_to_anthropic,
            },
        ]
    }

    /// 第一条匹配规则的处理；没有规则匹配时透传
    pub(crate) fn action_for(&self, name: &str) -> &HeaderAction {
        self.rules
            .iter()
            .find(|(matcher, _)| matcher.matches(name))
            .map(|(_, action)| action)
            .unwrap_or(&HeaderAction::Pass)
    }

    fn slot(&self, slot: Slot) -> Option<&SlotRule> {
        self.slots.iter().find(|rule| rule.slot == slot)
    }

    /// 按客户端原始顺序构建出站 HeaderMap
    pub(crate) fn apply(&self, incoming: &HeaderMap) -> HeaderMap {
        let mut out = HeaderMap::new();
        let mut claimed: Vec<Slot> = Vec::new();

        for (key, value) in incoming {
            match self.action_for(key.as_str()) {
                HeaderAction::Drop => {}
                HeaderAction::Pass => {
                    out.append(key.clone(), value.clone());
                }
                HeaderAction::Rewrite(replacement) => {
                    if let Some(replacement) = replacement {
                        out.append(key.clone(), replacement.clone());
                    }
                }
                HeaderAction::PassAndClaim(slot) => {
                    if !claimed.contains(slot) {
                        claimed.push(*slot);
                    }
                    out.append(key.clone(), value.clone());
                }
                HeaderAction::Fill(slot) => {
                    if claimed.contains(slot) {
                        continue;
                    }
                    claimed.push(*slot);
                    match self.slot(*slot).map(|rule| &rule.fill) {
                        Some(SlotFill::Headers(headers)) => {
                            for (name, value) in headers {
                                out.append(name.clone(), value.clone());
                            }
                        }
                        Some(SlotFill::Original) | None => {
                            out.append(key.clone(), value.clone());
                        }
                    }
                }
            }
        }

        for rule in &self.slots {
            if !rule.append_if_missing || claimed.contains(&rule.slot) {
                continue;
            }
            if let SlotFill::Headers(headers) = &rule.fill {
                for (name, value) in headers {
                    out.append(name.clone(), value.clone());
                }
            }
        }

        out
    }
}

/// 上游 authority（用于在原位替换 host header）
pub(crate) fn upstream_authority(url: &str) -> Option<String> {
    url.parse::<http::Uri>()
        .ok()
        .and_then(|u| u.authority().map(|a| a.to_string()))
}

/// 重建 anthropic-beta：原生 Anthropic 上游确保含 claude-code 标记；
/// Codex→Anthropic 按伪装 / [1m] 注入对应标记
pub(crate) fn anthropic_beta_value(
    incoming: &HeaderMap,
    send_anthropic_headers: bool,
    codex_impersonate_claude_code: bool,
    codex_anthropic_one_m: bool,
) -> Option<String> {
    if send_anthropic_headers {
        return Some(match incoming.get("anthropic-beta") {
            Some(beta) => match beta.to_str() {
                Ok(beta_str) if beta_str.contains(CLAUDE_CODE_BETA) => beta_str.to_string(),
                Ok(beta_str) => format!("{CLAUDE_CODE_BETA},{beta_str}"),
                Err(_) => CLAUDE_CODE_BETA.to_string(),
            },
            None => CLAUDE_CODE_BETA.to_string(),
        });
    }
    if codex_impersonate_claude_code || codex_anthropic_one_m {
        let mut betas: Vec<&str> = Vec::new();
        if codex_impersonate_claude_code {
            betas.push(CLAUDE_CODE_BETA);
        }
        if codex_anthropic_one_m {
            betas.push(CONTEXT_1M_BETA);
        }
        return Some(betas.join(","));
    }
    None
}

/// 自定义 User-Agent：与 stream_check / model_fetch 共用 parse_custom_user_agent，
/// 运行时静默忽略非法值（前端在输入处给非阻断提示，不在保存时阻断）。
/// Copilot 指纹 UA 不可覆盖。Codex→Anthropic 伪装且无自定义 UA 时使用 Claude Code UA。
pub(crate) fn custom_user_agent(
    provider: &Provider,
    is_copilot: bool,
    codex_impersonate_claude_code: bool,
) -> Option<HeaderValue> {
    let custom = if is_copilot {
        None
    } else {
        provider
            .meta
            .as_ref()
            .and_then(|meta| meta.custom_user_agent_header().ok().flatten())
    };
    if custom.is_none() && codex_impersonate_claude_code {
        Some(HeaderValue::from_static(CLAUDE_CODE_USER_AGENT))
    } else {
        custom
    }
}

/// Copilot 优化器：动态 header 注入（改写 get_auth_headers 生成的指纹头）
pub(crate) fn apply_copilot_optimization(
    auth_headers: &mut Vec<HeaderPair>,
    optimization: &CopilotOptimization,
    config: &CopilotOptimizerConfig,
) {
    let classification = &optimization.classification;
    for (name, value) in auth_headers.iter_mut() {
        match name.as_str() {
            "x-initiator" if config.request_classification => {
                *value = HeaderValue::from_static(classification.initiator);
            }
            "x-interaction-type" if classification.is_subagent => {
                // 子代理请求：conversation-subagent 不计 premium interaction
                *value = HeaderValue::from_static("conversation-subagent");
            }
            "x-request-id" | "x-agent-task-id" => {
                if let Some(ref det_id) = optimization.request_id {
                    if let Ok(hv) = HeaderValue::from_str(det_id) {
                        *value = hv;
                    }
                }
            }
            _ => {}
        }
    }

    // x-interaction-id：仅在有 session 时注入（不在 get_auth_headers 中）
    if let Some(ref iid) = optimization.interaction_id {
        if let Ok(hv) = HeaderValue::from_str(iid) {
            auth_headers.push((HeaderName::from_static("x-interaction-id"), hv));
        }
    }

    if classification.is_subagent {
        log::info!(
            "[Copilot] 子代理请求: x-initiator=agent, x-interaction-type=conversation-subagent"
        );
    }
}

/// 策略之后的定稿：Codex OAuth 会话头、content-type 默认值、provider 级头覆盖，
/// 最后拒绝把代理占位凭据发往托管账号上游
pub(crate) fn finalize(
    headers: &mut HeaderMap,
    codex_oauth_session_headers: Vec<HeaderPair>,
    provider: &Provider,
    is_copilot: bool,
    url: &str,
) -> Result<(), ProxyError> {
    // Codex OAuth 反代尽量对齐官方 Codex CLI 的会话路由信号。
    for (name, value) in codex_oauth_session_headers {
        headers.insert(name, value);
    }

    // 确保 content-type 存在
    if !headers.contains_key(http::header::CONTENT_TYPE) {
        headers.insert(
            http::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
    }

    let overrides = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.local_proxy_request_overrides.as_ref());
    apply_local_proxy_header_overrides(headers, overrides, is_copilot);

    reject_proxy_placeholder_for_managed_account_upstream(url, headers)
}

/// Codex OAuth 会话头。只发送客户端提供的 session_id；生成的 UUID 每次不同，
/// 反而会破坏前缀缓存。
pub(crate) fn build_codex_oauth_session_headers(session_id: &str) -> Vec<HeaderPair> {
    let session_id = session_id.trim();
    if session_id.is_empty() {
        return Vec::new();
    }

    let mut headers = Vec::new();
    if let Ok(value) = HeaderValue::from_str(session_id) {
        headers.push((HeaderName::from_static("session_id"), value.clone()));
        headers.push((HeaderName::from_static("x-client-request-id"), value));
    }

    let window_id = format!("{session_id}:0");
    if let Ok(value) = HeaderValue::from_str(&window_id) {
        headers.push((HeaderName::from_static("x-codex-window-id"), value));
    }

    headers
}

/// Headers a native Claude Code client never sends but the Codex/OpenAI CLI (and its
/// stainless SDK layer) do. Dropped for every Codex→Anthropic request so the upstream sees a
/// clean Anthropic client fingerprint. `key_str` is already lowercased by the http crate.
fn is_codex_client_fingerprint_header(key_str: &str) -> bool {
    matches!(
        key_str,
        "originator"
            | "session_id"
            | "session-id"
            | "thread-id"
            | "conversation_id"
            | "chatgpt-account-id"
            | "x-openai-subagent"
            | "x-client-request-id"
            | "openai-beta"
            | "openai-organization"
            | "openai-project"
    ) || key_str.starts_with("x-stainless-")
        || key_str.starts_with("x-codex-")
}

fn reject_proxy_placeholder_for_managed_account_upstream(
    url: &str,
    headers: &HeaderMap,
) -> Result<(), ProxyError> {
    if !is_managed_account_upstream_url(url) || !headers_contain_proxy_placeholder(headers) {
        return Ok(());
    }

    Err(ProxyError::AuthError(
        "Managed account proxy auth was not resolved; PROXY_MANAGED must not be sent upstream"
            .to_string(),
    ))
}

fn is_managed_account_upstream_url(url: &str) -> bool {
    let Ok(uri) = url.parse::<http::Uri>() else {
        return false;
    };

    let Some(host) = uri.host().map(str::to_ascii_lowercase) else {
        return false;
    };

    host == "githubcopilot.com"
        || host.ends_with(".githubcopilot.com")
        || (host == "chatgpt.com" && uri.path().starts_with("/backend-api/codex"))
}

fn headers_contain_proxy_placeholder(headers: &HeaderMap) -> bool {
    headers.values().any(|value| {
        value
            .to_str()
            .map(crate::proxy::inbound_auth::contains_proxy_token)
            .unwrap_or(false)
    })
}

fn apply_local_proxy_header_overrides(
    headers: &mut HeaderMap,
    overrides: Option<&LocalProxyRequestOverrides>,
    is_copilot: bool,
) {
    if is_copilot {
        return;
    }

    let Some(header_overrides) = overrides.map(|overrides| &overrides.headers) else {
        return;
    };

    for (raw_name, raw_value) in header_overrides {
        let header_name = raw_name.trim().to_ascii_lowercase();
        if header_name.is_empty() {
            log::warn!("[LocalProxyOverrides] Ignoring header override with empty name");
            continue;
        }

        let Ok(name) = HeaderName::from_bytes(header_name.as_bytes()) else {
            log::warn!("[LocalProxyOverrides] Ignoring invalid header override name: {raw_name}");
            continue;
        };

        if is_protected_local_proxy_override_header(&name) {
            log::debug!(
                "[LocalProxyOverrides] Ignoring protected header override: {}",
                name.as_str()
            );
            continue;
        }

        let Ok(value) = HeaderValue::from_str(raw_value) else {
            log::warn!(
                "[LocalProxyOverrides] Ignoring invalid header override value for {}",
                name.as_str()
            );
            continue;
        };

        headers.insert(name, value);
    }
}

fn is_protected_local_proxy_override_header(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "host"
            | "connection"
            | "proxy-authorization"
            | "proxy-authenticate"
            | "te"
            | "trailer"
            | "upgrade"
            | "accept-encoding"
            | "content-type"
            | "chatgpt-account-id"
            | "session_id"
            | "x-client-request-id"
            | "x-codex-window-id"
    ) || CREDENTIAL_HEADERS.contains(&name.as_str())
        || HOP_AND_TRACE_HEADERS.contains(&name.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header::HeaderValue;
    use std::collections::HashMap;

    fn input() -> HeaderPolicyInput {
        HeaderPolicyInput {
            upstream_host: Some("upstream.example".to_string()),
            auth_headers: vec![(
                http::header::AUTHORIZATION,
                HeaderValue::from_static("Bearer upstream"),
            )],
            codex_official_auth_passthrough: false,
            codex_responses_to_anthropic: false,
            codex_impersonate_claude_code: false,
            is_copilot: false,
            force_identity_encoding: false,
            custom_user_agent: None,
            anthropic_beta: None,
            send_anthropic_headers: false,
        }
    }

    fn incoming(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.append(
                HeaderName::from_static(name),
                HeaderValue::from_static(value),
            );
        }
        map
    }

    fn rendered(headers: &HeaderMap) -> Vec<String> {
        headers
            .iter()
            .map(|(name, value)| format!("{}: {}", name, value.to_str().unwrap()))
            .collect()
    }

    #[test]
    fn credentials_are_replaced_once_at_the_first_credential_position() {
        let policy = HeaderPolicy::for_route(input());
        let out = policy.apply(&incoming(&[
            ("host", "client.local"),
            ("x-custom", "1"),
            ("x-api-key", "sk-client"),
            ("traceparent", "00-abc"),
            ("authorization", "Bearer client"),
            ("accept-encoding", "gzip"),
            ("accept-encoding", "br"),
        ]));

        assert_eq!(
            rendered(&out),
            [
                "host: upstream.example",
                "x-custom: 1",
                "authorization: Bearer upstream",
                "accept-encoding: gzip",
            ]
        );
    }

    #[test]
    fn missing_slots_are_appended_in_declaration_order() {
        let policy = HeaderPolicy::for_route(HeaderPolicyInput {
            codex_responses_to_anthropic: true,
            codex_impersonate_claude_code: true,
            force_identity_encoding: true,
            custom_user_agent: Some(HeaderValue::from_static("ua/1")),
            anthropic_beta: Some("claude-code-20250219".to_string()),
            ..input()
        });
        let out = policy.apply(&incoming(&[
            ("x-app", "codex"),
            ("originator", "codex_cli_rs"),
            ("anthropic-version", "2024-01-01"),
        ]));

        assert_eq!(
            rendered(&out),
            [
                "authorization: Bearer upstream",
                "accept-encoding: identity",
                "accept: application/json",
                "x-app: cli",
                "user-agent: ua/1",
                "anthropic-beta: claude-code-20250219",
                "anthropic-version: 2023-06-01",
            ]
        );
    }

    #[test]
    fn codex_official_passes_client_authorization_through() {
        let policy = HeaderPolicy::for_route(HeaderPolicyInput {
            auth_headers: Vec::new(),
            codex_official_auth_passthrough: true,
            ..input()
        });
        let out = policy.apply(&incoming(&[
            ("authorization", "Bearer chatgpt"),
            ("x-api-key", "sk-other"),
        ]));

        assert_eq!(rendered(&out), ["authorization: Bearer chatgpt"]);
    }

    #[test]
    fn native_anthropic_keeps_client_version_and_unsent_beta_is_dropped() {
        let native = HeaderPolicy::for_route(HeaderPolicyInput {
            send_anthropic_headers: true,
            ..input()
        });
        let out = native.apply(&incoming(&[("anthropic-version", "2024-01-01")]));
        assert_eq!(
            rendered(&out),
            [
                "anthropic-version: 2024-01-01",
                "authorization: Bearer upstream"
            ]
        );

        let openai = HeaderPolicy::for_route(input());
        let out = openai.apply(&incoming(&[
            ("anthropic-beta", "x"),
            ("anthropic-version", "2023-06-01"),
        ]));
        assert_eq!(rendered(&out), ["authorization: Bearer upstream"]);
    }

    #[test]
    fn copilot_fingerprint_headers_come_only_from_auth_headers() {
        let policy = HeaderPolicy::for_route(HeaderPolicyInput {
            is_copilot: true,
            ..input()
        });
        let out = policy.apply(&incoming(&[
            ("user-agent", "claude-cli/2.0"),
            ("editor-version", "vscode/1"),
            ("x-initiator", "user"),
            ("accept", "application/json"),
        ]));

        assert_eq!(
            rendered(&out),
            ["accept: application/json", "authorization: Bearer upstream"]
        );
    }

    #[test]
    fn anthropic_beta_value_adds_claude_code_marker_once() {
        let headers = incoming(&[("anthropic-beta", "fine-grained-tool-streaming-2025-05-14")]);
        assert_eq!(
            anthropic_beta_value(&headers, true, false, false).as_deref(),
            Some("claude-code-20250219,fine-grained-tool-streaming-2025-05-14")
        );
        let headers = incoming(&[("anthropic-beta", "claude-code-20250219")]);
        assert_eq!(
            anthropic_beta_value(&headers, true, false, false).as_deref(),
            Some("claude-code-20250219")
        );
        assert_eq!(
            anthropic_beta_value(&HeaderMap::new(), false, true, true).as_deref(),
            Some("claude-code-20250219,context-1m-2025-08-07")
        );
        assert!(anthropic_beta_value(&headers, false, false, false).is_none());
    }

    #[test]
    fn local_proxy_header_overrides_replace_allowed_headers_only() {
        let mut headers = http::HeaderMap::new();
        headers.insert(
            http::header::USER_AGENT,
            http::HeaderValue::from_static("original"),
        );
        headers.insert(
            http::header::AUTHORIZATION,
            http::HeaderValue::from_static("Bearer good"),
        );
        headers.insert(
            http::header::CONTENT_TYPE,
            http::HeaderValue::from_static("application/json"),
        );

        let overrides = LocalProxyRequestOverrides {
            headers: HashMap::from([
                ("User-Agent".to_string(), "custom".to_string()),
                ("X-Test".to_string(), "ok".to_string()),
                ("Authorization".to_string(), "Bearer bad".to_string()),
                ("Content-Type".to_string(), "text/plain".to_string()),
                ("X-Bad".to_string(), "bad\nvalue".to_string()),
            ]),
            body: None,
        };

        apply_local_proxy_header_overrides(&mut headers, Some(&overrides), false);

        assert_eq!(
            headers
                .get(http::header::USER_AGENT)
                .and_then(|value| value.to_str().ok()),
            Some("custom")
        );
        assert_eq!(
            headers
                .get(http::header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer good")
        );
        assert_eq!(
            headers
                .get(http::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("application/json")
        );
        assert_eq!(
            headers.get("x-test").and_then(|value| value.to_str().ok()),
            Some("ok")
        );
        assert!(headers.get("x-bad").is_none());
    }

    #[test]
    fn local_proxy_header_overrides_are_skipped_for_copilot() {
        let mut headers = http::HeaderMap::new();
        headers.insert(
            http::header::USER_AGENT,
            http::HeaderValue::from_static("copilot"),
        );
        let overrides = LocalProxyRequestOverrides {
            headers: HashMap::from([("User-Agent".to_string(), "custom".to_string())]),
            body: None,
        };

        apply_local_proxy_header_overrides(&mut headers, Some(&overrides), true);

        assert_eq!(
            headers
                .get(http::header::USER_AGENT)
                .and_then(|value| value.to_str().ok()),
            Some("copilot")
        );
    }

    #[test]
    fn codex_oauth_session_headers_match_codex_cache_identity() {
        let headers = build_codex_oauth_session_headers("session-123");
        let mut map = HeaderMap::new();
        for (name, value) in headers {
            map.insert(name, value);
        }

        assert_eq!(
            map.get("session_id"),
            Some(&HeaderValue::from_static("session-123"))
        );
        assert_eq!(
            map.get("x-client-request-id"),
            Some(&HeaderValue::from_static("session-123"))
        );
        assert_eq!(
            map.get("x-codex-window-id"),
            Some(&HeaderValue::from_static("session-123:0"))
        );
    }

    #[test]
    fn managed_account_upstream_rejects_proxy_managed_placeholder_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer PROXY_MANAGED"),
        );

        let err = reject_proxy_placeholder_for_managed_account_upstream(
            "https://api.githubcopilot.com/chat/completions",
            &headers,
        )
        .expect_err("placeholder should be rejected before upstream");

        assert!(matches!(
            err,
            ProxyError::AuthError(message) if message.contains("PROXY_MANAGED")
        ));
    }

    #[test]
    fn codex_oauth_upstream_rejects_proxy_managed_placeholder_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer PROXY_MANAGED"),
        );

        let err = reject_proxy_placeholder_for_managed_account_upstream(
            "https://chatgpt.com/backend-api/codex/responses",
            &headers,
        )
        .expect_err("placeholder should be rejected before upstream");

        assert!(matches!(
            err,
            ProxyError::AuthError(message) if message.contains("PROXY_MANAGED")
        ));
    }

    #[test]
    fn non_managed_upstream_allows_proxy_managed_placeholder_guard() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer PROXY_MANAGED"),
        );

        reject_proxy_placeholder_for_managed_account_upstream(
            "https://api.example.com/v1/messages",
            &headers,
        )
        .expect("guard is scoped to managed-account upstreams");
    }

    #[test]
    fn codex_client_fingerprint_headers_are_dropped_for_anthropic_upstreams() {
        // Codex/OpenAI fingerprints a native Claude Code client never sends → must drop.
        for header in [
            "originator",
            "session_id",
            "session-id",
            "thread-id",
            "conversation_id",
            "chatgpt-account-id",
            "x-openai-subagent",
            "x-client-request-id",
            "x-codex-window-id",
            "openai-beta",
            "openai-organization",
            "openai-project",
            "x-stainless-lang",
            "x-stainless-runtime",
            "x-codex-turn-id",
        ] {
            assert!(
                is_codex_client_fingerprint_header(header),
                "expected {header} to be dropped while impersonating Claude Code"
            );
        }

        // Headers a real Claude Code client sends (or that the forwarder rebuilds) must
        // NOT be caught by the denylist.
        for header in [
            "anthropic-version",
            "anthropic-beta",
            "user-agent",
            "accept",
            "content-type",
            "x-app",
        ] {
            assert!(
                !is_codex_client_fingerprint_header(header),
                "{header} must be preserved while impersonating Claude Code"
            );
        }
    }
}
