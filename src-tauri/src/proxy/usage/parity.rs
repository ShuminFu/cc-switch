//! Usage parity - 规范化 usage 与上游原始 usage 块的一致性检查
//!
//! 计费落库的是经 [`super::ir::UsageIR`] 规范化后的 [`TokenUsage`]。缓存命中被重复计数
//! （上游 `cached_tokens` 超过总输入）、上游 `total_tokens` 与各桶之和对不上、
//! 以及成功的请求 usage 全 0（流缺 usage）都会悄无声息地进入账单。
//! 这里直接按上游原始 usage 块自身的字段重新求和（不经过 UsageIR），与规范化后的
//! 计费总量对比；不一致时在请求日志行上记一个 parity 标记供前端显示徽标。
//!
//! 存储：不加列（无 DDL）。成功行（2xx）的 `error_message` 恒为空，parity 标记以
//! `{"usageParity":{...}}` JSON 元数据写入该列，读取时由 [`decode_log_metadata`]
//! 拆回 `usage_parity` 字段，`error_message` 仍为空。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::parser::TokenUsage;

/// 元数据 JSON 中 parity 标记的键
const PARITY_META_KEY: &str = "usageParity";

/// 规范化 usage 与上游原始 usage 不一致的类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum UsageParityIssue {
    /// 成功的请求计费 token 全 0（上游缺 usage 或 usage 块全 0）
    ZeroUsage,
    /// 上游缓存读写 token 超过其报告的总输入（缓存被重复计数）
    CacheExceedsInput,
    /// 上游报告的总 token 与规范化后的计费总量不一致
    TotalMismatch,
}

/// 请求日志行上的 parity 标记
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct UsageParity {
    pub issue: UsageParityIssue,
    /// 规范化后的计费总量（输入三桶 + 输出）
    pub normalized_total: u64,
    /// 上游原始 usage 块自身的总量（`total_tokens` / `totalTokenCount` 或各字段之和）
    pub raw_total: Option<u64>,
    /// 上游原始 usage 块报告的总输入（含缓存）
    pub raw_input: Option<u64>,
    /// 上游原始 usage 块报告的缓存读写之和
    pub raw_cache: Option<u64>,
}

/// 从上游原始 usage 块独立求出的总量
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RawTotals {
    input: u64,
    cache: u64,
    total: u64,
    /// input 是否已含缓存（OpenAI/Gemini 语义）
    input_includes_cache: bool,
}

fn field(usage: &Value, pointer: &str) -> Option<u64> {
    usage.pointer(pointer).and_then(Value::as_u64)
}

fn first_field(usage: &Value, pointers: &[&str]) -> Option<u64> {
    pointers.iter().find_map(|pointer| field(usage, pointer))
}

fn raw_totals(raw: &Value) -> Option<RawTotals> {
    if !raw.is_object() {
        return None;
    }

    // Gemini usageMetadata：promptTokenCount 含 cachedContentTokenCount
    if raw.get("promptTokenCount").is_some() || raw.get("totalTokenCount").is_some() {
        let input = field(raw, "/promptTokenCount").unwrap_or(0);
        let output = field(raw, "/candidatesTokenCount").unwrap_or(0)
            + field(raw, "/thoughtsTokenCount").unwrap_or(0);
        return Some(RawTotals {
            input,
            cache: field(raw, "/cachedContentTokenCount").unwrap_or(0),
            total: field(raw, "/totalTokenCount").unwrap_or(input + output),
            input_includes_cache: true,
        });
    }

    let cache_read = first_field(
        raw,
        &[
            "/cache_read_input_tokens",
            "/input_tokens_details/cached_tokens",
            "/prompt_tokens_details/cached_tokens",
        ],
    )
    .unwrap_or(0);
    let cache_write = first_field(
        raw,
        &[
            "/cache_creation_input_tokens",
            "/input_tokens_details/cache_write_tokens",
            "/prompt_tokens_details/cache_write_tokens",
        ],
    )
    .unwrap_or(0);
    let cache = cache_read + cache_write;

    // OpenAI Chat / Responses：prompt/input 含缓存
    let openai_shape = [
        "total_tokens",
        "prompt_tokens",
        "completion_tokens",
        "input_tokens_details",
        "prompt_tokens_details",
    ]
    .iter()
    .any(|key| raw.get(*key).is_some());
    if openai_shape {
        let input = first_field(raw, &["/prompt_tokens", "/input_tokens"]).unwrap_or(0);
        let output = first_field(raw, &["/completion_tokens", "/output_tokens"]).unwrap_or(0);
        return Some(RawTotals {
            input,
            cache,
            total: field(raw, "/total_tokens").unwrap_or(input + output),
            input_includes_cache: true,
        });
    }

    // Anthropic Messages：input_tokens 不含缓存
    if raw.get("input_tokens").is_some() || raw.get("output_tokens").is_some() {
        let fresh = field(raw, "/input_tokens").unwrap_or(0);
        let output = field(raw, "/output_tokens").unwrap_or(0);
        return Some(RawTotals {
            input: fresh + cache,
            cache,
            total: fresh + cache + output,
            input_includes_cache: false,
        });
    }

    None
}

/// 取出响应 / SSE 事件中的原始 usage 块
///
/// 依次识别：顶层 `usage`（Anthropic、Chat、Responses）、`response.usage`
/// （Responses `response.completed` 事件）、`usageMetadata` 与
/// `response.usageMetadata`（Gemini / Gemini CLI）。
pub fn raw_usage_block(response: &Value) -> Option<&Value> {
    [
        "/usage",
        "/response/usage",
        "/usageMetadata",
        "/response/usageMetadata",
    ]
    .iter()
    .filter_map(|pointer| response.pointer(pointer))
    .find(|usage| usage.is_object())
}

/// 规范化后的计费总量；`input_includes_cache` 为 TokenUsage.input 的落库语义
fn normalized_total(usage: &TokenUsage, input_includes_cache: bool) -> u64 {
    let input = u64::from(usage.input_tokens);
    let cache = u64::from(usage.cache_read_tokens) + u64::from(usage.cache_creation_tokens);
    let input = if input_includes_cache {
        input.max(cache)
    } else {
        input + cache
    };
    input + u64::from(usage.output_tokens)
}

/// 对比规范化 usage 与上游原始 usage 块，不一致时返回 parity 标记
///
/// - `input_includes_cache`：`usage.input_tokens` 的落库语义（codex/gemini 等为 true）
/// - `raw`：上游原始 usage 块；缺失时只检查全 0
pub fn check_usage_parity(
    usage: &TokenUsage,
    input_includes_cache: bool,
    raw: Option<&Value>,
) -> Option<UsageParity> {
    let normalized_total = normalized_total(usage, input_includes_cache);
    let totals = raw.and_then(raw_totals);
    let parity = |issue| UsageParity {
        issue,
        normalized_total,
        raw_total: totals.map(|t| t.total),
        raw_input: totals.map(|t| t.input),
        raw_cache: totals.map(|t| t.cache),
    };

    if normalized_total == 0 {
        return Some(parity(UsageParityIssue::ZeroUsage));
    }
    let totals = totals?;
    if totals.input_includes_cache && totals.cache > totals.input {
        return Some(parity(UsageParityIssue::CacheExceedsInput));
    }
    (totals.total != normalized_total).then(|| parity(UsageParityIssue::TotalMismatch))
}

/// 把 parity 标记编码为成功行 `error_message` 列中的 JSON 元数据
pub fn encode_log_metadata(parity: &UsageParity) -> String {
    serde_json::json!({ PARITY_META_KEY: parity }).to_string()
}

/// 拆分请求日志行的 `error_message` 列：成功行（< 400）中的 parity JSON 元数据
/// 返回为 `(None, Some(parity))`，其余原样返回。
pub fn decode_log_metadata(
    status_code: u16,
    error_message: Option<String>,
) -> (Option<String>, Option<UsageParity>) {
    let Some(message) = error_message else {
        return (None, None);
    };
    if status_code >= 400 || !message.starts_with('{') {
        return (Some(message), None);
    }
    match serde_json::from_str::<Value>(&message)
        .ok()
        .and_then(|mut meta| meta.get_mut(PARITY_META_KEY).map(Value::take))
        .and_then(|parity| serde_json::from_value::<UsageParity>(parity).ok())
    {
        Some(parity) => (None, Some(parity)),
        None => (Some(message), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn usage(input: u32, output: u32, cache_read: u32, cache_creation: u32) -> TokenUsage {
        TokenUsage {
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: cache_read,
            cache_creation_tokens: cache_creation,
            model: None,
            message_id: None,
        }
    }

    #[test]
    fn consistent_usage_has_no_parity_issue() {
        // Anthropic：fresh input + 缓存 + 输出
        let raw = json!({
            "input_tokens": 10, "output_tokens": 5,
            "cache_read_input_tokens": 20, "cache_creation_input_tokens": 3
        });
        assert_eq!(
            check_usage_parity(&usage(10, 5, 20, 3), false, Some(&raw)),
            None
        );

        // Chat：prompt 含缓存，total_tokens 自洽
        let raw = json!({
            "prompt_tokens": 100, "completion_tokens": 20, "total_tokens": 120,
            "prompt_tokens_details": {"cached_tokens": 40}
        });
        assert_eq!(
            check_usage_parity(&usage(100, 20, 40, 0), true, Some(&raw)),
            None
        );
        // Claude 客户端经转换落 fresh 语义，总量仍守恒
        assert_eq!(
            check_usage_parity(&usage(60, 20, 40, 0), false, Some(&raw)),
            None
        );

        // Gemini
        let raw = json!({
            "promptTokenCount": 100, "cachedContentTokenCount": 30,
            "candidatesTokenCount": 40, "thoughtsTokenCount": 10, "totalTokenCount": 150
        });
        assert_eq!(
            check_usage_parity(&usage(100, 50, 30, 0), true, Some(&raw)),
            None
        );
    }

    #[test]
    fn inconsistent_cache_read_fixture_sets_flag() {
        // 上游把 prompt_tokens 报成 fresh input，cached_tokens 却又大于它：
        // 规范化时缓存被钳入总输入，计费总量多出一截。
        let raw = json!({
            "prompt_tokens": 100, "completion_tokens": 20, "total_tokens": 120,
            "prompt_tokens_details": {"cached_tokens": 150}
        });
        let ir = super::super::ir::UsageIR::from_chat_completions(&raw);
        let parity = check_usage_parity(&ir.to_token_usage(), true, Some(&raw))
            .expect("inconsistent usage must be flagged");
        assert_eq!(parity.issue, UsageParityIssue::CacheExceedsInput);
        assert_eq!(parity.normalized_total, 170);
        assert_eq!(parity.raw_total, Some(120));
        assert_eq!(parity.raw_input, Some(100));
        assert_eq!(parity.raw_cache, Some(150));
    }

    #[test]
    fn total_mismatch_sets_flag() {
        let raw = json!({"input_tokens": 50, "output_tokens": 10, "total_tokens": 90});
        let parity = check_usage_parity(&usage(50, 10, 0, 0), true, Some(&raw))
            .expect("total mismatch must be flagged");
        assert_eq!(parity.issue, UsageParityIssue::TotalMismatch);
        assert_eq!(parity.normalized_total, 60);
        assert_eq!(parity.raw_total, Some(90));
    }

    #[test]
    fn zero_usage_sets_flag_with_or_without_raw_block() {
        let parity = check_usage_parity(&TokenUsage::default(), false, None).unwrap();
        assert_eq!(parity.issue, UsageParityIssue::ZeroUsage);
        assert_eq!(parity.raw_total, None);

        let raw = json!({"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0});
        let parity = check_usage_parity(&TokenUsage::default(), true, Some(&raw)).unwrap();
        assert_eq!(parity.issue, UsageParityIssue::ZeroUsage);
        assert_eq!(parity.raw_total, Some(0));
    }

    #[test]
    fn missing_raw_block_only_checks_zero_usage() {
        assert_eq!(check_usage_parity(&usage(1, 1, 0, 0), false, None), None);
        assert_eq!(
            check_usage_parity(&usage(1, 1, 0, 0), false, Some(&json!({"foo": 1}))),
            None
        );
    }

    #[test]
    fn raw_usage_block_finds_each_format() {
        let chat = json!({"usage": {"prompt_tokens": 1}});
        assert_eq!(raw_usage_block(&chat), Some(&json!({"prompt_tokens": 1})));
        let completed =
            json!({"type": "response.completed", "response": {"usage": {"input_tokens": 2}}});
        assert_eq!(
            raw_usage_block(&completed),
            Some(&json!({"input_tokens": 2}))
        );
        let gemini = json!({"usageMetadata": {"promptTokenCount": 3}});
        assert_eq!(
            raw_usage_block(&gemini),
            Some(&json!({"promptTokenCount": 3}))
        );
        let gemini_cli = json!({"response": {"usageMetadata": {"promptTokenCount": 4}}});
        assert_eq!(
            raw_usage_block(&gemini_cli),
            Some(&json!({"promptTokenCount": 4}))
        );
        assert_eq!(raw_usage_block(&json!({"usage": null})), None);
    }

    #[test]
    fn log_metadata_roundtrips_and_leaves_errors_alone() {
        let parity = UsageParity {
            issue: UsageParityIssue::TotalMismatch,
            normalized_total: 60,
            raw_total: Some(90),
            raw_input: Some(50),
            raw_cache: Some(0),
        };
        let encoded = encode_log_metadata(&parity);
        assert!(encoded.starts_with("{\"usageParity\""));
        assert_eq!(
            decode_log_metadata(200, Some(encoded.clone())),
            (None, Some(parity))
        );
        // 错误行的 error_message 原样保留（即使形如 JSON）
        assert_eq!(
            decode_log_metadata(500, Some(encoded.clone())),
            (Some(encoded), None)
        );
        assert_eq!(
            decode_log_metadata(200, Some("{not json".to_string())),
            (Some("{not json".to_string()), None)
        );
        assert_eq!(decode_log_metadata(200, None), (None, None));
    }
}
