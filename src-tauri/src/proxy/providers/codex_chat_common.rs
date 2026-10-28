use serde_json::{json, Map, Value};

const THINK_OPEN_TAG: &str = "<think>";
const THINK_CLOSE_TAG: &str = "</think>";

// 穷举上游可能的 reasoning 回传字段，优先级：reasoning_content > reasoning(字符串/对象) > reasoning_details。
// 不依赖 provider meta 的 outputFormat 声明，因此对各家 Chat 兼容接口都能兜底提取。
pub(crate) fn extract_reasoning_field_text(value: &Value) -> Option<String> {
    for key in ["reasoning_content", "reasoning"] {
        if let Some(text) = value.get(key).and_then(|v| v.as_str()) {
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }

    if let Some(reasoning) = value.get("reasoning") {
        for key in ["content", "text", "summary"] {
            if let Some(text) = reasoning.get(key).and_then(|v| v.as_str()) {
                if !text.is_empty() {
                    return Some(text.to_string());
                }
            }
        }
    }

    if let Some(details) = value.get("reasoning_details") {
        if let Some(text) = extract_reasoning_details_text(details) {
            return Some(text);
        }
    }

    None
}

fn extract_reasoning_details_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => (!text.is_empty()).then(|| text.to_string()),
        Value::Array(parts) => {
            let text = parts
                .iter()
                .filter_map(extract_reasoning_detail_part_text)
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n");
            (!text.is_empty()).then_some(text)
        }
        Value::Object(_) => extract_reasoning_detail_part_text(value),
        _ => None,
    }
}

fn extract_reasoning_detail_part_text(value: &Value) -> Option<String> {
    for key in ["text", "content", "summary"] {
        if let Some(text) = value.get(key).and_then(|v| v.as_str()) {
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }

    if let Some(parts) = value.get("parts").and_then(|v| v.as_array()) {
        let text = parts
            .iter()
            .filter_map(extract_reasoning_detail_part_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        return (!text.is_empty()).then_some(text);
    }

    None
}

pub(crate) fn extract_reasoning_summary_text(value: &Value) -> Option<String> {
    for key in ["reasoning_content", "content", "text"] {
        if let Some(text) = value.get(key).and_then(|v| v.as_str()) {
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }

    let summary = value.get("summary")?;
    if let Some(text) = summary.as_str() {
        return (!text.is_empty()).then(|| text.to_string());
    }

    let parts = summary.as_array()?;
    let text = parts
        .iter()
        .filter_map(|part| {
            part.get("text")
                .and_then(|v| v.as_str())
                .or_else(|| part.get("content").and_then(|v| v.as_str()))
                .or_else(|| part.as_str())
        })
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");

    (!text.is_empty()).then_some(text)
}

pub(crate) fn append_reasoning_content(message: &mut Map<String, Value>, reasoning: &str) -> bool {
    let reasoning = reasoning.trim();
    if reasoning.is_empty() {
        return false;
    }

    match message.get_mut("reasoning_content") {
        Some(Value::String(existing)) if !existing.is_empty() => {
            existing.push_str("\n\n");
            existing.push_str(reasoning);
        }
        _ => {
            message.insert(
                "reasoning_content".to_string(),
                Value::String(reasoning.to_string()),
            );
        }
    }
    true
}

pub(crate) fn attach_reasoning_content_field(item: &mut Value, reasoning: &str) -> bool {
    let reasoning = reasoning.trim();
    if reasoning.is_empty() {
        return false;
    }

    if let Some(obj) = item.as_object_mut() {
        obj.insert(
            "reasoning_content".to_string(),
            Value::String(reasoning.to_string()),
        );
        return true;
    }

    false
}

pub(crate) fn attach_optional_reasoning_content_field(
    item: &mut Value,
    reasoning: Option<&str>,
) -> bool {
    let Some(reasoning) = reasoning else {
        return false;
    };
    attach_reasoning_content_field(item, reasoning)
}

pub(crate) fn response_function_call_item(
    item_id: &str,
    status: &str,
    call_id: &str,
    name: &str,
    arguments: &str,
    reasoning: Option<&str>,
) -> Value {
    let mut item = json!({
        "id": item_id,
        "type": "function_call",
        "status": status,
        "call_id": call_id,
        "name": name,
        "arguments": arguments
    });
    attach_optional_reasoning_content_field(&mut item, reasoning);
    item
}

pub(crate) fn response_function_call_item_with_namespace(
    item_id: &str,
    status: &str,
    call_id: &str,
    name: &str,
    namespace: Option<&str>,
    arguments: &str,
    reasoning: Option<&str>,
) -> Value {
    let mut item =
        response_function_call_item(item_id, status, call_id, name, arguments, reasoning);
    if let Some(namespace) = namespace.filter(|value| !value.is_empty()) {
        if let Some(obj) = item.as_object_mut() {
            obj.insert("namespace".to_string(), json!(namespace));
        }
    }
    item
}

pub(crate) fn response_item_call_id(item: &Value) -> Option<String> {
    item.get("call_id")
        .or_else(|| item.get("id"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub(crate) fn is_empty_value(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(value) => value.trim().is_empty(),
        Value::Array(value) => value.is_empty(),
        Value::Object(value) => value.is_empty(),
        _ => false,
    }
}

pub(crate) fn split_leading_think_block(text: &str) -> Option<(String, String)> {
    let leading_ws_len = text.len() - text.trim_start().len();
    let after_ws = &text[leading_ws_len..];
    if !after_ws.starts_with(THINK_OPEN_TAG) {
        return None;
    }

    let body_start = leading_ws_len + THINK_OPEN_TAG.len();
    let close_relative = text[body_start..].find(THINK_CLOSE_TAG)?;
    let close_start = body_start + close_relative;
    let answer_start = close_start + THINK_CLOSE_TAG.len();

    Some((
        text[body_start..close_start].trim().to_string(),
        strip_think_answer_separator(&text[answer_start..]).to_string(),
    ))
}

pub(crate) fn strip_leading_think_open_tag(text: &str) -> Option<String> {
    let leading_ws_len = text.len() - text.trim_start().len();
    let after_ws = &text[leading_ws_len..];
    after_ws
        .strip_prefix(THINK_OPEN_TAG)
        .map(|value| value.trim().to_string())
}

fn strip_think_answer_separator(text: &str) -> &str {
    text.trim_start_matches(['\r', '\n', '\t', ' '])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasoning_field_prefers_reasoning_content_string() {
        let value = json!({
            "reasoning_content": "from content",
            "reasoning": "from reasoning",
            "reasoning_details": [{ "text": "from details" }]
        });
        assert_eq!(
            extract_reasoning_field_text(&value).as_deref(),
            Some("from content")
        );

        let value = json!({ "reasoning_content": "", "reasoning": "from reasoning" });
        assert_eq!(
            extract_reasoning_field_text(&value).as_deref(),
            Some("from reasoning")
        );
    }

    #[test]
    fn reasoning_field_reads_reasoning_object_keys_in_order() {
        let value = json!({ "reasoning": { "text": "t", "summary": "s" } });
        assert_eq!(extract_reasoning_field_text(&value).as_deref(), Some("t"));

        let value = json!({ "reasoning": { "content": "", "summary": "s" } });
        assert_eq!(extract_reasoning_field_text(&value).as_deref(), Some("s"));
    }

    #[test]
    fn reasoning_field_joins_reasoning_details_parts() {
        let value = json!({
            "reasoning_details": [
                { "type": "reasoning.text", "text": "first" },
                { "type": "reasoning.summary", "summary": "second" },
                { "parts": [{ "text": "nested-a" }, { "content": "nested-b" }] },
                { "type": "reasoning.encrypted", "data": "opaque" },
                { "text": "" }
            ]
        });
        assert_eq!(
            extract_reasoning_field_text(&value).as_deref(),
            Some("first\n\nsecond\n\nnested-a\n\nnested-b")
        );

        let value = json!({ "reasoning_details": "plain" });
        assert_eq!(
            extract_reasoning_field_text(&value).as_deref(),
            Some("plain")
        );

        let value = json!({ "reasoning_details": { "content": "single" } });
        assert_eq!(
            extract_reasoning_field_text(&value).as_deref(),
            Some("single")
        );
    }

    #[test]
    fn reasoning_field_returns_none_when_absent_or_empty() {
        assert_eq!(extract_reasoning_field_text(&json!({})), None);
        assert_eq!(
            extract_reasoning_field_text(&json!({
                "reasoning_content": "",
                "reasoning": { "text": "" },
                "reasoning_details": [{ "data": "opaque" }]
            })),
            None
        );
        assert_eq!(
            extract_reasoning_field_text(&json!({ "reasoning_details": 42 })),
            None
        );
    }

    #[test]
    fn reasoning_summary_prefers_direct_text_fields() {
        let value = json!({ "reasoning_content": "rc", "content": "c", "text": "t" });
        assert_eq!(
            extract_reasoning_summary_text(&value).as_deref(),
            Some("rc")
        );

        let value = json!({ "content": "", "text": "t", "summary": "s" });
        assert_eq!(extract_reasoning_summary_text(&value).as_deref(), Some("t"));
    }

    #[test]
    fn reasoning_summary_handles_string_and_array_summary() {
        let value = json!({ "type": "reasoning", "summary": "short" });
        assert_eq!(
            extract_reasoning_summary_text(&value).as_deref(),
            Some("short")
        );

        let value = json!({
            "type": "reasoning",
            "summary": [
                { "type": "summary_text", "text": "a" },
                { "content": "b" },
                "c",
                { "text": "" },
                { "other": 1 }
            ]
        });
        assert_eq!(
            extract_reasoning_summary_text(&value).as_deref(),
            Some("a\n\nb\n\nc")
        );
    }

    #[test]
    fn reasoning_summary_returns_none_when_empty() {
        assert_eq!(extract_reasoning_summary_text(&json!({})), None);
        assert_eq!(
            extract_reasoning_summary_text(&json!({ "summary": "" })),
            None
        );
        assert_eq!(
            extract_reasoning_summary_text(&json!({ "summary": [] })),
            None
        );
        assert_eq!(
            extract_reasoning_summary_text(&json!({ "summary": { "text": "x" } })),
            None
        );
    }

    #[test]
    fn append_reasoning_content_inserts_and_appends_trimmed_text() {
        let mut message = Map::new();
        assert!(!append_reasoning_content(&mut message, "   "));
        assert!(!message.contains_key("reasoning_content"));

        assert!(append_reasoning_content(&mut message, "  first \n"));
        assert_eq!(message["reasoning_content"], json!("first"));

        assert!(append_reasoning_content(&mut message, "second"));
        assert_eq!(message["reasoning_content"], json!("first\n\nsecond"));
    }

    #[test]
    fn append_reasoning_content_replaces_empty_or_non_string_existing() {
        let mut message = Map::new();
        message.insert("reasoning_content".to_string(), json!(""));
        assert!(append_reasoning_content(&mut message, "new"));
        assert_eq!(message["reasoning_content"], json!("new"));

        let mut message = Map::new();
        message.insert("reasoning_content".to_string(), json!(null));
        assert!(append_reasoning_content(&mut message, "new"));
        assert_eq!(message["reasoning_content"], json!("new"));
    }

    #[test]
    fn attach_reasoning_content_field_only_for_objects_and_non_empty_text() {
        let mut item = json!({ "type": "function_call", "reasoning_content": "old" });
        assert!(attach_reasoning_content_field(&mut item, " thinking "));
        assert_eq!(item["reasoning_content"], json!("thinking"));

        let mut item = json!({ "type": "function_call" });
        assert!(!attach_reasoning_content_field(&mut item, "\n\t"));
        assert!(item.get("reasoning_content").is_none());

        let mut not_object = json!(["x"]);
        assert!(!attach_reasoning_content_field(&mut not_object, "thinking"));
        assert_eq!(not_object, json!(["x"]));

        let mut item = json!({});
        assert!(!attach_optional_reasoning_content_field(&mut item, None));
        assert!(attach_optional_reasoning_content_field(
            &mut item,
            Some("r")
        ));
        assert_eq!(item["reasoning_content"], json!("r"));
    }

    #[test]
    fn response_function_call_item_builds_expected_shape() {
        let item = response_function_call_item(
            "fc_1",
            "completed",
            "call_1",
            "get_weather",
            r#"{"city":"Paris"}"#,
            None,
        );
        assert_eq!(
            item,
            json!({
                "id": "fc_1",
                "type": "function_call",
                "status": "completed",
                "call_id": "call_1",
                "name": "get_weather",
                "arguments": "{\"city\":\"Paris\"}"
            })
        );

        let item = response_function_call_item(
            "fc_1",
            "in_progress",
            "call_1",
            "get_weather",
            "",
            Some(" because "),
        );
        assert_eq!(item["status"], json!("in_progress"));
        assert_eq!(item["reasoning_content"], json!("because"));

        let item =
            response_function_call_item("fc_1", "completed", "call_1", "f", "{}", Some("  "));
        assert!(item.get("reasoning_content").is_none());
    }

    #[test]
    fn response_function_call_item_with_namespace_skips_empty_namespace() {
        let item = response_function_call_item_with_namespace(
            "fc_1",
            "completed",
            "call_1",
            "search",
            Some("mcp__docs"),
            "{}",
            Some("r"),
        );
        assert_eq!(item["namespace"], json!("mcp__docs"));
        assert_eq!(item["name"], json!("search"));
        assert_eq!(item["reasoning_content"], json!("r"));

        for namespace in [None, Some("")] {
            let item = response_function_call_item_with_namespace(
                "fc_1",
                "completed",
                "call_1",
                "search",
                namespace,
                "{}",
                None,
            );
            assert!(item.get("namespace").is_none());
        }
    }

    #[test]
    fn response_item_call_id_prefers_call_id_and_trims() {
        assert_eq!(
            response_item_call_id(&json!({ "call_id": " call_1 ", "id": "fc_1" })).as_deref(),
            Some("call_1")
        );
        assert_eq!(
            response_item_call_id(&json!({ "id": "fc_1" })).as_deref(),
            Some("fc_1")
        );
        // call_id 存在但为空白时不回退到 id（与 or_else 语义一致）
        assert_eq!(
            response_item_call_id(&json!({ "call_id": "  ", "id": "fc_1" })),
            None
        );
        assert_eq!(response_item_call_id(&json!({ "call_id": 1 })), None);
        assert_eq!(response_item_call_id(&json!({})), None);
    }

    #[test]
    fn is_empty_value_covers_each_json_kind() {
        assert!(is_empty_value(&json!(null)));
        assert!(is_empty_value(&json!("  ")));
        assert!(is_empty_value(&json!([])));
        assert!(is_empty_value(&json!({})));
        assert!(!is_empty_value(&json!("x")));
        assert!(!is_empty_value(&json!([0])));
        assert!(!is_empty_value(&json!({ "a": 1 })));
        assert!(!is_empty_value(&json!(0)));
        assert!(!is_empty_value(&json!(false)));
    }

    #[test]
    fn split_leading_think_block_separates_reasoning_and_answer() {
        assert_eq!(
            split_leading_think_block("\n <think> plan it </think>\n\nAnswer"),
            Some(("plan it".to_string(), "Answer".to_string()))
        );
        assert_eq!(split_leading_think_block("Answer <think>x</think>"), None);
        assert_eq!(split_leading_think_block("<think>unterminated"), None);
    }

    #[test]
    fn strip_leading_think_open_tag_returns_trimmed_body() {
        assert_eq!(
            strip_leading_think_open_tag("  <think> partial "),
            Some("partial".to_string())
        );
        assert_eq!(strip_leading_think_open_tag("no tag"), None);
    }
}
