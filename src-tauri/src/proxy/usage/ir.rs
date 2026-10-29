//! UsageIR - 各协议 usage 的规范中间表示
//!
//! 缓存 token 的算术（OpenAI/Gemini 的 input 含缓存命中，Anthropic 的 input 不含）
//! 曾在每个格式桥接器与 `TokenUsage::from_*` 中各自实现，字段优先级逐渐漂移。
//! 现统一为：`from_<format>` 把上游 usage 读成三个互斥输入桶 + 输出，
//! `to_<format>` 再按目标协议的语义写回。
//!
//! 不变量（billable-token conservation）：
//! `fresh_input + cache_read + cache_write` 等于上游报告的总输入（缓存超过总输入时
//! fresh 钳为 0，防下溢）；任意 from/to 往返不增不减计费 token。
//!
//! 缓存字段读取优先级（所有 OpenAI 系格式一致）：
//! 1. Anthropic 风格直传字段 `cache_read_input_tokens` / `cache_creation_input_tokens`
//!    （部分兼容服务器直接返回，视为权威）
//! 2. Responses API `input_tokens_details.{cached_tokens, cache_write_tokens}`
//! 3. Chat Completions `prompt_tokens_details.{cached_tokens, cache_write_tokens}`

use serde_json::{json, Value};

use super::parser::TokenUsage;

/// 上游 input 字段的计量语义
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum InputSemantics {
    /// input 不含缓存读写（Anthropic Messages）
    #[default]
    Fresh,
    /// input 含缓存读写（OpenAI Chat/Responses、Gemini）
    Total,
}

/// 规范化的 usage：三个互斥输入桶 + 输出
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UsageIR {
    /// 不含任何缓存的新输入
    pub fresh_input: u64,
    /// 缓存命中（读取）
    pub cache_read: u64,
    /// 缓存写入（创建）
    pub cache_write: u64,
    /// 输出 token（含 reasoning）
    pub output: u64,
    /// 输出中的 reasoning/thinking token（output 的子集，仅用于透出明细）
    pub reasoning: u64,
    /// 来源格式的 input 语义，决定 `to_token_usage` 如何记录 input
    pub semantics: InputSemantics,
}

fn first_u64(usage: &Value, pointers: &[&str]) -> Option<u64> {
    pointers
        .iter()
        .find_map(|pointer| usage.pointer(pointer).and_then(Value::as_u64))
}

fn openai_cache_read(usage: &Value) -> u64 {
    first_u64(
        usage,
        &[
            "/cache_read_input_tokens",
            "/input_tokens_details/cached_tokens",
            "/prompt_tokens_details/cached_tokens",
        ],
    )
    .unwrap_or(0)
}

fn openai_cache_write(usage: &Value) -> u64 {
    first_u64(
        usage,
        &[
            "/cache_creation_input_tokens",
            "/input_tokens_details/cache_write_tokens",
            "/prompt_tokens_details/cache_write_tokens",
        ],
    )
    .unwrap_or(0)
}

impl UsageIR {
    /// 由含缓存的总输入构造（OpenAI/Gemini 语义），fresh 为总输入扣除缓存读写。
    fn from_total_input(
        total_input: u64,
        cache_read: u64,
        cache_write: u64,
        output: u64,
        reasoning: u64,
    ) -> Self {
        Self {
            fresh_input: total_input
                .saturating_sub(cache_read)
                .saturating_sub(cache_write),
            cache_read,
            cache_write,
            output,
            reasoning,
            semantics: InputSemantics::Total,
        }
    }

    /// Anthropic Messages usage（`input_tokens` 已是 fresh input）
    pub fn from_anthropic(usage: &Value) -> Self {
        Self {
            fresh_input: first_u64(usage, &["/input_tokens"]).unwrap_or(0),
            cache_read: first_u64(usage, &["/cache_read_input_tokens"]).unwrap_or(0),
            cache_write: first_u64(usage, &["/cache_creation_input_tokens"]).unwrap_or(0),
            output: first_u64(usage, &["/output_tokens"]).unwrap_or(0),
            reasoning: first_u64(usage, &["/output_tokens_details/thinking_tokens"]).unwrap_or(0),
            semantics: InputSemantics::Fresh,
        }
    }

    /// OpenAI Responses usage（`input_tokens` 优先，`prompt_tokens` 兜底）
    pub fn from_responses(usage: &Value) -> Self {
        Self::from_total_input(
            first_u64(usage, &["/input_tokens", "/prompt_tokens"]).unwrap_or(0),
            openai_cache_read(usage),
            openai_cache_write(usage),
            first_u64(usage, &["/output_tokens", "/completion_tokens"]).unwrap_or(0),
            first_u64(usage, &["/output_tokens_details/reasoning_tokens"]).unwrap_or(0),
        )
    }

    /// OpenAI Chat Completions usage（`prompt_tokens` 优先，`input_tokens` 兜底）
    pub fn from_chat_completions(usage: &Value) -> Self {
        Self::from_total_input(
            first_u64(usage, &["/prompt_tokens", "/input_tokens"]).unwrap_or(0),
            openai_cache_read(usage),
            openai_cache_write(usage),
            first_u64(usage, &["/completion_tokens", "/output_tokens"]).unwrap_or(0),
            first_u64(usage, &["/completion_tokens_details/reasoning_tokens"]).unwrap_or(0),
        )
    }

    /// Gemini `usageMetadata`：promptTokenCount 含 cachedContentTokenCount，
    /// 输出 = totalTokenCount - promptTokenCount（含 candidates 与 thoughts）。
    pub fn from_gemini(usage_metadata: &Value) -> Self {
        let prompt = first_u64(usage_metadata, &["/promptTokenCount"]).unwrap_or(0);
        let total = first_u64(usage_metadata, &["/totalTokenCount"]).unwrap_or(0);
        Self::from_total_input(
            prompt,
            first_u64(usage_metadata, &["/cachedContentTokenCount"]).unwrap_or(0),
            0,
            total.saturating_sub(prompt),
            first_u64(usage_metadata, &["/thoughtsTokenCount"]).unwrap_or(0),
        )
    }

    /// 含缓存读写的总输入
    pub fn total_input(&self) -> u64 {
        self.fresh_input
            .saturating_add(self.cache_read)
            .saturating_add(self.cache_write)
    }

    /// 全部计费 token（三个输入桶 + 输出）
    #[cfg(test)]
    pub fn billable_tokens(&self) -> u64 {
        self.total_input().saturating_add(self.output)
    }

    /// Anthropic Messages usage：input 为 fresh，缓存桶仅在非零时输出。
    pub fn to_anthropic(self) -> Value {
        let mut usage = json!({
            "input_tokens": self.fresh_input,
            "output_tokens": self.output
        });
        if self.cache_read > 0 {
            usage["cache_read_input_tokens"] = json!(self.cache_read);
        }
        if self.cache_write > 0 {
            usage["cache_creation_input_tokens"] = json!(self.cache_write);
        }
        usage
    }

    /// OpenAI Responses usage：input 为总输入，缓存读写作为
    /// `input_tokens_details` 子集透出。
    pub fn to_responses(self) -> Value {
        let input_tokens = self.total_input();
        let mut usage = json!({
            "input_tokens": input_tokens,
            "output_tokens": self.output,
            "total_tokens": input_tokens.saturating_add(self.output),
            "output_tokens_details": { "reasoning_tokens": self.reasoning }
        });
        if self.cache_read > 0 || self.cache_write > 0 {
            usage["input_tokens_details"] = json!({
                "cached_tokens": self.cache_read,
                "cache_write_tokens": self.cache_write
            });
        }
        // Keep the legacy top-level alias for one compatibility window. New code reads
        // the official nested cache_write_tokens field first.
        if self.cache_write > 0 {
            usage["cache_creation_input_tokens"] = json!(self.cache_write);
        }
        usage
    }

    /// 计费记录：input 按来源格式的语义记录（Total 记总输入，由 calculator 扣减缓存）。
    pub fn to_token_usage(self) -> TokenUsage {
        let input = match self.semantics {
            InputSemantics::Fresh => self.fresh_input,
            InputSemantics::Total => self.total_input(),
        };
        TokenUsage {
            input_tokens: input as u32,
            output_tokens: self.output as u32,
            cache_read_tokens: self.cache_read as u32,
            cache_creation_tokens: self.cache_write as u32,
            model: None,
            message_id: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn cache_priority_is_direct_then_responses_then_chat_details() {
        let usage = json!({
            "prompt_tokens": 100,
            "cache_read_input_tokens": 40,
            "input_tokens_details": {"cached_tokens": 30, "cache_write_tokens": 20},
            "prompt_tokens_details": {"cached_tokens": 10, "cache_write_tokens": 5}
        });
        let ir = UsageIR::from_chat_completions(&usage);
        assert_eq!(ir.cache_read, 40);
        assert_eq!(ir.cache_write, 20);
        assert_eq!(ir.fresh_input, 40);

        let chat_only = json!({
            "prompt_tokens": 100,
            "prompt_tokens_details": {"cached_tokens": 10, "cache_write_tokens": 5}
        });
        let ir = UsageIR::from_chat_completions(&chat_only);
        assert_eq!((ir.fresh_input, ir.cache_read, ir.cache_write), (85, 10, 5));
    }

    #[test]
    fn non_numeric_direct_field_falls_through_to_details() {
        let usage = json!({
            "input_tokens": 10,
            "cache_read_input_tokens": null,
            "input_tokens_details": {"cached_tokens": 4}
        });
        assert_eq!(UsageIR::from_responses(&usage).cache_read, 4);
    }

    #[test]
    fn fresh_input_clamps_when_cache_exceeds_total() {
        let ir = UsageIR::from_responses(&json!({
            "input_tokens": 100,
            "cache_read_input_tokens": 60,
            "cache_creation_input_tokens": 50
        }));
        assert_eq!(ir.fresh_input, 0);
        assert_eq!(ir.to_anthropic()["cache_read_input_tokens"], 60);
        assert_eq!(ir.to_anthropic()["cache_creation_input_tokens"], 50);
    }

    #[test]
    fn gemini_input_excludes_cached_content() {
        let ir = UsageIR::from_gemini(&json!({
            "promptTokenCount": 100,
            "cachedContentTokenCount": 30,
            "totalTokenCount": 150,
            "thoughtsTokenCount": 20
        }));
        assert_eq!(ir.fresh_input, 70);
        assert_eq!(ir.cache_read, 30);
        assert_eq!(ir.output, 50);
        assert_eq!(ir.reasoning, 20);
        assert_eq!(ir.to_token_usage().input_tokens, 100);
    }

    #[test]
    fn empty_usage_converts_to_zero_shapes() {
        assert_eq!(
            UsageIR::default().to_anthropic(),
            json!({"input_tokens": 0, "output_tokens": 0})
        );
        assert_eq!(
            UsageIR::default().to_responses(),
            json!({
                "input_tokens": 0,
                "output_tokens": 0,
                "total_tokens": 0,
                "output_tokens_details": {"reasoning_tokens": 0}
            })
        );
    }

    #[test]
    fn token_usage_records_input_per_source_semantics() {
        let anthropic = UsageIR::from_anthropic(&json!({
            "input_tokens": 5, "output_tokens": 2, "cache_read_input_tokens": 4
        }));
        assert_eq!(anthropic.to_token_usage().input_tokens, 5);
        let responses = UsageIR::from_responses(&json!({
            "input_tokens": 9, "output_tokens": 2,
            "input_tokens_details": {"cached_tokens": 4}
        }));
        assert_eq!(responses.to_token_usage().input_tokens, 9);
        assert_eq!(responses.to_token_usage().cache_read_tokens, 4);
    }

    fn arb_ir() -> impl Strategy<Value = UsageIR> {
        (
            0u64..1_000_000,
            0u64..1_000_000,
            0u64..1_000_000,
            0u64..1_000_000,
        )
            .prop_flat_map(|(fresh, read, write, output)| {
                (0..=output).prop_map(move |reasoning| UsageIR {
                    fresh_input: fresh,
                    cache_read: read,
                    cache_write: write,
                    output,
                    reasoning,
                    semantics: InputSemantics::Fresh,
                })
            })
    }

    fn buckets(ir: &UsageIR) -> (u64, u64, u64, u64) {
        (ir.fresh_input, ir.cache_read, ir.cache_write, ir.output)
    }

    proptest! {
        /// Anthropic → Responses → Anthropic 往返保持每个计费桶不变。
        #[test]
        fn billable_tokens_conserved_anthropic_responses_roundtrip(ir in arb_ir()) {
            let responses = ir.to_responses();
            let back = UsageIR::from_responses(&responses);
            prop_assert_eq!(buckets(&back), buckets(&ir));
            prop_assert_eq!(back.reasoning, ir.reasoning);
            prop_assert_eq!(back.billable_tokens(), ir.billable_tokens());
            prop_assert_eq!(
                responses["total_tokens"].as_u64(),
                Some(ir.billable_tokens())
            );

            let anthropic = back.to_anthropic();
            let again = UsageIR::from_anthropic(&anthropic);
            prop_assert_eq!(buckets(&again), buckets(&ir));
        }

        /// 含缓存的 OpenAI Chat usage 转 Anthropic 后，三桶之和仍等于 prompt_tokens。
        #[test]
        fn billable_tokens_conserved_chat_to_anthropic(
            read in 0u64..1_000_000,
            write in 0u64..1_000_000,
            fresh in 0u64..1_000_000,
            output in 0u64..1_000_000,
        ) {
            let prompt = fresh + read + write;
            let chat = json!({
                "prompt_tokens": prompt,
                "completion_tokens": output,
                "prompt_tokens_details": {"cached_tokens": read, "cache_write_tokens": write}
            });
            let ir = UsageIR::from_chat_completions(&chat);
            prop_assert_eq!(ir.total_input(), prompt);
            let anthropic = UsageIR::from_anthropic(&ir.to_anthropic());
            prop_assert_eq!(anthropic.fresh_input, fresh);
            prop_assert_eq!(anthropic.billable_tokens(), prompt + output);
            prop_assert_eq!(
                anthropic.to_token_usage().input_tokens as u64
                    + anthropic.cache_read
                    + anthropic.cache_write,
                ir.to_token_usage().input_tokens as u64
            );
        }

        /// Gemini 的总输入（promptTokenCount）在转换后守恒；缓存超出时不下溢。
        #[test]
        fn gemini_prompt_conserved_or_clamped(
            prompt in 0u64..1_000_000,
            cached in 0u64..1_000_000,
            output in 0u64..1_000_000,
        ) {
            let ir = UsageIR::from_gemini(&json!({
                "promptTokenCount": prompt,
                "cachedContentTokenCount": cached,
                "totalTokenCount": prompt + output
            }));
            prop_assert_eq!(ir.output, output);
            prop_assert_eq!(ir.fresh_input, prompt.saturating_sub(cached));
            prop_assert_eq!(ir.total_input(), prompt.max(cached));
        }
    }
}
