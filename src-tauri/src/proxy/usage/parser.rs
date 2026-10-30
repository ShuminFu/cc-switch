//! Response Parser - 从 API 响应中提取 token 使用量
//!
//! 支持多种 API 格式：
//! - Claude API (非流式和流式)
//! - OpenRouter (OpenAI 格式)
//! - Codex API (非流式和流式)
//! - Gemini API (非流式和流式)

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ir::UsageIR;
#[cfg(any(test, feature = "testkit"))]
use crate::proxy::stream::{StreamUsageFormat, UsageTap};

/// Session 日志 request_id 前缀，与 `session_usage.rs` 中的格式保持一致
pub const SESSION_REQUEST_ID_PREFIX: &str = "session:";

/// Token 使用量统计
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_creation_tokens: u32,
    /// 从响应中提取的实际模型名称（如果可用）
    pub model: Option<String>,
    /// 从响应中提取的消息 ID（用于跨源去重）
    ///
    /// Claude API: `msg_xxx`，与 session JSONL 中的 `message.id` 一致
    #[serde(skip)]
    pub message_id: Option<String>,
}

impl TokenUsage {
    /// 生成与 session 日志共享的 request_id，用于跨源去重。
    /// 有 message_id 时返回 `session:{id}`，否则回退到随机 UUID。
    pub fn dedup_request_id(&self) -> String {
        self.message_id
            .as_ref()
            .map(|mid| format!("{SESSION_REQUEST_ID_PREFIX}{mid}"))
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
    }

    /// 是否产生了任一计费维度的 token。
    ///
    /// 用于在写入前过滤全 0 的空 usage：当 OpenAI 兼容上游在流式下省略 usage 时，
    /// 转换器会合成一个全 0 的终止事件，若无 message_id 则 `dedup_request_id`
    /// 退化为随机 UUID，导致每笔请求插入一条无意义的空行、虚增请求数。
    pub fn has_billable_tokens(&self) -> bool {
        self.input_tokens > 0
            || self.output_tokens > 0
            || self.cache_read_tokens > 0
            || self.cache_creation_tokens > 0
    }
}

impl TokenUsage {
    /// 从 Claude API 非流式响应解析
    pub fn from_claude_response(body: &Value) -> Option<Self> {
        let usage = body.get("usage")?;
        // 提取响应中的模型名称
        let model = body
            .get("model")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let message_id = body
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // input/output 字段必须存在（哪怕为 0），否则视为无 usage
        usage.get("input_tokens")?.as_u64()?;
        usage.get("output_tokens")?.as_u64()?;

        Some(Self {
            model,
            message_id,
            ..UsageIR::from_anthropic(usage).to_token_usage()
        })
    }

    /// 从 Claude API 流式响应解析（批量折叠，运行时由 `UsageTap` 逐事件累计）
    #[cfg(any(test, feature = "testkit"))]
    pub fn from_claude_stream_events(events: &[Value]) -> Option<Self> {
        UsageTap::from_events(StreamUsageFormat::Anthropic, events).usage()
    }

    /// 从 Codex API 非流式响应解析
    pub fn from_codex_response(body: &Value) -> Option<Self> {
        let usage = body.get("usage");
        if usage.is_none() {
            log::debug!(
                "[Codex] 响应中没有 usage 字段，body keys: {:?}",
                body.as_object().map(|o| o.keys().collect::<Vec<_>>())
            );
            return None;
        }
        let usage = usage?;

        let input_tokens = usage.get("input_tokens").and_then(|v| v.as_u64());
        let output_tokens = usage.get("output_tokens").and_then(|v| v.as_u64());

        if input_tokens.is_none() || output_tokens.is_none() {
            log::debug!("[Codex] usage 字段缺少 input_tokens 或 output_tokens，usage: {usage:?}");
            return None;
        }

        // 提取响应中的模型名称
        let model = body
            .get("model")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Some(Self {
            model,
            ..UsageIR::from_responses(usage).to_token_usage()
        })
    }

    /// 智能 Codex 响应解析 - 自动检测 OpenAI 或 Codex 格式
    ///
    /// Codex 支持两种 API 格式：
    /// - `/v1/responses`: 使用 input_tokens/output_tokens
    /// - `/v1/chat/completions`: 使用 prompt_tokens/completion_tokens (OpenAI 格式)
    ///
    /// 注意：记录原始 input_tokens，费用计算时再减去 cached_tokens
    pub fn from_codex_response_auto(body: &Value) -> Option<Self> {
        let usage = body.get("usage")?;

        // 检测格式：OpenAI 使用 prompt_tokens，Codex 使用 input_tokens
        if usage.get("prompt_tokens").is_some() {
            log::debug!("[Codex] 检测到 OpenAI 格式 (prompt_tokens)");
            Self::from_openai_response(body)
        } else if usage.get("input_tokens").is_some() {
            log::debug!("[Codex] 检测到 Codex 格式 (input_tokens)");
            // 使用非调整版本，记录原始 input_tokens
            Self::from_codex_response(body)
        } else {
            log::debug!("[Codex] 无法识别响应格式，usage: {usage:?}");
            None
        }
    }

    /// 智能 Codex 流式响应解析 - 自动检测 OpenAI 或 Codex 格式
    #[cfg(any(test, feature = "testkit"))]
    pub fn from_codex_stream_events_auto(events: &[Value]) -> Option<Self> {
        UsageTap::from_events(StreamUsageFormat::CodexAuto, events).usage()
    }

    /// 从 OpenAI Chat Completions API 响应解析 (prompt_tokens, completion_tokens)
    pub fn from_openai_response(body: &Value) -> Option<Self> {
        let usage = body.get("usage")?;

        // OpenAI 使用 prompt_tokens 和 completion_tokens，两者必须存在
        usage.get("prompt_tokens").and_then(|v| v.as_u64())?;
        usage.get("completion_tokens").and_then(|v| v.as_u64())?;

        // 提取响应中的模型名称
        let model = body
            .get("model")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Some(Self {
            model,
            ..UsageIR::from_chat_completions(usage).to_token_usage()
        })
    }

    /// 从 OpenAI Chat Completions API 流式响应解析
    #[cfg(any(test, feature = "testkit"))]
    pub fn from_openai_stream_events(events: &[Value]) -> Option<Self> {
        UsageTap::from_events(StreamUsageFormat::OpenAiChat, events).usage()
    }

    /// 从 Gemini API 非流式响应解析
    pub fn from_gemini_response(body: &Value) -> Option<Self> {
        let usage = body.get("usageMetadata")?;
        // 提取实际使用的模型名称（modelVersion 字段）
        let model = body
            .get("modelVersion")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // promptTokenCount/totalTokenCount 必须存在；输出 = 总 tokens - 输入 tokens
        // （包含 candidatesTokenCount + thoughtsTokenCount）
        usage.get("promptTokenCount")?.as_u64()?;
        usage.get("totalTokenCount")?.as_u64()?;

        Some(Self {
            model,
            ..UsageIR::from_gemini(usage).to_token_usage()
        })
    }

    /// 从 Gemini API 流式响应解析
    #[cfg(any(test, feature = "testkit"))]
    pub fn from_gemini_stream_chunks(chunks: &[Value]) -> Option<Self> {
        UsageTap::from_events(StreamUsageFormat::Gemini, chunks).usage()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_claude_response_parsing() {
        let response = json!({
            "model": "claude-sonnet-4-20250514",
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50,
                "cache_read_input_tokens": 20,
                "cache_creation_input_tokens": 10
            }
        });

        let usage = TokenUsage::from_claude_response(&response).unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cache_read_tokens, 20);
        assert_eq!(usage.cache_creation_tokens, 10);
        assert_eq!(usage.model, Some("claude-sonnet-4-20250514".to_string()));
    }

    #[test]
    fn test_has_billable_tokens_gates_empty_usage() {
        // 全 0 usage（如上游省略 usage 时合成的全 0 终止事件）不应计费——
        // 这是 Codex 流式空行多记修复（D）的闸门依据。
        assert!(!TokenUsage::default().has_billable_tokens());
        // 仅有 cache_read 也属于真实计费 token，必须计入。
        let only_cache = TokenUsage {
            cache_read_tokens: 100,
            ..Default::default()
        };
        assert!(only_cache.has_billable_tokens());
        let normal = TokenUsage {
            input_tokens: 10,
            output_tokens: 5,
            ..Default::default()
        };
        assert!(normal.has_billable_tokens());
    }

    #[test]
    fn test_claude_stream_cache_only_request_is_recorded() {
        // P2 回归：完全缓存命中、无输出的流式请求（input==0 && output==0 但 cache_read>0）
        // 是真实计费，必须保留——旧 gate `input>0 || output>0` 会把它丢弃。
        let events = vec![
            json!({
                "type": "message_start",
                "message": {
                    "id": "msg_cacheonly",
                    "model": "claude-opus-4-8",
                    "usage": {
                        "input_tokens": 0,
                        "cache_read_input_tokens": 50000,
                        "cache_creation_input_tokens": 0
                    }
                }
            }),
            json!({
                "type": "message_delta",
                "usage": { "output_tokens": 0 }
            }),
        ];
        let usage = TokenUsage::from_claude_stream_events(&events)
            .expect("cache-only 流式请求必须被记录，不能被 input/output gate 丢弃");
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
        assert_eq!(usage.cache_read_tokens, 50000);
        assert_eq!(usage.message_id, Some("msg_cacheonly".to_string()));
    }

    #[test]
    fn test_codex_response_auto_returns_some_for_synthetic_all_zero() {
        // P3 回归：上游非流式 Chat 省略 usage 时转换器合成的全 0 usage，from_codex_response_auto
        // 仍返回 Some（字段存在、无 positivity check）——证明 handlers 必须用 has_billable_tokens
        // 闸门才能挡住空行，单靠 `if let Some` 不够。
        let synthetic = json!({
            "usage": { "input_tokens": 0, "output_tokens": 0, "total_tokens": 0 }
        });
        let usage = TokenUsage::from_codex_response_auto(&synthetic)
            .expect("全 0 usage 字段存在时 from_codex_response_auto 返回 Some");
        assert!(
            !usage.has_billable_tokens(),
            "全 0 usage 必须被 has_billable_tokens 判为非计费，由 handlers 闸门跳过"
        );
    }

    #[test]
    fn test_claude_response_parsing_no_model() {
        let response = json!({
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50,
                "cache_read_input_tokens": 20,
                "cache_creation_input_tokens": 10
            }
        });

        let usage = TokenUsage::from_claude_response(&response).unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cache_read_tokens, 20);
        assert_eq!(usage.cache_creation_tokens, 10);
        assert_eq!(usage.model, None);
    }

    #[test]
    fn test_claude_stream_parsing() {
        let events = vec![
            json!({
                "type": "message_start",
                "message": {
                    "model": "claude-sonnet-4-20250514",
                    "usage": {
                        "input_tokens": 100,
                        "cache_read_input_tokens": 20,
                        "cache_creation_input_tokens": 10
                    }
                }
            }),
            json!({
                "type": "message_delta",
                "usage": {
                    "output_tokens": 50
                }
            }),
        ];

        let usage = TokenUsage::from_claude_stream_events(&events).unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cache_read_tokens, 20);
        assert_eq!(usage.cache_creation_tokens, 10);
        assert_eq!(usage.model, Some("claude-sonnet-4-20250514".to_string()));
    }

    #[test]
    fn test_claude_stream_parsing_no_model() {
        let events = vec![
            json!({
                "type": "message_start",
                "message": {
                    "usage": {
                        "input_tokens": 100,
                        "cache_read_input_tokens": 20,
                        "cache_creation_input_tokens": 10
                    }
                }
            }),
            json!({
                "type": "message_delta",
                "usage": {
                    "output_tokens": 50
                }
            }),
        ];

        let usage = TokenUsage::from_claude_stream_events(&events).unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cache_read_tokens, 20);
        assert_eq!(usage.cache_creation_tokens, 10);
        assert_eq!(usage.model, None);
    }

    #[test]
    fn test_gemini_response_parsing() {
        let response = json!({
            "modelVersion": "gemini-3-pro-high",
            "usageMetadata": {
                "promptTokenCount": 8383,
                "candidatesTokenCount": 50,
                "thoughtsTokenCount": 114,
                "totalTokenCount": 8547,
                "cachedContentTokenCount": 20
            }
        });

        let usage = TokenUsage::from_gemini_response(&response).unwrap();
        assert_eq!(usage.input_tokens, 8383);
        // output_tokens = totalTokenCount - promptTokenCount = 8547 - 8383 = 164
        assert_eq!(usage.output_tokens, 164);
        assert_eq!(usage.cache_read_tokens, 20);
        assert_eq!(usage.cache_creation_tokens, 0);
        assert_eq!(usage.model, Some("gemini-3-pro-high".to_string()));
    }

    #[test]
    fn test_gemini_response_parsing_no_model() {
        // 测试没有 modelVersion 字段的情况
        let response = json!({
            "usageMetadata": {
                "promptTokenCount": 100,
                "totalTokenCount": 150,
                "cachedContentTokenCount": 20
            }
        });

        let usage = TokenUsage::from_gemini_response(&response).unwrap();
        assert_eq!(usage.input_tokens, 100);
        // output_tokens = totalTokenCount - promptTokenCount = 150 - 100 = 50
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cache_read_tokens, 20);
        assert_eq!(usage.cache_creation_tokens, 0);
        assert_eq!(usage.model, None);
    }

    #[test]
    fn test_gemini_response_with_thoughts() {
        // 测试包含 thoughtsTokenCount 的实际响应
        // 这是用户报告的真实场景
        let response = json!({
            "candidates": [
                {
                    "content": {
                        "parts": [
                            {
                                "text": "",
                                "thoughtSignature": "EvcECvQE..."
                            }
                        ],
                        "role": "model"
                    },
                    "finishReason": "STOP"
                }
            ],
            "modelVersion": "gemini-3-pro-high",
            "responseId": "yupTafqLDu-PjMcPhrOx4QQ",
            "usageMetadata": {
                "candidatesTokenCount": 50,
                "promptTokenCount": 8383,
                "thoughtsTokenCount": 114,
                "totalTokenCount": 8547
            }
        });

        let usage = TokenUsage::from_gemini_response(&response).unwrap();
        assert_eq!(usage.input_tokens, 8383);
        // output_tokens = totalTokenCount - promptTokenCount
        // = 8547 - 8383 = 164 (包含 candidatesTokenCount 50 + thoughtsTokenCount 114)
        assert_eq!(usage.output_tokens, 164);
        assert_eq!(usage.cache_read_tokens, 0);
        assert_eq!(usage.cache_creation_tokens, 0);
        assert_eq!(usage.model, Some("gemini-3-pro-high".to_string()));
    }

    #[test]
    fn test_codex_response_parsing_cached_tokens_in_details() {
        let response = json!({
            "usage": {
                "input_tokens": 1000,
                "output_tokens": 500,
                "input_tokens_details": {
                    "cached_tokens": 300
                }
            }
        });

        let usage = TokenUsage::from_codex_response(&response).unwrap();
        // 非调整模式：input_tokens 保持原值，但应记录缓存命中
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.output_tokens, 500);
        assert_eq!(usage.cache_read_tokens, 300);
    }

    #[test]
    fn test_codex_response_parsing_cache_write_tokens_in_details() {
        let response = json!({
            "usage": {
                "input_tokens": 1000,
                "output_tokens": 500,
                "input_tokens_details": {
                    "cached_tokens": 300,
                    "cache_write_tokens": 200
                }
            }
        });

        let usage = TokenUsage::from_codex_response(&response).unwrap();
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.cache_read_tokens, 300);
        assert_eq!(usage.cache_creation_tokens, 200);
    }

    #[test]
    fn test_openrouter_stream_parsing() {
        // 测试 OpenRouter 转换后的流式响应解析
        // OpenRouter 流式响应经过转换后，input_tokens 在 message_delta 中
        let events = vec![
            json!({
                "type": "message_start",
                "message": {
                    "model": "claude-sonnet-4-20250514",
                    "usage": {
                        "input_tokens": 0,
                        "output_tokens": 0
                    }
                }
            }),
            json!({
                "type": "message_delta",
                "delta": {
                    "stop_reason": "end_turn"
                },
                "usage": {
                    "input_tokens": 150,
                    "output_tokens": 75
                }
            }),
        ];

        let usage = TokenUsage::from_claude_stream_events(&events).unwrap();
        assert_eq!(usage.input_tokens, 150);
        assert_eq!(usage.output_tokens, 75);
        assert_eq!(usage.model, Some("claude-sonnet-4-20250514".to_string()));
    }

    #[test]
    fn test_claude_stream_prefers_smaller_delta_input_and_cache_pair() {
        // 部分 Anthropic-compatible provider 会在 message_start 给出包含缓存的总上下文，
        // 再在 message_delta 给出修正后的 fresh input，需要以 delta usage 为准。
        let events = vec![
            json!({
                "type": "message_start",
                "message": {
                    "model": "qwen-max",
                    "usage": {
                        "input_tokens": 200_000,
                        "cache_read_input_tokens": 180_000,
                        "cache_creation_input_tokens": 2_000
                    }
                }
            }),
            json!({
                "type": "message_delta",
                "usage": {
                    "input_tokens": 80_000,
                    "output_tokens": 1_000,
                    "cache_read_input_tokens": 120_000,
                    "cache_creation_input_tokens": 500
                }
            }),
        ];

        let usage = TokenUsage::from_claude_stream_events(&events).unwrap();
        assert_eq!(usage.input_tokens, 80_000);
        assert_eq!(usage.output_tokens, 1_000);
        assert_eq!(usage.cache_read_tokens, 120_000);
        assert_eq!(usage.cache_creation_tokens, 500);
        assert_eq!(usage.model, Some("qwen-max".to_string()));
    }

    #[test]
    fn test_claude_stream_updates_cache_pair_from_later_delta_input() {
        // 有些 provider 会多次发送带 input 的 message_delta；一旦采用过 delta input，
        // 后续相同/更小 input 的 delta 应继续更新同一块里的缓存计数。
        let events = vec![
            json!({
                "type": "message_start",
                "message": {
                    "model": "qwen-max",
                    "usage": {
                        "input_tokens": 200_000,
                        "cache_read_input_tokens": 180_000,
                        "cache_creation_input_tokens": 2_000
                    }
                }
            }),
            json!({
                "type": "message_delta",
                "usage": {
                    "input_tokens": 80_000,
                    "output_tokens": 100,
                    "cache_read_input_tokens": 110_000,
                    "cache_creation_input_tokens": 300
                }
            }),
            json!({
                "type": "message_delta",
                "usage": {
                    "input_tokens": 80_000,
                    "output_tokens": 1_000,
                    "cache_read_input_tokens": 120_000,
                    "cache_creation_input_tokens": 500
                }
            }),
        ];

        let usage = TokenUsage::from_claude_stream_events(&events).unwrap();
        assert_eq!(usage.input_tokens, 80_000);
        assert_eq!(usage.output_tokens, 1_000);
        assert_eq!(usage.cache_read_tokens, 120_000);
        assert_eq!(usage.cache_creation_tokens, 500);
        assert_eq!(usage.model, Some("qwen-max".to_string()));
    }

    #[test]
    fn test_claude_stream_keeps_start_when_delta_input_is_larger() {
        // 正常 Anthropic 语义下，message_start 的 input_tokens 已经可信；
        // 如果 delta input 变大，不应覆盖 start input/cache。
        let events = vec![
            json!({
                "type": "message_start",
                "message": {
                    "usage": {
                        "input_tokens": 100,
                        "cache_read_input_tokens": 20
                    }
                }
            }),
            json!({
                "type": "message_delta",
                "usage": {
                    "input_tokens": 150,
                    "output_tokens": 75,
                    "cache_read_input_tokens": 30
                }
            }),
        ];

        let usage = TokenUsage::from_claude_stream_events(&events).unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 75);
        assert_eq!(usage.cache_read_tokens, 20);
    }

    #[test]
    fn test_native_claude_stream_parsing() {
        // 测试原生 Claude API 流式响应解析
        // 原生 Claude API 的 input_tokens 在 message_start 中
        let events = vec![
            json!({
                "type": "message_start",
                "message": {
                    "model": "claude-sonnet-4-20250514",
                    "usage": {
                        "input_tokens": 200,
                        "cache_read_input_tokens": 50
                    }
                }
            }),
            json!({
                "type": "message_delta",
                "usage": {
                    "output_tokens": 100
                }
            }),
        ];

        let usage = TokenUsage::from_claude_stream_events(&events).unwrap();
        assert_eq!(usage.input_tokens, 200);
        assert_eq!(usage.output_tokens, 100);
        assert_eq!(usage.cache_read_tokens, 50);
        assert_eq!(usage.model, Some("claude-sonnet-4-20250514".to_string()));
    }

    // ============================================================================
    // 智能 Codex 解析测试
    // ============================================================================

    #[test]
    fn test_codex_response_auto_openai_format() {
        // OpenAI 格式 (prompt_tokens/completion_tokens)
        let response = json!({
            "model": "gpt-4o",
            "usage": {
                "prompt_tokens": 1000,
                "completion_tokens": 500,
                "prompt_tokens_details": {
                    "cached_tokens": 200
                }
            }
        });

        let usage = TokenUsage::from_codex_response_auto(&response).unwrap();
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.output_tokens, 500);
        assert_eq!(usage.cache_read_tokens, 200);
        assert_eq!(usage.model, Some("gpt-4o".to_string()));
    }

    #[test]
    fn test_codex_response_auto_codex_format() {
        // Codex 格式 (input_tokens/output_tokens)
        let response = json!({
            "model": "o3",
            "usage": {
                "input_tokens": 1000,
                "output_tokens": 500,
                "input_tokens_details": {
                    "cached_tokens": 300
                }
            }
        });

        let usage = TokenUsage::from_codex_response_auto(&response).unwrap();
        // 记录原始 input_tokens，不调整
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.output_tokens, 500);
        assert_eq!(usage.cache_read_tokens, 300);
        assert_eq!(usage.model, Some("o3".to_string()));
    }

    #[test]
    fn test_codex_stream_events_auto_codex_format() {
        // Codex Responses API 流式格式 (response.completed 事件)
        let events = vec![
            json!({
                "type": "response.created",
                "response": {
                    "id": "resp_123"
                }
            }),
            json!({
                "type": "response.completed",
                "response": {
                    "model": "o3",
                    "usage": {
                        "input_tokens": 1000,
                        "output_tokens": 500,
                        "input_tokens_details": {
                            "cached_tokens": 200
                        }
                    }
                }
            }),
        ];

        let usage = TokenUsage::from_codex_stream_events_auto(&events).unwrap();
        // 记录原始 input_tokens，不调整
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.output_tokens, 500);
        assert_eq!(usage.cache_read_tokens, 200);
        assert_eq!(usage.model, Some("o3".to_string()));
    }

    #[test]
    fn test_codex_stream_events_auto_openai_format() {
        // OpenAI Chat Completions 流式格式 (最后一个 chunk 包含 usage)
        let events = vec![
            json!({
                "id": "chatcmpl-123",
                "model": "gpt-4o",
                "choices": [{"delta": {"content": "Hello"}}]
            }),
            json!({
                "id": "chatcmpl-123",
                "model": "gpt-4o",
                "choices": [{"delta": {}}],
                "usage": {
                    "prompt_tokens": 100,
                    "completion_tokens": 50
                }
            }),
        ];

        let usage = TokenUsage::from_codex_stream_events_auto(&events).unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.model, Some("gpt-4o".to_string()));
    }
}
