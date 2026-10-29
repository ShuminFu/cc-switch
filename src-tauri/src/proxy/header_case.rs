//! Per-request capture of the client's original header-name casing.
//!
//! hyper lowercases header names when it parses a request, so the inbound
//! connection is wrapped in [`RecordingIo`]: every byte hyper reads also goes
//! through a [`HeaderCaseRecorder`], which frames the HTTP/1.1 stream itself
//! (request head up to the blank line, then the `Content-Length` or chunked
//! body) and queues one [`OriginalHeaderCases`] per request head. The service
//! takes the entry for the request it is handling, so every keep-alive
//! request gets its own casing, however large the head or however it was
//! split across TCP reads.
//!
//! The recorder is passive: when it cannot follow the stream (malformed
//! framing, oversized head, protocol upgrade, a request it did not see) it
//! stops recording and the affected requests fall back to an empty case map.

use super::hyper_client::OriginalHeaderCases;
use std::collections::VecDeque;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// Largest request head or framing line the recorder buffers; hyper rejects
/// heads well below this (its default read buffer limit is ~400 KB).
const MAX_PENDING_BYTES: usize = 512 * 1024;

/// One parsed request head waiting for the service.
#[derive(Debug)]
struct RecordedHead {
    method: String,
    target: String,
    cases: OriginalHeaderCases,
}

/// Where the recorder is in the inbound byte stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Framing {
    /// Reading a request head up to the empty line.
    Head,
    /// Skipping a `Content-Length` body; bytes remaining.
    Fixed(u64),
    /// Reading a chunk-size line.
    ChunkSize,
    /// Skipping chunk data; bytes remaining.
    ChunkData(u64),
    /// Reading the CRLF that ends a chunk's data.
    ChunkDataEnd,
    /// Reading trailer lines after the last chunk.
    Trailers,
    /// Lost track of the stream; nothing more is recorded.
    Disabled,
}

/// Frames an inbound HTTP/1.1 byte stream and records each request head's
/// header-name casing.
#[derive(Debug)]
pub(crate) struct HeaderCaseRecorder {
    framing: Framing,
    /// Current head or framing line.
    pending: Vec<u8>,
    heads: VecDeque<RecordedHead>,
}

impl Default for HeaderCaseRecorder {
    fn default() -> Self {
        Self {
            framing: Framing::Head,
            pending: Vec::new(),
            heads: VecDeque::new(),
        }
    }
}

impl HeaderCaseRecorder {
    /// Feed bytes exactly as they were read from the client.
    pub(crate) fn feed(&mut self, mut data: &[u8]) {
        while !data.is_empty() {
            match self.framing {
                Framing::Disabled => return,
                Framing::Fixed(remaining) | Framing::ChunkData(remaining) => {
                    let take = remaining.min(data.len() as u64);
                    data = &data[take as usize..];
                    let remaining = remaining - take;
                    self.framing = match (self.framing, remaining) {
                        (Framing::Fixed(_), 0) => Framing::Head,
                        (Framing::Fixed(_), n) => Framing::Fixed(n),
                        (_, 0) => Framing::ChunkDataEnd,
                        (_, n) => Framing::ChunkData(n),
                    };
                }
                Framing::Head | Framing::ChunkSize | Framing::ChunkDataEnd | Framing::Trailers => {
                    let Some(newline) = data.iter().position(|&b| b == b'\n') else {
                        self.pending.extend_from_slice(data);
                        self.check_pending_limit();
                        return;
                    };
                    self.pending.extend_from_slice(&data[..=newline]);
                    data = &data[newline + 1..];
                    if !self.check_pending_limit() {
                        return;
                    }
                    self.line_complete();
                }
            }
        }
    }

    /// Take the casing recorded for the request hyper is now handling.
    ///
    /// Heads are queued in arrival order and hyper serves HTTP/1.1 requests
    /// in that same order; a head that does not match the request means the
    /// recorder lost sync, so it stops recording instead of guessing.
    pub(crate) fn take_for(
        &mut self,
        method: &http::Method,
        uri: &http::Uri,
    ) -> OriginalHeaderCases {
        match self.heads.pop_front() {
            Some(head) if head.method == method.as_str() && head.target == uri.to_string() => {
                head.cases
            }
            other => {
                if other.is_some() || self.framing != Framing::Disabled {
                    log::debug!(
                        "[ProxyServer] header case recorder out of sync for {method} {uri}; \
                         falling back to default casing"
                    );
                }
                self.disable();
                OriginalHeaderCases::default()
            }
        }
    }

    fn disable(&mut self) {
        self.framing = Framing::Disabled;
        self.pending = Vec::new();
        self.heads.clear();
    }

    /// Returns `false` (and disables recording) when the pending buffer is too large.
    fn check_pending_limit(&mut self) -> bool {
        if self.pending.len() > MAX_PENDING_BYTES {
            self.disable();
            return false;
        }
        true
    }

    /// `self.pending` ends with `\n`.
    fn line_complete(&mut self) {
        let line_is_empty = matches!(self.pending.as_slice(), b"\n" | b"\r\n");
        match self.framing {
            Framing::Head => {
                if line_is_empty {
                    // Empty lines before a request line are ignored (RFC 9112 §2.2).
                    self.pending.clear();
                } else if self.pending.ends_with(b"\n\n") || self.pending.ends_with(b"\n\r\n") {
                    let head = std::mem::take(&mut self.pending);
                    self.head_complete(&head);
                }
            }
            Framing::ChunkSize => {
                let size = match httparse::parse_chunk_size(&self.pending) {
                    Ok(httparse::Status::Complete((_, size))) => size,
                    _ => return self.disable(),
                };
                self.pending.clear();
                self.framing = if size == 0 {
                    Framing::Trailers
                } else {
                    Framing::ChunkData(size)
                };
            }
            Framing::ChunkDataEnd => {
                if !line_is_empty {
                    return self.disable();
                }
                self.pending.clear();
                self.framing = Framing::ChunkSize;
            }
            Framing::Trailers => {
                self.pending.clear();
                if line_is_empty {
                    self.framing = Framing::Head;
                }
            }
            Framing::Fixed(_) | Framing::ChunkData(_) | Framing::Disabled => {}
        }
    }

    fn head_complete(&mut self, head: &[u8]) {
        let Some((recorded, framing)) = parse_head(head) else {
            return self.disable();
        };
        self.heads.push_back(recorded);
        self.framing = framing;
        if framing == Framing::Disabled {
            // Keep the head just queued (the upgrade request itself still
            // reaches the service); later bytes are not HTTP/1.1 requests.
            self.pending = Vec::new();
        }
    }
}

/// Parse a complete request head and work out how its body is framed.
fn parse_head(head: &[u8]) -> Option<(RecordedHead, Framing)> {
    // Every header occupies at least one line, so this never runs out.
    let line_count = head.iter().filter(|&&b| b == b'\n').count();
    let mut headers = vec![httparse::EMPTY_HEADER; line_count];
    let mut req = httparse::Request::new(&mut headers);
    match req.parse(head) {
        Ok(httparse::Status::Complete(_)) => {}
        _ => return None,
    }

    let method = req.method?.to_string();
    let target = req.path?.to_string();
    let mut cases = Vec::with_capacity(req.headers.len());
    let mut content_length: Option<u64> = None;
    let mut chunked = false;
    let mut has_transfer_encoding = false;
    let mut upgrade = method.eq_ignore_ascii_case("CONNECT");

    for header in req.headers.iter() {
        let lower = header.name.to_ascii_lowercase();
        match lower.as_str() {
            "content-length" => {
                let value = std::str::from_utf8(header.value)
                    .ok()?
                    .trim()
                    .parse()
                    .ok()?;
                if content_length.is_some_and(|existing| existing != value) {
                    return None;
                }
                content_length = Some(value);
            }
            "transfer-encoding" => {
                has_transfer_encoding = true;
                // Only the final coding decides the framing.
                chunked = std::str::from_utf8(header.value)
                    .ok()?
                    .rsplit(',')
                    .next()
                    .is_some_and(|coding| coding.trim().eq_ignore_ascii_case("chunked"));
            }
            "upgrade" => upgrade = true,
            _ => {}
        }
        cases.push((lower, header.name.as_bytes().to_vec()));
    }

    let framing = if upgrade {
        Framing::Disabled
    } else if has_transfer_encoding {
        if !chunked {
            return None;
        }
        Framing::ChunkSize
    } else {
        match content_length {
            Some(0) | None => Framing::Head,
            Some(n) => Framing::Fixed(n),
        }
    };

    Some((
        RecordedHead {
            method,
            target,
            cases: OriginalHeaderCases { cases },
        },
        framing,
    ))
}

/// Recorder shared between a connection's IO wrapper and its service.
pub(crate) type SharedHeaderCaseRecorder = Arc<Mutex<HeaderCaseRecorder>>;

/// Take the casing for `req` from a shared recorder.
pub(crate) fn take_cases<B>(
    recorder: &SharedHeaderCaseRecorder,
    req: &http::Request<B>,
) -> OriginalHeaderCases {
    recorder
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take_for(req.method(), req.uri())
}

/// IO wrapper that feeds every byte read into a [`HeaderCaseRecorder`].
pub(crate) struct RecordingIo<S> {
    inner: S,
    recorder: SharedHeaderCaseRecorder,
}

impl<S> RecordingIo<S> {
    pub(crate) fn new(inner: S, recorder: SharedHeaderCaseRecorder) -> Self {
        Self { inner, recorder }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for RecordingIo<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let before = buf.filled().len();
        let result = Pin::new(&mut this.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = result {
            let read = &buf.filled()[before..];
            if !read.is_empty() {
                this.recorder
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .feed(read);
            }
        }
        result
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for RecordingIo<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, buf)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(cases: &OriginalHeaderCases) -> Vec<String> {
        cases
            .cases
            .iter()
            .map(|(_, raw)| String::from_utf8(raw.clone()).unwrap())
            .collect()
    }

    fn take(recorder: &mut HeaderCaseRecorder, method: &str, target: &str) -> Vec<String> {
        let method = http::Method::from_bytes(method.as_bytes()).unwrap();
        let uri: http::Uri = target.parse().unwrap();
        names(&recorder.take_for(&method, &uri))
    }

    #[test]
    fn keep_alive_requests_each_get_their_own_casing() {
        let mut recorder = HeaderCaseRecorder::default();
        recorder.feed(
            b"POST /a HTTP/1.1\r\nHost: x\r\nX-Foo-A: 1\r\nContent-Length: 5\r\n\r\nhello\
              POST /b HTTP/1.1\r\nHost: x\r\nX-Bar-B: 2\r\nContent-Length: 0\r\n\r\n",
        );
        assert_eq!(
            take(&mut recorder, "POST", "/a"),
            ["Host", "X-Foo-A", "Content-Length"]
        );
        assert_eq!(
            take(&mut recorder, "POST", "/b"),
            ["Host", "X-Bar-B", "Content-Length"]
        );
    }

    #[test]
    fn head_split_across_reads_byte_by_byte_is_recorded() {
        let raw = b"GET /v1/models?x=1 HTTP/1.1\r\nHost: x\r\nAnthropic-Beta: b\r\n\r\n";
        let mut recorder = HeaderCaseRecorder::default();
        for byte in raw.iter() {
            recorder.feed(std::slice::from_ref(byte));
        }
        assert_eq!(
            take(&mut recorder, "GET", "/v1/models?x=1"),
            ["Host", "Anthropic-Beta"]
        );
    }

    #[test]
    fn heads_larger_than_8k_and_with_many_headers_are_complete() {
        let mut raw = b"POST /v1/messages HTTP/1.1\r\nHost: x\r\n".to_vec();
        for i in 0..200 {
            raw.extend_from_slice(format!("X-Many-{i}: v\r\n").as_bytes());
        }
        raw.extend_from_slice(format!("X-Big: {}\r\n", "a".repeat(10_000)).as_bytes());
        raw.extend_from_slice(b"X-After-Big: 1\r\n\r\n");
        let mut recorder = HeaderCaseRecorder::default();
        recorder.feed(&raw);
        let got = take(&mut recorder, "POST", "/v1/messages");
        assert_eq!(got.len(), 1 + 200 + 2);
        assert_eq!(got.last().unwrap(), "X-After-Big");
    }

    #[test]
    fn chunked_bodies_are_skipped_including_trailers() {
        let mut recorder = HeaderCaseRecorder::default();
        recorder.feed(
            b"POST /a HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n\
              5\r\nhel\nl\r\n3;ext=1\r\n\r\n\r\r\n0\r\nX-Trailer: t\r\n\r\n\
              GET /b HTTP/1.1\r\nX-Second: 1\r\n\r\n",
        );
        assert_eq!(
            take(&mut recorder, "POST", "/a"),
            ["Host", "Transfer-Encoding"]
        );
        assert_eq!(take(&mut recorder, "GET", "/b"), ["X-Second"]);
    }

    #[test]
    fn mismatch_disables_recording() {
        let mut recorder = HeaderCaseRecorder::default();
        recorder.feed(b"GET /a HTTP/1.1\r\nX-A: 1\r\n\r\n");
        assert!(take(&mut recorder, "GET", "/other").is_empty());
        recorder.feed(b"GET /b HTTP/1.1\r\nX-B: 1\r\n\r\n");
        assert!(take(&mut recorder, "GET", "/b").is_empty());
    }

    #[test]
    fn upgrade_stops_recording_after_the_upgrade_request() {
        let mut recorder = HeaderCaseRecorder::default();
        recorder.feed(b"GET /ws HTTP/1.1\r\nUpgrade: websocket\r\n\r\nGET /x HTTP/1.1\r\n\r\n");
        assert_eq!(take(&mut recorder, "GET", "/ws"), ["Upgrade"]);
        assert!(take(&mut recorder, "GET", "/x").is_empty());
    }

    #[test]
    fn oversized_head_disables_recording() {
        let mut recorder = HeaderCaseRecorder::default();
        recorder.feed(b"GET / HTTP/1.1\r\nX-Huge: ");
        recorder.feed(&vec![b'a'; MAX_PENDING_BYTES + 1]);
        assert_eq!(recorder.framing, Framing::Disabled);
        assert!(recorder.pending.is_empty());
    }
}
