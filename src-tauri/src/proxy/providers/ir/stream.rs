//! Canonical streaming IR (PROV-18 / PROV-19).
//!
//! A stream is `MessageStart, (BlockStart, BlockDelta*, BlockStop)*,
//! MessageDelta, MessageStop`, or an `Error` at any point. Two pure helpers
//! connect it with [`Response`]:
//!
//! * [`Collector`] folds events into a response ("stream then aggregate"), used
//!   by the SSE-body sniffing fallbacks;
//! * [`events_from_response`] expands a response into the event lifecycle
//!   ("JSON then stream"), used when a client asked for a stream but the
//!   upstream answered with plain JSON.
//!
//! Protocol codecs translate these events to and from their wire framing.

use super::{Part, Response, StopReason, ToolInput, Usage};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Kind of a content block at `BlockStart`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BlockStart {
    Text,
    Thinking {
        signature: Option<String>,
    },
    RedactedThinking {
        data: String,
    },
    ToolUse {
        id: String,
        name: String,
        /// Input announced at start (Anthropic sends `{}` and streams the rest).
        input: Option<Value>,
    },
    /// A block the IR does not model, carried verbatim.
    Other(Value),
}

/// Incremental content for an open block.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Delta {
    Text(String),
    Thinking(String),
    Signature(String),
    InputJson(String),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum StreamEvent {
    MessageStart {
        id: Option<String>,
        model: String,
        usage: Usage,
    },
    BlockStart {
        index: u64,
        block: BlockStart,
    },
    BlockDelta {
        index: u64,
        delta: Delta,
    },
    BlockStop {
        index: u64,
    },
    MessageDelta {
        stop: Option<StopReason>,
        /// Full final usage, when the protocol reports it at the end.
        usage: Option<Usage>,
        /// Cumulative output tokens only (Anthropic `message_delta.usage`).
        output_tokens: Option<u64>,
    },
    MessageStop,
    Error {
        error_type: String,
        message: String,
    },
}

/// Why [`Collector::finish`] could not produce a response.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CollectError {
    /// The stream carried an upstream error event.
    Upstream { error_type: String, message: String },
    /// No `MessageStart` was seen.
    MissingStart,
    /// The stream ended before any terminal event and produced no content.
    EndedEarly,
}

/// Folds canonical stream events into a [`Response`].
///
/// Tolerances (kept from the original Anthropic SSE aggregator): deltas for a
/// block that never started open a block of the delta's kind; tool-input JSON
/// that fails to parse at `BlockStop` becomes `{}`; a stream that ends without
/// `MessageStop`/stop reason but with content is reported as `max_tokens` so
/// truncation stays visible.
#[derive(Debug, Default)]
pub(crate) struct Collector {
    started: Option<(Option<String>, String, Usage)>,
    blocks: BTreeMap<u64, Part>,
    json_accum: BTreeMap<u64, String>,
    stop: Option<StopReason>,
    final_usage: Option<Usage>,
    output_tokens: Option<u64>,
    saw_message_stop: bool,
    error: Option<CollectError>,
}

impl Collector {
    pub(crate) fn push(&mut self, event: StreamEvent) {
        if self.error.is_some() {
            return;
        }
        match event {
            StreamEvent::MessageStart { id, model, usage } => {
                self.started = Some((id, model, usage));
            }
            StreamEvent::BlockStart { index, block } => {
                let part = match block {
                    BlockStart::Text => Part::Text(String::new()),
                    BlockStart::Thinking { signature } => Part::Thinking {
                        text: String::new(),
                        signature,
                    },
                    BlockStart::RedactedThinking { data } => Part::RedactedThinking { data },
                    BlockStart::ToolUse { id, name, input } => Part::ToolUse {
                        id,
                        name,
                        input: ToolInput::Json(input.unwrap_or_else(|| json!({}))),
                    },
                    BlockStart::Other(value) => Part::Other(value),
                };
                self.blocks.insert(index, part);
                self.json_accum.entry(index).or_default();
            }
            StreamEvent::BlockDelta { index, delta } => match delta {
                Delta::Text(text) => match self
                    .blocks
                    .entry(index)
                    .or_insert_with(|| Part::Text(String::new()))
                {
                    Part::Text(existing) | Part::Refusal(existing) => existing.push_str(&text),
                    _ => {}
                },
                Delta::Thinking(text) => {
                    if let Part::Thinking { text: existing, .. } =
                        self.blocks.entry(index).or_insert_with(|| Part::Thinking {
                            text: String::new(),
                            signature: None,
                        })
                    {
                        existing.push_str(&text);
                    }
                }
                Delta::Signature(sig) => {
                    if let Some(Part::Thinking { signature, .. }) = self.blocks.get_mut(&index) {
                        *signature = Some(sig);
                    }
                }
                Delta::InputJson(partial) => {
                    self.json_accum.entry(index).or_default().push_str(&partial);
                }
            },
            StreamEvent::BlockStop { index } => {
                let Some(accum) = self.json_accum.get(&index) else {
                    return;
                };
                if accum.trim().is_empty() {
                    return;
                }
                let parsed: Value = serde_json::from_str(accum).unwrap_or_else(|_| json!({}));
                if let Some(Part::ToolUse { input, .. }) = self.blocks.get_mut(&index) {
                    *input = ToolInput::Json(parsed);
                }
            }
            StreamEvent::MessageDelta {
                stop,
                usage,
                output_tokens,
            } => {
                if stop.is_some() {
                    self.stop = stop;
                }
                if usage.is_some() {
                    self.final_usage = usage;
                }
                if output_tokens.is_some() {
                    self.output_tokens = output_tokens;
                }
            }
            StreamEvent::MessageStop => self.saw_message_stop = true,
            StreamEvent::Error {
                error_type,
                message,
            } => {
                self.error = Some(CollectError::Upstream {
                    error_type,
                    message,
                });
            }
        }
    }

    pub(crate) fn finish(self) -> Result<Response, CollectError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        let (id, model, start_usage) = self.started.ok_or(CollectError::MissingStart)?;
        let mut stop = self.stop;
        if !self.saw_message_stop && stop.is_none() {
            if self.blocks.is_empty() {
                return Err(CollectError::EndedEarly);
            }
            stop = Some(StopReason::MaxTokens);
        }
        let mut usage = self.final_usage.unwrap_or(start_usage);
        if let Some(output) = self.output_tokens {
            // Anthropic `message_delta.usage.output_tokens` is cumulative and
            // overrides the 0 announced at `message_start`.
            usage.output = output;
        }
        Ok(Response {
            id,
            model,
            created: None,
            content: self.blocks.into_values().collect(),
            stop,
            usage,
        })
    }
}

/// Expand a complete response into the canonical event lifecycle.
///
/// Block indexes follow content positions; parts that have no stream
/// representation ([`Part::Other`]) emit nothing but keep their index. Tool
/// inputs must already be resolved to JSON ([`ToolInput::Json`]); raw inputs are
/// streamed as-is.
pub(crate) fn events_from_response(response: &Response) -> Vec<StreamEvent> {
    let mut events = vec![StreamEvent::MessageStart {
        id: response.id.clone(),
        model: response.model.clone(),
        usage: response.usage.clone(),
    }];
    for (index, part) in response.content.iter().enumerate() {
        let index = index as u64;
        let (start, deltas) = match part {
            Part::Text(text) | Part::Refusal(text) => (
                BlockStart::Text,
                (!text.is_empty())
                    .then(|| Delta::Text(text.clone()))
                    .into_iter()
                    .collect::<Vec<_>>(),
            ),
            Part::Thinking { text, signature } => {
                let mut deltas = Vec::new();
                if !text.is_empty() {
                    deltas.push(Delta::Thinking(text.clone()));
                }
                if let Some(signature) = signature.as_ref().filter(|s| !s.is_empty()) {
                    deltas.push(Delta::Signature(signature.clone()));
                }
                (
                    BlockStart::Thinking {
                        signature: signature.clone(),
                    },
                    deltas,
                )
            }
            Part::RedactedThinking { data } => (
                BlockStart::RedactedThinking { data: data.clone() },
                Vec::new(),
            ),
            Part::ToolUse { id, name, input } => {
                let partial = match input {
                    ToolInput::Json(value) => {
                        serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
                    }
                    ToolInput::Raw(Some(Value::String(raw))) => raw.clone(),
                    ToolInput::Raw(Some(value)) => {
                        serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
                    }
                    ToolInput::Raw(None) => "{}".to_string(),
                };
                (
                    BlockStart::ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        input: None,
                    },
                    vec![Delta::InputJson(partial)],
                )
            }
            Part::Other(_) => continue,
        };
        events.push(StreamEvent::BlockStart {
            index,
            block: start,
        });
        events.extend(
            deltas
                .into_iter()
                .map(|delta| StreamEvent::BlockDelta { index, delta }),
        );
        events.push(StreamEvent::BlockStop { index });
    }
    events.push(StreamEvent::MessageDelta {
        stop: response.stop.clone(),
        usage: Some(response.usage.clone()),
        output_tokens: None,
    });
    events.push(StreamEvent::MessageStop);
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Response {
        Response {
            id: Some("msg_1".into()),
            model: "m".into(),
            created: None,
            content: vec![
                Part::Thinking {
                    text: "think".into(),
                    signature: Some("sig".into()),
                },
                Part::Text("hello".into()),
                Part::ToolUse {
                    id: "call_1".into(),
                    name: "Read".into(),
                    input: ToolInput::Json(json!({"path": "a"})),
                },
            ],
            stop: Some(StopReason::ToolUse),
            usage: Usage {
                input: 3,
                output: 7,
                cache_read: Some(2),
                ..Usage::default()
            },
        }
    }

    /// `collect(events_from_response(x)) == x`: the stream expansion and the
    /// aggregator are inverse (PROV-18 equivalence).
    #[test]
    fn collect_inverts_events_from_response() {
        let response = sample();
        let mut collector = Collector::default();
        for event in events_from_response(&response) {
            collector.push(event);
        }
        assert_eq!(collector.finish().unwrap(), response);
    }

    #[test]
    fn truncated_stream_with_content_reports_max_tokens() {
        let mut collector = Collector::default();
        collector.push(StreamEvent::MessageStart {
            id: None,
            model: String::new(),
            usage: Usage::default(),
        });
        collector.push(StreamEvent::BlockStart {
            index: 0,
            block: BlockStart::Text,
        });
        collector.push(StreamEvent::BlockDelta {
            index: 0,
            delta: Delta::Text("partial".into()),
        });
        let response = collector.finish().unwrap();
        assert_eq!(response.stop, Some(StopReason::MaxTokens));
        assert_eq!(response.content, vec![Part::Text("partial".into())]);
    }

    #[test]
    fn empty_stream_without_terminal_event_is_an_error() {
        let mut collector = Collector::default();
        collector.push(StreamEvent::MessageStart {
            id: None,
            model: String::new(),
            usage: Usage::default(),
        });
        assert_eq!(collector.finish(), Err(CollectError::EndedEarly));
        assert_eq!(
            Collector::default().finish(),
            Err(CollectError::MissingStart)
        );
    }
}
