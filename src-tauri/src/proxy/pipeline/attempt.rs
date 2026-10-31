//! Attempt stage types: what one provider attempt produced, and the pure
//! decision whether a rectifier can rewrite the request for a same-provider retry.

use crate::proxy::hyper_client::ProxyResponse;
use crate::proxy::media_sanitizer;
use crate::proxy::thinking_budget_rectifier::{
    rectify_thinking_budget_with_ceiling, should_rectify_thinking_budget,
};
use crate::proxy::thinking_rectifier::{
    rectify_anthropic_request, should_rectify_thinking_signature,
};
use crate::proxy::types::RectifierConfig;
use crate::proxy::ProxyError;
use serde_json::Value;
use std::borrow::Cow;

/// Result of one provider's attempt (including its optional rectifier retry).
pub enum AttemptOutcome {
    /// The provider served the request.
    Served {
        response: ProxyResponse,
        claude_api_format: Option<String>,
        outbound_model: Option<String>,
    },
    /// A provider-specific failure (already recorded against the breaker):
    /// try the next provider, starting from `carried_body` when a
    /// provider-independent rectification applies.
    Failover {
        error: ProxyError,
        carried_body: Option<Value>,
    },
    /// A client-side failure no provider can fix: return it to the client.
    Abort { error: ProxyError },
}

impl AttemptOutcome {
    pub fn served(
        (response, claude_api_format, outbound_model): (
            ProxyResponse,
            Option<String>,
            Option<String>,
        ),
    ) -> Self {
        Self::Served {
            response,
            claude_api_format,
            outbound_model,
        }
    }
}

/// The request rectifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RectifierKind {
    /// Image blocks replaced by a text marker after an "image not supported" error.
    Media,
    /// Thinking blocks / signatures removed after a signature error.
    Signature,
    /// Thinking budget raised to the minimum after a budget constraint error.
    Budget,
}

impl RectifierKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Media => "media 降级",
            Self::Signature => "整流",
            Self::Budget => "budget 整流",
        }
    }

    pub fn log_code(&self) -> &'static str {
        match self {
            Self::Media => "Media",
            Self::Signature => "RECT-001",
            Self::Budget => "RECT-010",
        }
    }

    /// Signature/budget rewrites fix the request itself, so the next provider
    /// starts from the rewritten body (PROXY-24). The media fallback depends on
    /// the model's capabilities and is not carried.
    pub fn carries_to_next_provider(&self) -> bool {
        !matches!(self, Self::Media)
    }

    /// Re-apply this rectification to another copy of the body.
    pub fn reapply(&self, body: &mut Value, max_output_tokens: Option<u64>) {
        match self {
            Self::Media => {
                media_sanitizer::replace_image_blocks_with_marker(body);
            }
            Self::Signature => {
                rectify_anthropic_request(body);
            }
            Self::Budget => {
                rectify_thinking_budget_with_ceiling(body, max_output_tokens);
            }
        }
    }
}

/// What to do with a failed attempt before classifying it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RectifyDecision {
    /// The body was rewritten: retry the same provider once.
    Retry(RectifierKind),
    /// A rectifier recognised the error but found nothing to rewrite: the
    /// request is invalid for every provider.
    RejectClient,
    /// No rectifier applies; classify the error normally.
    NotApplicable,
}

/// Upstream error text the rectifiers match on.
pub fn extract_error_message(error: &ProxyError) -> Option<String> {
    match error {
        ProxyError::UpstreamError { body, .. } => body.clone(),
        _ => Some(error.to_string()),
    }
}

/// Decide whether `error` can be fixed by rewriting `body` (rewritten in place
/// on [`RectifyDecision::Retry`]). Order: media fallback, thinking signature,
/// thinking budget — the first that applies wins.
pub fn plan_rectification(
    config: &RectifierConfig,
    adapter_name: &str,
    is_anthropic_provider: bool,
    error: &ProxyError,
    body: &mut Cow<'_, Value>,
    max_output_tokens: Option<u64>,
) -> RectifyDecision {
    if matches!(adapter_name, "Claude" | "Codex")
        && config.enabled
        && config.request_media_fallback
        && media_sanitizer::contains_image_blocks(body)
        && media_sanitizer::is_unsupported_image_error(error)
    {
        let mut media_body = body.as_ref().clone();
        if media_sanitizer::replace_image_blocks_with_marker(&mut media_body) > 0 {
            *body = Cow::Owned(media_body);
            return RectifyDecision::Retry(RectifierKind::Media);
        }
    }

    if !is_anthropic_provider {
        return RectifyDecision::NotApplicable;
    }
    let message = extract_error_message(error);

    let mut signature_matched_without_rewrite = false;
    if should_rectify_thinking_signature(message.as_deref(), config) {
        let rectified = rectify_anthropic_request(body.to_mut());
        if rectified.applied {
            log::info!(
                "[RECT-001] thinking signature rectifier removed {} thinking, {} redacted_thinking blocks and {} signature fields",
                rectified.removed_thinking_blocks,
                rectified.removed_redacted_thinking_blocks,
                rectified.removed_signature_fields
            );
            return RectifyDecision::Retry(RectifierKind::Signature);
        }
        log::warn!(
            "[RECT-006] thinking signature rectifier matched but found nothing to rewrite; checking budget"
        );
        signature_matched_without_rewrite = true;
    }

    if should_rectify_thinking_budget(message.as_deref(), config) {
        let rectified = rectify_thinking_budget_with_ceiling(body.to_mut(), max_output_tokens);
        if !rectified.applied {
            log::warn!("[RECT-014] thinking budget rectifier matched but found nothing to rewrite");
            return RectifyDecision::RejectClient;
        }
        log::info!(
            "[RECT-010] thinking budget rectifier: before={:?}, after={:?}",
            rectified.before,
            rectified.after
        );
        return RectifyDecision::Retry(RectifierKind::Budget);
    }

    if signature_matched_without_rewrite {
        RectifyDecision::RejectClient
    } else {
        RectifyDecision::NotApplicable
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn config() -> RectifierConfig {
        RectifierConfig {
            enabled: true,
            request_thinking_signature: true,
            request_thinking_budget: true,
            request_media_fallback: true,
            request_media_heuristic: true,
        }
    }

    fn upstream_400(message: &str) -> ProxyError {
        ProxyError::UpstreamError {
            status: 400,
            body: Some(
                json!({"type": "error", "error": {"type": "invalid_request_error", "message": message}})
                    .to_string(),
            ),
        }
    }

    fn thinking_body() -> Value {
        json!({
            "max_tokens": 16,
            "messages": [{"role": "assistant", "content": [
                {"type": "thinking", "thinking": "x", "signature": "s"},
                {"type": "text", "text": "hi"}
            ]}]
        })
    }

    #[test]
    fn signature_error_rewrites_the_body_for_a_retry() {
        let original = thinking_body();
        let mut body = Cow::Borrowed(&original);
        let decision = plan_rectification(
            &config(),
            "Claude",
            true,
            &upstream_400("Invalid `signature` in `thinking` block"),
            &mut body,
            None,
        );
        assert_eq!(decision, RectifyDecision::Retry(RectifierKind::Signature));
        assert!(matches!(body, Cow::Owned(_)));
        assert_eq!(body["messages"][0]["content"].as_array().unwrap().len(), 1);
        // The client's body is untouched.
        assert_eq!(
            original["messages"][0]["content"].as_array().unwrap().len(),
            2
        );
    }

    #[test]
    fn signature_error_with_nothing_to_rewrite_is_a_client_error() {
        let original = json!({"messages": [{"role": "user", "content": "hi"}]});
        let mut body = Cow::Borrowed(&original);
        let decision = plan_rectification(
            &config(),
            "Claude",
            true,
            &upstream_400("Invalid `signature` in `thinking` block"),
            &mut body,
            None,
        );
        assert_eq!(decision, RectifyDecision::RejectClient);
    }

    #[test]
    fn budget_error_respects_the_provider_ceiling() {
        let original =
            json!({"max_tokens": 600, "thinking": {"type": "enabled", "budget_tokens": 500}});
        let mut body = Cow::Borrowed(&original);
        let decision = plan_rectification(
            &config(),
            "Claude",
            true,
            &upstream_400("thinking.budget_tokens: Input should be greater than or equal to 1024"),
            &mut body,
            Some(4096),
        );
        assert_eq!(decision, RectifyDecision::Retry(RectifierKind::Budget));
        assert_eq!(body["thinking"]["budget_tokens"], 1024);
        assert_eq!(body["max_tokens"], 1025);
    }

    #[test]
    fn unrelated_errors_and_non_anthropic_providers_are_not_rectified() {
        let original = thinking_body();
        let mut body = Cow::Borrowed(&original);
        assert_eq!(
            plan_rectification(
                &config(),
                "Claude",
                true,
                &upstream_400("Invalid request: max_tokens too large"),
                &mut body,
                None
            ),
            RectifyDecision::NotApplicable
        );
        assert_eq!(
            plan_rectification(
                &config(),
                "Codex",
                false,
                &upstream_400("Invalid `signature` in `thinking` block"),
                &mut body,
                None
            ),
            RectifyDecision::NotApplicable
        );
        assert!(matches!(body, Cow::Borrowed(_)));
    }

    #[test]
    fn disabled_rectifiers_never_rewrite() {
        let mut disabled = config();
        disabled.enabled = false;
        let original = thinking_body();
        let mut body = Cow::Borrowed(&original);
        assert_eq!(
            plan_rectification(
                &disabled,
                "Claude",
                true,
                &upstream_400("Invalid `signature` in `thinking` block"),
                &mut body,
                None
            ),
            RectifyDecision::NotApplicable
        );
    }

    #[test]
    fn only_signature_and_budget_rewrites_are_carried() {
        assert!(RectifierKind::Signature.carries_to_next_provider());
        assert!(RectifierKind::Budget.carries_to_next_provider());
        assert!(!RectifierKind::Media.carries_to_next_provider());
    }
}
