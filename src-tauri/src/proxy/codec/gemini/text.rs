//! Gemini 可见文本的提取与增量化
//!
//! `streamGenerateContent?alt=sse` 的 `text` 在原生 Gemini 中是增量，部分中转站却发
//! 累计快照；[`GeminiTextDeltaTracker`] 按整个流判定模式后输出增量。

use serde_json::Value;

pub(crate) fn extract_visible_text(parts: &[Value]) -> String {
    parts
        .iter()
        .filter(|part| part.get("thought").and_then(|value| value.as_bool()) != Some(true))
        .filter_map(|part| part.get("text").and_then(|value| value.as_str()))
        .collect::<String>()
}

pub(crate) fn extract_text_thought_signature(parts: &[Value]) -> Option<String> {
    parts
        .iter()
        .filter(|part| part.get("text").is_some() && part.get("functionCall").is_none())
        .filter_map(|part| {
            part.get("thoughtSignature")
                .or_else(|| part.get("thought_signature"))
                .and_then(|value| value.as_str())
        })
        .next_back()
        .map(ToString::to_string)
}

/// How a Gemini stream reports `text` parts across chunks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GeminiTextMode {
    /// Each chunk carries only the newly generated text (native Gemini).
    Incremental,
    /// Each chunk carries the full text generated so far (some relays).
    Cumulative,
}

/// Turns per-chunk Gemini visible text into Anthropic text deltas.
///
/// The mode is decided once per stream instead of per chunk: classifying
/// every chunk independently drops text whenever an incremental chunk
/// happens to start with all prior text (`"ha"`, `"haha"`) and swallows
/// identical incremental chunks entirely.
///
/// - A chunk that does not start with the text seen so far is only possible
///   in incremental mode, so it locks `Incremental`.
/// - A chunk that does start with it (a longer or identical snapshot) is
///   ambiguous and is held back. If the next chunk starts with that held
///   snapshot too, the stream is locked as `Cumulative`; otherwise it is
///   `Incremental` and the held chunk is released verbatim.
/// - A chunk still held when the stream ends is resolved as a cumulative
///   snapshot (relays commonly repeat the final snapshot with
///   `finishReason`).
///
/// Once `Incremental` is locked, chunks that repeat prior text are appended
/// verbatim.
#[derive(Debug, Default)]
pub(crate) struct GeminiTextDeltaTracker {
    mode: Option<GeminiTextMode>,
    emitted: String,
    pending: Option<String>,
}

impl GeminiTextDeltaTracker {
    /// Feeds the visible text of one chunk and returns the delta to emit now.
    pub(crate) fn push(&mut self, chunk: String) -> String {
        if chunk.is_empty() {
            return String::new();
        }
        match self.mode {
            Some(GeminiTextMode::Incremental) => self.append(chunk),
            Some(GeminiTextMode::Cumulative) => {
                if chunk.starts_with(&self.emitted) {
                    let delta = chunk[self.emitted.len()..].to_string();
                    self.emitted = chunk;
                    delta
                } else {
                    // Snapshot diverged from what was already sent; the
                    // emitted text cannot be retracted, so append it as-is.
                    self.append(chunk)
                }
            }
            None => {
                if let Some(pending) = self.pending.take() {
                    if chunk.starts_with(&pending) {
                        self.mode = Some(GeminiTextMode::Cumulative);
                        let delta = chunk[self.emitted.len()..].to_string();
                        self.emitted = chunk;
                        delta
                    } else {
                        self.mode = Some(GeminiTextMode::Incremental);
                        let mut delta = self.append(pending);
                        delta.push_str(&self.append(chunk));
                        delta
                    }
                } else if self.emitted.is_empty() {
                    self.append(chunk)
                } else if chunk.starts_with(&self.emitted) {
                    self.pending = Some(chunk);
                    String::new()
                } else {
                    self.mode = Some(GeminiTextMode::Incremental);
                    self.append(chunk)
                }
            }
        }
    }

    /// Releases any held chunk at end of stream and returns its delta.
    pub(crate) fn finish(&mut self) -> String {
        match self.pending.take() {
            Some(pending) => {
                let delta = pending[self.emitted.len()..].to_string();
                self.emitted = pending;
                delta
            }
            None => String::new(),
        }
    }

    /// Full text emitted so far.
    pub(crate) fn text(&self) -> &str {
        &self.emitted
    }

    fn append(&mut self, chunk: String) -> String {
        self.emitted.push_str(&chunk);
        chunk
    }
}
