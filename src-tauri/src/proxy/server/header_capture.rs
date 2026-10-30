//! Per-request capture of the client's original header-name casing.
//!
//! hyper lowercases header names while parsing and keeps its own case map
//! private, so the proxy records the raw bytes it reads from the client and
//! re-parses the header block of each request when hyper dispatches it.
//! Capturing per request (rather than peeking once per connection) keeps
//! keep-alive connections from reusing the first request's casing and order
//! for every later request (SEC-25).

use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::proxy::hyper_client::OriginalHeaderCases;

/// Upper bound on bytes kept between two requests. Request bodies also flow
/// through the recorder; only the most recent bytes are retained.
const MAX_RECORDED_BYTES: usize = 1024 * 1024;
const MAX_HEADERS: usize = 128;

/// Shared buffer of raw bytes read from one client connection.
#[derive(Clone, Default)]
pub(super) struct HeaderRecorder {
    buf: Arc<Mutex<Vec<u8>>>,
}

impl HeaderRecorder {
    fn record(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let mut buf = self.buf.lock().unwrap_or_else(|e| e.into_inner());
        buf.extend_from_slice(bytes);
        if buf.len() > MAX_RECORDED_BYTES {
            let excess = buf.len() - MAX_RECORDED_BYTES;
            buf.drain(..excess);
        }
    }

    /// Extract the header casing of the request hyper just dispatched.
    ///
    /// Finds the first complete request head in the recorded bytes whose
    /// method and target match the dispatched request (skipping any unread
    /// tail of the previous request's body), returns its header names, and
    /// discards everything up to the end of that head so the next request on
    /// the connection starts from fresh bytes. Returns an empty map when no
    /// matching head is found; the upstream client then falls back to
    /// title-case headers.
    pub(super) fn take_request_cases(&self, method: &str, target: &str) -> OriginalHeaderCases {
        let mut buf = self.buf.lock().unwrap_or_else(|e| e.into_inner());
        match find_request_head(&buf, method, target) {
            Some((start, len)) => {
                let cases = OriginalHeaderCases::from_raw_bytes(&buf[start..start + len]);
                buf.drain(..start + len);
                cases
            }
            None => {
                buf.clear();
                OriginalHeaderCases::default()
            }
        }
    }
}

/// Returns `(offset, length)` of the first complete request head matching
/// `method` and `target`. Candidates are located by the request line itself
/// (`"{method} {target} HTTP/"`), because an unread body tail of the previous
/// request need not end with a newline.
fn find_request_head(buf: &[u8], method: &str, target: &str) -> Option<(usize, usize)> {
    let needle = format!("{method} {target} HTTP/");
    let needle = needle.as_bytes();
    if needle.len() > buf.len() {
        return None;
    }
    for start in 0..=buf.len() - needle.len() {
        if &buf[start..start + needle.len()] != needle {
            continue;
        }
        let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
        let mut req = httparse::Request::new(&mut headers);
        if let Ok(httparse::Status::Complete(len)) = req.parse(&buf[start..]) {
            return Some((start, len));
        }
    }
    None
}

/// IO wrapper that copies every byte read from the client into a
/// [`HeaderRecorder`].
pub(super) struct RecordingIo<T> {
    inner: T,
    recorder: HeaderRecorder,
}

impl<T> RecordingIo<T> {
    pub(super) fn new(inner: T, recorder: HeaderRecorder) -> Self {
        Self { inner, recorder }
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for RecordingIo<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let this = &mut *self;
        let res = Pin::new(&mut this.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = res {
            this.recorder.record(&buf.filled()[before..]);
        }
        res
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for RecordingIo<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper_util::rt::TokioIo;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn names(cases: &OriginalHeaderCases) -> Vec<String> {
        cases
            .cases
            .iter()
            .map(|(_, raw)| String::from_utf8_lossy(raw).into_owned())
            .collect()
    }

    #[test]
    fn skips_previous_body_tail_and_matches_method_and_target() {
        let rec = HeaderRecorder::default();
        rec.record(b"{\"a\":1}POST /v1/messages HTTP/1.1\r\nX-Foo: 1\r\nHost: h\r\n\r\nbody");
        let cases = rec.take_request_cases("POST", "/v1/messages");
        assert_eq!(names(&cases), vec!["X-Foo", "Host"]);
        // The unread body bytes remain for the next request scan.
        assert_eq!(&*rec.buf.lock().unwrap(), b"body");
        // No matching head: empty cases, buffer reset.
        assert!(rec.take_request_cases("GET", "/x").cases.is_empty());
        assert!(rec.buf.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn keep_alive_requests_each_get_their_own_header_case() {
        let (client, server) = tokio::io::duplex(64 * 1024);
        let recorder = HeaderRecorder::default();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Vec<String>>();

        let svc_recorder = recorder.clone();
        let service =
            hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                let target = req.uri().to_string();
                let cases = svc_recorder.take_request_cases(req.method().as_str(), &target);
                let _ = tx.send(names(&cases));
                async move {
                    use http_body_util::BodyExt;
                    let _ = req.into_body().collect().await;
                    Ok::<_, std::convert::Infallible>(hyper::Response::new(
                        http_body_util::Full::new(bytes::Bytes::from_static(b"ok")),
                    ))
                }
            });
        tokio::spawn(async move {
            let _ = hyper::server::conn::http1::Builder::new()
                .preserve_header_case(true)
                .serve_connection(TokioIo::new(RecordingIo::new(server, recorder)), service)
                .await;
        });

        let (mut rd, mut wr) = tokio::io::split(client);
        let mut resp = vec![0u8; 4096];
        wr.write_all(b"POST /a HTTP/1.1\r\nHost: h\r\nX-First: 1\r\nContent-Length: 2\r\n\r\nhi")
            .await
            .unwrap();
        let _ = rd.read(&mut resp).await.unwrap();
        wr.write_all(b"GET /b?q=1 HTTP/1.1\r\nx-second: 2\r\nHOST: h\r\n\r\n")
            .await
            .unwrap();
        let _ = rd.read(&mut resp).await.unwrap();

        assert_eq!(
            rx.recv().await.unwrap(),
            vec!["Host", "X-First", "Content-Length"]
        );
        assert_eq!(rx.recv().await.unwrap(), vec!["x-second", "HOST"]);
    }
}
