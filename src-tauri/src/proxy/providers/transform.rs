//! 格式转换模块
//!
//! 实现 Anthropic ↔ OpenAI 格式转换，用于 OpenRouter 支持
//! 参考: anthropic-proxy-rs

use crate::proxy::{error::ProxyError, json_canonical::canonical_json_string};
use serde_json::{json, Value};

const ANTHROPIC_BILLING_HEADER_PREFIX: &str = "x-anthropic-billing-header:";

/// Strip only a leading Claude Code attribution line from system text.
///
/// Claude Code can send dynamic `x-anthropic-billing-header` metadata at the
/// start of `system`. If forwarded into OpenAI Chat messages or Responses
/// `instructions`, the rotating `cch=` value changes the prompt prefix on every
/// request and prevents prefix cache reuse (#2350). Later occurrences are kept
/// to avoid deleting user-authored prompt text.
pub(crate) fn strip_leading_anthropic_billing_header(text: &str) -> &str {
    if !text.starts_with(ANTHROPIC_BILLING_HEADER_PREFIX) {
        return text;
    }

    let Some(line_end) = text
        .as_bytes()
        .iter()
        .position(|byte| *byte == b'\n' || *byte == b'\r')
    else {
        return "";
    };

    let bytes = text.as_bytes();
    let mut rest_start = line_end + 1;
    if bytes[line_end] == b'\r' && bytes.get(line_end + 1) == Some(&b'\n') {
        rest_start += 1;
    }

    let rest = &text[rest_start..];
    if let Some(stripped) = rest.strip_prefix("\r\n") {
        stripped
    } else if let Some(stripped) = rest.strip_prefix('\n') {
        stripped
    } else if let Some(stripped) = rest.strip_prefix('\r') {
        stripped
    } else {
        rest
    }
}

/// Detect OpenAI o-series reasoning models (o1, o3, o4-mini, etc.)
/// These models require `max_completion_tokens` instead of `max_tokens`.
pub fn is_openai_o_series(model: &str) -> bool {
    model.len() > 1
        && model.starts_with('o')
        && model.as_bytes().get(1).is_some_and(|b| b.is_ascii_digit())
}

/// Detect OpenAI models that support reasoning_effort.
///
/// Supported families:
/// - o-series: o1, o3, o4-mini, etc.
/// - GPT-5+: gpt-5, gpt-5.1, gpt-5.4, gpt-5-codex, etc.
pub fn supports_reasoning_effort(model: &str) -> bool {
    is_openai_o_series(model)
        || model
            .to_lowercase()
            .strip_prefix("gpt-")
            .and_then(|rest| rest.chars().next())
            .is_some_and(|c| c.is_ascii_digit() && c >= '5')
}

/// Detect OpenAI reasoning families (o-series and GPT-5+) that, through Chat
/// Completions, reject `max_tokens` in favour of `max_completion_tokens`.
/// A vendor prefix such as `openai/gpt-5` (OpenRouter style) is ignored.
pub fn is_openai_reasoning_model(model: &str) -> bool {
    let name = model
        .rsplit('/')
        .next()
        .unwrap_or(model)
        .to_ascii_lowercase();
    supports_reasoning_effort(&name)
}

/// OpenAI reasoning models only accept the default sampling parameters and
/// reject a non-default `temperature`/`top_p` with a 400. The `*-chat*`
/// variants (e.g. `gpt-5-chat-latest`) are non-reasoning and accept them.
fn rejects_custom_sampling(model: &str) -> bool {
    is_openai_reasoning_model(model) && !model.to_ascii_lowercase().contains("-chat")
}

/// Write the Chat Completions token limit and sampling parameters for `model`:
/// reasoning models get `max_completion_tokens` and no `temperature`/`top_p`;
/// other models keep `max_tokens` and the sampling values as given.
pub(crate) fn apply_chat_token_limit_and_sampling(
    result: &mut Value,
    model: &str,
    max_tokens: Option<&Value>,
    temperature: Option<&Value>,
    top_p: Option<&Value>,
) {
    if let Some(v) = max_tokens {
        if is_openai_reasoning_model(model) {
            result["max_completion_tokens"] = v.clone();
        } else {
            result["max_tokens"] = v.clone();
        }
    }
    let strip_sampling = rejects_custom_sampling(model);
    for (key, value) in [("temperature", temperature), ("top_p", top_p)] {
        let Some(value) = value else {
            continue;
        };
        if strip_sampling {
            log::debug!("[Transform] dropping `{key}` for OpenAI reasoning model {model}");
        } else {
            result[key] = value.clone();
        }
    }
}

/// Resolve the appropriate OpenAI `reasoning_effort` from an Anthropic request body.
///
/// Priority:
/// 1. Explicit `output_config.effort` — preserves the user's intent directly.
///    `low`/`medium`/`high` map 1:1; `max` maps to `xhigh`
///    (supported by mainstream GPT models). Unknown values are ignored.
/// 2. Fallback: `thinking.type` + `budget_tokens`:
///    - `adaptive` → `xhigh` (adaptive = maximum reasoning effort)
///    - `enabled` with budget → `low` (<4 000) / `medium` (4 000–15 999) / `high` (≥16 000)
///    - `enabled` without budget → `high` (conservative default)
///    - `disabled` / absent → `None`
pub fn resolve_reasoning_effort(body: &Value) -> Option<&'static str> {
    // --- Priority 1: explicit output_config.effort ---
    if let Some(effort) = body
        .pointer("/output_config/effort")
        .and_then(|v| v.as_str())
    {
        return match effort {
            "low" => Some("low"),
            "medium" => Some("medium"),
            "high" => Some("high"),
            "max" => Some("xhigh"), // OpenAI xhigh = maximum reasoning effort
            _ => None,              // unknown value — do not inject
        };
    }

    // --- Priority 2: thinking.type + budget_tokens fallback ---
    let thinking = body.get("thinking")?;
    match thinking.get("type").and_then(|t| t.as_str()) {
        Some("adaptive") => Some("xhigh"),
        Some("enabled") => {
            let budget = thinking.get("budget_tokens").and_then(|b| b.as_u64());
            match budget {
                Some(b) if b < 4_000 => Some("low"),
                Some(b) if b < 16_000 => Some("medium"),
                Some(_) => Some("high"),
                None => Some("high"), // enabled but no budget — assume strong reasoning
            }
        }
        _ => None, // disabled or missing
    }
}

/// Anthropic 请求 → OpenAI Chat Completions 请求
///
/// 转换工具库 API：当前无生产调用方（连通性检查不再发真实请求，曾是其唯一 crate 内
/// 消费者），但保留其转换逻辑与下方测试套件，供代理转换路径复用 / 未来接线。
#[allow(dead_code)]
pub fn anthropic_to_openai(body: Value) -> Result<Value, ProxyError> {
    anthropic_to_openai_with_reasoning_content(body, false)
}

/// Anthropic 请求 → OpenAI Chat Completions 请求
///
/// `preserve_reasoning_content` 仅用于明确需要 Moonshot/Kimi/DeepSeek
/// `reasoning_content` 兼容字段的 provider。默认转换保持通用 OpenAI-compatible
/// 请求体，避免向严格后端发送未知字段。
pub fn anthropic_to_openai_with_reasoning_content(
    body: Value,
    preserve_reasoning_content: bool,
) -> Result<Value, ProxyError> {
    let mut result = json!({});

    // NOTE: 模型映射由上游统一处理（proxy::model_mapper），格式转换层只做结构转换。
    if let Some(model) = body.get("model").and_then(|m| m.as_str()) {
        result["model"] = json!(model);
    }

    let mut messages = Vec::new();

    // 处理 system prompt
    if let Some(system) = body.get("system") {
        if let Some(text) = system.as_str() {
            let text = strip_leading_anthropic_billing_header(text);
            if !text.is_empty() {
                messages.push(json!({"role": "system", "content": text}));
            }
        } else if let Some(arr) = system.as_array() {
            for msg in arr {
                if let Some(text) = msg.get("text").and_then(|t| t.as_str()) {
                    let text = strip_leading_anthropic_billing_header(text);
                    if text.is_empty() {
                        continue;
                    }
                    messages.push(json!({"role": "system", "content": text}));
                }
            }
        }
    }

    // 转换 messages
    if let Some(msgs) = body.get("messages").and_then(|m| m.as_array()) {
        for msg in msgs {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
            let content = msg.get("content");
            let converted = convert_message_to_openai(role, content, preserve_reasoning_content)?;
            messages.extend(converted);
        }
    }

    normalize_openai_system_messages(&mut messages);
    result["messages"] = json!(messages);

    // 转换参数 — o-series / gpt-5+ 推理模型需要 max_completion_tokens，且不接受自定义采样参数
    let model = body.get("model").and_then(|m| m.as_str()).unwrap_or("");
    apply_chat_token_limit_and_sampling(
        &mut result,
        model,
        body.get("max_tokens"),
        body.get("temperature"),
        body.get("top_p"),
    );
    if let Some(v) = body.get("stop_sequences") {
        result["stop"] = v.clone();
    }
    if let Some(v) = body.get("stream") {
        result["stream"] = v.clone();
    }

    // Map Anthropic thinking → OpenAI reasoning_effort
    if supports_reasoning_effort(model) {
        if let Some(effort) = resolve_reasoning_effort(&body) {
            result["reasoning_effort"] = json!(effort);
        }
    }

    // 转换 tools (过滤 BatchTool)
    if let Some(tools) = body.get("tools").and_then(|t| t.as_array()) {
        let openai_tools: Vec<Value> = tools
            .iter()
            .filter(|t| t.get("type").and_then(|v| v.as_str()) != Some("BatchTool"))
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.get("name").and_then(|n| n.as_str()).unwrap_or(""),
                        "description": t.get("description"),
                        "parameters": clean_schema(t.get("input_schema").cloned().unwrap_or(json!({})))
                    }
                })
            })
            .collect();

        if !openai_tools.is_empty() {
            result["tools"] = json!(openai_tools);
        }
    }

    if let Some(v) = body.get("tool_choice") {
        result["tool_choice"] = map_tool_choice_to_chat(v);
        // PROV-26: honour Anthropic's parallelism hint (only alongside tools:
        // strict upstreams reject parallel_tool_calls without a tools array).
        if tool_choice_disables_parallel(v) && result.get("tools").is_some() {
            result["parallel_tool_calls"] = json!(false);
        }
    }

    Ok(result)
}

/// Whether an Anthropic `tool_choice` carries `disable_parallel_tool_use: true`.
pub(crate) fn tool_choice_disables_parallel(tool_choice: &Value) -> bool {
    tool_choice
        .get("disable_parallel_tool_use")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// 为 OpenAI Chat Completions 流式请求注入 `stream_options.include_usage`。
///
/// OpenAI 兼容上游在流式下默认不在 SSE 里返回 usage，必须显式声明 include_usage
/// 才会在末尾吐 usage chunk。缺这一注入会导致流式请求的 token/成本/缓存全部漏记
/// （input/output/cache 全为 0）。保留客户端可能透传的其它 stream_options 字段，
/// 仅补 include_usage；非流式请求不动。
///
/// 由 Claude→openai_chat（claude.rs）与 Codex Responses→Chat（transform_codex_chat.rs）
/// 两条转换路径共用，确保两个客户端方向行为一致。
pub(crate) fn inject_openai_stream_include_usage(result: &mut Value) {
    let is_stream = result
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !is_stream {
        return;
    }
    match result.get_mut("stream_options") {
        Some(Value::Object(opts)) => {
            opts.insert("include_usage".to_string(), json!(true));
        }
        _ => {
            result["stream_options"] = json!({ "include_usage": true });
        }
    }
}

/// Translate an Anthropic `tool_choice` into the OpenAI Chat Completions form.
///
/// Anthropic forms:
///   "auto" / "any" / "none"           (string enum)
///   {"type": "auto" | "any" | "none"}
///   {"type": "tool", "name": "<X>"}
///
/// OpenAI Chat forms:
///   "auto" / "none" / "required"      (note: no "any" — use "required")
///   {"type": "function", "function": {"name": "<X>"}}
///
/// The Responses API uses a flatter `{"type":"function","name":"X"}` selector,
/// so it has a sibling `map_tool_choice_to_responses` in `transform_responses.rs`.
/// Keep the two in sync.
pub(crate) fn map_tool_choice_to_chat(tool_choice: &Value) -> Value {
    match tool_choice {
        Value::String(s) => match s.as_str() {
            "any" => json!("required"),
            _ => json!(s),
        },
        Value::Object(obj) => match obj.get("type").and_then(|t| t.as_str()) {
            Some("any") => json!("required"),
            Some("auto") => json!("auto"),
            Some("none") => json!("none"),
            Some("tool") => {
                let name = obj.get("name").and_then(|n| n.as_str()).unwrap_or("");
                json!({
                    "type": "function",
                    "function": { "name": name }
                })
            }
            _ => tool_choice.clone(),
        },
        _ => tool_choice.clone(),
    }
}

fn normalize_openai_system_messages(messages: &mut Vec<Value>) {
    let system_count = messages
        .iter()
        .filter(|message| message.get("role").and_then(|value| value.as_str()) == Some("system"))
        .count();

    if system_count == 0 {
        return;
    }

    if system_count == 1 {
        if let Some(index) = messages.iter().position(|message| {
            message.get("role").and_then(|value| value.as_str()) == Some("system")
        }) {
            if index > 0 {
                let message = messages.remove(index);
                messages.insert(0, message);
            }
        }
        return;
    }

    let mut parts = Vec::new();
    messages.retain(|message| {
        if message.get("role").and_then(|value| value.as_str()) != Some("system") {
            return true;
        }

        match message.get("content") {
            Some(Value::String(text)) if !text.is_empty() => parts.push(text.clone()),
            Some(Value::Array(content_parts)) => {
                let text = content_parts
                    .iter()
                    .filter_map(|part| part.get("text").and_then(|value| value.as_str()))
                    .collect::<Vec<_>>()
                    .join("\n");
                if !text.is_empty() {
                    parts.push(text);
                }
            }
            _ => {}
        }

        false
    });

    if !parts.is_empty() {
        messages.insert(0, json!({"role": "system", "content": parts.join("\n")}));
    }
}

/// 转换单条消息到 OpenAI 格式（可能产生多条消息）
fn convert_message_to_openai(
    role: &str,
    content: Option<&Value>,
    preserve_reasoning_content: bool,
) -> Result<Vec<Value>, ProxyError> {
    let mut result = Vec::new();

    let content = match content {
        Some(c) => c,
        None => {
            result.push(json!({"role": role, "content": null}));
            return Ok(result);
        }
    };

    // 字符串内容
    if let Some(text) = content.as_str() {
        result.push(json!({"role": role, "content": text}));
        return Ok(result);
    }

    // 数组内容（多模态/工具调用）
    if let Some(blocks) = content.as_array() {
        let mut content_parts = Vec::new();
        let mut tool_calls = Vec::new();
        // reasoning_parts: 仅在兼容 Moonshot/Kimi/DeepSeek thinking tool-call 路径时
        // 生成 reasoning_content，通用 OpenAI-compatible 路径不发送该非标准字段。
        let mut reasoning_parts = Vec::new();

        for block in blocks {
            let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");

            match block_type {
                "text" => {
                    if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                        content_parts.push(json!({"type": "text", "text": text}));
                    }
                }
                "image" => {
                    if let Some(part) = anthropic_image_to_chat_part(block) {
                        content_parts.push(part);
                    }
                }
                "document" => {
                    if let Some(part) = anthropic_document_to_chat_part(block) {
                        content_parts.push(part);
                    }
                }
                "tool_use" => {
                    let id = block.get("id").and_then(|i| i.as_str()).unwrap_or("");
                    let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    let input = block.get("input").cloned().unwrap_or(json!({}));
                    tool_calls.push(json!({
                        "id": id,
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": canonical_json_string(&input)
                        }
                    }));
                }
                "tool_result" => {
                    // tool_result 变成单独的 tool role 消息
                    let tool_use_id = block
                        .get("tool_use_id")
                        .and_then(|i| i.as_str())
                        .unwrap_or("");
                    let (content_str, media_parts) = tool_result_to_chat_content(block);
                    result.push(json!({
                        "role": "tool",
                        "tool_call_id": tool_use_id,
                        "content": content_str
                    }));
                    // Chat `tool` messages are text-only; images / documents
                    // returned by the tool ride along in the following user
                    // message instead of being serialized as base64 JSON text.
                    content_parts.extend(media_parts);
                }
                "thinking" => {
                    // 提取 thinking 内容，后续可作为 reasoning_content 传给需要它的上游。
                    if let Some(thinking) = block.get("thinking").and_then(|t| t.as_str()) {
                        if !thinking.is_empty() {
                            reasoning_parts.push(thinking.to_string());
                        }
                    }
                }
                "redacted_thinking" if preserve_reasoning_content => {
                    // Claude Code encrypts historical thinking into redacted_thinking blocks.
                    // MiMo/DeepSeek require non-empty reasoning_content on assistant tool-call
                    // messages, so inject a minimal placeholder when the real content is
                    // unavailable. Skip when preserve_reasoning_content is off (generic
                    // OpenAI-compatible path).
                    reasoning_parts.push("[redacted thinking]".to_string());
                }
                _ => {}
            }
        }

        // 添加带内容和/或工具调用的消息
        if !content_parts.is_empty() || !tool_calls.is_empty() {
            let mut msg = json!({"role": role});

            // 内容处理
            if content_parts.is_empty() {
                msg["content"] = Value::Null;
            } else if content_parts.len() == 1 {
                // 单 text block 简化为纯字符串
                if let Some(text) = content_parts[0].get("text") {
                    msg["content"] = text.clone();
                } else {
                    msg["content"] = json!(content_parts);
                }
            } else {
                msg["content"] = json!(content_parts);
            }

            // 工具调用
            if !tool_calls.is_empty() {
                msg["tool_calls"] = json!(tool_calls);
            }

            if preserve_reasoning_content && role == "assistant" {
                if !tool_calls.is_empty() {
                    let reasoning_content = if reasoning_parts.is_empty() {
                        "tool call".to_string()
                    } else {
                        reasoning_parts.join("\n")
                    };
                    msg["reasoning_content"] = json!(reasoning_content);
                } else if !reasoning_parts.is_empty() {
                    // PROV-32: Kimi/DeepSeek-style providers expect the reasoning
                    // history on every assistant turn, not only tool-call turns
                    // (mirrors the #5508 forward-attach on the Codex→Chat path).
                    msg["reasoning_content"] = json!(reasoning_parts.join("\n"));
                }
            }

            result.push(msg);
        }

        return Ok(result);
    }

    // 其他情况直接透传
    result.push(json!({"role": role, "content": content}));
    Ok(result)
}

/// Anthropic `image` block → Chat `image_url` part (PROV-15): base64 sources
/// become data URLs, `url` sources keep their http(s) URL instead of producing
/// an empty `data:` URL.
fn anthropic_image_to_chat_part(block: &Value) -> Option<Value> {
    let source = block.get("source")?;
    let url = match source.get("type").and_then(|t| t.as_str()) {
        Some("url") => source.get("url").and_then(|u| u.as_str())?.to_string(),
        _ => {
            let media_type = source
                .get("media_type")
                .and_then(|m| m.as_str())
                .unwrap_or("image/png");
            let data = source.get("data").and_then(|d| d.as_str()).unwrap_or("");
            if data.is_empty() {
                log::warn!("[Claude/OpenAI] dropping image block without data");
                return None;
            }
            format!("data:{media_type};base64,{data}")
        }
    };
    Some(json!({"type": "image_url", "image_url": {"url": url}}))
}

/// Anthropic `document` block → Chat content part (PROV-15): plain-text
/// documents become text, base64 PDFs become an OpenAI `file` part, anything
/// else is reported instead of silently dropped.
fn anthropic_document_to_chat_part(block: &Value) -> Option<Value> {
    let source = block.get("source")?;
    match source.get("type").and_then(|t| t.as_str()) {
        Some("text") => {
            let text = source.get("data").and_then(|d| d.as_str())?;
            Some(json!({"type": "text", "text": text}))
        }
        Some("base64") => {
            let media_type = source
                .get("media_type")
                .and_then(|m| m.as_str())
                .unwrap_or("application/pdf");
            let data = source.get("data").and_then(|d| d.as_str())?;
            let filename = block
                .get("title")
                .and_then(|t| t.as_str())
                .unwrap_or("document.pdf");
            Some(json!({
                "type": "file",
                "file": {
                    "filename": filename,
                    "file_data": format!("data:{media_type};base64,{data}")
                }
            }))
        }
        Some("url") => {
            let url = source.get("url").and_then(|u| u.as_str())?;
            Some(json!({"type": "text", "text": format!("[document: {url}]")}))
        }
        other => {
            log::warn!("[Claude/OpenAI] dropping unsupported document source type {other:?}");
            None
        }
    }
}

/// Flatten an Anthropic `tool_result` into Chat `tool` message text plus any
/// media parts it carried (PROV-15). `is_error: true` is preserved as an
/// `[error]` prefix because Chat has no error flag on tool messages.
fn tool_result_to_chat_content(block: &Value) -> (String, Vec<Value>) {
    let mut media_parts = Vec::new();
    let mut text = match block.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(items)) => {
            let mut texts = Vec::new();
            let mut others = Vec::new();
            for item in items {
                match item.get("type").and_then(|t| t.as_str()) {
                    Some("text") => {
                        if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                            texts.push(t.to_string());
                        }
                    }
                    Some("image") => media_parts.extend(anthropic_image_to_chat_part(item)),
                    Some("document") => media_parts.extend(anthropic_document_to_chat_part(item)),
                    _ => others.push(item.clone()),
                }
            }
            if !others.is_empty() {
                texts.push(canonical_json_string(&Value::Array(others)));
            }
            if texts.is_empty() && !media_parts.is_empty() {
                texts.push("[tool returned media; see the following message]".to_string());
            }
            texts.join("\n")
        }
        Some(v) => canonical_json_string(v),
        None => String::new(),
    };
    if block.get("is_error").and_then(|v| v.as_bool()) == Some(true) {
        text = if text.is_empty() {
            "[error]".to_string()
        } else {
            format!("[error] {text}")
        };
    }
    (text, media_parts)
}

/// Synthesize a stable tool-call id when the upstream omitted it (PROV-14).
///
/// An empty `tool_use.id` would round-trip as `tool_call_id: ""` on the next
/// request and be rejected upstream, so mirror the streaming converters and
/// the Codex↔Chat bridge which synthesize ids from the call position.
pub(crate) fn normalize_tool_call_id(id: Option<&str>, index: usize) -> String {
    match id.map(str::trim) {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => format!("call_{index}"),
    }
}

/// Parse Chat `function.arguments` into a tool input object (PROV-14).
///
/// Missing / blank arguments mean "no inputs". Unparsable arguments are an
/// error unless the completion was cut off by the token limit, in which case
/// `{}` is used (stop_reason becomes `max_tokens`), matching the Responses rule.
pub(crate) fn parse_tool_call_arguments(
    arguments: Option<&Value>,
    tool_name: &str,
    truncated: bool,
) -> Result<Value, ProxyError> {
    match arguments {
        None | Some(Value::Null) => Ok(json!({})),
        Some(Value::String(raw)) if raw.trim().is_empty() => Ok(json!({})),
        Some(Value::String(raw)) => match serde_json::from_str::<Value>(raw) {
            Ok(value) => Ok(value),
            Err(err) if truncated => {
                log::warn!(
                    "[Claude/OpenAI] Truncated tool call arguments for {tool_name} (finish_reason=length): {err}"
                );
                Ok(json!({}))
            }
            Err(err) => Err(ProxyError::TransformError(format!(
                "Invalid JSON arguments for tool call {tool_name}: {err}"
            ))),
        },
        Some(v @ Value::Object(_)) | Some(v @ Value::Array(_)) => Ok(v.clone()),
        Some(other) => Err(ProxyError::TransformError(format!(
            "Invalid arguments type for tool call {tool_name}: {other}"
        ))),
    }
}

/// Use the upstream message id, or synthesize `msg_<uuid>` when it is missing
/// or empty so usage dedup keys and client ids stay unique (PROV-35).
pub(crate) fn anthropic_message_id(id: Option<&str>) -> String {
    match id.map(str::trim) {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => format!("msg_{}", uuid::Uuid::new_v4().simple()),
    }
}

/// Streaming variant of [`anthropic_message_id`]: fills `slot` with a
/// synthesized id when the upstream id is missing/empty and returns it, so the
/// rest of the stream keeps using the same id.
pub(crate) fn ensure_message_id(slot: &mut Option<String>) -> String {
    if slot.as_deref().is_none_or(|id| id.trim().is_empty()) {
        *slot = Some(anthropic_message_id(None));
    }
    slot.clone().unwrap_or_default()
}

/// 清理工具参数的 JSON schema，并为根 schema 补齐 OpenAI 要求的 object 类型。
pub fn clean_schema(schema: Value) -> Value {
    clean_schema_inner(schema, true)
}

fn clean_schema_inner(mut schema: Value, is_root: bool) -> Value {
    if is_root && !schema.is_object() {
        // A null / non-object root is meaningless for a tool; strict
        // OpenAI-compatible upstreams require an object schema (#4706).
        return json!({"type": "object", "properties": {}});
    }
    if let Some(obj) = schema.as_object_mut() {
        if is_root && obj.get("type").and_then(|v| v.as_str()) != Some("object") {
            // PROV-25: also cover `type: null`, `"Object"` and type arrays, not
            // only a missing key — mirrors `normalize_function_parameters`.
            obj.insert("type".to_string(), json!("object"));
        }
        if is_root && !obj.get("properties").is_some_and(Value::is_object) {
            obj.insert("properties".to_string(), json!({}));
        }

        // 移除 "format": "uri"
        if obj.get("format").and_then(|v| v.as_str()) == Some("uri") {
            obj.remove("format");
        }

        // 递归清理嵌套 schema（所有 JSON Schema 子 schema 关键字，PROV-25）
        for key in ["properties", "$defs", "definitions", "patternProperties"] {
            if let Some(map) = obj.get_mut(key).and_then(|v| v.as_object_mut()) {
                for (_, value) in map.iter_mut() {
                    *value = clean_schema_inner(value.take(), false);
                }
            }
        }

        for key in [
            "items",
            "additionalProperties",
            "not",
            "if",
            "then",
            "else",
            "contains",
        ] {
            if let Some(value) = obj.get_mut(key) {
                if value.is_object() || value.is_array() {
                    *value = clean_schema_inner(value.take(), false);
                }
            }
        }

        for key in ["anyOf", "oneOf", "allOf", "prefixItems"] {
            if let Some(values) = obj.get_mut(key).and_then(|v| v.as_array_mut()) {
                for value in values.iter_mut() {
                    *value = clean_schema_inner(value.take(), false);
                }
            }
        }
    } else if let Some(values) = schema.as_array_mut() {
        // Tuple-form `items: [..]`.
        for value in values.iter_mut() {
            *value = clean_schema_inner(value.take(), false);
        }
    }
    schema
}

/// OpenAI Chat 响应 → Anthropic 响应
///
/// Thin facade over the bridge registry (PROV-19/21):
/// `ir::chat::decode_response` → `ir::anthropic::encode_response`.
pub fn openai_to_anthropic(body: Value) -> Result<Value, ProxyError> {
    use super::bridge::{Bridge, BridgePolicy, WireProtocol};
    Bridge::new(WireProtocol::AnthropicMessages, WireProtocol::OpenAiChat)
        .response_to_client(body, BridgePolicy::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anthropic_to_openai_simple() {
        let input = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["model"], "claude-3-opus");
        assert_eq!(result["max_tokens"], 1024);
        assert_eq!(result["messages"][0]["role"], "user");
        assert_eq!(result["messages"][0]["content"], "Hello");
    }

    #[test]
    fn test_anthropic_to_openai_with_system() {
        let input = json!({
            "model": "claude-3-sonnet",
            "max_tokens": 1024,
            "system": "You are a helpful assistant.",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["messages"][0]["role"], "system");
        assert_eq!(
            result["messages"][0]["content"],
            "You are a helpful assistant."
        );
        assert_eq!(result["messages"][1]["role"], "user");
    }

    #[test]
    fn test_anthropic_to_openai_strips_leading_billing_header_from_system_string() {
        let input = json!({
            "model": "claude-3-sonnet",
            "max_tokens": 1024,
            "system": "x-anthropic-billing-header: cc_version=2.1.119.47e; cc_entrypoint=sdk-cli; cch=a7754;\n\nYou are a helpful assistant.",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["messages"][0]["role"], "system");
        assert_eq!(
            result["messages"][0]["content"],
            "You are a helpful assistant."
        );
        assert_eq!(result["messages"][1]["role"], "user");
    }

    #[test]
    fn test_anthropic_to_openai_strips_billing_header_from_system_array_parts() {
        let input = json!({
            "model": "claude-3-sonnet",
            "max_tokens": 1024,
            "system": [
                {"type": "text", "text": "x-anthropic-billing-header: cc_version=2.1.119.47e; cc_entrypoint=sdk-cli; cch=a7754;\n"},
                {"type": "text", "text": "Stable prompt"}
            ],
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["messages"][0]["role"], "system");
        assert_eq!(result["messages"][0]["content"], "Stable prompt");
        assert_eq!(result["messages"][1]["role"], "user");
    }

    #[test]
    fn test_anthropic_to_openai_preserves_prompt_after_billing_header_in_same_part() {
        let input = json!({
            "model": "claude-3-sonnet",
            "max_tokens": 1024,
            "system": [
                {"type": "text", "text": "x-anthropic-billing-header: cc_version=2.1.119.47e; cc_entrypoint=sdk-cli; cch=a7754;\n\nStable prompt part 1"},
                {"type": "text", "text": "Stable prompt part 2"}
            ],
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["messages"][0]["role"], "system");
        assert_eq!(
            result["messages"][0]["content"],
            "Stable prompt part 1\nStable prompt part 2"
        );
        assert_eq!(result["messages"][1]["role"], "user");
    }

    #[test]
    fn test_anthropic_to_openai_keeps_non_leading_billing_header_text() {
        let input = json!({
            "model": "claude-3-sonnet",
            "max_tokens": 1024,
            "system": "Keep this literal:\nx-anthropic-billing-header: example",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["messages"][0]["role"], "system");
        assert_eq!(
            result["messages"][0]["content"],
            "Keep this literal:\nx-anthropic-billing-header: example"
        );
    }

    #[test]
    fn test_anthropic_to_openai_with_tools() {
        let input = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "What's the weather?"}],
            "tools": [{
                "name": "get_weather",
                "description": "Get weather info",
                "input_schema": {"type": "object", "properties": {"location": {"type": "string"}}}
            }]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["tools"][0]["type"], "function");
        assert_eq!(result["tools"][0]["function"]["name"], "get_weather");
        assert_eq!(
            result["tools"][0]["function"]["parameters"]["type"],
            json!("object")
        );
        assert_eq!(
            result["tools"][0]["function"]["parameters"]["properties"]["location"]["type"],
            json!("string")
        );
    }

    #[test]
    fn test_anthropic_to_openai_defaults_missing_tool_schema_type() {
        let input = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "What's the weather?"}],
            "tools": [{
                "name": "get_weather",
                "description": "Get weather info",
                "input_schema": {"properties": {"location": {"type": "string"}}}
            }]
        });

        let result = anthropic_to_openai(input).unwrap();
        let parameters = &result["tools"][0]["function"]["parameters"];
        assert_eq!(parameters["type"], json!("object"));
        assert_eq!(
            parameters["properties"]["location"]["type"],
            json!("string")
        );
    }

    #[test]
    fn test_anthropic_to_openai_defaults_empty_tool_schema() {
        let input = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Do work"}],
            "tools": [{"name": "do_work", "input_schema": {}}]
        });

        let result = anthropic_to_openai(input).unwrap();
        let parameters = &result["tools"][0]["function"]["parameters"];
        assert_eq!(parameters, &json!({"type": "object", "properties": {}}));
    }

    #[test]
    fn test_clean_schema_only_defaults_root_to_object() {
        let schema = json!({
            "properties": {
                "nullable_value": {
                    "anyOf": [{"type": "string"}, {"type": "null"}]
                },
                "list": {
                    "items": {"type": "string"}
                }
            }
        });

        let result = clean_schema(schema);
        assert_eq!(result["type"], json!("object"));
        assert_eq!(
            result["properties"]["nullable_value"],
            json!({"anyOf": [{"type": "string"}, {"type": "null"}]})
        );
        assert_eq!(
            result["properties"]["list"],
            json!({"items": {"type": "string"}})
        );
    }

    #[test]
    fn test_anthropic_to_openai_strips_cache_control_from_merged_system() {
        let input = json!({
            "model": "claude-3-sonnet",
            "max_tokens": 1024,
            "system": [
                {"type": "text", "text": "You are Claude Code.", "cache_control": {"type": "ephemeral"}},
                {"type": "text", "text": "Be concise.", "cache_control": {"type": "ephemeral"}}
            ],
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["messages"].as_array().unwrap().len(), 2);
        assert_eq!(result["messages"][0]["role"], "system");
        assert_eq!(
            result["messages"][0]["content"],
            "You are Claude Code.\nBe concise."
        );
        assert!(result["messages"][0].get("cache_control").is_none());
        assert_eq!(result["messages"][1]["role"], "user");
    }

    #[test]
    fn test_anthropic_to_openai_strips_cache_control_from_mixed_system() {
        let input = json!({
            "model": "claude-3-sonnet",
            "max_tokens": 1024,
            "system": [
                {"type": "text", "text": "You are Claude Code.", "cache_control": {"type": "ephemeral"}},
                {"type": "text", "text": "Be concise."}
            ],
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["messages"][0]["role"], "system");
        assert_eq!(
            result["messages"][0]["content"],
            "You are Claude Code.\nBe concise."
        );
        assert!(result["messages"][0].get("cache_control").is_none());
    }

    #[test]
    fn test_anthropic_to_openai_strips_cache_control_from_conflicting_system() {
        let input = json!({
            "model": "claude-3-sonnet",
            "max_tokens": 1024,
            "system": [
                {"type": "text", "text": "You are Claude Code.", "cache_control": {"type": "ephemeral"}},
                {"type": "text", "text": "Be concise.", "cache_control": {"type": "ephemeral", "ttl": "5m"}}
            ],
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["messages"][0]["role"], "system");
        assert_eq!(
            result["messages"][0]["content"],
            "You are Claude Code.\nBe concise."
        );
        assert!(result["messages"][0].get("cache_control").is_none());
    }

    #[test]
    fn test_anthropic_to_openai_tool_use() {
        let input = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "messages": [{
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "Let me check"},
                    {"type": "tool_use", "id": "call_123", "name": "get_weather", "input": {"location": "Tokyo"}}
                ]
            }]
        });

        let result = anthropic_to_openai(input).unwrap();
        let msg = &result["messages"][0];
        assert_eq!(msg["role"], "assistant");
        assert!(msg.get("tool_calls").is_some());
        assert_eq!(msg["tool_calls"][0]["id"], "call_123");
        assert!(msg.get("reasoning_content").is_none());
    }

    #[test]
    fn test_anthropic_to_openai_tool_use_preserves_reasoning_content() {
        let input = json!({
            "model": "kimi-k2.6",
            "max_tokens": 1024,
            "messages": [{
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "I should call the tool."},
                    {"type": "tool_use", "id": "call_123", "name": "get_weather", "input": {"location": "Tokyo"}}
                ]
            }]
        });

        let result = anthropic_to_openai_with_reasoning_content(input, true).unwrap();
        let msg = &result["messages"][0];
        assert_eq!(msg["role"], "assistant");
        assert_eq!(msg["reasoning_content"], "I should call the tool.");
        assert!(msg.get("tool_calls").is_some());
        assert_eq!(msg["tool_calls"][0]["id"], "call_123");
    }

    #[test]
    fn test_anthropic_to_openai_tool_use_injects_placeholder_reasoning_content_when_missing() {
        let input = json!({
            "model": "kimi-k2.6",
            "max_tokens": 1024,
            "messages": [{
                "role": "assistant",
                "content": [
                    {"type": "tool_use", "id": "call_123", "name": "get_weather", "input": {"location": "Tokyo"}}
                ]
            }]
        });

        let result = anthropic_to_openai_with_reasoning_content(input, true).unwrap();
        let msg = &result["messages"][0];
        assert_eq!(msg["role"], "assistant");
        assert_eq!(msg["reasoning_content"], "tool call");
        assert!(msg.get("tool_calls").is_some());
        assert_eq!(msg["tool_calls"][0]["id"], "call_123");
    }

    #[test]
    fn test_anthropic_to_openai_tool_use_uses_redacted_thinking_placeholder() {
        let input = json!({
            "model": "mimo-v2.5-pro",
            "max_tokens": 1024,
            "messages": [{
                "role": "assistant",
                "content": [
                    {"type": "redacted_thinking", "data": "opaque"},
                    {"type": "tool_use", "id": "call_123", "name": "get_weather", "input": {"location": "Tokyo"}}
                ]
            }]
        });

        let result = anthropic_to_openai_with_reasoning_content(input, true).unwrap();
        let msg = &result["messages"][0];
        assert_eq!(msg["reasoning_content"], "[redacted thinking]");
        assert_eq!(msg["tool_calls"][0]["id"], "call_123");
    }

    #[test]
    fn test_anthropic_to_openai_text_only_assistant_keeps_reasoning_content() {
        let input = json!({
            "model": "kimi-k2-thinking",
            "max_tokens": 100,
            "messages": [
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "Greet back.", "signature": "s"},
                    {"type": "text", "text": "Hello!"}
                ]},
                {"role": "user", "content": "again"}
            ]
        });
        let result = anthropic_to_openai_with_reasoning_content(input.clone(), true).unwrap();
        let msg = &result["messages"][1];
        assert_eq!(msg["role"], "assistant");
        assert_eq!(msg["content"], "Hello!");
        assert_eq!(msg["reasoning_content"], "Greet back.");

        // Generic provider: unchanged.
        let result = anthropic_to_openai_with_reasoning_content(input, false).unwrap();
        assert!(result["messages"][1].get("reasoning_content").is_none());
    }

    #[test]
    fn test_anthropic_to_openai_text_only_assistant_without_thinking_has_no_reasoning() {
        let input = json!({
            "model": "kimi-k2-thinking",
            "max_tokens": 100,
            "messages": [
                {"role": "assistant", "content": [{"type": "text", "text": "Hello!"}]}
            ]
        });
        let result = anthropic_to_openai_with_reasoning_content(input, true).unwrap();
        assert!(result["messages"][0].get("reasoning_content").is_none());
    }

    #[test]
    fn test_anthropic_to_openai_does_not_emit_reasoning_content_by_default() {
        let input = json!({
            "model": "gpt-5.4",
            "max_tokens": 1024,
            "messages": [{
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "I should call the tool."},
                    {"type": "tool_use", "id": "call_123", "name": "get_weather", "input": {"location": "Tokyo"}}
                ]
            }]
        });

        let result = anthropic_to_openai(input).unwrap();
        let msg = &result["messages"][0];
        assert_eq!(msg["role"], "assistant");
        assert!(msg.get("tool_calls").is_some());
        assert!(msg.get("reasoning_content").is_none());
    }

    #[test]
    fn test_anthropic_to_openai_skips_thinking_only_message() {
        let input = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "messages": [{
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "No visible content yet."}
                ]
            }]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["messages"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_anthropic_to_openai_tool_result() {
        let input = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "tool_result", "tool_use_id": "call_123", "content": "Sunny, 25°C"}
                ]
            }]
        });

        let result = anthropic_to_openai(input).unwrap();
        let msg = &result["messages"][0];
        assert_eq!(msg["role"], "tool");
        assert_eq!(msg["tool_call_id"], "call_123");
        assert_eq!(msg["content"], "Sunny, 25°C");
    }

    #[test]
    fn test_openai_to_anthropic_simple() {
        let input = json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1234567890,
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hello!"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        });

        let result = openai_to_anthropic(input).unwrap();
        assert_eq!(result["id"], "chatcmpl-123");
        assert_eq!(result["type"], "message");
        assert_eq!(result["content"][0]["type"], "text");
        assert_eq!(result["content"][0]["text"], "Hello!");
        assert_eq!(result["stop_reason"], "end_turn");
        assert_eq!(result["usage"]["input_tokens"], 10);
        assert_eq!(result["usage"]["output_tokens"], 5);
    }

    #[test]
    fn test_openai_to_anthropic_preserves_id_for_usage_dedup() {
        let input = json!({
            "id": "chatcmpl-claude-compatible",
            "object": "chat.completion",
            "model": "claude-sonnet-4-5",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hello!"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        });

        let result = openai_to_anthropic(input).unwrap();
        let usage = crate::proxy::usage::parser::TokenUsage::from_claude_response(&result)
            .expect("converted Anthropic response should parse usage");

        assert_eq!(
            usage.message_id.as_deref(),
            Some("chatcmpl-claude-compatible")
        );
        assert_eq!(
            usage.dedup_request_id(),
            "session:chatcmpl-claude-compatible"
        );
    }

    #[test]
    fn test_openai_to_anthropic_with_tool_calls() {
        let input = json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1234567890,
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_123",
                        "type": "function",
                        "function": {"name": "get_weather", "arguments": "{\"location\": \"Tokyo\"}"}
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        });

        let result = openai_to_anthropic(input).unwrap();
        assert_eq!(result["content"][0]["type"], "tool_use");
        assert_eq!(result["content"][0]["id"], "call_123");
        assert_eq!(result["content"][0]["name"], "get_weather");
        assert_eq!(result["content"][0]["input"]["location"], "Tokyo");
        assert_eq!(result["stop_reason"], "tool_use");
    }

    #[test]
    fn test_deepseek_reasoning_content_round_trips_for_tool_calls() {
        let upstream_response = json!({
            "id": "chatcmpl-deepseek",
            "object": "chat.completion",
            "created": 1234567890,
            "model": "deepseek-v4-flash",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "reasoning_content": "Need the current date before calling weather.",
                    "content": "Let me check the date first.",
                    "tool_calls": [{
                        "id": "call_date",
                        "type": "function",
                        "function": {"name": "get_date", "arguments": "{}"}
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        });

        let anthropic_response = openai_to_anthropic(upstream_response).unwrap();
        assert_eq!(anthropic_response["content"][0]["type"], "thinking");
        assert_eq!(
            anthropic_response["content"][0]["thinking"],
            "Need the current date before calling weather."
        );
        assert_eq!(anthropic_response["content"][1]["type"], "text");
        assert_eq!(anthropic_response["content"][2]["type"], "tool_use");
        assert_eq!(anthropic_response["content"][2]["id"], "call_date");

        let follow_up_request = json!({
            "model": "deepseek-v4-flash",
            "max_tokens": 1024,
            "messages": [{
                "role": "assistant",
                "content": anthropic_response["content"].clone()
            }]
        });
        let replayed = anthropic_to_openai_with_reasoning_content(follow_up_request, true).unwrap();
        let msg = &replayed["messages"][0];

        assert_eq!(
            msg["reasoning_content"],
            "Need the current date before calling weather."
        );
        assert_eq!(msg["tool_calls"][0]["id"], "call_date");
        assert_eq!(msg["tool_calls"][0]["function"]["name"], "get_date");
    }

    #[test]
    fn test_model_passthrough() {
        // 格式转换层只做结构转换，模型映射由上游 proxy::model_mapper 处理
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["model"], "gpt-4o");
    }

    #[test]
    fn test_anthropic_to_openai_does_not_inject_prompt_cache_key() {
        let input = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert!(result.get("prompt_cache_key").is_none());
    }

    #[test]
    fn test_anthropic_to_openai_strips_all_cache_control() {
        let input = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "system": [
                {"type": "text", "text": "System prompt", "cache_control": {"type": "ephemeral"}}
            ],
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "Hello", "cache_control": {"type": "ephemeral", "ttl": "5m"}}
                ]
            }],
            "tools": [{
                "name": "get_weather",
                "description": "Get weather",
                "input_schema": {"type": "object"},
                "cache_control": {"type": "ephemeral"}
            }]
        });

        let result = anthropic_to_openai(input).unwrap();
        // System message: no cache_control
        assert!(result["messages"][0].get("cache_control").is_none());
        // User message: content simplified to string (no cache_control → flat string)
        assert_eq!(result["messages"][1]["content"], "Hello");
        // Tool: no cache_control
        assert!(result["tools"][0].get("cache_control").is_none());
    }

    /// 精确复现 Issue #3805 报告的 400 错误场景:
    /// GLM/Qwen 等严格校验模型拒绝 cache_control 和 content 数组格式
    #[test]
    fn test_regression_gh3805_no_cache_control_leak_to_openai() {
        let input = json!({
            "model": "glm-5.1",
            "max_tokens": 1024,
            "system": [
                {"type": "text", "text": "You are helpful.", "cache_control": {"type": "ephemeral"}}
            ],
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "Hello", "cache_control": {"type": "ephemeral"}}
                ]}
            ],
            "tools": [{
                "name": "search",
                "description": "Search the web",
                "input_schema": {"type": "object"},
                "cache_control": {"type": "ephemeral"}
            }]
        });

        let result = anthropic_to_openai(input).unwrap();

        // 验证: messages 中不存在 cache_control
        for (i, msg) in result["messages"].as_array().unwrap().iter().enumerate() {
            assert!(
                msg.get("cache_control").is_none(),
                "messages[{i}] must not have cache_control"
            );
        }

        // 验证: content 中没有 cache_control
        for (i, msg) in result["messages"].as_array().unwrap().iter().enumerate() {
            if let Some(content) = msg.get("content") {
                assert!(
                    !content.is_array()
                        || content
                            .as_array()
                            .unwrap()
                            .iter()
                            .all(|part| part.get("cache_control").is_none()),
                    "messages[{i}] content parts must not have cache_control"
                );
            }
        }

        // 验证: system content 为纯字符串格式（不是数组）
        let sys_msg = &result["messages"][0];
        assert_eq!(sys_msg["role"], "system");
        assert!(
            sys_msg["content"].is_string(),
            "system content must be string, got: {}",
            sys_msg["content"]
        );

        // 验证: user content 为纯字符串格式（不是数组）
        let user_msg = &result["messages"][1];
        assert_eq!(user_msg["role"], "user");
        assert!(
            user_msg["content"].is_string(),
            "user content must be string, got: {}",
            user_msg["content"]
        );

        // 验证: tools 中不存在 cache_control
        if let Some(tools) = result["tools"].as_array() {
            for (i, tool) in tools.iter().enumerate() {
                assert!(
                    tool.get("cache_control").is_none(),
                    "tools[{i}] must not have cache_control"
                );
            }
        }
    }

    #[test]
    fn test_openai_to_anthropic_with_cache_tokens() {
        let input = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hello!"},
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 50,
                "prompt_tokens_details": {
                    "cached_tokens": 80
                }
            }
        });

        let result = openai_to_anthropic(input).unwrap();
        // prompt_tokens(100) 含 cached(80)，转换后 input 应为 fresh = 100 - 80 = 20
        assert_eq!(result["usage"]["input_tokens"], 20);
        assert_eq!(result["usage"]["output_tokens"], 50);
        assert_eq!(result["usage"]["cache_read_input_tokens"], 80);
    }

    #[test]
    fn test_openai_to_anthropic_with_direct_cache_fields() {
        let input = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hello!"},
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 50,
                "cache_read_input_tokens": 60,
                "cache_creation_input_tokens": 20
            }
        });

        let result = openai_to_anthropic(input).unwrap();
        // cache_read(60)+cache_creation(20) 均从 prompt(100) 扣除，fresh = 100 - 60 - 20 = 20
        // 守恒：input(20) + cache_read(60) + cache_creation(20) == prompt(100)
        assert_eq!(result["usage"]["input_tokens"], 20);
        assert_eq!(result["usage"]["cache_read_input_tokens"], 60);
        assert_eq!(result["usage"]["cache_creation_input_tokens"], 20);
    }

    #[test]
    fn test_openai_to_anthropic_clamps_input_when_cache_exceeds_prompt() {
        // prompt(100) < cache_read(60)+cache_creation(50)=110：saturating 钳到 0，防下溢。
        // 钉桩：阻止未来把 saturating_sub 误改成普通减法(debug panic / release wrap)。
        let input = json!({
            "id": "chatcmpl-uf",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "x"},
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 10,
                "cache_read_input_tokens": 60,
                "cache_creation_input_tokens": 50
            }
        });
        let result = openai_to_anthropic(input).unwrap();
        assert_eq!(result["usage"]["input_tokens"], 0);
        assert_eq!(result["usage"]["cache_read_input_tokens"], 60);
        assert_eq!(result["usage"]["cache_creation_input_tokens"], 50);
    }

    #[test]
    fn test_openai_to_anthropic_finish_reason_content_filter_maps_refusal() {
        let input = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Blocked"},
                "finish_reason": "content_filter"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 1}
        });

        let result = openai_to_anthropic(input).unwrap();
        assert_eq!(result["stop_reason"], "refusal");
    }

    #[test]
    fn test_anthropic_to_openai_url_image_document_and_error_tool_result() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 100,
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "t1", "name": "read", "input": {}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "t1", "is_error": true, "content": [
                        {"type": "text", "text": "not found"},
                        {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "AAAA"}}
                    ]},
                    {"type": "image", "source": {"type": "url", "url": "https://example.com/a.png"}},
                    {"type": "document", "title": "spec.pdf", "source": {"type": "base64", "media_type": "application/pdf", "data": "JVBE"}},
                    {"type": "document", "source": {"type": "text", "media_type": "text/plain", "data": "plain doc"}}
                ]}
            ]
        });
        let result = anthropic_to_openai(input).unwrap();
        let messages = result["messages"].as_array().unwrap();
        let tool = messages.iter().find(|m| m["role"] == "tool").unwrap();
        assert_eq!(tool["content"], "[error] not found");
        assert!(!tool["content"].as_str().unwrap().contains("AAAA"));

        let user = messages.iter().rfind(|m| m["role"] == "user").unwrap();
        let parts = user["content"].as_array().unwrap();
        assert_eq!(parts[0]["image_url"]["url"], "data:image/png;base64,AAAA");
        assert_eq!(parts[1]["image_url"]["url"], "https://example.com/a.png");
        assert_eq!(parts[2]["type"], "file");
        assert_eq!(parts[2]["file"]["filename"], "spec.pdf");
        assert_eq!(
            parts[2]["file"]["file_data"],
            "data:application/pdf;base64,JVBE"
        );
        assert_eq!(parts[3]["text"], "plain doc");
    }

    #[test]
    fn test_clean_schema_forces_object_root_for_all_shapes() {
        for input in [
            json!({"type": null}),
            json!({"type": "Object"}),
            json!({"type": ["object", "null"]}),
            json!({"properties": {}}),
            json!(null),
        ] {
            let cleaned = clean_schema(input.clone());
            assert_eq!(cleaned["type"], "object", "input = {input}");
            assert!(cleaned["properties"].is_object(), "input = {input}");
        }
    }

    #[test]
    fn test_clean_schema_strips_uri_format_in_nested_applicators() {
        let cleaned = clean_schema(json!({
            "type": "object",
            "properties": {
                "target": {"anyOf": [{"type": "string", "format": "uri"}, {"type": "null"}]},
                "list": {"type": "array", "items": {"type": "object", "properties": {
                    "href": {"type": "string", "format": "uri"}
                }}},
                "ref": {"$ref": "#/$defs/link"}
            },
            "$defs": {"link": {"type": "string", "format": "uri"}}
        }));
        assert!(cleaned["properties"]["target"]["anyOf"][0]
            .get("format")
            .is_none());
        assert!(cleaned["properties"]["list"]["items"]["properties"]["href"]
            .get("format")
            .is_none());
        assert!(cleaned["$defs"]["link"].get("format").is_none());
        assert_eq!(cleaned["properties"]["ref"]["$ref"], "#/$defs/link");
    }

    #[test]
    fn test_openai_to_anthropic_missing_tool_call_id_is_synthesized() {
        let input = json!({
            "id": "chatcmpl-1",
            "model": "local",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": null, "tool_calls": [
                    {"type": "function", "function": {"name": "a", "arguments": "{}"}},
                    {"id": "", "type": "function", "function": {"name": "b", "arguments": ""}}
                ]},
                "finish_reason": "tool_calls"
            }]
        });
        let result = openai_to_anthropic(input).unwrap();
        assert_eq!(result["content"][0]["id"], "call_0");
        assert_eq!(result["content"][1]["id"], "call_1");
        assert_eq!(result["content"][1]["input"], json!({}));
    }

    #[test]
    fn test_openai_to_anthropic_invalid_tool_arguments_error_unless_truncated() {
        let make = |finish: &str| {
            json!({
                "id": "chatcmpl-1",
                "model": "m",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": null, "tool_calls": [
                        {"id": "c1", "type": "function", "function": {"name": "a", "arguments": "{\"a\":"}}
                    ]},
                    "finish_reason": finish
                }]
            })
        };
        assert!(openai_to_anthropic(make("stop")).is_err());
        assert!(openai_to_anthropic(make("tool_calls")).is_err());

        let result = openai_to_anthropic(make("length")).unwrap();
        assert_eq!(result["content"][0]["input"], json!({}));
        assert_eq!(result["stop_reason"], "max_tokens");
    }

    #[test]
    fn test_openai_to_anthropic_empty_id_synthesizes_message_id() {
        let input = json!({
            "id": "",
            "model": "m",
            "choices": [{"index": 0, "message": {"role": "assistant", "content": "hi"}, "finish_reason": "stop"}]
        });
        let a = openai_to_anthropic(input.clone()).unwrap();
        let b = openai_to_anthropic(input).unwrap();
        let id_a = a["id"].as_str().unwrap();
        assert!(id_a.starts_with("msg_") && id_a.len() > 4);
        assert_ne!(a["id"], b["id"]);
    }

    #[test]
    fn test_openai_to_anthropic_with_legacy_function_call() {
        let input = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "function_call": {
                        "name": "get_weather",
                        "arguments": "{\"location\":\"Tokyo\"}"
                    }
                },
                "finish_reason": "function_call"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5}
        });

        let result = openai_to_anthropic(input).unwrap();
        assert_eq!(result["content"][0]["type"], "tool_use");
        assert_eq!(result["content"][0]["name"], "get_weather");
        assert_eq!(result["content"][0]["input"]["location"], "Tokyo");
        assert_eq!(result["stop_reason"], "tool_use");
    }

    #[test]
    fn test_openai_to_anthropic_with_content_parts_and_refusal() {
        let input = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": [
                        {"type": "text", "text": "Hello"},
                        {"type": "refusal", "refusal": "I can't do that"}
                    ]
                },
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5}
        });

        let result = openai_to_anthropic(input).unwrap();
        assert_eq!(result["content"][0]["type"], "text");
        assert_eq!(result["content"][0]["text"], "Hello");
        assert_eq!(result["content"][1]["type"], "text");
        assert_eq!(result["content"][1]["text"], "I can't do that");
    }

    #[test]
    fn test_is_openai_o_series() {
        assert!(is_openai_o_series("o1"));
        assert!(is_openai_o_series("o1-preview"));
        assert!(is_openai_o_series("o1-mini"));
        assert!(is_openai_o_series("o3"));
        assert!(is_openai_o_series("o3-mini"));
        assert!(is_openai_o_series("o4-mini"));
        assert!(!is_openai_o_series("gpt-4o"));
        assert!(!is_openai_o_series("openai-gpt"));
        assert!(!is_openai_o_series("o"));
        assert!(!is_openai_o_series(""));
    }

    #[test]
    fn test_supports_reasoning_effort() {
        assert!(supports_reasoning_effort("o1"));
        assert!(supports_reasoning_effort("o3-mini"));
        assert!(supports_reasoning_effort("gpt-5"));
        assert!(supports_reasoning_effort("gpt-5.4"));
        assert!(supports_reasoning_effort("gpt-5-codex"));
        assert!(!supports_reasoning_effort("gpt-4o"));
        assert!(!supports_reasoning_effort("claude-sonnet-4-6"));
    }

    // ── resolve_reasoning_effort unit tests ──

    #[test]
    fn test_output_config_low_maps_to_reasoning_effort_low() {
        let body = json!({"output_config": {"effort": "low"}});
        assert_eq!(resolve_reasoning_effort(&body), Some("low"));
    }

    #[test]
    fn test_output_config_medium_maps_to_reasoning_effort_medium() {
        let body = json!({"output_config": {"effort": "medium"}});
        assert_eq!(resolve_reasoning_effort(&body), Some("medium"));
    }

    #[test]
    fn test_output_config_high_maps_to_reasoning_effort_high() {
        let body = json!({"output_config": {"effort": "high"}});
        assert_eq!(resolve_reasoning_effort(&body), Some("high"));
    }

    #[test]
    fn test_output_config_max_maps_to_reasoning_effort_xhigh() {
        let body = json!({"output_config": {"effort": "max"}});
        assert_eq!(resolve_reasoning_effort(&body), Some("xhigh"));
    }

    #[test]
    fn test_output_config_takes_priority_over_thinking() {
        // Even with thinking.adaptive present, explicit effort wins
        let body = json!({
            "output_config": {"effort": "low"},
            "thinking": {"type": "adaptive"}
        });
        assert_eq!(resolve_reasoning_effort(&body), Some("low"));
    }

    #[test]
    fn test_output_config_unknown_value_no_reasoning_effort() {
        let body = json!({"output_config": {"effort": "turbo"}});
        assert_eq!(resolve_reasoning_effort(&body), None);
    }

    #[test]
    fn test_thinking_enabled_small_budget_maps_low() {
        let body = json!({"thinking": {"type": "enabled", "budget_tokens": 1024}});
        assert_eq!(resolve_reasoning_effort(&body), Some("low"));
    }

    #[test]
    fn test_thinking_enabled_medium_budget_maps_medium() {
        let body = json!({"thinking": {"type": "enabled", "budget_tokens": 8000}});
        assert_eq!(resolve_reasoning_effort(&body), Some("medium"));
    }

    #[test]
    fn test_thinking_enabled_large_budget_maps_high() {
        let body = json!({"thinking": {"type": "enabled", "budget_tokens": 32000}});
        assert_eq!(resolve_reasoning_effort(&body), Some("high"));
    }

    #[test]
    fn test_thinking_enabled_without_budget_maps_high() {
        let body = json!({"thinking": {"type": "enabled"}});
        assert_eq!(resolve_reasoning_effort(&body), Some("high"));
    }

    #[test]
    fn test_thinking_adaptive_maps_xhigh() {
        let body = json!({"thinking": {"type": "adaptive"}});
        assert_eq!(resolve_reasoning_effort(&body), Some("xhigh"));
    }

    #[test]
    fn test_thinking_disabled_no_reasoning_effort() {
        let body = json!({"thinking": {"type": "disabled"}});
        assert_eq!(resolve_reasoning_effort(&body), None);
    }

    #[test]
    fn test_no_thinking_field_no_reasoning_effort() {
        let body = json!({"messages": [{"role": "user", "content": "Hello"}]});
        assert_eq!(resolve_reasoning_effort(&body), None);
    }

    // ── Integration: anthropic_to_openai with resolve_reasoning_effort ──

    #[test]
    fn test_non_reasoning_model_no_reasoning_effort() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "thinking": {"type": "enabled", "budget_tokens": 2048},
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert!(result.get("reasoning_effort").is_none());
    }

    #[test]
    fn test_reasoning_model_with_output_config_effort() {
        let input = json!({
            "model": "gpt-5.4",
            "max_tokens": 1024,
            "output_config": {"effort": "medium"},
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["reasoning_effort"], "medium");
    }

    #[test]
    fn test_reasoning_model_with_output_config_max() {
        let input = json!({
            "model": "gpt-5.4",
            "max_tokens": 1024,
            "output_config": {"effort": "max"},
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["reasoning_effort"], "xhigh");
    }

    #[test]
    fn test_reasoning_model_thinking_enabled_small_budget() {
        let input = json!({
            "model": "o3",
            "max_tokens": 1024,
            "thinking": {"type": "enabled", "budget_tokens": 2048},
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["reasoning_effort"], "low");
    }

    #[test]
    fn test_reasoning_model_thinking_adaptive() {
        let input = json!({
            "model": "gpt-5.4",
            "max_tokens": 1024,
            "thinking": {"type": "adaptive"},
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["reasoning_effort"], "xhigh");
    }

    #[test]
    fn test_reasoning_model_no_thinking_no_effort() {
        let input = json!({
            "model": "gpt-5.4",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert!(result.get("reasoning_effort").is_none());
    }

    #[test]
    fn test_anthropic_to_openai_o_series_max_completion_tokens() {
        for model in &["o1", "o3-mini", "o4-mini"] {
            let input = json!({
                "model": model,
                "max_tokens": 4096,
                "messages": [{"role": "user", "content": "Hello"}]
            });

            let result = anthropic_to_openai(input).unwrap();
            assert!(
                result.get("max_tokens").is_none(),
                "{model} should not have max_tokens"
            );
            assert_eq!(
                result["max_completion_tokens"], 4096,
                "{model} should use max_completion_tokens"
            );
        }
    }

    #[test]
    fn test_anthropic_to_openai_gpt5_uses_max_completion_tokens_and_drops_sampling() {
        for model in &["gpt-5.4", "gpt-5-mini", "openai/gpt-5", "o3"] {
            let input = json!({
                "model": model,
                "max_tokens": 100,
                "temperature": 0,
                "top_p": 0.9,
                "messages": [{"role": "user", "content": "Hello"}]
            });

            let result = anthropic_to_openai(input).unwrap();
            assert_eq!(result["max_completion_tokens"], 100, "{model}");
            assert!(result.get("max_tokens").is_none(), "{model}");
            assert!(result.get("temperature").is_none(), "{model}");
            assert!(result.get("top_p").is_none(), "{model}");
        }
    }

    #[test]
    fn test_anthropic_to_openai_gpt5_chat_keeps_sampling() {
        let input = json!({
            "model": "gpt-5-chat-latest",
            "max_tokens": 100,
            "temperature": 0.2,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["max_completion_tokens"], 100);
        assert_eq!(result["temperature"], 0.2);
    }

    #[test]
    fn test_anthropic_to_openai_gpt41_keeps_max_tokens_and_temperature() {
        let input = json!({
            "model": "gpt-4.1",
            "max_tokens": 100,
            "temperature": 0,
            "top_p": 0.5,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["max_tokens"], 100);
        assert!(result.get("max_completion_tokens").is_none());
        assert_eq!(result["temperature"], 0);
        assert_eq!(result["top_p"], 0.5);
    }

    #[test]
    fn test_anthropic_to_openai_non_o_series_keeps_max_tokens() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["max_tokens"], 1024);
        assert!(result.get("max_completion_tokens").is_none());
    }

    fn run_tool_choice(value: Value) -> Value {
        let input = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "Hello"}],
            "tools": [{
                "name": "search",
                "description": "search the web",
                "input_schema": {"type": "object", "properties": {}}
            }],
            "tool_choice": value,
        });
        anthropic_to_openai(input).unwrap()["tool_choice"].clone()
    }

    #[test]
    fn tool_choice_disable_parallel_emits_parallel_tool_calls_false() {
        let input = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "Hello"}],
            "tools": [{"name": "search", "input_schema": {"type": "object"}}],
            "tool_choice": {"type": "any", "disable_parallel_tool_use": true}
        });
        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["tool_choice"], "required");
        assert_eq!(result["parallel_tool_calls"], false);

        // Not set unless requested.
        let input = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "Hello"}],
            "tools": [{"name": "search", "input_schema": {"type": "object"}}],
            "tool_choice": {"type": "auto"}
        });
        assert!(anthropic_to_openai(input)
            .unwrap()
            .get("parallel_tool_calls")
            .is_none());
    }

    #[test]
    fn tool_choice_string_any_maps_to_required() {
        assert_eq!(run_tool_choice(json!("any")), json!("required"));
    }

    #[test]
    fn tool_choice_string_auto_and_none_pass_through() {
        assert_eq!(run_tool_choice(json!("auto")), json!("auto"));
        assert_eq!(run_tool_choice(json!("none")), json!("none"));
    }

    #[test]
    fn tool_choice_object_any_maps_to_required() {
        assert_eq!(run_tool_choice(json!({"type": "any"})), json!("required"));
    }

    #[test]
    fn tool_choice_object_auto_and_none_collapse_to_string() {
        assert_eq!(run_tool_choice(json!({"type": "auto"})), json!("auto"));
        assert_eq!(run_tool_choice(json!({"type": "none"})), json!("none"));
    }

    #[test]
    fn tool_choice_forced_tool_maps_to_nested_function_selector() {
        // Anthropic {"type":"tool","name":"X"} must become OpenAI Chat
        // {"type":"function","function":{"name":"X"}} — the *nested* form, not
        // the flat Responses-API form.
        assert_eq!(
            run_tool_choice(json!({"type": "tool", "name": "search"})),
            json!({"type": "function", "function": {"name": "search"}}),
        );
    }
}
