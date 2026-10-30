//! Incremental Server-Sent Events decoder.
//!
//! One implementation of SSE framing for every place that reads an upstream
//! event stream, following the WHATWG `text/event-stream` parsing rules:
//!
//! - lines end with `\r\n`, `\n` or a lone `\r` (a `\r` at the end of a chunk
//!   waits for the next chunk to decide whether a `\n` follows);
//! - a blank line dispatches the pending event; an event is only dispatched when
//!   it carried at least one `data` line;
//! - multiple `data` lines are joined with `\n`;
//! - lines starting with `:` are comments and are ignored;
//! - a single space after the field colon is stripped; a field without a colon
//!   has an empty value;
//! - a UTF-8 BOM at the very start of the stream is ignored;
//! - multi-byte UTF-8 characters split across chunks are reassembled.
//!
//! Two deliberate leniencies keep the behaviour the proxy already had for
//! non-conforming gateways: leading spaces/tabs before a field name are
//! ignored (`  data: {}` is a data line), and the `event` value is trimmed.
//!
//! The decoder's output depends only on the byte stream, never on how it was
//! chunked; a property test below checks this.

use crate::proxy::sse::append_utf8_safe;

/// One dispatched SSE event.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SseEvent {
    /// Trimmed `event:` value; `None` when absent or empty.
    pub event: Option<String>,
    /// `data:` lines joined with `\n`.
    pub data: String,
    /// `id:` value seen in this event, if any.
    pub id: Option<String>,
    /// `retry:` value seen in this event, if it was a valid integer.
    pub retry: Option<u64>,
}

impl SseEvent {
    /// Event name, or `""` when the event had no (non-empty) `event:` line.
    pub fn event_name(&self) -> &str {
        self.event.as_deref().unwrap_or("")
    }
}

#[derive(Debug, Default)]
pub(crate) struct SseDecoder {
    /// Decoded text of the current, not yet terminated line.
    line: String,
    /// Trailing bytes of an incomplete UTF-8 sequence.
    utf8_remainder: Vec<u8>,
    /// The previous line ended with `\r`; a leading `\n` in the next chunk
    /// belongs to the same line terminator.
    pending_cr: bool,
    /// Whether the start of the stream has been checked for a BOM.
    bom_checked: bool,
    event: Option<String>,
    data: Option<String>,
    id: Option<String>,
    retry: Option<u64>,
}

impl SseDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed raw upstream bytes and return the events completed by them.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<SseEvent> {
        let start = self.line.len();
        append_utf8_safe(&mut self.line, &mut self.utf8_remainder, bytes);
        self.drain_lines(start)
    }

    /// Feed already decoded text and return the events completed by it.
    pub fn feed_str(&mut self, text: &str) -> Vec<SseEvent> {
        if !self.utf8_remainder.is_empty() {
            let remainder = std::mem::take(&mut self.utf8_remainder);
            self.line.push_str(&String::from_utf8_lossy(&remainder));
        }
        let start = self.line.len();
        self.line.push_str(text);
        self.drain_lines(start)
    }

    /// Signal end of stream: an unterminated final line and an event that was
    /// not followed by a blank line are still dispatched. The decoder is reset
    /// afterwards.
    pub fn finish(&mut self) -> Option<SseEvent> {
        if !self.utf8_remainder.is_empty() {
            let remainder = std::mem::take(&mut self.utf8_remainder);
            self.line.push_str(&String::from_utf8_lossy(&remainder));
        }
        self.strip_bom();
        let line = std::mem::take(&mut self.line);
        let mut event = None;
        if !line.is_empty() {
            event = self.process_line(&line);
        }
        let event = event.or_else(|| self.dispatch());
        *self = Self::default();
        event
    }

    fn strip_bom(&mut self) {
        if self.bom_checked || self.line.is_empty() {
            return;
        }
        if self.line.starts_with('\u{feff}') {
            self.line.drain(..'\u{feff}'.len_utf8());
        }
        self.bom_checked = true;
    }

    /// Split complete lines off `self.line`. `start` is where newly appended
    /// text begins; everything before it is known to contain no terminator.
    fn drain_lines(&mut self, start: usize) -> Vec<SseEvent> {
        let mut events = Vec::new();
        let had_bom_check = self.bom_checked;
        self.strip_bom();
        if !self.bom_checked {
            return events;
        }
        let mut search_from = if had_bom_check { start } else { 0 };

        let mut consumed = 0;
        if self.pending_cr && search_from < self.line.len() {
            self.pending_cr = false;
            if self.line.as_bytes()[search_from] == b'\n' {
                consumed = search_from + 1;
                search_from = consumed;
            }
        }

        let line = std::mem::take(&mut self.line);
        let bytes = line.as_bytes();
        let mut pos = search_from;
        while let Some(offset) = bytes[pos..].iter().position(|b| *b == b'\r' || *b == b'\n') {
            let end = pos + offset;
            let mut next = end + 1;
            if bytes[end] == b'\r' {
                if next < bytes.len() {
                    if bytes[next] == b'\n' {
                        next += 1;
                    }
                } else {
                    self.pending_cr = true;
                }
            }
            if let Some(event) = self.process_line(&line[consumed..end]) {
                events.push(event);
            }
            consumed = next;
            pos = next;
        }
        self.line = line;
        self.line.drain(..consumed);
        events
    }

    fn process_line(&mut self, line: &str) -> Option<SseEvent> {
        if line.is_empty() {
            return self.dispatch();
        }
        let line = line.trim_start_matches([' ', '\t']);
        if line.is_empty() || line.starts_with(':') {
            return None;
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };
        match field {
            "data" => match self.data.as_mut() {
                Some(data) => {
                    data.push('\n');
                    data.push_str(value);
                }
                None => self.data = Some(value.to_string()),
            },
            "event" => {
                let value = value.trim();
                self.event = (!value.is_empty()).then(|| value.to_string());
            }
            "id" if !value.contains('\0') => self.id = Some(value.to_string()),
            "retry" if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) => {
                self.retry = value.parse().ok();
            }
            _ => {}
        }
        None
    }

    fn dispatch(&mut self) -> Option<SseEvent> {
        let event = self.event.take();
        let id = self.id.take();
        let retry = self.retry.take();
        let data = self.data.take()?;
        Some(SseEvent {
            event,
            data,
            id,
            retry,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{SseDecoder, SseEvent};
    use proptest::prelude::*;

    fn ev(event: Option<&str>, data: &str) -> SseEvent {
        SseEvent {
            event: event.map(str::to_string),
            data: data.to_string(),
            ..SseEvent::default()
        }
    }

    fn decode_all(input: &[u8]) -> Vec<SseEvent> {
        let mut decoder = SseDecoder::new();
        let mut events = decoder.feed(input);
        events.extend(decoder.finish());
        events
    }

    fn decode_chunked(input: &[u8], chunk: usize) -> Vec<SseEvent> {
        let mut decoder = SseDecoder::new();
        let mut events = Vec::new();
        for piece in input.chunks(chunk.max(1)) {
            events.extend(decoder.feed(piece));
        }
        events.extend(decoder.finish());
        events
    }

    #[test]
    fn decodes_lf_crlf_and_cr_line_endings() {
        let expected = vec![ev(Some("a"), "1"), ev(None, "2")];
        for input in [
            "event: a\ndata: 1\n\ndata: 2\n\n",
            "event: a\r\ndata: 1\r\n\r\ndata: 2\r\n\r\n",
            "event: a\rdata: 1\r\rdata: 2\r\r",
            // mixed terminators: LF line followed by a CRLF blank line
            "event: a\ndata: 1\n\r\ndata: 2\r\n\n",
        ] {
            assert_eq!(decode_all(input.as_bytes()), expected, "{input:?}");
            assert_eq!(decode_chunked(input.as_bytes(), 1), expected, "{input:?}");
        }
    }

    #[test]
    fn cr_at_chunk_boundary_does_not_double_count_lf() {
        let mut decoder = SseDecoder::new();
        assert!(decoder.feed(b"data: 1\r").is_empty());
        // The `\n` completes the CRLF of the data line, it is not a blank line.
        assert!(decoder.feed(b"\n").is_empty());
        assert_eq!(decoder.feed(b"\r\n"), vec![ev(None, "1")]);
    }

    #[test]
    fn joins_multi_line_data_and_skips_comments() {
        let input =
            ": keepalive\n\nevent: message\n: inline comment\ndata: line1\ndata:line2\ndata\n\n";
        assert_eq!(
            decode_all(input.as_bytes()),
            vec![ev(Some("message"), "line1\nline2\n")]
        );
    }

    #[test]
    fn events_without_data_are_not_dispatched() {
        let input = "event: ping\n\nretry: 3000\nid: 7\n\ndata: x\n\n";
        assert_eq!(decode_all(input.as_bytes()), vec![ev(None, "x")]);
    }

    #[test]
    fn records_id_and_retry_for_the_event() {
        let input = "id: 42\nretry: 1500\ndata: x\n\ndata: y\n\n";
        let events = decode_all(input.as_bytes());
        assert_eq!(events[0].id.as_deref(), Some("42"));
        assert_eq!(events[0].retry, Some(1500));
        assert_eq!(events[1].id, None);
        assert_eq!(events[1].retry, None);
    }

    #[test]
    fn strips_only_one_space_after_colon() {
        assert_eq!(decode_all(b"data:  two\n\n"), vec![ev(None, " two")]);
    }

    #[test]
    fn tolerates_indented_fields_and_trims_event_name() {
        assert_eq!(
            decode_all(b"  event:  message_start \n\tdata: {}\n\n"),
            vec![ev(Some("message_start"), "{}")]
        );
    }

    #[test]
    fn strips_bom_split_across_chunks() {
        let mut input = "\u{feff}".as_bytes().to_vec();
        input.extend_from_slice(b"data: x\n\n");
        assert_eq!(decode_chunked(&input, 1), vec![ev(None, "x")]);
        // A BOM in the middle of the stream is data, not a marker.
        assert_eq!(
            decode_all("data: \u{feff}x\n\n".as_bytes()),
            vec![ev(None, "\u{feff}x")]
        );
    }

    #[test]
    fn finish_flushes_unterminated_event() {
        let mut decoder = SseDecoder::new();
        assert!(decoder.feed(b"event: done\ndata: {\"a\":1}").is_empty());
        assert_eq!(decoder.finish(), Some(ev(Some("done"), "{\"a\":1}")));
        assert_eq!(decoder.finish(), None);

        let mut decoder = SseDecoder::new();
        assert!(decoder.feed(b"data: tail\r").is_empty());
        assert_eq!(decoder.finish(), Some(ev(None, "tail")));
    }

    #[test]
    fn feed_str_matches_feed() {
        let input = "event: a\r\ndata: 你好\r\n\r\ndata: b";
        let mut decoder = SseDecoder::new();
        let mut events = decoder.feed_str(input);
        events.extend(decoder.finish());
        assert_eq!(events, decode_all(input.as_bytes()));
    }

    #[test]
    fn reassembles_utf8_split_across_chunks() {
        let input = "data: {\"text\":\"你好😀\"}\n\n";
        assert_eq!(
            decode_chunked(input.as_bytes(), 1),
            vec![ev(None, "{\"text\":\"你好😀\"}")]
        );
    }

    // ------------------------------------------------------------------
    // property: output is independent of chunking
    // ------------------------------------------------------------------

    #[derive(Debug, Clone)]
    struct GenEvent {
        comment: Option<String>,
        event: Option<String>,
        id: Option<String>,
        data: Vec<String>,
        no_space: Vec<bool>,
        ending: &'static str,
    }

    fn value_strategy() -> impl Strategy<Value = String> {
        proptest::collection::vec(
            prop_oneof![
                Just('a'),
                Just('Z'),
                Just(' '),
                Just(':'),
                Just('{'),
                Just('"'),
                Just('你'),
                Just('😀'),
                Just('\u{feff}'),
            ],
            0..12,
        )
        .prop_map(|chars| chars.into_iter().collect())
    }

    fn event_strategy() -> impl Strategy<Value = GenEvent> {
        (
            proptest::option::of(value_strategy()),
            proptest::option::of("[a-z][a-z._]{0,10}"),
            proptest::option::of("[0-9a-z]{1,4}"),
            proptest::collection::vec(value_strategy(), 1..4),
            proptest::collection::vec(any::<bool>(), 4),
            prop_oneof![Just("\n"), Just("\r\n"), Just("\r")],
        )
            .prop_map(|(comment, event, id, data, no_space, ending)| GenEvent {
                comment,
                event,
                id,
                data,
                no_space,
                ending,
            })
    }

    fn render(events: &[GenEvent], bom: bool, terminate_last: bool) -> (String, Vec<SseEvent>) {
        let mut out = String::new();
        if bom {
            out.push('\u{feff}');
        }
        let mut expected = Vec::new();
        for (i, gen) in events.iter().enumerate() {
            let nl = gen.ending;
            if let Some(comment) = &gen.comment {
                out.push_str(&format!(":{comment}{nl}"));
            }
            if let Some(event) = &gen.event {
                out.push_str(&format!("event: {event}{nl}"));
            }
            if let Some(id) = &gen.id {
                out.push_str(&format!("id:{id}{nl}"));
            }
            for (j, value) in gen.data.iter().enumerate() {
                // The colon-only form is unambiguous only when the value does
                // not itself start with a space.
                if gen.no_space[j] && !value.starts_with(' ') {
                    out.push_str(&format!("data:{value}{nl}"));
                } else {
                    out.push_str(&format!("data: {value}{nl}"));
                }
            }
            let last = i + 1 == events.len();
            if !last || terminate_last {
                out.push_str(nl);
            }
            expected.push(SseEvent {
                event: gen.event.clone(),
                data: gen.data.join("\n"),
                id: gen.id.clone(),
                retry: None,
            });
        }
        (out, expected)
    }

    proptest! {
        #[test]
        fn output_is_identical_under_any_chunking(
            events in proptest::collection::vec(event_strategy(), 0..6),
            bom in any::<bool>(),
            terminate_last in any::<bool>(),
            cuts in proptest::collection::vec(any::<usize>(), 0..16),
        ) {
            let (text, expected) = render(&events, bom, terminate_last);
            let bytes = text.as_bytes();

            let whole = decode_all(bytes);
            prop_assert_eq!(&whole, &expected);

            let mut cuts: Vec<usize> = cuts
                .into_iter()
                .map(|c| if bytes.is_empty() { 0 } else { c % (bytes.len() + 1) })
                .collect();
            cuts.sort_unstable();
            let mut decoder = SseDecoder::new();
            let mut chunked = Vec::new();
            let mut prev = 0;
            for cut in cuts.into_iter().chain(std::iter::once(bytes.len())) {
                chunked.extend(decoder.feed(&bytes[prev..cut]));
                prev = cut;
            }
            chunked.extend(decoder.finish());
            prop_assert_eq!(&chunked, &expected);

            let mut decoder = SseDecoder::new();
            let mut by_byte = Vec::new();
            for byte in bytes {
                by_byte.extend(decoder.feed(std::slice::from_ref(byte)));
            }
            by_byte.extend(decoder.finish());
            prop_assert_eq!(&by_byte, &expected);
        }
    }
}
