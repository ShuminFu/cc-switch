//! 同一 provider 上的请求恢复策略
//!
//! 上游以「请求体不被接受」类错误拒绝时，按 [`RecoveryStrategy::ORDER`] 依次检查
//! 可用的恢复策略。命中的策略就地改写请求体，由 forwarder 的故障转移循环对同一
//! provider 重新发送一次——forwarder 只有一个 `forward()` 调用点，不再在错误分支里
//! 嵌套重试。
//!
//! 每个策略在一次尝试（同一 provider）内最多应用一次；已应用的策略按顺序记录在
//! [`RecoveryState`] 中，并在日志中以 `[Recovery] ... trace=[...]` 输出，供请求追踪使用。

use crate::proxy::error::ProxyError;
use crate::proxy::media_sanitizer;
use crate::proxy::thinking_budget_rectifier::{
    rectify_thinking_budget, should_rectify_thinking_budget,
};
use crate::proxy::thinking_rectifier::{
    rectify_anthropic_request, should_rectify_thinking_signature,
};
use crate::proxy::types::RectifierConfig;
use serde_json::Value;

/// 恢复策略
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveryStrategy {
    /// 上游拒绝图片输入：把图片块替换为 [`media_sanitizer::UNSUPPORTED_IMAGE_MARKER`]
    MediaDowngrade,
    /// thinking 签名校验失败：移除 thinking / redacted_thinking 块与签名字段
    ThinkingSignatureRectifier,
    /// thinking budget 约束失败：把 budget_tokens / max_tokens 调整到合法值
    ThinkingBudgetRectifier,
}

impl RecoveryStrategy {
    /// 检查顺序
    pub(crate) const ORDER: [RecoveryStrategy; 3] = [
        RecoveryStrategy::MediaDowngrade,
        RecoveryStrategy::ThinkingSignatureRectifier,
        RecoveryStrategy::ThinkingBudgetRectifier,
    ];

    /// 稳定的追踪标识
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            RecoveryStrategy::MediaDowngrade => "media_downgrade",
            RecoveryStrategy::ThinkingSignatureRectifier => "thinking_signature_rectifier",
            RecoveryStrategy::ThinkingBudgetRectifier => "thinking_budget_rectifier",
        }
    }

    /// 写入 `last_error` 的展示名
    pub(crate) fn label(self) -> &'static str {
        match self {
            RecoveryStrategy::MediaDowngrade => "media 降级",
            RecoveryStrategy::ThinkingSignatureRectifier => "整流",
            RecoveryStrategy::ThinkingBudgetRectifier => "budget 整流",
        }
    }

    /// 恢复重试成功后的日志
    pub(crate) fn log_retry_succeeded(self, app_type: &str) {
        match self {
            RecoveryStrategy::MediaDowngrade => {
                log::info!("[{app_type}] [Media] Unsupported-image retry succeeded")
            }
            RecoveryStrategy::ThinkingSignatureRectifier => {
                log::info!("[{app_type}] [RECT-002] 整流重试成功")
            }
            RecoveryStrategy::ThinkingBudgetRectifier => {
                log::info!("[{app_type}] [RECT-011] budget 整流重试成功")
            }
        }
    }

    /// 恢复重试仍失败（且没有其他策略可用）时的日志
    pub(crate) fn log_retry_failed(self, app_type: &str, error: &ProxyError) {
        match self {
            RecoveryStrategy::MediaDowngrade => {
                log::warn!("[{app_type}] [Media] Unsupported-image retry still failed: {error}")
            }
            RecoveryStrategy::ThinkingSignatureRectifier => {
                log::warn!("[{app_type}] [RECT-003] 整流重试仍失败: {error}")
            }
            RecoveryStrategy::ThinkingBudgetRectifier => {
                log::warn!("[{app_type}] [RECT-012] budget 整流重试仍失败: {error}")
            }
        }
    }
}

/// 一次失败的上游响应及其判定所需的上下文
pub(crate) struct RecoveryInput<'a> {
    pub app_type: &'a str,
    pub provider_id: &'a str,
    /// 适配器名（"Claude" / "Codex" / ...）
    pub adapter_name: &'a str,
    /// 供应商是否走 Anthropic 原生协议（thinking 整流器只对它生效）
    pub is_anthropic_provider: bool,
    pub config: &'a RectifierConfig,
    pub error: &'a ProxyError,
}

/// [`RecoveryState::recover`] 的判定结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveryDecision {
    /// 策略已改写请求体，应对同一 provider 重新发送
    Retry(RecoveryStrategy),
    /// 有策略识别出该错误，但无法（或已不能再）修复：这是客户端兼容性错误，
    /// 换哪家 provider 都会被拒绝，应直接返回给客户端
    ClientError,
    /// 没有策略识别该错误，按常规错误分类处理
    Unhandled,
}

/// 单个策略对一次错误的处理结果
enum StrategyOutcome {
    NotTriggered,
    Applied,
    /// 识别出错误但无法修复
    Exhausted,
}

/// 媒体降级的触发条件：上游因图片输入报错，且请求体里还有图片块。
///
/// 受 `enabled && request_media_fallback` 管辖；不涉及 `request_media_heuristic`——
/// 这里是上游"实测"错误后的纯恢复，不是预测，故启发式开关与它无关。
pub(crate) fn media_downgrade_should_trigger(
    config: &RectifierConfig,
    adapter_name: &str,
    provider_body: &Value,
    error: &ProxyError,
) -> bool {
    matches!(adapter_name, "Claude" | "Codex")
        && config.enabled
        && config.request_media_fallback
        && media_sanitizer::contains_image_blocks(provider_body)
        && media_sanitizer::is_unsupported_image_error(error)
}

/// 从 ProxyError 中提取错误消息
fn extract_error_message(error: &ProxyError) -> Option<String> {
    match error {
        ProxyError::UpstreamError { body, .. } => body.clone(),
        _ => Some(error.to_string()),
    }
}

/// 一次 provider 尝试内的恢复状态（已应用的策略，按应用顺序）
#[derive(Debug, Default)]
pub(crate) struct RecoveryState {
    applied: Vec<RecoveryStrategy>,
}

impl RecoveryState {
    /// 已应用的策略，按应用顺序
    pub(crate) fn applied(&self) -> &[RecoveryStrategy] {
        &self.applied
    }

    /// 最近一次应用的策略
    pub(crate) fn last(&self) -> Option<RecoveryStrategy> {
        self.applied.last().copied()
    }

    /// 追踪用的策略列表，例如 `media_downgrade,thinking_signature_rectifier`
    pub(crate) fn trace(&self) -> String {
        self.applied
            .iter()
            .map(|strategy| strategy.as_str())
            .collect::<Vec<_>>()
            .join(",")
    }

    fn is_applied(&self, strategy: RecoveryStrategy) -> bool {
        self.applied.contains(&strategy)
    }

    /// 按 [`RecoveryStrategy::ORDER`] 检查上游错误，第一个能改写 `body` 的策略生效
    pub(crate) fn recover(
        &mut self,
        input: &RecoveryInput<'_>,
        body: &mut Value,
    ) -> RecoveryDecision {
        let error_message = extract_error_message(input.error);
        let mut exhausted = false;

        for strategy in RecoveryStrategy::ORDER {
            match self.try_strategy(strategy, input, error_message.as_deref(), body) {
                StrategyOutcome::NotTriggered => {}
                StrategyOutcome::Exhausted => exhausted = true,
                StrategyOutcome::Applied => {
                    self.applied.push(strategy);
                    log::info!(
                        "[{}] [Recovery] provider={} strategy={} trace=[{}]",
                        input.app_type,
                        input.provider_id,
                        strategy.as_str(),
                        self.trace()
                    );
                    return RecoveryDecision::Retry(strategy);
                }
            }
        }

        if exhausted {
            RecoveryDecision::ClientError
        } else {
            RecoveryDecision::Unhandled
        }
    }

    fn try_strategy(
        &self,
        strategy: RecoveryStrategy,
        input: &RecoveryInput<'_>,
        error_message: Option<&str>,
        body: &mut Value,
    ) -> StrategyOutcome {
        let app_type = input.app_type;
        match strategy {
            RecoveryStrategy::MediaDowngrade => {
                // 降级后请求体已不含图片块，同一策略不会再次触发
                if self.is_applied(strategy)
                    || !media_downgrade_should_trigger(
                        input.config,
                        input.adapter_name,
                        body,
                        input.error,
                    )
                {
                    return StrategyOutcome::NotTriggered;
                }
                let replaced_images = media_sanitizer::replace_image_blocks_with_marker(body);
                if replaced_images == 0 {
                    return StrategyOutcome::NotTriggered;
                }
                let model = body.get("model").and_then(Value::as_str).unwrap_or("");
                log::info!(
                    "[{app_type}] [Media] Upstream rejected image input; retrying provider={} model={} with {replaced_images} image block(s) replaced by {}",
                    input.provider_id,
                    model,
                    media_sanitizer::UNSUPPORTED_IMAGE_MARKER
                );
                StrategyOutcome::Applied
            }
            RecoveryStrategy::ThinkingSignatureRectifier => {
                if !input.is_anthropic_provider
                    || !should_rectify_thinking_signature(error_message, input.config)
                {
                    return StrategyOutcome::NotTriggered;
                }
                if self.is_applied(strategy) {
                    log::warn!("[{app_type}] [RECT-005] 整流器已触发过，不再重试");
                    return StrategyOutcome::Exhausted;
                }
                let rectified = rectify_anthropic_request(body);
                if !rectified.applied {
                    log::warn!(
                        "[{app_type}] [RECT-006] thinking 签名整流器触发但无可整流内容，继续检查 budget；若 budget 也未命中则按客户端错误返回"
                    );
                    return StrategyOutcome::Exhausted;
                }
                log::info!(
                    "[{}] [RECT-001] thinking 签名整流器触发, 移除 {} thinking blocks, {} redacted_thinking blocks, {} signature fields",
                    app_type,
                    rectified.removed_thinking_blocks,
                    rectified.removed_redacted_thinking_blocks,
                    rectified.removed_signature_fields
                );
                StrategyOutcome::Applied
            }
            RecoveryStrategy::ThinkingBudgetRectifier => {
                if !input.is_anthropic_provider
                    || !should_rectify_thinking_budget(error_message, input.config)
                {
                    return StrategyOutcome::NotTriggered;
                }
                if self.is_applied(strategy) {
                    log::warn!("[{app_type}] [RECT-013] budget 整流器已触发过，不再重试");
                    return StrategyOutcome::Exhausted;
                }
                let budget_rectified = rectify_thinking_budget(body);
                if !budget_rectified.applied {
                    log::warn!(
                        "[{app_type}] [RECT-014] budget 整流器触发但无可整流内容，不做无意义重试"
                    );
                    return StrategyOutcome::Exhausted;
                }
                log::info!(
                    "[{}] [RECT-010] thinking budget 整流器触发, before={:?}, after={:?}",
                    app_type,
                    budget_rectified.before,
                    budget_rectified.after
                );
                StrategyOutcome::Applied
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn upstream_400(message: &str) -> ProxyError {
        ProxyError::UpstreamError {
            status: 400,
            body: Some(json!({ "error": { "message": message } }).to_string()),
        }
    }

    fn input<'a>(config: &'a RectifierConfig, error: &'a ProxyError) -> RecoveryInput<'a> {
        RecoveryInput {
            app_type: "claude",
            provider_id: "p1",
            adapter_name: "Claude",
            is_anthropic_provider: true,
            config,
            error,
        }
    }

    fn image_body() -> Value {
        json!({
            "model": "m",
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "thinking", "thinking": "t", "signature": "s" },
                    { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "abc" } }
                ]
            }]
        })
    }

    const SIGNATURE_ERROR: &str = "messages.0.content.0: Invalid `signature` in `thinking` block";
    const BUDGET_ERROR: &str =
        "thinking.enabled.budget_tokens: Input should be greater than or equal to 1024";

    #[test]
    fn strategies_chain_and_each_applies_at_most_once() {
        let config = RectifierConfig::default();
        let mut state = RecoveryState::default();
        let mut body = image_body();

        let image_error = upstream_400("This model does not support image input");
        assert_eq!(
            state.recover(&input(&config, &image_error), &mut body),
            RecoveryDecision::Retry(RecoveryStrategy::MediaDowngrade)
        );
        assert!(!media_sanitizer::contains_image_blocks(&body));

        let signature_error = upstream_400(SIGNATURE_ERROR);
        assert_eq!(
            state.recover(&input(&config, &signature_error), &mut body),
            RecoveryDecision::Retry(RecoveryStrategy::ThinkingSignatureRectifier)
        );
        // 同一策略不会第二次应用：再次遇到签名错误按客户端错误返回
        assert_eq!(
            state.recover(&input(&config, &signature_error), &mut body),
            RecoveryDecision::ClientError
        );
        assert_eq!(
            state.applied(),
            &[
                RecoveryStrategy::MediaDowngrade,
                RecoveryStrategy::ThinkingSignatureRectifier
            ]
        );
        assert_eq!(
            state.trace(),
            "media_downgrade,thinking_signature_rectifier"
        );
    }

    #[test]
    fn budget_rectifier_applies_once_then_gives_up() {
        let config = RectifierConfig::default();
        let mut state = RecoveryState::default();
        let mut body = json!({
            "model": "m",
            "max_tokens": 512,
            "thinking": { "type": "enabled", "budget_tokens": 256 },
            "messages": []
        });
        let error = upstream_400(BUDGET_ERROR);

        assert_eq!(
            state.recover(&input(&config, &error), &mut body),
            RecoveryDecision::Retry(RecoveryStrategy::ThinkingBudgetRectifier)
        );
        assert_eq!(body["thinking"]["budget_tokens"], 32000);
        assert_eq!(
            state.recover(&input(&config, &error), &mut body),
            RecoveryDecision::ClientError
        );
        assert_eq!(
            state.last(),
            Some(RecoveryStrategy::ThinkingBudgetRectifier)
        );
    }

    #[test]
    fn signature_error_without_rectifiable_content_is_a_client_error() {
        let config = RectifierConfig::default();
        let mut state = RecoveryState::default();
        let mut body = json!({ "model": "m", "messages": [{ "role": "user", "content": "hi" }] });
        let error = upstream_400(SIGNATURE_ERROR);

        assert_eq!(
            state.recover(&input(&config, &error), &mut body),
            RecoveryDecision::ClientError
        );
        assert!(state.applied().is_empty());
    }

    #[test]
    fn thinking_rectifiers_skip_non_anthropic_providers() {
        let config = RectifierConfig::default();
        let mut state = RecoveryState::default();
        let mut body = image_body();
        let error = upstream_400(SIGNATURE_ERROR);
        let mut recovery_input = input(&config, &error);
        recovery_input.is_anthropic_provider = false;

        assert_eq!(
            state.recover(&recovery_input, &mut body),
            RecoveryDecision::Unhandled
        );
        assert_eq!(body, image_body());
    }

    #[test]
    fn unrelated_errors_are_unhandled() {
        let config = RectifierConfig::default();
        let mut state = RecoveryState::default();
        let mut body = image_body();
        let error = ProxyError::UpstreamError {
            status: 500,
            body: Some("boom".to_string()),
        };

        assert_eq!(
            state.recover(&input(&config, &error), &mut body),
            RecoveryDecision::Unhandled
        );
        assert!(state.applied().is_empty());
    }

    #[test]
    fn disabled_rectifiers_never_trigger() {
        let config = RectifierConfig {
            enabled: false,
            ..RectifierConfig::default()
        };
        let mut state = RecoveryState::default();
        let mut body = image_body();
        for message in [
            "This model does not support image input",
            SIGNATURE_ERROR,
            BUDGET_ERROR,
        ] {
            let error = upstream_400(message);
            assert_eq!(
                state.recover(&input(&config, &error), &mut body),
                RecoveryDecision::Unhandled
            );
        }
        assert_eq!(body, image_body());
    }
}
