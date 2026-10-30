//! Anthropic thinking 块在 Responses `reasoning.encrypted_content` 中的信封
//!
//! Responses 没有承载 Anthropic 签名 thinking 的字段：整个块（含签名）序列化后加上
//! 版本前缀放进不透明的 `encrypted_content`，Codex 在下一次工具结果请求中原样带回，
//! 由请求解码还原。前缀把其他供应商的密文隔离开。

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde_json::{json, Value};

pub(crate) const ANTHROPIC_THINKING_ENCRYPTED_PREFIX: &str = "ccswitch-anthropic-thinking-v1:";

/// Preserve an Anthropic signed thinking/redacted-thinking block inside the opaque
/// Responses `reasoning.encrypted_content` field so Codex replays it on the next
/// tool-result request. The prefix keeps unrelated providers' ciphertext isolated.
pub(crate) fn encode_anthropic_thinking_block(block: &Value) -> Option<String> {
    match block.get("type").and_then(|value| value.as_str()) {
        Some("thinking")
            if block
                .get("signature")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.is_empty()) => {}
        Some("redacted_thinking")
            if block
                .get("data")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.is_empty()) => {}
        _ => return None,
    }
    let bytes = serde_json::to_vec(block).ok()?;
    Some(format!(
        "{ANTHROPIC_THINKING_ENCRYPTED_PREFIX}{}",
        URL_SAFE_NO_PAD.encode(bytes)
    ))
}

pub(crate) fn decode_anthropic_thinking_block(encrypted_content: &str) -> Option<Value> {
    let encoded = encrypted_content.strip_prefix(ANTHROPIC_THINKING_ENCRYPTED_PREFIX)?;
    let bytes = URL_SAFE_NO_PAD.decode(encoded).ok()?;
    let block: Value = serde_json::from_slice(&bytes).ok()?;
    // Reuse the encoder's validation so legacy/malformed bridge envelopes cannot
    // replay an unsigned thinking block into an Anthropic tool turn.
    encode_anthropic_thinking_block(&block).map(|_| block)
}

pub(crate) fn responses_reasoning_item_from_anthropic_block(
    item_id: &str,
    block: &Value,
) -> Option<Value> {
    let encrypted_content = encode_anthropic_thinking_block(block)?;
    let summary = block
        .get("thinking")
        .and_then(|value| value.as_str())
        .filter(|text| !text.is_empty())
        .map(|text| vec![json!({ "type": "summary_text", "text": text })])
        .unwrap_or_default();
    Some(json!({
        "id": item_id,
        "type": "reasoning",
        "summary": summary,
        "encrypted_content": encrypted_content
    }))
}
