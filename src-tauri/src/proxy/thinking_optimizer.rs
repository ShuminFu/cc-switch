//! Thinking 优化器

use super::types::OptimizerConfig;
use serde_json::{json, Value};

/// 根据模型类型自动优化 thinking 配置
///
/// 三路径分发：
/// - skip: haiku 模型直接跳过
/// - adaptive: current adaptive-thinking Claude models use adaptive thinking
/// - legacy: 其他模型注入 enabled thinking + budget_tokens
pub fn optimize(body: &mut Value, config: &OptimizerConfig) {
    if !config.thinking_optimizer {
        return;
    }

    let model = match body.get("model").and_then(|m| m.as_str()) {
        Some(m) => m.to_lowercase(),
        None => return,
    };

    if model.contains("haiku") {
        log::info!("[OPT] thinking: skip(haiku)");
        return;
    }

    let thinking_type = body
        .get("thinking")
        .and_then(|t| t.get("type"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string());

    // PROXY-47: a caller that explicitly disabled thinking (e.g. a cheap
    // subagent call) is respected; the optimizer only fills in what is absent.
    if thinking_type.as_deref() == Some("disabled") && !thinking_cannot_be_disabled(&model) {
        log::info!("[OPT] thinking: skip(caller disabled)");
        return;
    }

    if uses_adaptive_thinking(&model) {
        log::info!("[OPT] thinking: adaptive({model})");
        if thinking_type.as_deref() != Some("adaptive") {
            body["thinking"] = json!({"type": "adaptive"});
        }
        // Max effort and the 1M-context beta change cost/latency and are
        // rejected for accounts without 1M access: opt-in sub-switches only.
        if config.thinking_max_effort && body.get("output_config").is_none() {
            body["output_config"] = json!({"effort": "max"});
        }
        if config.context_1m_beta {
            append_beta(body, "context-1m-2025-08-07");
        }
        return;
    }

    // legacy path
    log::info!("[OPT] thinking: legacy({model})");

    let max_tokens = body
        .get("max_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(16384);

    let budget_target = injected_budget(max_tokens);

    match thinking_type.as_deref() {
        None => {
            if budget_target >= MIN_THINKING_BUDGET {
                body["thinking"] = json!({
                    "type": "enabled",
                    "budget_tokens": budget_target
                });
            }
            append_beta(body, "interleaved-thinking-2025-05-14");
        }
        Some("enabled") => {
            // Never raise a caller-provided budget; only fill a missing one.
            let has_budget = body
                .get("thinking")
                .and_then(|t| t.get("budget_tokens"))
                .and_then(|b| b.as_u64())
                .is_some();
            if !has_budget && budget_target >= MIN_THINKING_BUDGET {
                body["thinking"]["budget_tokens"] = json!(budget_target);
            }
            append_beta(body, "interleaved-thinking-2025-05-14");
        }
        _ => {
            append_beta(body, "interleaved-thinking-2025-05-14");
        }
    }
}

/// Anthropic's minimum `budget_tokens`.
const MIN_THINKING_BUDGET: u64 = 1024;
/// Upper bound for an injected thinking budget.
const MAX_INJECTED_THINKING_BUDGET: u64 = 32_000;

/// Budget injected when the caller sent none: three quarters of `max_tokens`
/// (so the visible answer keeps room), capped at 32k (PROXY-47).
fn injected_budget(max_tokens: u64) -> u64 {
    (max_tokens.saturating_mul(3) / 4).min(MAX_INJECTED_THINKING_BUDGET)
}

pub(crate) fn uses_adaptive_thinking(model: &str) -> bool {
    let normalized = normalize_model_name(model);
    [
        "fable-5",
        "mythos-5",
        "mythos-preview",
        "sonnet-5",
        "opus-4-8",
        "opus-4-7",
        "opus-4-6",
        "sonnet-4-6",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

/// Models where omitting `thinking` still leaves adaptive thinking enabled.
pub(crate) fn adaptive_thinking_is_default(model: &str) -> bool {
    let normalized = normalize_model_name(model);
    ["fable-5", "mythos-5", "mythos-preview", "sonnet-5"]
        .iter()
        .any(|needle| normalized.contains(needle))
}

/// Models that reject `thinking: {"type":"disabled"}`.
pub(crate) fn thinking_cannot_be_disabled(model: &str) -> bool {
    let normalized = normalize_model_name(model);
    ["fable-5", "mythos-5"]
        .iter()
        .any(|needle| normalized.contains(needle))
}

fn normalize_model_name(model: &str) -> String {
    model.trim().to_ascii_lowercase().replace(['.', '_'], "-")
}

/// 追加 beta 标识到 anthropic_beta 数组（去重）
fn append_beta(body: &mut Value, beta: &str) {
    match body.get_mut("anthropic_beta") {
        Some(Value::Array(arr)) => {
            if arr.iter().any(|v| v.as_str() == Some(beta)) {
                return;
            }
            arr.push(json!(beta));
        }
        Some(Value::Null) | None => {
            body["anthropic_beta"] = json!([beta]);
        }
        _ => {
            body["anthropic_beta"] = json!([beta]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn enabled_config() -> OptimizerConfig {
        OptimizerConfig {
            enabled: true,
            thinking_optimizer: true,
            cache_injection: true,
            thinking_max_effort: true,
            context_1m_beta: true,
        }
    }

    /// Default sub-switches: no max effort, no 1M beta.
    fn default_subswitch_config() -> OptimizerConfig {
        OptimizerConfig {
            enabled: true,
            ..OptimizerConfig::default()
        }
    }

    fn disabled_config() -> OptimizerConfig {
        OptimizerConfig {
            enabled: true,
            thinking_optimizer: false,
            cache_injection: true,
            ..OptimizerConfig::default()
        }
    }

    #[test]
    fn caller_budget_is_kept_and_injected_budget_is_capped() {
        let mut body = json!({
            "model": "anthropic.claude-sonnet-4-5-20250514-v1:0",
            "max_tokens": 4096,
            "thinking": {"type": "enabled", "budget_tokens": 1024},
        });
        optimize(&mut body, &default_subswitch_config());
        assert_eq!(body["thinking"]["budget_tokens"], 1024);

        let mut body = json!({
            "model": "anthropic.claude-sonnet-4-5-20250514-v1:0",
            "max_tokens": 4096,
        });
        optimize(&mut body, &default_subswitch_config());
        assert_eq!(body["thinking"]["budget_tokens"], 3072);

        let mut body = json!({
            "model": "anthropic.claude-sonnet-4-5-20250514-v1:0",
            "max_tokens": 128000,
        });
        optimize(&mut body, &default_subswitch_config());
        assert_eq!(body["thinking"]["budget_tokens"], 32000);
    }

    #[test]
    fn adaptive_extras_only_with_subswitches() {
        let mut body = json!({
            "model": "anthropic.claude-opus-4-6-20250514-v1:0",
            "max_tokens": 16384,
        });
        optimize(&mut body, &default_subswitch_config());
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert!(body.get("output_config").is_none());
        assert!(body.get("anthropic_beta").is_none());
    }

    #[test]
    fn caller_disabled_thinking_is_respected() {
        for model in [
            "anthropic.claude-sonnet-4-5-20250514-v1:0",
            "anthropic.claude-opus-4-6-20250514-v1:0",
        ] {
            let mut body = json!({"model": model, "thinking": {"type": "disabled"}});
            let original = body.clone();
            optimize(&mut body, &enabled_config());
            assert_eq!(body, original, "{model}");
        }
    }

    #[test]
    fn test_adaptive_opus_4_8() {
        let mut body = json!({
            "model": "anthropic/claude-opus-4.8",
            "max_tokens": 16384,
            "thinking": {"type": "enabled", "budget_tokens": 8000},
            "messages": [{"role": "user", "content": "hello"}]
        });

        optimize(&mut body, &enabled_config());

        assert_eq!(body["thinking"]["type"], "adaptive");
        assert!(body["thinking"].get("budget_tokens").is_none());
        assert_eq!(body["output_config"]["effort"], "max");
        let betas = body["anthropic_beta"].as_array().unwrap();
        assert!(betas.iter().any(|v| v == "context-1m-2025-08-07"));
    }

    #[test]
    fn current_generation_models_use_adaptive_thinking() {
        for model in [
            "claude-sonnet-5",
            "anthropic/claude-fable-5",
            "claude-mythos-5",
            "claude-opus-4.8",
        ] {
            assert!(uses_adaptive_thinking(model), "model={model}");
        }
        assert!(adaptive_thinking_is_default("claude-sonnet-5"));
        assert!(thinking_cannot_be_disabled("claude-fable-5"));
        assert!(!thinking_cannot_be_disabled("claude-sonnet-5"));
    }

    #[test]
    fn test_adaptive_opus_4_6() {
        let mut body = json!({
            "model": "anthropic.claude-opus-4-6-20250514-v1:0",
            "max_tokens": 16384,
            "thinking": {"type": "enabled", "budget_tokens": 8000},
            "messages": [{"role": "user", "content": "hello"}]
        });

        optimize(&mut body, &enabled_config());

        assert_eq!(body["thinking"]["type"], "adaptive");
        assert!(body["thinking"].get("budget_tokens").is_none());
        assert_eq!(body["output_config"]["effort"], "max");
        let betas = body["anthropic_beta"].as_array().unwrap();
        assert!(betas.iter().any(|v| v == "context-1m-2025-08-07"));
    }

    #[test]
    fn test_adaptive_sonnet_4_6() {
        let mut body = json!({
            "model": "anthropic.claude-sonnet-4-6-20250514-v1:0",
            "max_tokens": 16384,
            "messages": [{"role": "user", "content": "hello"}]
        });

        optimize(&mut body, &enabled_config());

        assert_eq!(body["thinking"]["type"], "adaptive");
        assert!(body["thinking"].get("budget_tokens").is_none());
        assert_eq!(body["output_config"]["effort"], "max");
        let betas = body["anthropic_beta"].as_array().unwrap();
        assert!(betas.iter().any(|v| v == "context-1m-2025-08-07"));
    }

    #[test]
    fn test_legacy_sonnet_4_5_thinking_null() {
        let mut body = json!({
            "model": "anthropic.claude-sonnet-4-5-20250514-v1:0",
            "max_tokens": 16384,
            "messages": [{"role": "user", "content": "hello"}]
        });

        optimize(&mut body, &enabled_config());

        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["thinking"]["budget_tokens"], 12288);
        let betas = body["anthropic_beta"].as_array().unwrap();
        assert!(betas.iter().any(|v| v == "interleaved-thinking-2025-05-14"));
    }

    #[test]
    fn test_legacy_small_caller_budget_is_not_raised() {
        let mut body = json!({
            "model": "anthropic.claude-sonnet-4-5-20250514-v1:0",
            "max_tokens": 16384,
            "thinking": {"type": "enabled", "budget_tokens": 1024},
            "messages": [{"role": "user", "content": "hello"}]
        });

        optimize(&mut body, &enabled_config());

        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["thinking"]["budget_tokens"], 1024);
    }

    #[test]
    fn test_skip_haiku() {
        let mut body = json!({
            "model": "anthropic.claude-haiku-4-5-20250514-v1:0",
            "max_tokens": 8192,
            "messages": [{"role": "user", "content": "hello"}]
        });
        let original = body.clone();

        optimize(&mut body, &enabled_config());

        assert_eq!(body, original);
    }

    #[test]
    fn test_thinking_optimizer_disabled() {
        let mut body = json!({
            "model": "anthropic.claude-opus-4-6-20250514-v1:0",
            "max_tokens": 16384,
            "messages": [{"role": "user", "content": "hello"}]
        });
        let original = body.clone();

        optimize(&mut body, &disabled_config());

        assert_eq!(body, original);
    }

    #[test]
    fn test_adaptive_dedup_beta() {
        let mut body = json!({
            "model": "anthropic.claude-opus-4-6-20250514-v1:0",
            "max_tokens": 16384,
            "anthropic_beta": ["context-1m-2025-08-07"],
            "messages": [{"role": "user", "content": "hello"}]
        });

        optimize(&mut body, &enabled_config());

        let betas = body["anthropic_beta"].as_array().unwrap();
        let count = betas
            .iter()
            .filter(|v| v == &&json!("context-1m-2025-08-07"))
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_legacy_disabled_thinking_is_kept() {
        let mut body = json!({
            "model": "anthropic.claude-sonnet-4-5-20250514-v1:0",
            "max_tokens": 8192,
            "thinking": {"type": "disabled"},
            "messages": [{"role": "user", "content": "hello"}]
        });

        optimize(&mut body, &enabled_config());

        assert_eq!(body["thinking"]["type"], "disabled");
    }

    #[test]
    fn test_legacy_default_max_tokens() {
        let mut body = json!({
            "model": "anthropic.claude-sonnet-4-5-20250514-v1:0",
            "messages": [{"role": "user", "content": "hello"}]
        });

        optimize(&mut body, &enabled_config());

        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["thinking"]["budget_tokens"], 12288);
    }

    #[test]
    fn test_append_beta_null_field() {
        let mut body = json!({
            "model": "anthropic.claude-opus-4-6-20250514-v1:0",
            "anthropic_beta": null,
            "messages": [{"role": "user", "content": "hello"}]
        });

        optimize(&mut body, &enabled_config());

        let betas = body["anthropic_beta"].as_array().unwrap();
        assert!(betas.iter().any(|v| v == "context-1m-2025-08-07"));
    }
}
