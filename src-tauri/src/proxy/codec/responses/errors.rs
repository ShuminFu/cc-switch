//! 上游错误体 → Responses 错误对象

use serde_json::{json, Value};

/// 把 Chat Completions 上游的错误体规整成 OpenAI Responses API 风格的错误对象。
///
/// 兼容三类输入：
/// 1. 标准 OpenAI 形式 `{"error": {"message": "...", "type": "...", "code": ...}}`
/// 2. MiniMax 等非标形式（如 `{"base_resp": {"status_code": 2013, "status_msg": "..."}}`）
/// 3. 顶层只有 `message` / `detail` / 裸字符串的最小错误
///
/// 输出统一为 `{"error": {"message", "type", "code", "param"}}`，与 OpenAI Responses
/// API 错误响应一致；Codex 客户端的错误处理只识别这个形状。
pub fn chat_error_to_response_error(body: Option<&Value>) -> Value {
    let Some(value) = body else {
        return json!({
            "error": {
                "message": "Upstream returned an empty error response",
                "type": "upstream_error",
                "code": serde_json::Value::Null,
                "param": serde_json::Value::Null,
            }
        });
    };

    if let Some(text) = value.as_str() {
        return json!({
            "error": {
                "message": text,
                "type": "upstream_error",
                "code": serde_json::Value::Null,
                "param": serde_json::Value::Null,
            }
        });
    }

    let source = value.get("error").unwrap_or(value);

    let message = source
        .get("message")
        .or_else(|| source.get("detail"))
        .or_else(|| source.get("status_msg"))
        .or_else(|| source.pointer("/base_resp/status_msg"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .or_else(|| source.as_str().map(ToString::to_string))
        .unwrap_or_else(|| {
            // 没法从字段提取出文本，就把整个 JSON 序列化回去，方便用户排查。
            serde_json::to_string(source).unwrap_or_else(|_| "Upstream error".to_string())
        });

    let error_type = source
        .get("type")
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .unwrap_or_else(|| "upstream_error".to_string());

    let code = source
        .get("code")
        .cloned()
        .or_else(|| source.pointer("/base_resp/status_code").cloned())
        .unwrap_or(serde_json::Value::Null);

    let param = source
        .get("param")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    json!({
        "error": {
            "message": message,
            "type": error_type,
            "code": code,
            "param": param,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn chat_error_to_response_error_normalizes_standard_openai_shape() {
        let input = json!({
            "error": {
                "message": "Invalid API key",
                "type": "invalid_request_error",
                "code": "invalid_api_key",
                "param": "api_key"
            }
        });

        let result = chat_error_to_response_error(Some(&input));

        assert_eq!(result["error"]["message"], "Invalid API key");
        assert_eq!(result["error"]["type"], "invalid_request_error");
        assert_eq!(result["error"]["code"], "invalid_api_key");
        assert_eq!(result["error"]["param"], "api_key");
    }

    #[test]
    fn chat_error_to_response_error_normalizes_minimax_base_resp() {
        // MiniMax 把错误塞在 base_resp 里，code 是数字而不是字符串
        let input = json!({
            "base_resp": {
                "status_code": 2013,
                "status_msg": "invalid params, chat content has invalid message role: system"
            }
        });

        let result = chat_error_to_response_error(Some(&input));

        assert_eq!(
            result["error"]["message"],
            "invalid params, chat content has invalid message role: system"
        );
        assert_eq!(result["error"]["code"], 2013);
        // type 没有显式给出，应该回落到 upstream_error
        assert_eq!(result["error"]["type"], "upstream_error");
    }

    #[test]
    fn chat_error_to_response_error_handles_plain_text_body() {
        let input = json!("Upstream timeout");

        let result = chat_error_to_response_error(Some(&input));

        assert_eq!(result["error"]["message"], "Upstream timeout");
        assert_eq!(result["error"]["type"], "upstream_error");
        assert!(result["error"]["code"].is_null());
        assert!(result["error"]["param"].is_null());
    }

    #[test]
    fn chat_error_to_response_error_handles_missing_body() {
        let result = chat_error_to_response_error(None);

        assert_eq!(
            result["error"]["message"],
            "Upstream returned an empty error response"
        );
        assert_eq!(result["error"]["type"], "upstream_error");
    }

    #[test]
    fn chat_error_to_response_error_falls_back_to_detail_field() {
        // 部分中转把错误塞在顶层 detail 字段（OpenAI 兼容层常见）
        let input = json!({
            "detail": "rate limit exceeded"
        });

        let result = chat_error_to_response_error(Some(&input));

        assert_eq!(result["error"]["message"], "rate limit exceeded");
        assert_eq!(result["error"]["type"], "upstream_error");
    }
}
