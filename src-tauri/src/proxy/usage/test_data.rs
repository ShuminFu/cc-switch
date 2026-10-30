//! 测试用的上游 usage 块与会话日志行
//!
//! 缓存 token 字段名只允许出现在 codec/ 与 usage/ 中（CCS-243），
//! 其他模块的测试经这里构造带缓存字段的 JSON。

use serde_json::{json, Value};

/// OpenAI Chat Completions usage（`prompt_tokens` 含缓存命中）
pub fn chat_usage(prompt: u64, completion: u64, cached: u64) -> Value {
    json!({
        "prompt_tokens": prompt,
        "completion_tokens": completion,
        "total_tokens": prompt + completion,
        "prompt_tokens_details": {"cached_tokens": cached}
    })
}

/// OpenAI Responses usage（`input_tokens` 含缓存读写）
pub fn responses_usage(input: u64, output: u64, cached: u64, cache_write: Option<u64>) -> Value {
    let mut details = json!({"cached_tokens": cached});
    if let Some(cache_write) = cache_write {
        details["cache_write_tokens"] = json!(cache_write);
    }
    json!({
        "input_tokens": input,
        "output_tokens": output,
        "input_tokens_details": details
    })
}

/// Claude Code 会话日志中的一行 assistant 消息（JSONL）
///
/// `tokens` 依次为 input、output、cache read、cache creation。
pub fn claude_session_line(
    message_id: &str,
    model: &str,
    stop_reason: Option<&str>,
    tokens: [u64; 4],
    timestamp: &str,
    session_id: &str,
) -> String {
    let [input, output, cache_read, cache_creation] = tokens;
    json!({
        "type": "assistant",
        "message": {
            "id": message_id,
            "model": model,
            "stop_reason": stop_reason,
            "usage": {
                "input_tokens": input,
                "output_tokens": output,
                "cache_read_input_tokens": cache_read,
                "cache_creation_input_tokens": cache_creation
            }
        },
        "timestamp": timestamp,
        "sessionId": session_id
    })
    .to_string()
}
