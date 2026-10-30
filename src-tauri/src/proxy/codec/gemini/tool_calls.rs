//! Gemini functionCall 的共用处理：合成 id、按客户端工具 schema 纠正参数、
//! 流式累计快照的合并与 shadow 回放所需的元数据
//!
//! 旧转换器（`providers::transform_gemini` / `streaming_gemini`）与 Gemini 编解码
//! 共用这里的实现，删除旧转换器时无需再搬动。

use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};

use crate::proxy::providers::gemini_shadow::GeminiToolCallMeta;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnthropicToolSchemaHint {
    expected_keys: Vec<String>,
    required_keys: Vec<String>,
}

pub type AnthropicToolSchemaHints = HashMap<String, AnthropicToolSchemaHint>;

/// Prefix used for Anthropic-visible tool call ids that we synthesize when
/// Gemini's `functionCall` omits the `id` field (Gemini 2.x parallel calls
/// often do). The prefix is how downstream request-path code recognizes that
/// the id is not a real Gemini id and must be stripped before forwarding back
/// to Gemini as `functionResponse.id`.
pub(crate) const SYNTHESIZED_ID_PREFIX: &str = "gemini_synth_";

/// Generate a unique tool-call id that is safe to expose to Anthropic clients
/// but must not be sent upstream to Gemini. Uses UUID v4 simple encoding
/// (32 lowercase hex chars) so that any number of parallel calls in the same
/// response remain distinguishable.
pub(crate) fn synthesize_tool_call_id() -> String {
    format!("{SYNTHESIZED_ID_PREFIX}{}", uuid::Uuid::new_v4().simple())
}

/// Returns true if `id` was produced by [`synthesize_tool_call_id`] and
/// therefore must be stripped when building Gemini request bodies.
pub(crate) fn is_synthesized_tool_call_id(id: &str) -> bool {
    id.starts_with(SYNTHESIZED_ID_PREFIX)
}

pub(crate) fn shadow_parts(content: &Value) -> Option<Vec<Value>> {
    let mut parts = content
        .get("parts")
        .and_then(|value| value.as_array())
        .cloned()
        .or_else(|| content.as_array().cloned())?;
    // Strip synthesized ids before these parts are replayed into a Gemini
    // request body. The shadow store records the Anthropic-facing id so that
    // a tool_result round-trip can find the tool's name, but sending the
    // synthetic value as `functionCall.id` upstream would leak an internal
    // identifier.
    for part in &mut parts {
        let Some(function_call) = part.get_mut("functionCall").and_then(|v| v.as_object_mut())
        else {
            continue;
        };
        let drop_id = function_call
            .get("id")
            .and_then(|v| v.as_str())
            .map(|id| id.is_empty() || is_synthesized_tool_call_id(id))
            .unwrap_or(true);
        if drop_id {
            function_call.remove("id");
        }
    }
    Some(parts)
}

pub fn extract_anthropic_tool_schema_hints(body: &Value) -> AnthropicToolSchemaHints {
    body.get("tools")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|tool| {
            let name = tool.get("name").and_then(|value| value.as_str())?;
            let input_schema = tool
                .get("input_schema")
                .and_then(|value| value.as_object())?;
            let properties = input_schema
                .get("properties")
                .and_then(|value| value.as_object())?;
            if properties.is_empty() {
                return None;
            }

            let expected_keys = properties.keys().cloned().collect::<Vec<_>>();
            let required_keys = input_schema
                .get("required")
                .and_then(|value| value.as_array())
                .map(|values| {
                    values
                        .iter()
                        .filter_map(|value| value.as_str().map(ToString::to_string))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            Some((
                name.to_string(),
                AnthropicToolSchemaHint {
                    expected_keys,
                    required_keys,
                },
            ))
        })
        .collect()
}

pub fn rectify_tool_call_parts(
    parts: &mut [Value],
    tool_schema_hints: Option<&AnthropicToolSchemaHints>,
) {
    for part in parts {
        let Some(function_call) = part
            .get_mut("functionCall")
            .and_then(|value| value.as_object_mut())
        else {
            continue;
        };
        let Some(name) = function_call
            .get("name")
            .and_then(|value| value.as_str())
            .map(ToString::to_string)
        else {
            continue;
        };
        let Some(args) = function_call.get_mut("args") else {
            continue;
        };

        if rectify_tool_call_args(&name, args, tool_schema_hints) {
            log::info!("[Claude/Gemini] Rectified tool args for `{name}`");
        }
    }
}

pub fn rectify_tool_call_args(
    tool_name: &str,
    args: &mut Value,
    tool_schema_hints: Option<&AnthropicToolSchemaHints>,
) -> bool {
    let Some(tool_schema_hints) = tool_schema_hints else {
        return false;
    };
    let Some(hint) = tool_schema_hints.get(tool_name) else {
        return false;
    };
    let Some(args_object) = args.as_object_mut() else {
        return false;
    };
    if args_object.is_empty() || hint.expected_keys.is_empty() {
        return false;
    }
    let mut changed = false;

    if hint.expected_keys.iter().any(|key| key == "skill") && !args_object.contains_key("skill") {
        if let Some(value) = args_object.remove("name") {
            args_object.insert("skill".to_string(), value);
            changed = true;
        }
    }

    let expects_parameters_key = hint.expected_keys.iter().any(|key| key == "parameters");
    if !expects_parameters_key {
        let extracted_parameters = args_object
            .get("parameters")
            .and_then(|value| value.as_object())
            .map(|parameters_object| {
                hint.expected_keys
                    .iter()
                    .filter_map(|expected_key| {
                        if args_object.contains_key(expected_key) {
                            return None;
                        }
                        let value = parameters_object.get(expected_key)?;
                        let normalized_value = match value {
                            Value::Array(values) if values.len() == 1 => values[0].clone(),
                            _ => value.clone(),
                        };
                        Some((expected_key.clone(), normalized_value))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        if !extracted_parameters.is_empty() {
            for (expected_key, normalized_value) in extracted_parameters {
                args_object.insert(expected_key, normalized_value);
            }
            args_object.remove("parameters");
            changed = true;
        }
    }

    if hint
        .required_keys
        .iter()
        .all(|key| args_object.contains_key(key.as_str()))
    {
        return changed;
    }

    let expected_key_set = hint
        .expected_keys
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let unexpected_keys = args_object
        .keys()
        .filter(|key| !expected_key_set.contains(key.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if unexpected_keys.len() != 1 {
        return false;
    }

    let target_key = hint
        .required_keys
        .iter()
        .find(|key| !args_object.contains_key(key.as_str()))
        .cloned()
        .or_else(|| {
            if hint.expected_keys.len() == 1 && args_object.len() == 1 {
                hint.expected_keys.first().cloned()
            } else {
                None
            }
        });
    let Some(target_key) = target_key else {
        return false;
    };
    if args_object.contains_key(&target_key) {
        return false;
    }

    let source_key = &unexpected_keys[0];
    let Some(value) = args_object.remove(source_key) else {
        return false;
    };
    args_object.insert(target_key, value);
    true
}

pub(crate) fn extract_tool_call_meta(parts: &[Value]) -> Vec<GeminiToolCallMeta> {
    parts
        .iter()
        .filter_map(|part| {
            let function_call = part.get("functionCall")?;
            // Ensure every surfaced tool call carries a distinguishing id.
            // Gemini 2.x may omit `id` on parallel calls; synthesizing a
            // unique replacement prevents downstream merge/replay logic from
            // collapsing distinct calls onto a single empty-string key.
            let id = function_call
                .get("id")
                .and_then(|value| value.as_str())
                .filter(|s| !s.is_empty())
                .map(ToString::to_string)
                .unwrap_or_else(synthesize_tool_call_id);
            Some(GeminiToolCallMeta::new(
                Some(id),
                function_call
                    .get("name")
                    .and_then(|value| value.as_str())
                    .unwrap_or(""),
                function_call
                    .get("args")
                    .cloned()
                    .unwrap_or_else(|| json!({})),
                part.get("thoughtSignature")
                    .or_else(|| part.get("thought_signature"))
                    .and_then(|value| value.as_str()),
            ))
        })
        .collect()
}

pub(crate) fn extract_tool_calls(
    parts: &[Value],
    tool_schema_hints: Option<&AnthropicToolSchemaHints>,
) -> Vec<GeminiToolCallMeta> {
    let mut rectified_parts = parts.to_vec();
    rectify_tool_call_parts(&mut rectified_parts, tool_schema_hints);

    rectified_parts
        .iter()
        .filter_map(|part| {
            let function_call = part.get("functionCall")?;
            // Treat an explicit empty-string id as equivalent to a missing
            // one. Some Gemini relays serialize absent ids as `"id": ""`;
            // without this filter the `Some("")` value would flow into
            // `merge_tool_call_snapshots`, match itself across chunks, and
            // collapse parallel no-id calls into a single snapshot with an
            // empty-string tool_use id.
            let id = function_call
                .get("id")
                .and_then(|value| value.as_str())
                .filter(|s| !s.is_empty())
                .map(ToString::to_string);
            Some(GeminiToolCallMeta::new(
                id,
                function_call
                    .get("name")
                    .and_then(|value| value.as_str())
                    .unwrap_or(""),
                function_call
                    .get("args")
                    .cloned()
                    .unwrap_or_else(|| json!({})),
                part.get("thoughtSignature")
                    .or_else(|| part.get("thought_signature"))
                    .and_then(|value| value.as_str()),
            ))
        })
        .collect()
}

pub(crate) fn merge_tool_call_snapshots(
    tool_call_snapshots: &mut Vec<GeminiToolCallMeta>,
    incoming: Vec<GeminiToolCallMeta>,
) {
    // Gemini's `streamGenerateContent?alt=sse` delivers each chunk as the
    // cumulative snapshot of `content.parts`. For the same tool call across
    // chunks we therefore need to map an incoming entry back to whichever
    // snapshot entry it describes:
    //
    // 1. If both sides carry a genuine Gemini id, match by id.
    // 2. Otherwise match by position in the cumulative `parts` array — this
    //    is how parallel no-id calls stay distinguishable.
    //
    // A previous implementation fell back to matching by `name`, which silently
    // merged two parallel calls to the same function into one entry (losing
    // the first call's args). That fallback is removed here.
    for (position, mut tool_call) in incoming.into_iter().enumerate() {
        // Treat an empty-string id as "missing" throughout this function.
        // `extract_tool_calls` already filters `""` at the source, but upstream
        // callers that build `GeminiToolCallMeta` by hand (tests, future code)
        // could still send `Some("")` — collapsing it here keeps the invariant
        // local to this merge step.
        if tool_call.id.as_deref() == Some("") {
            tool_call.id = None;
        }

        let existing_index = match tool_call.id.as_deref() {
            Some(incoming_id) => tool_call_snapshots
                .iter()
                .position(|existing| existing.id.as_deref() == Some(incoming_id))
                .or_else(|| {
                    // Fallback for the "synth -> real id upgrade" case:
                    // Gemini's cumulative stream may deliver the first chunk
                    // of a tool call without an id (we synthesize one) and
                    // then upgrade it to a genuine id on a later chunk. A
                    // pure id-match would miss the existing synthesized
                    // snapshot and push a second entry, yielding duplicate
                    // `tool_use` content blocks at stream end. If the
                    // same-position slot currently holds a synthesized id,
                    // merge into it — `or(preserved_id)` below will keep
                    // the real id, dropping the synthesized one.
                    tool_call_snapshots
                        .get(position)
                        .filter(|existing| {
                            matches!(
                                existing.id.as_deref(),
                                Some(id) if is_synthesized_tool_call_id(id)
                            )
                        })
                        .map(|_| position)
                }),
            None => tool_call_snapshots
                .get(position)
                .filter(|existing| match existing.id.as_deref() {
                    // Only merge into a positional match when the prior
                    // snapshot was itself id-less (or we synthesized one).
                    // A snapshot with a genuine Gemini id at this index is
                    // treated as a different call — the incoming entry gets
                    // its own synthesized id below.
                    Some(id) => is_synthesized_tool_call_id(id),
                    None => true,
                })
                .map(|_| position),
        };

        if let Some(index) = existing_index {
            // Preserve any synthesized id assigned on a previous chunk so the
            // Anthropic-visible id stays stable across the whole stream.
            // When incoming carries a real Gemini id and the slot holds a
            // synthesized one, `Some(real).or(Some(synth)) == Some(real)`
            // so the upgrade wins naturally.
            let preserved_id = tool_call_snapshots[index].id.clone();
            tool_call.id = tool_call.id.or(preserved_id);

            // Preserve `thought_signature` across chunks. Gemini's cumulative
            // stream may include `thoughtSignature` on one chunk and omit it
            // on a subsequent cumulative snapshot of the same part, even
            // though the signature still belongs to the call. A blind
            // overwrite would drop it, so the shadow turn we record (and
            // later replay) would be missing `thoughtSignature` and the
            // upstream would reject the follow-up for invalid signature.
            if tool_call.thought_signature.is_none() {
                tool_call
                    .thought_signature
                    .clone_from(&tool_call_snapshots[index].thought_signature);
            }
        }
        if tool_call.id.is_none() {
            tool_call.id = Some(synthesize_tool_call_id());
        }

        match existing_index {
            Some(index) => tool_call_snapshots[index] = tool_call,
            None => tool_call_snapshots.push(tool_call),
        }
    }
}

pub(crate) fn build_shadow_assistant_parts(
    text: Option<&str>,
    text_thought_signature: Option<&str>,
    tool_calls: &[GeminiToolCallMeta],
) -> Vec<Value> {
    let mut parts = Vec::new();

    if text.filter(|text| !text.is_empty()).is_some() || text_thought_signature.is_some() {
        let mut part = json!({
            "text": text.unwrap_or("")
        });
        if let Some(signature) = text_thought_signature {
            part["thoughtSignature"] = json!(signature);
        }
        parts.push(part);
    }

    for tool_call in tool_calls {
        let mut part = json!({
            "functionCall": {
                "id": tool_call.id.clone().unwrap_or_default(),
                "name": tool_call.name,
                "args": tool_call.args
            }
        });

        if let Some(signature) = &tool_call.thought_signature {
            part["thoughtSignature"] = json!(signature);
        }

        parts.push(part);
    }

    parts
}
