//! Responses usage → Anthropic usage（Anthropic 客户端 ← Responses 上游的计量）

use serde_json::Value;

use crate::proxy::usage::ir::UsageIR;

/// Build Anthropic-style usage JSON from Responses API usage, including cache tokens.
///
/// Field resolution and cache arithmetic live in [`UsageIR::from_responses`]:
/// - input: `input_tokens` → OpenAI `prompt_tokens` → 0; output likewise with
///   `output_tokens` → `completion_tokens`
/// - cache reads/writes: direct Anthropic-style fields → `input_tokens_details` →
///   `prompt_tokens_details`
///
/// OpenAI/Responses 的 input 含缓存命中，Anthropic input_tokens 不含 → IR 扣减 cache_read 与
/// cache_creation 得到 fresh input。本函数在计量意义上是 claude 专属（Codex Responses 透传走
/// from_codex_response_*，不调用本函数）。三桶互斥，恒等：input + cache_read + cache_creation
/// == 上游 input(inclusive)；一处同时覆盖非流式与流式(streaming_responses)。
///
/// **Logging**:
/// - Warns on empty objects {}; debug-logs partial objects (only one field present)
pub(crate) fn build_anthropic_usage_from_responses(usage: Option<&Value>) -> Value {
    let u = match usage {
        Some(v) if !v.is_null() && v.is_object() => v,
        _ => return UsageIR::default().to_anthropic(),
    };

    // Detect empty object {} and log warning
    if u.as_object().map(|obj| obj.is_empty()).unwrap_or(false) {
        log::warn!("[Responses] Empty usage object received, using defaults");
        return UsageIR::default().to_anthropic();
    }

    let ir = UsageIR::from_responses(u);

    // Log if only one field present (partial object). Streaming chunks legitimately
    // arrive with partial usage, so this stays at debug level to avoid noise.
    let input = ir.total_input();
    if (input == 0 && ir.output > 0) || (input > 0 && ir.output == 0) {
        log::debug!("[Responses] Partial usage object: {:?}", u);
    }

    let mut result = ir.to_anthropic();
    // Anthropic 的 TTL 分桶明细不参与计费算术，原样透传
    if let Some(v) = u.get("cache_creation") {
        result["cache_creation"] = v.clone();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_build_usage_from_null_parameter() {
        let result = build_anthropic_usage_from_responses(None);
        assert_eq!(result["input_tokens"], json!(0));
        assert_eq!(result["output_tokens"], json!(0));
    }

    #[test]
    fn test_build_usage_from_null_json_value() {
        let result = build_anthropic_usage_from_responses(Some(&json!(null)));
        assert_eq!(result["input_tokens"], json!(0));
        assert_eq!(result["output_tokens"], json!(0));
    }

    #[test]
    fn test_build_usage_from_empty_object() {
        let result = build_anthropic_usage_from_responses(Some(&json!({})));
        assert_eq!(result["input_tokens"], json!(0));
        assert_eq!(result["output_tokens"], json!(0));
    }

    #[test]
    fn test_build_usage_from_partial_input_only() {
        let result = build_anthropic_usage_from_responses(Some(&json!({
            "input_tokens": 100
        })));
        assert_eq!(result["input_tokens"], json!(100));
        assert_eq!(result["output_tokens"], json!(0));
    }

    #[test]
    fn test_build_usage_from_partial_output_only() {
        let result = build_anthropic_usage_from_responses(Some(&json!({
            "output_tokens": 50
        })));
        assert_eq!(result["input_tokens"], json!(0));
        assert_eq!(result["output_tokens"], json!(50));
    }

    #[test]
    fn test_build_usage_with_openai_field_names() {
        let result = build_anthropic_usage_from_responses(Some(&json!({
            "prompt_tokens": 120,
            "completion_tokens": 45
        })));
        assert_eq!(result["input_tokens"], json!(120));
        assert_eq!(result["output_tokens"], json!(45));
    }

    #[test]
    fn test_build_usage_anthropic_names_precedence() {
        let result = build_anthropic_usage_from_responses(Some(&json!({
            "input_tokens": 100,
            "prompt_tokens": 120,
            "output_tokens": 50,
            "completion_tokens": 45
        })));
        assert_eq!(result["input_tokens"], json!(100)); // Anthropic name takes precedence
        assert_eq!(result["output_tokens"], json!(50)); // Anthropic name takes precedence
    }

    #[test]
    fn test_build_usage_cache_tokens_from_nested_details() {
        let result = build_anthropic_usage_from_responses(Some(&json!({
            "input_tokens": 100,
            "output_tokens": 50,
            "input_tokens_details": {
                "cached_tokens": 80
            }
        })));
        // input_tokens(100) 含 nested cached(80)，转换后 input 应为 fresh = 100 - 80 = 20
        assert_eq!(result["input_tokens"], json!(20));
        assert_eq!(result["output_tokens"], json!(50));
        assert_eq!(result["cache_read_input_tokens"], json!(80));
    }

    #[test]
    fn test_build_usage_cache_write_tokens_from_nested_details() {
        let result = build_anthropic_usage_from_responses(Some(&json!({
            "input_tokens": 100,
            "output_tokens": 10,
            "input_tokens_details": {
                "cached_tokens": 30,
                "cache_write_tokens": 20
            }
        })));
        assert_eq!(result["input_tokens"], json!(50));
        assert_eq!(result["cache_read_input_tokens"], json!(30));
        assert_eq!(result["cache_creation_input_tokens"], json!(20));
    }

    #[test]
    fn test_build_usage_cache_tokens_direct_override() {
        let result = build_anthropic_usage_from_responses(Some(&json!({
            "input_tokens": 100,
            "output_tokens": 50,
            "input_tokens_details": {
                "cached_tokens": 80
            },
            "cache_read_input_tokens": 100
        })));
        // 直传 cache_read(100) 优先于 nested(80)；input(100) - 100 = 0（fresh）
        assert_eq!(result["input_tokens"], json!(0));
        assert_eq!(result["cache_read_input_tokens"], json!(100)); // Direct field overrides nested
    }

    #[test]
    fn test_build_usage_clamps_input_when_cache_exceeds_input() {
        // input(100) < cache_read(60)+cache_creation(50)=110：saturating 钳到 0，防下溢。
        // 钉桩：阻止未来把 saturating_sub 误改成普通减法(debug panic / release wrap)。
        let result = build_anthropic_usage_from_responses(Some(&json!({
            "input_tokens": 100,
            "output_tokens": 10,
            "cache_read_input_tokens": 60,
            "cache_creation_input_tokens": 50
        })));
        assert_eq!(result["input_tokens"], json!(0));
        assert_eq!(result["cache_read_input_tokens"], json!(60));
        assert_eq!(result["cache_creation_input_tokens"], json!(50));
    }

    #[test]
    fn test_build_usage_cache_tokens_without_input_output() {
        let result = build_anthropic_usage_from_responses(Some(&json!({
            "cache_read_input_tokens": 60,
            "cache_creation_input_tokens": 20
        })));
        assert_eq!(result["input_tokens"], json!(0));
        assert_eq!(result["output_tokens"], json!(0));
        assert_eq!(result["cache_read_input_tokens"], json!(60));
        assert_eq!(result["cache_creation_input_tokens"], json!(20));
    }
}
