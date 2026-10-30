//! 流式响应旁路观测（tap）：在不缓存事件的前提下累计 usage 与时序。
//!
//! 旧实现把每个 usage 相关的 SSE 事件攒进 `Vec<Value>`，流结束后整体重解析；
//! 长流（大量 `message_delta` / 带 usage 的 chunk）的内存随事件数线性增长。
//! [`UsageTap`] 改为逐事件折叠出运行总量，内存与事件数无关；各格式的取值
//! 优先级与原 `TokenUsage::from_*_stream_*` + 模型提取器逐条对齐。
//! [`TimingTap`] 记录首字节（TTFB）与首个 token 事件的时间点。

use std::time::Instant;

use serde_json::Value;

use crate::proxy::usage::{
    anthropic_fields::AnthropicUsageFields, ir::UsageIR, parser::TokenUsage,
};

/// 流式 usage 的事件格式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamUsageFormat {
    /// Anthropic Messages（`message_start` / `message_delta`）
    Anthropic,
    /// OpenAI Chat Completions（最后一个带 usage 的 chunk）
    OpenAiChat,
    /// Codex：优先 Responses `response.completed`，否则回退 Chat chunk
    CodexAuto,
    /// Gemini（`usageMetadata` 为累计值，取最后一个）
    Gemini,
}

/// Anthropic 流式 usage 的折叠状态
#[derive(Debug, Default)]
struct AnthropicFold {
    usage: TokenUsage,
    model: Option<String>,
    message_id: Option<String>,
    input_from_delta: bool,
}

impl AnthropicFold {
    fn push(&mut self, event: &Value) {
        let Some(event_type) = event.get("type").and_then(|v| v.as_str()) else {
            return;
        };
        let usage = &mut self.usage;
        match event_type {
            "message_start" => {
                if let Some(message) = event.get("message") {
                    if self.model.is_none() {
                        if let Some(m) = message.get("model").and_then(|v| v.as_str()) {
                            self.model = Some(m.to_string());
                        }
                    }
                    if self.message_id.is_none() {
                        if let Some(id) = message.get("id").and_then(|v| v.as_str()) {
                            self.message_id = Some(id.to_string());
                        }
                    }
                }
                if let Some(msg_usage) = event.get("message").and_then(|m| m.get("usage")) {
                    let fields = AnthropicUsageFields::read(msg_usage);
                    // 从 message_start 获取 input_tokens（原生 Claude API）
                    if let Some(input) = fields.input_tokens {
                        usage.input_tokens = input;
                    }
                    usage.cache_read_tokens = fields.cache_read_tokens.unwrap_or(0);
                    usage.cache_creation_tokens = fields.cache_creation_tokens.unwrap_or(0);
                }
            }
            "message_delta" => {
                let Some(delta_usage) = event.get("usage") else {
                    return;
                };
                let AnthropicUsageFields {
                    input_tokens: delta_input,
                    output_tokens: delta_output,
                    cache_read_tokens: delta_cache_read,
                    cache_creation_tokens: delta_cache_creation,
                } = AnthropicUsageFields::read(delta_usage);
                // 从 message_delta 获取 output_tokens
                if let Some(output) = delta_output {
                    usage.output_tokens = output;
                }

                // 部分 Anthropic-compatible SSE provider 会在 message_start 上报完整上下文，
                // 但在 message_delta 上报修正后的 fresh input。遇到更小的正数 delta input
                // 时采用 delta；若同一 usage 块带有缓存计数，也同步采用以避免重复计数。
                // 若 delta 缺少缓存字段，则保留 start 中已有的缓存值作为 best-effort fallback。
                if let Some(input) = delta_input {
                    let should_use_delta_input = input > 0
                        && (usage.input_tokens == 0
                            || input < usage.input_tokens
                            || (self.input_from_delta && input <= usage.input_tokens));

                    if should_use_delta_input {
                        usage.input_tokens = input;
                        self.input_from_delta = true;
                        if let Some(cache_read) = delta_cache_read {
                            usage.cache_read_tokens = cache_read;
                        }
                        if let Some(cache_creation) = delta_cache_creation {
                            usage.cache_creation_tokens = cache_creation;
                        }
                    }
                }
                // 从 message_delta 中处理缓存命中
                if usage.cache_read_tokens == 0 {
                    if let Some(cache_read) = delta_cache_read {
                        usage.cache_read_tokens = cache_read;
                    }
                }
                // 从 message_delta 中处理缓存创建
                // 注: 现在 zhipu 的 message_delta 不返回缓存创建字段
                if usage.cache_creation_tokens == 0 {
                    if let Some(cache_creation) = delta_cache_creation {
                        usage.cache_creation_tokens = cache_creation;
                    }
                }
            }
            _ => {}
        }
    }

    fn usage(&self) -> Option<TokenUsage> {
        // 用 has_billable_tokens 而非仅看 input/output：完全缓存命中、无输出的流式请求
        // （input==0 && output==0 但 cache_read>0）是真实的 cache-read 计费，必须保留。
        self.usage.has_billable_tokens().then(|| TokenUsage {
            model: self.model.clone(),
            message_id: self.message_id.clone(),
            ..self.usage.clone()
        })
    }
}

/// OpenAI Chat Completions 流式 usage 的折叠状态
#[derive(Debug, Default)]
struct ChatFold {
    /// 最后一个 usage 非 null 的 chunk 的解析结果（该 chunk 字段不全时为 None）
    last_usage: Option<TokenUsage>,
    /// 首个非空的顶层 `model`（usage 缺模型时的回退）
    first_model: Option<String>,
}

impl ChatFold {
    fn push(&mut self, event: &Value) {
        // OpenAI 流式响应在最后一个 chunk 中包含 usage
        if event.get("usage").is_some_and(|usage| !usage.is_null()) {
            self.last_usage = TokenUsage::from_openai_response(event);
        }
        if self.first_model.is_none() {
            self.first_model = non_empty_str(event.get("model"));
        }
    }
}

/// Codex 自动检测格式的折叠状态
#[derive(Debug, Default)]
struct CodexFold {
    /// 首个带 `response` 的 `response.completed` 事件的解析结果（一经出现即锁定）
    completed: Option<Option<TokenUsage>>,
    /// 首个 `response.completed` 中非空的 `response.model`
    completed_model: Option<String>,
    chat: ChatFold,
}

impl CodexFold {
    fn push(&mut self, event: &Value) {
        if event.get("type").and_then(|v| v.as_str()) == Some("response.completed") {
            if let Some(response) = event.get("response") {
                if self.completed.is_none() {
                    self.completed = Some(TokenUsage::from_codex_response_auto(response));
                }
                if self.completed_model.is_none() {
                    self.completed_model = non_empty_str(response.get("model"));
                }
            }
        }
        // 未出现 response.completed 时回退到 OpenAI Chat Completions 格式
        if self.completed.is_none() {
            self.chat.push(event);
        } else if self.chat.first_model.is_none() {
            self.chat.first_model = non_empty_str(event.get("model"));
        }
    }

    fn usage(&self) -> Option<TokenUsage> {
        match &self.completed {
            Some(usage) => usage.clone(),
            None => self.chat.last_usage.clone(),
        }
    }
}

/// Gemini 流式 usage 的折叠状态
#[derive(Debug, Default)]
struct GeminiFold {
    usage: UsageIR,
    model: Option<String>,
}

impl GeminiFold {
    fn push(&mut self, chunk: &Value) {
        // usageMetadata 为累计值（输入通常在所有 chunk 中保持不变），取最后一个
        if let Some(metadata) = chunk.get("usageMetadata") {
            self.usage = UsageIR::from_gemini(metadata);
        }
        // 提取实际使用的模型名称（modelVersion 字段）
        if self.model.is_none() {
            if let Some(model_version) = chunk.get("modelVersion").and_then(|v| v.as_str()) {
                self.model = Some(model_version.to_string());
            }
        }
    }

    fn usage(&self) -> Option<TokenUsage> {
        let usage = self.usage.to_token_usage();
        (usage.input_tokens > 0 || usage.output_tokens > 0).then(|| TokenUsage {
            model: self.model.clone(),
            ..usage
        })
    }
}

#[derive(Debug)]
enum Fold {
    Anthropic(AnthropicFold),
    OpenAiChat(ChatFold),
    CodexAuto(CodexFold),
    Gemini(GeminiFold),
}

/// 流式 usage 的运行总量；内存占用与事件数无关
#[derive(Debug)]
pub struct UsageTap {
    format: StreamUsageFormat,
    fold: Fold,
    events: u64,
    /// 与 `usage()` 同源的上游原始 usage 块（仅保留一个，供 usage parity 检查）；
    /// Anthropic 的 usage 分散在 message_start/message_delta 中，不保留。
    raw_usage: Option<Value>,
}

impl UsageTap {
    pub fn new(format: StreamUsageFormat) -> Self {
        let fold = match format {
            StreamUsageFormat::Anthropic => Fold::Anthropic(AnthropicFold::default()),
            StreamUsageFormat::OpenAiChat => Fold::OpenAiChat(ChatFold::default()),
            StreamUsageFormat::CodexAuto => Fold::CodexAuto(CodexFold::default()),
            StreamUsageFormat::Gemini => Fold::Gemini(GeminiFold::default()),
        };
        Self {
            format,
            fold,
            events: 0,
            raw_usage: None,
        }
    }

    pub fn format(&self) -> StreamUsageFormat {
        self.format
    }

    /// 一次性折叠一组事件（批量解析与测试用）
    #[cfg(any(test, feature = "testkit"))]
    pub fn from_events<'a>(
        format: StreamUsageFormat,
        events: impl IntoIterator<Item = &'a Value>,
    ) -> Self {
        let mut tap = Self::new(format);
        for event in events {
            tap.push(event);
        }
        tap
    }

    /// 折叠一个已解析的 SSE `data:` JSON 事件
    pub fn push(&mut self, event: &Value) {
        self.events += 1;
        self.capture_raw_usage(event);
        match &mut self.fold {
            Fold::Anthropic(fold) => fold.push(event),
            Fold::OpenAiChat(fold) => fold.push(event),
            Fold::CodexAuto(fold) => fold.push(event),
            Fold::Gemini(fold) => fold.push(event),
        }
    }

    /// 按各格式 `usage()` 的取值规则记下对应的原始 usage 块（需在折叠前调用）
    fn capture_raw_usage(&mut self, event: &Value) {
        let raw = match &self.fold {
            Fold::Anthropic(_) => return,
            // response.completed 一经出现即锁定，其后的 Chat chunk 不再覆盖
            Fold::CodexAuto(fold) if fold.completed.is_some() => return,
            Fold::CodexAuto(_)
                if event.get("type").and_then(|v| v.as_str()) == Some("response.completed") =>
            {
                if event.get("response").is_some() {
                    self.raw_usage = event
                        .pointer("/response/usage")
                        .filter(|usage| usage.is_object())
                        .cloned();
                }
                return;
            }
            Fold::OpenAiChat(_) | Fold::CodexAuto(_) => event.get("usage"),
            Fold::Gemini(_) => event.get("usageMetadata"),
        };
        if let Some(raw) = raw.filter(|usage| usage.is_object()) {
            self.raw_usage = Some(raw.clone());
        }
    }

    /// 上游原始 usage 块（Anthropic 格式恒为 None）
    pub fn raw_usage(&self) -> Option<&Value> {
        self.raw_usage.as_ref()
    }

    /// 已折叠的事件数
    pub fn events(&self) -> u64 {
        self.events
    }

    /// 当前累计的 usage；与 `TokenUsage::from_*_stream_*` 对同一事件序列的结果一致
    pub fn usage(&self) -> Option<TokenUsage> {
        match &self.fold {
            Fold::Anthropic(fold) => fold.usage(),
            Fold::OpenAiChat(fold) => fold.last_usage.clone(),
            Fold::CodexAuto(fold) => fold.usage(),
            Fold::Gemini(fold) => fold.usage(),
        }
    }

    /// 归因模型：usage 中的非空模型优先，其次按格式回退到事件里的模型字段，
    /// 最后才是 `fallback_model`（映射后的出站模型或客户端请求模型）。
    ///
    /// 空字符串模型名视为缺失（转换层对无回显上游会合成 model:""）。
    pub fn model(&self, fallback_model: &str) -> String {
        if let Some(model) = self.usage().and_then(|u| u.model).filter(|m| !m.is_empty()) {
            return model;
        }
        let event_model = match &self.fold {
            Fold::OpenAiChat(fold) => fold.first_model.clone(),
            Fold::CodexAuto(fold) => fold
                .completed_model
                .clone()
                .or_else(|| fold.chat.first_model.clone()),
            Fold::Anthropic(_) | Fold::Gemini(_) => None,
        };
        event_model.unwrap_or_else(|| fallback_model.to_string())
    }

    /// tap 自身持有的堆内存字节数（字符串容量），用于验证内存不随事件数增长
    #[cfg(test)]
    fn retained_bytes(&self) -> usize {
        fn cap(s: &Option<String>) -> usize {
            s.as_ref().map_or(0, String::capacity)
        }
        fn usage_cap(u: &TokenUsage) -> usize {
            cap(&u.model) + cap(&u.message_id)
        }
        match &self.fold {
            Fold::Anthropic(f) => usage_cap(&f.usage) + cap(&f.model) + cap(&f.message_id),
            Fold::OpenAiChat(f) => f.last_usage.as_ref().map_or(0, usage_cap) + cap(&f.first_model),
            Fold::CodexAuto(f) => {
                f.completed
                    .as_ref()
                    .and_then(Option::as_ref)
                    .map_or(0, usage_cap)
                    + cap(&f.completed_model)
                    + f.chat.last_usage.as_ref().map_or(0, usage_cap)
                    + cap(&f.chat.first_model)
            }
            Fold::Gemini(f) => cap(&f.model),
        }
    }
}

fn non_empty_str(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// 流式响应的时序观测：首字节（TTFB）与首个 token 事件
#[derive(Debug, Clone, Copy)]
pub struct TimingTap {
    start: Instant,
    first_byte: Option<Instant>,
    first_token: Option<Instant>,
}

impl TimingTap {
    pub fn new(start: Instant) -> Self {
        Self {
            start,
            first_byte: None,
            first_token: None,
        }
    }

    /// 收到上游首个字节块时调用；只记录第一次
    pub fn on_first_byte(&mut self) {
        self.first_byte.get_or_insert_with(Instant::now);
    }

    /// 收到首个 token 事件时调用；只记录第一次
    ///
    /// 沿用 `first_token_ms` 的既有近似语义：以首个被 usage 收集的 SSE 事件为准。
    pub fn on_token(&mut self) {
        self.first_token.get_or_insert_with(Instant::now);
    }

    pub fn ttfb_ms(&self) -> Option<u64> {
        self.first_byte.map(|t| self.elapsed_ms(t))
    }

    pub fn first_token_ms(&self) -> Option<u64> {
        self.first_token.map(|t| self.elapsed_ms(t))
    }

    fn elapsed_ms(&self, t: Instant) -> u64 {
        t.saturating_duration_since(self.start).as_millis() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn model_of(format: StreamUsageFormat, events: &[Value], fallback: &str) -> String {
        UsageTap::from_events(format, events).model(fallback)
    }

    #[test]
    fn codex_chunk_model_after_completed_is_still_a_fallback() {
        // response.completed 锁定 usage 后，后续 chunk 的顶层 model 仍可作模型回退
        let events = vec![
            json!({"type": "response.completed", "response": {"model": ""}}),
            json!({"model": "late-model"}),
        ];
        assert_eq!(
            model_of(StreamUsageFormat::CodexAuto, &events, "fallback"),
            "late-model"
        );
    }

    #[test]
    fn codex_first_completed_event_wins_over_later_chat_usage() {
        let events = vec![
            json!({"type": "response.completed", "response": {"model": "m", "usage": {"input_tokens": 3, "output_tokens": 4}}}),
            json!({"usage": {"prompt_tokens": 100, "completion_tokens": 200}}),
            json!({"type": "response.completed", "response": {"usage": {"input_tokens": 9, "output_tokens": 9}}}),
        ];
        let usage = UsageTap::from_events(StreamUsageFormat::CodexAuto, &events)
            .usage()
            .unwrap();
        assert_eq!((usage.input_tokens, usage.output_tokens), (3, 4));
    }

    #[test]
    fn raw_usage_tracks_the_block_usage_is_read_from() {
        let events = vec![
            json!({"usage": {"prompt_tokens": 5, "completion_tokens": 6}}),
            json!({"usage": null}),
        ];
        let tap = UsageTap::from_events(StreamUsageFormat::OpenAiChat, &events);
        assert_eq!(
            tap.raw_usage(),
            Some(&json!({"prompt_tokens": 5, "completion_tokens": 6}))
        );

        // Codex：首个 response.completed 锁定，其后的 Chat chunk 不覆盖
        let events = vec![
            json!({"usage": {"prompt_tokens": 1, "completion_tokens": 1}}),
            json!({"type": "response.completed", "response": {"usage": {"input_tokens": 7, "output_tokens": 8}}}),
            json!({"usage": {"prompt_tokens": 9, "completion_tokens": 9}}),
        ];
        let tap = UsageTap::from_events(StreamUsageFormat::CodexAuto, &events);
        assert_eq!(
            tap.raw_usage(),
            Some(&json!({"input_tokens": 7, "output_tokens": 8}))
        );

        let events = vec![
            json!({"usageMetadata": {"promptTokenCount": 1, "totalTokenCount": 2}}),
            json!({"usageMetadata": {"promptTokenCount": 1, "totalTokenCount": 5}}),
        ];
        let tap = UsageTap::from_events(StreamUsageFormat::Gemini, &events);
        assert_eq!(
            tap.raw_usage(),
            Some(&json!({"promptTokenCount": 1, "totalTokenCount": 5}))
        );

        let events =
            vec![json!({"type": "message_start", "message": {"usage": {"input_tokens": 3}}})];
        let tap = UsageTap::from_events(StreamUsageFormat::Anthropic, &events);
        assert_eq!(tap.raw_usage(), None);
    }

    #[test]
    fn openai_last_usage_chunk_wins_even_if_unparseable() {
        // 与原实现一致：取最后一个 usage 非 null 的 chunk，字段不全时整体为 None
        let events = vec![
            json!({"usage": {"prompt_tokens": 5, "completion_tokens": 6}}),
            json!({"usage": {"prompt_tokens": 5}}),
        ];
        assert!(
            UsageTap::from_events(StreamUsageFormat::OpenAiChat, &events)
                .usage()
                .is_none()
        );
        let events = vec![
            json!({"usage": {"prompt_tokens": 5, "completion_tokens": 6}}),
            json!({"usage": null}),
        ];
        let usage = UsageTap::from_events(StreamUsageFormat::OpenAiChat, &events)
            .usage()
            .unwrap();
        assert_eq!((usage.input_tokens, usage.output_tokens), (5, 6));
    }

    #[test]
    fn usage_tap_memory_is_constant_in_event_count() {
        fn anthropic_event(i: u64) -> Value {
            if i == 0 {
                json!({"type": "message_start", "message": {"id": "msg_1", "model": "claude-x", "usage": {"input_tokens": 10}}})
            } else {
                json!({"type": "message_delta", "model": format!("m-{i}"), "usage": {"output_tokens": i}})
            }
        }
        fn chat_event(i: u64) -> Value {
            json!({"model": format!("gpt-{i}"), "usage": {"prompt_tokens": 1, "completion_tokens": i}})
        }
        fn codex_event(i: u64) -> Value {
            if i.is_multiple_of(2) {
                json!({"type": "response.completed", "response": {"model": format!("r-{i}"), "usage": {"input_tokens": 1, "output_tokens": i}}})
            } else {
                chat_event(i)
            }
        }
        fn gemini_event(i: u64) -> Value {
            json!({"modelVersion": format!("g-{i}"), "usageMetadata": {"promptTokenCount": 1, "candidatesTokenCount": i, "totalTokenCount": i + 1}})
        }

        const SMALL: u64 = 100;
        const LARGE: u64 = 100_000;
        type MakeEvent = fn(u64) -> Value;
        let cases: [(StreamUsageFormat, MakeEvent); 4] = [
            (StreamUsageFormat::Anthropic, anthropic_event),
            (StreamUsageFormat::OpenAiChat, chat_event),
            (StreamUsageFormat::CodexAuto, codex_event),
            (StreamUsageFormat::Gemini, gemini_event),
        ];
        for (format, make) in cases {
            let mut tap = UsageTap::new(format);
            let mut small_bytes = 0;
            for i in 0..LARGE {
                tap.push(&make(i));
                if i + 1 == SMALL {
                    small_bytes = tap.retained_bytes();
                }
            }
            assert_eq!(tap.events(), LARGE);
            // 运行总量只按格式规则保留首个/最后一个值，不随事件数增长
            let large_bytes = tap.retained_bytes();
            assert!(
                large_bytes <= small_bytes + 8,
                "{format:?}: retained {large_bytes} bytes after {LARGE} events vs {small_bytes} after {SMALL}"
            );
            assert!(tap.usage().is_some(), "{format:?} lost usage");
        }
    }

    #[test]
    fn timing_tap_records_first_byte_and_first_token_once() {
        let start = Instant::now() - std::time::Duration::from_millis(50);
        let mut timing = TimingTap::new(start);
        assert_eq!(timing.ttfb_ms(), None);
        assert_eq!(timing.first_token_ms(), None);

        timing.on_first_byte();
        let ttfb = timing.ttfb_ms().unwrap();
        assert!(ttfb >= 50);
        std::thread::sleep(std::time::Duration::from_millis(5));
        timing.on_token();
        let first_token = timing.first_token_ms().unwrap();
        assert!(first_token >= ttfb + 5);

        std::thread::sleep(std::time::Duration::from_millis(5));
        timing.on_first_byte();
        timing.on_token();
        assert_eq!(timing.ttfb_ms(), Some(ttfb));
        assert_eq!(timing.first_token_ms(), Some(first_token));
    }
}
