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
    fn split_think_block_extracts_reasoning_and_answer() {
        assert_eq!(
            split_leading_think_block("<think>\n plan \n</think>\n\nanswer"),
            Some(("plan".to_string(), "answer".to_string()))
        );
    }

    #[test]
    fn split_think_block_allows_leading_whitespace_and_unicode() {
        assert_eq!(
            split_leading_think_block(" \r\n<think>思考🤔</think>答案：好"),
            Some(("思考🤔".to_string(), "答案：好".to_string()))
        );
    }

    #[test]
    fn split_think_block_requires_leading_open_and_a_close_tag() {
        assert_eq!(split_leading_think_block("answer <think>x</think>"), None);
        assert_eq!(split_leading_think_block("<think>still thinking"), None);
        assert_eq!(split_leading_think_block(""), None);
        assert_eq!(split_leading_think_block("<THINK>x</THINK>"), None);
    }

    #[test]
    fn split_think_block_handles_empty_body_and_answer() {
        assert_eq!(
            split_leading_think_block("<think></think>"),
            Some((String::new(), String::new()))
        );
        // Only the first close tag terminates the reasoning
        assert_eq!(
            split_leading_think_block("<think>a</think>b</think>c"),
            Some(("a".to_string(), "b</think>c".to_string()))
        );
    }

    #[test]
    fn strip_leading_open_tag_only_at_start() {
        assert_eq!(
            strip_leading_think_open_tag("\n<think> partial "),
            Some("partial".to_string())
        );
        assert_eq!(strip_leading_think_open_tag("x<think>"), None);
    }

    #[test]
    fn reasoning_field_priority_and_shapes() {
        assert_eq!(
            extract_reasoning_field_text(&json!({"reasoning_content": "rc", "reasoning": "r"})),
            Some("rc".to_string())
        );
        // Empty reasoning_content falls through to reasoning
        assert_eq!(
            extract_reasoning_field_text(&json!({"reasoning_content": "", "reasoning": "r"})),
            Some("r".to_string())
        );
        assert_eq!(
            extract_reasoning_field_text(&json!({"reasoning": {"summary": "s"}})),
            Some("s".to_string())
        );
        assert_eq!(
            extract_reasoning_field_text(&json!({
                "reasoning_details": [
                    {"type": "reasoning.text", "text": "one"},
                    {"type": "reasoning.encrypted", "data": "xx"},
                    {"parts": [{"text": "two"}, {"content": "three"}]}
                ]
            })),
            Some("one\n\ntwo\n\nthree".to_string())
        );
        assert_eq!(extract_reasoning_field_text(&json!({"content": "x"})), None);
        assert_eq!(
            extract_reasoning_field_text(&json!({"reasoning_details": []})),
            None
        );
    }

    #[test]
    fn reasoning_summary_text_joins_parts() {
        assert_eq!(
            extract_reasoning_summary_text(&json!({
                "summary": [{"type": "summary_text", "text": "a"}, "b", {"text": ""}]
            })),
            Some("a\n\nb".to_string())
        );
        assert_eq!(
            extract_reasoning_summary_text(&json!({"summary": ""})),
            None
        );
        assert_eq!(extract_reasoning_summary_text(&json!({})), None);
    }

    #[test]
    fn append_reasoning_content_concatenates_and_ignores_blank() {
        let mut msg = Map::new();
        assert!(!append_reasoning_content(&mut msg, "  \n"));
        assert!(msg.is_empty());
        assert!(append_reasoning_content(&mut msg, " first "));
        assert!(append_reasoning_content(&mut msg, "second"));
        assert_eq!(msg["reasoning_content"], json!("first\n\nsecond"));

        // A non-string existing value is replaced rather than corrupted
        let mut msg = Map::new();
        msg.insert("reasoning_content".into(), json!(null));
        assert!(append_reasoning_content(&mut msg, "x"));
        assert_eq!(msg["reasoning_content"], json!("x"));
    }

    #[test]
    fn function_call_item_attaches_namespace_and_reasoning_when_present() {
        let item = response_function_call_item_with_namespace(
            "fc_1",
            "completed",
            "call_1",
            "run",
            Some("mcp__fs"),
            "{}",
            Some(" why "),
        );
        assert_eq!(item["namespace"], json!("mcp__fs"));
        assert_eq!(item["reasoning_content"], json!("why"));

        let bare = response_function_call_item_with_namespace(
            "fc_1",
            "completed",
            "call_1",
            "run",
            Some(""),
            "{}",
            Some("  "),
        );
        assert!(bare.get("namespace").is_none());
        assert!(bare.get("reasoning_content").is_none());
    }

    #[test]
    fn call_id_prefers_call_id_then_id_and_trims() {
        assert_eq!(
            response_item_call_id(&json!({"call_id": " c1 ", "id": "i1"})),
            Some("c1".to_string())
        );
        assert_eq!(
            response_item_call_id(&json!({"id": "i1"})),
            Some("i1".to_string())
        );
        assert_eq!(response_item_call_id(&json!({"call_id": "  "})), None);
    }

    #[test]
    fn is_empty_value_semantics() {
        assert!(is_empty_value(&json!(null)));
        assert!(is_empty_value(&json!(" ")));
        assert!(is_empty_value(&json!([])));
        assert!(is_empty_value(&json!({})));
        assert!(!is_empty_value(&json!(0)));
        assert!(!is_empty_value(&json!(false)));
        assert!(!is_empty_value(&json!("x")));
    }
}
