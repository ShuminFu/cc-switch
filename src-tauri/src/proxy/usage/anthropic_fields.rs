//! Anthropic Messages usage 块的逐字段读取
//!
//! 流式 tap（`message_start` / `message_delta` 的 usage）与 Claude Code 会话日志
//! （`~/.claude/projects/*/*.jsonl` 中的 `message.usage`）都使用 Anthropic 的
//! 字段名。缓存 token 字段名只允许出现在 codec/ 与 usage/ 中（CCS-243），
//! 调用方经 [`AnthropicUsageFields`] 读取，自行决定缺失字段的取值规则。

use serde_json::Value;

use super::parser::TokenUsage;

/// Anthropic usage 块中的各计数字段；缺失或非整数时为 `None`
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AnthropicUsageFields {
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub cache_read_tokens: Option<u32>,
    pub cache_creation_tokens: Option<u32>,
}

impl AnthropicUsageFields {
    /// 读取一个 Anthropic 风格的 usage 对象
    pub fn read(usage: &Value) -> Self {
        let field = |name: &str| usage.get(name).and_then(Value::as_u64).map(|v| v as u32);
        Self {
            input_tokens: field("input_tokens"),
            output_tokens: field("output_tokens"),
            cache_read_tokens: field("cache_read_input_tokens"),
            cache_creation_tokens: field("cache_creation_input_tokens"),
        }
    }

    /// 缺失字段按 0 计的 token 计数（不含 model / message_id）
    pub fn to_token_usage(self) -> TokenUsage {
        TokenUsage {
            input_tokens: self.input_tokens.unwrap_or(0),
            output_tokens: self.output_tokens.unwrap_or(0),
            cache_read_tokens: self.cache_read_tokens.unwrap_or(0),
            cache_creation_tokens: self.cache_creation_tokens.unwrap_or(0),
            ..TokenUsage::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_all_four_counts() {
        let fields = AnthropicUsageFields::read(&json!({
            "input_tokens": 3,
            "output_tokens": 150,
            "cache_read_input_tokens": 5000,
            "cache_creation_input_tokens": 10000
        }));
        assert_eq!(
            fields,
            AnthropicUsageFields {
                input_tokens: Some(3),
                output_tokens: Some(150),
                cache_read_tokens: Some(5000),
                cache_creation_tokens: Some(10000),
            }
        );
        let usage = fields.to_token_usage();
        assert_eq!(
            (
                usage.input_tokens,
                usage.output_tokens,
                usage.cache_read_tokens,
                usage.cache_creation_tokens
            ),
            (3, 150, 5000, 10000)
        );
        assert!(usage.model.is_none() && usage.message_id.is_none());
    }

    #[test]
    fn missing_or_non_integer_fields_are_none_and_count_as_zero() {
        let fields = AnthropicUsageFields::read(&json!({
            "output_tokens": 7,
            "cache_read_input_tokens": "12",
            "cache_creation_input_tokens": null
        }));
        assert_eq!(
            fields,
            AnthropicUsageFields {
                output_tokens: Some(7),
                ..Default::default()
            }
        );
        let usage = fields.to_token_usage();
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 7);
        assert_eq!(usage.cache_read_tokens, 0);
        assert_eq!(usage.cache_creation_tokens, 0);
    }
}
