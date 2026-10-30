//! 编解码 trait 的契约测试
//!
//! 用一个只支持文本的最小 Messages 风格编解码（[`TextCodec`]）验证提供的方法：
//! 非流式与流式之间的转换只依赖流式编解码 + 聚合。

use super::*;
use crate::proxy::protocol::ir::{Message, Part, PartDelta, Role};
use crate::proxy::protocol::StopReason;
use crate::proxy::stream::SseDecoder;
use proptest::prelude::*;
use serde_json::json;

struct TextCodec;

fn invalid(message: impl Into<String>) -> CodecError {
    CodecError::Invalid {
        format: WireFormat::AnthropicMessages,
        message: message.into(),
    }
}

fn stop_reason_str(reason: Option<StopReason>) -> Value {
    reason.map_or(Value::Null, |r| json!(r.as_anthropic_str()))
}

fn parse_stop_reason(value: &Value) -> Option<StopReason> {
    match value.as_str()? {
        "end_turn" => Some(StopReason::EndTurn),
        "max_tokens" => Some(StopReason::MaxTokens),
        "tool_use" => Some(StopReason::ToolUse),
        "refusal" => Some(StopReason::Refusal),
        _ => None,
    }
}

fn text_parts(content: &Value) -> Result<Vec<Part>, CodecError> {
    content
        .as_array()
        .ok_or_else(|| invalid("content must be an array"))?
        .iter()
        .map(|block| {
            block["text"]
                .as_str()
                .map(Part::text)
                .ok_or_else(|| invalid("only text blocks are supported"))
        })
        .collect()
}

fn encode_text_parts(parts: &[Part]) -> Result<Value, CodecError> {
    parts
        .iter()
        .map(|part| match part {
            Part::Text { text } => Ok(json!({"type": "text", "text": text})),
            other => Err(CodecError::Unsupported {
                format: WireFormat::AnthropicMessages,
                what: format!("{other:?}"),
            }),
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Value::Array)
}

impl RequestCodec for TextCodec {
    fn format(&self) -> WireFormat {
        WireFormat::AnthropicMessages
    }

    fn decode_request(&self, body: &Value) -> Result<ChatRequest, CodecError> {
        let messages = body["messages"]
            .as_array()
            .ok_or_else(|| invalid("messages must be an array"))?
            .iter()
            .map(|m| {
                let role = match m["role"].as_str() {
                    Some("user") => Role::User,
                    Some("assistant") => Role::Assistant,
                    _ => return Err(invalid("unknown role")),
                };
                Ok(Message::new(role, text_parts(&m["content"])?))
            })
            .collect::<Result<_, _>>()?;
        Ok(ChatRequest {
            model: body["model"].as_str().unwrap_or_default().to_string(),
            messages,
            max_tokens: body["max_tokens"].as_u64(),
            stream: body["stream"].as_bool().unwrap_or(false),
            ..Default::default()
        })
    }

    fn encode_request(&self, request: &ChatRequest) -> Result<Value, CodecError> {
        let messages = request
            .messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    Role::User => "user",
                    Role::Assistant => "assistant",
                };
                Ok(json!({"role": role, "content": encode_text_parts(&m.parts)?}))
            })
            .collect::<Result<Vec<_>, CodecError>>()?;
        let mut body =
            json!({"model": request.model, "messages": messages, "stream": request.stream});
        if let Some(max_tokens) = request.max_tokens {
            body["max_tokens"] = json!(max_tokens);
        }
        Ok(body)
    }
}

impl ResponseCodec for TextCodec {
    fn decode_response(&self, body: &Value) -> Result<ChatResponse, CodecError> {
        Ok(ChatResponse {
            id: body["id"].as_str().unwrap_or_default().to_string(),
            model: body["model"].as_str().unwrap_or_default().to_string(),
            parts: text_parts(&body["content"])?,
            stop_reason: parse_stop_reason(&body["stop_reason"]),
            usage: None,
        })
    }

    fn encode_response(&self, response: &ChatResponse) -> Result<Value, CodecError> {
        Ok(json!({
            "id": response.id,
            "model": response.model,
            "content": encode_text_parts(&response.parts)?,
            "stop_reason": stop_reason_str(response.stop_reason),
        }))
    }
}

struct TextDecoder;

impl EventDecoder for TextDecoder {
    fn decode(&mut self, event: &str, data: &str) -> Result<Vec<StreamEvent>, CodecError> {
        let data: Value = serde_json::from_str(data).map_err(|e| invalid(e.to_string()))?;
        let index = || data["index"].as_u64().unwrap_or(0) as usize;
        Ok(match event {
            "message_start" => vec![StreamEvent::MessageStart {
                id: data["id"].as_str().unwrap_or_default().to_string(),
                model: data["model"].as_str().unwrap_or_default().to_string(),
                usage: None,
            }],
            "content_block_start" => vec![StreamEvent::PartStart {
                index: index(),
                part: Part::text(""),
            }],
            "content_block_delta" => vec![StreamEvent::PartDelta {
                index: index(),
                delta: PartDelta::Text(data["text"].as_str().unwrap_or_default().to_string()),
            }],
            "content_block_stop" => vec![StreamEvent::PartStop { index: index() }],
            "message_stop" => vec![StreamEvent::MessageStop {
                stop_reason: parse_stop_reason(&data["stop_reason"]),
            }],
            _ => Vec::new(),
        })
    }
}

struct TextEncoder;

fn frame(event: &str, data: Value) -> Bytes {
    Bytes::from(format!("event: {event}\ndata: {data}\n\n"))
}

impl EventEncoder for TextEncoder {
    fn encode(&mut self, event: &StreamEvent) -> Vec<Bytes> {
        let frame = match event {
            StreamEvent::MessageStart { id, model, .. } => {
                frame("message_start", json!({"id": id, "model": model}))
            }
            StreamEvent::PartStart { index, .. } => {
                frame("content_block_start", json!({"index": index}))
            }
            StreamEvent::PartDelta {
                index,
                delta: PartDelta::Text(text),
            } => frame("content_block_delta", json!({"index": index, "text": text})),
            StreamEvent::PartStop { index } => frame("content_block_stop", json!({"index": index})),
            StreamEvent::MessageStop { stop_reason } => frame(
                "message_stop",
                json!({"stop_reason": stop_reason_str(*stop_reason)}),
            ),
            _ => return Vec::new(),
        };
        vec![frame]
    }

    fn finish(&mut self) -> Vec<Bytes> {
        vec![Bytes::from_static(b"data: [DONE]\n\n")]
    }
}

impl StreamCodec for TextCodec {
    fn stream_decoder(&self) -> Result<Box<dyn EventDecoder>, CodecError> {
        Ok(Box::new(TextDecoder))
    }
    fn stream_encoder(&self) -> Result<Box<dyn EventEncoder>, CodecError> {
        Ok(Box::new(TextEncoder))
    }
}

/// 把编码出的 SSE 帧重新切成事件，再交给 [`Codec::aggregate_stream`]
fn reaggregate(codec: &dyn Codec, frames: &[Bytes]) -> Result<ChatResponse, CodecError> {
    let mut sse = SseDecoder::new();
    let events: Vec<_> = frames
        .iter()
        .flat_map(|frame| sse.feed(frame))
        .filter(|e| e.data != "[DONE]")
        .collect();
    let mut pairs = events.iter().map(|e| (e.event_name(), e.data.as_str()));
    codec.aggregate_stream(&mut pairs)
}

fn text_response(texts: &[&str], stop_reason: Option<StopReason>) -> ChatResponse {
    ChatResponse {
        id: "msg_1".into(),
        model: "m".into(),
        parts: texts.iter().map(|t| Part::text(*t)).collect(),
        stop_reason,
        usage: None,
    }
}

#[test]
fn non_stream_response_is_stream_plus_aggregate() {
    let codec = TextCodec;
    let body = json!({
        "id": "msg_1",
        "model": "m",
        "content": [{"type": "text", "text": "hello"}, {"type": "text", "text": "world"}],
        "stop_reason": "end_turn"
    });
    let response = codec.decode_response(&body).unwrap();
    assert_eq!(
        response,
        text_response(&["hello", "world"], Some(StopReason::EndTurn))
    );

    let frames = codec.encode_response_as_stream(&response).unwrap();
    let sse: String = frames
        .iter()
        .map(|f| String::from_utf8(f.to_vec()).unwrap())
        .collect();
    assert!(sse.starts_with("event: message_start\n"));
    assert!(sse.ends_with("data: [DONE]\n\n"));

    let aggregated = reaggregate(&codec, &frames).unwrap();
    assert_eq!(aggregated, response);
    assert_eq!(codec.encode_response(&aggregated).unwrap(), body);
}

#[test]
fn request_round_trips_through_ir() {
    let codec = TextCodec;
    let body = json!({
        "model": "m",
        "messages": [
            {"role": "user", "content": [{"type": "text", "text": "hi"}]},
            {"role": "assistant", "content": [{"type": "text", "text": "yo"}]}
        ],
        "stream": true,
        "max_tokens": 64
    });
    let request = codec.decode_request(&body).unwrap();
    assert_eq!(request.messages[1].role, Role::Assistant);
    assert_eq!(codec.encode_request(&request).unwrap(), body);
}

#[test]
fn codec_errors_surface_unsupported_parts_and_bad_streams() {
    let codec = TextCodec;
    let response = ChatResponse {
        parts: vec![Part::RedactedReasoning { data: "x".into() }],
        ..Default::default()
    };
    assert!(matches!(
        codec.encode_response(&response),
        Err(CodecError::Unsupported { .. })
    ));

    // 增量指向未开始的块：聚合错误经 CodecError 透出
    let mut events = [("content_block_delta", r#"{"index":3,"text":"x"}"#)].into_iter();
    assert_eq!(
        codec.aggregate_stream(&mut events),
        Err(CodecError::Aggregate(AggregateError::UnknownPart(3)))
    );
    let mut events = [("message_start", "not json")].into_iter();
    assert!(matches!(
        codec.aggregate_stream(&mut events),
        Err(CodecError::Invalid { .. })
    ));
}

#[test]
fn codec_engine_setting_parses_leniently() {
    assert_eq!(CodecEngine::default(), CodecEngine::Legacy);
    for (raw, expected) in [
        ("legacy", CodecEngine::Legacy),
        ("ir", CodecEngine::Ir),
        (" IR ", CodecEngine::Ir),
        ("something-new", CodecEngine::Legacy),
    ] {
        let parsed: CodecEngine = serde_json::from_value(json!(raw)).unwrap();
        assert_eq!(parsed, expected, "{raw}");
    }
    for engine in [CodecEngine::Legacy, CodecEngine::Ir] {
        assert_eq!(
            serde_json::to_value(engine).unwrap(),
            json!(engine.as_str())
        );
    }
}

#[test]
fn legacy_engine_and_unmigrated_bridges_keep_the_old_converters() {
    assert!(CodecEngine::Ir.uses_ir(true));
    assert!(!CodecEngine::Ir.uses_ir(false));
    assert!(!CodecEngine::Legacy.uses_ir(true));
    assert!(!CodecEngine::Legacy.uses_ir(false));
}

proptest! {
    /// 文本回复在该格式内无损：完整回复 → SSE → 聚合 → 完整回复
    #[test]
    fn text_response_round_trips_through_sse(
        texts in proptest::collection::vec("\\PC{0,16}", 0..5),
        stop in proptest::option::of(prop_oneof![
            Just(StopReason::EndTurn),
            Just(StopReason::MaxTokens),
            Just(StopReason::Refusal),
        ]),
    ) {
        let codec = TextCodec;
        let texts: Vec<&str> = texts.iter().map(String::as_str).collect();
        let response = text_response(&texts, stop);
        let frames = codec.encode_response_as_stream(&response).unwrap();
        prop_assert_eq!(reaggregate(&codec, &frames), Ok(response.clone()));
        let body = codec.encode_response(&response).unwrap();
        prop_assert_eq!(codec.decode_response(&body), Ok(response));
    }
}

#[test]
fn settings_codec_engine_defaults_to_legacy_and_tolerates_unknown_values() {
    use crate::settings::AppSettings;

    let settings: AppSettings = serde_json::from_value(json!({})).unwrap();
    assert_eq!(settings.codec_engine, CodecEngine::Legacy);
    // 默认值不写出，旧版本读取新文件不受影响
    let saved = serde_json::to_value(&settings).unwrap();
    assert!(saved.get("codecEngine").is_none());

    let settings: AppSettings = serde_json::from_value(json!({"codecEngine": "ir"})).unwrap();
    assert_eq!(settings.codec_engine, CodecEngine::Ir);
    assert_eq!(
        serde_json::to_value(&settings).unwrap()["codecEngine"],
        json!("ir")
    );

    let settings: AppSettings =
        serde_json::from_value(json!({"codecEngine": "v2", "language": "en"})).unwrap();
    assert_eq!(settings.codec_engine, CodecEngine::Legacy);
    assert_eq!(settings.language.as_deref(), Some("en"));

    // 非字符串取值同样不影响其余设置
    for raw in [json!(null), json!(1), json!({"engine": "ir"}), json!(true)] {
        let settings: AppSettings =
            serde_json::from_value(json!({"codecEngine": raw, "language": "en"})).unwrap();
        assert_eq!(settings.codec_engine, CodecEngine::Legacy, "{raw}");
        assert_eq!(settings.language.as_deref(), Some("en"));
    }
}
