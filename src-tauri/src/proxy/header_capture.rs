//! Per-request capture of the client's original header casing and order.
//!
//! hyper lowercases header names while parsing, but some upstreams fingerprint
//! the exact wire casing/order of a CLI request. The server used to `peek` the
//! first bytes of each TCP connection once and attach that capture to every
//! request served on the connection. On keep-alive connections every later
//! request then reused the first request's casing and order, even when its
//! header set was different.
//!
//! `HeaderCaptureIo` wraps the accepted stream and records the bytes hyper
//! reads. When a request reaches the service, `HeaderCapture::take_for_request`
//! locates that request's own head in the recorded bytes (matching method and
//! request target), parses it, and verifies that the parsed header names match
//! the request hyper produced. If anything does not line up, it returns an
//! empty capture so the forwarder falls back to default casing instead of
//! applying a stale one.

use super::hyper_client::OriginalHeaderCases;
use std::collections::HashMap;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// Upper bound on retained bytes. Only the most recent bytes are kept: a
/// request head always arrives after the previous request's body, so trimming
/// from the front never drops a head that has not been consumed yet unless the
/// head alone exceeds this size (then we fall back to default casing).
const MAX_CAPTURE_BYTES: usize = 128 * 1024;

const MAX_PARSED_HEADERS: usize = 128;

/// Shared handle to the bytes read from one client connection.
#[derive(Clone, Default)]
pub(crate) struct HeaderCapture {
    buf: Arc<Mutex<Vec<u8>>>,
}

impl HeaderCapture {
    fn push(&self, bytes: &[u8]) {
        let Ok(mut buf) = self.buf.lock() else {
            return;
        };
        buf.extend_from_slice(bytes);
        if buf.len() > MAX_CAPTURE_BYTES {
            let excess = buf.len() - MAX_CAPTURE_BYTES;
            buf.drain(..excess);
        }
    }

    /// Extract the original header casing for the request described by `parts`.
    ///
    /// On success the consumed head (and anything before it, i.e. the previous
    /// request's body) is discarded so the next request starts searching after it.
    pub(crate) fn take_for_request(&self, parts: &http::request::Parts) -> OriginalHeaderCases {
        let Ok(mut buf) = self.buf.lock() else {
            return OriginalHeaderCases::default();
        };

        let method = parts.method.as_str().as_bytes();
        let target = parts
            .uri
            .path_and_query()
            .map(|pq| pq.as_str())
            .unwrap_or_else(|| parts.uri.path());

        let mut start = 0;
        while let Some(rel) = find(&buf[start..], method) {
            let pos = start + rel;
            if let Some((head_len, cases)) = parse_head_at(&buf[pos..], method, target) {
                if header_names_match(&cases, &parts.headers) {
                    buf.drain(..pos + head_len);
                    return cases;
                }
            }
            start = pos + 1;
        }

        log::debug!(
            "[HeaderCapture] no matching request head for {} {target}; using default casing",
            parts.method
        );
        OriginalHeaderCases::default()
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn parse_head_at(
    bytes: &[u8],
    method: &[u8],
    target: &str,
) -> Option<(usize, OriginalHeaderCases)> {
    let mut headers = [httparse::EMPTY_HEADER; MAX_PARSED_HEADERS];
    let mut req = httparse::Request::new(&mut headers);
    let head_len = match req.parse(bytes) {
        Ok(httparse::Status::Complete(len)) => len,
        _ => return None,
    };
    if req.method.map(str::as_bytes) != Some(method) || req.path != Some(target) {
        return None;
    }
    let cases = req
        .headers
        .iter()
        .map(|h| (h.name.to_ascii_lowercase(), h.name.as_bytes().to_vec()))
        .collect();
    Some((head_len, OriginalHeaderCases { cases }))
}

/// The capture must describe exactly the header set hyper parsed for this request.
fn header_names_match(cases: &OriginalHeaderCases, headers: &http::HeaderMap) -> bool {
    if cases.cases.len() != headers.len() {
        return false;
    }
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for (name, _) in &cases.cases {
        *counts.entry(name.as_str()).or_default() += 1;
    }
    counts
        .into_iter()
        .all(|(name, count)| headers.get_all(name).iter().count() == count)
}

/// Stream wrapper that records every byte read from the client.
pub(crate) struct HeaderCaptureIo<S> {
    inner: S,
    capture: HeaderCapture,
}

impl<S> HeaderCaptureIo<S> {
    pub(crate) fn new(inner: S) -> (Self, HeaderCapture) {
        let capture = HeaderCapture::default();
        (
            Self {
                inner,
                capture: capture.clone(),
            },
            capture,
        )
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for HeaderCaptureIo<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let before = buf.filled().len();
        let result = Pin::new(&mut this.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = &result {
            let new_bytes = &buf.filled()[before..];
            if !new_bytes.is_empty() {
                this.capture.push(new_bytes);
            }
        }
        result
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for HeaderCaptureIo<S> {
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
    use bytes::Bytes;
    use http_body_util::{BodyExt, Full};
    use hyper_util::rt::TokioIo;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn parts(method: &str, target: &str, headers: &[(&str, &str)]) -> http::request::Parts {
        let mut builder = http::Request::builder().method(method).uri(target);
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        builder.body(()).unwrap().into_parts().0
    }

    fn names(cases: &OriginalHeaderCases) -> Vec<String> {
        cases
            .cases
            .iter()
            .map(|(_, raw)| String::from_utf8(raw.clone()).unwrap())
            .collect()
    }

    #[test]
    fn captures_each_keep_alive_request_separately() {
        let capture = HeaderCapture::default();
        capture.push(
            b"POST /v1/messages HTTP/1.1\r\nHost: a\r\nX-Api-Key: k\r\nContent-Length: 4\r\n\r\nbody",
        );
        let first = capture.take_for_request(&parts(
            "POST",
            "/v1/messages",
            &[("host", "a"), ("x-api-key", "k"), ("content-length", "4")],
        ));
        assert_eq!(names(&first), ["Host", "X-Api-Key", "Content-Length"]);

        // Second request on the same connection with different casing and order.
        capture.push(
            b"POST /v1/messages?beta=true HTTP/1.1\r\nanthropic-version: 1\r\nHOST: a\r\ncontent-length: 2\r\n\r\n{}",
        );
        let second = capture.take_for_request(&parts(
            "POST",
            "/v1/messages?beta=true",
            &[
                ("anthropic-version", "1"),
                ("host", "a"),
                ("content-length", "2"),
            ],
        ));
        assert_eq!(
            names(&second),
            ["anthropic-version", "HOST", "content-length"]
        );
    }

    #[test]
    fn mismatched_header_set_falls_back_to_default_casing() {
        let capture = HeaderCapture::default();
        capture.push(b"GET /health HTTP/1.1\r\nHost: a\r\nX-Extra: 1\r\n\r\n");
        let cases = capture.take_for_request(&parts("GET", "/health", &[("host", "a")]));
        assert!(cases.cases.is_empty());
    }

    #[test]
    fn request_line_inside_previous_body_is_not_mistaken_for_head() {
        let capture = HeaderCapture::default();
        // A body that happens to contain a partial request line must be skipped.
        capture.push(b"{\"text\":\"POST /v1/messages HTTP/1.1\"}");
        capture.push(b"POST /v1/messages HTTP/1.1\r\nHost: a\r\n\r\n");
        let cases = capture.take_for_request(&parts("POST", "/v1/messages", &[("host", "a")]));
        assert_eq!(names(&cases), ["Host"]);
    }

    #[tokio::test]
    async fn hyper_keep_alive_connection_gets_fresh_capture_per_request() {
        let (client, server) = tokio::io::duplex(64 * 1024);
        let (io, capture) = HeaderCaptureIo::new(server);

        let service =
            hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                let capture = capture.clone();
                async move {
                    let (parts, body) = req.into_parts();
                    let cases = capture.take_for_request(&parts);
                    let _ = body.collect().await;
                    let joined = names(&cases).join(",");
                    Ok::<_, std::convert::Infallible>(hyper::Response::new(Full::new(Bytes::from(
                        joined,
                    ))))
                }
            });
        tokio::spawn(async move {
            let _ = hyper::server::conn::http1::Builder::new()
                .preserve_header_case(true)
                .serve_connection(TokioIo::new(io), service)
                .await;
        });

        let (mut reader, mut writer) = tokio::io::split(client);
        async fn read_response(reader: &mut (impl AsyncReadExt + Unpin)) -> String {
            let mut out = Vec::new();
            let mut buf = [0u8; 1024];
            loop {
                let n = reader.read(&mut buf).await.unwrap();
                assert!(n > 0);
                out.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&out).to_string();
                if let Some(end) = text.find("\r\n\r\n") {
                    let len: usize = text[..end]
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse().unwrap())
                        })
                        .unwrap_or(0);
                    if out.len() >= end + 4 + len {
                        return text[end + 4..end + 4 + len].to_string();
                    }
                }
            }
        }

        writer
            .write_all(b"POST /v1/messages HTTP/1.1\r\nHost: a\r\nX-Api-Key: k\r\nContent-Length: 2\r\n\r\n{}")
            .await
            .unwrap();
        assert_eq!(
            read_response(&mut reader).await,
            "Host,X-Api-Key,Content-Length"
        );

        writer
            .write_all(b"POST /v1/messages HTTP/1.1\r\ncontent-length: 2\r\nAnthropic-Beta: x\r\nhost: a\r\n\r\n{}")
            .await
            .unwrap();
        assert_eq!(
            read_response(&mut reader).await,
            "content-length,Anthropic-Beta,host"
        );
    }
}
