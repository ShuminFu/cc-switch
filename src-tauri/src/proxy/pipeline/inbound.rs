//! Parse stage: turn the client's HTTP request into an [`InboundRequest`]
//! (decoded JSON body plus the parts the forwarder needs).

use crate::proxy::content_encoding::{
    decompress_body, get_content_encoding, is_supported_content_encoding,
};
use crate::proxy::ProxyError;
use axum::http::{HeaderMap, Method, Uri};
use bytes::Bytes;
use http_body_util::BodyExt;
use serde_json::Value;

/// How a route reads its request body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BodyPolicy {
    /// Decompress `content-encoding` bodies (the Codex desktop client zstd-compresses).
    pub decode_content_encoding: bool,
    /// An empty body is `null` (Gemini GET endpoints) instead of a JSON error.
    pub allow_empty: bool,
}

impl BodyPolicy {
    pub const JSON: Self = Self {
        decode_content_encoding: false,
        allow_empty: false,
    };
    pub const CODEX: Self = Self {
        decode_content_encoding: true,
        allow_empty: false,
    };
    pub const GEMINI: Self = Self {
        decode_content_encoding: false,
        allow_empty: true,
    };
}

/// The client's (decoded) request body bytes, kept in the request extensions
/// so an unchanged body is forwarded verbatim (PROXY-17). `Bytes` is
/// reference-counted: every attempt shares the one buffer.
#[derive(Debug, Clone)]
pub struct ClientBodyBytes(pub Bytes);

/// A parsed client request.
pub struct InboundRequest {
    pub method: Method,
    pub uri: Uri,
    pub headers: HeaderMap,
    pub extensions: http::Extensions,
    pub body: Value,
}

impl InboundRequest {
    /// `stream: true` in the body.
    pub fn is_stream(&self) -> bool {
        self.body
            .get("stream")
            .and_then(|s| s.as_bool())
            .unwrap_or(false)
    }

    /// Path and query as received, minus a route prefix such as `/claude`
    /// (PROXY-38: prefixed aliases must reach the upstream unprefixed).
    pub fn endpoint_without_prefix(&self, prefix: Option<&str>) -> String {
        strip_route_prefix(&self.uri, prefix)
    }
}

/// Read, decode and parse the request body.
pub async fn read_inbound(
    request: axum::extract::Request,
    policy: BodyPolicy,
) -> Result<InboundRequest, ProxyError> {
    let (parts, body) = request.into_parts();
    let mut headers = parts.headers;
    let bytes = body
        .collect()
        .await
        .map_err(|e| ProxyError::Internal(format!("Failed to read request body: {e}")))?
        .to_bytes();
    let bytes = if policy.decode_content_encoding {
        decode_request_body(&mut headers, bytes)?
    } else {
        bytes
    };
    let body = parse_body(&bytes, policy.allow_empty)?;
    let mut extensions = parts.extensions;
    extensions.insert(ClientBodyBytes(bytes));
    Ok(InboundRequest {
        method: parts.method,
        uri: parts.uri,
        headers,
        extensions,
        body,
    })
}

fn parse_body(bytes: &[u8], allow_empty: bool) -> Result<Value, ProxyError> {
    if allow_empty && bytes.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_slice(bytes).map_err(|e| ProxyError::invalid_request_json(&e))
}

/// Decompress a compressed request body and drop the entity headers that no
/// longer describe it (the forwarder re-derives them from the plain JSON).
pub fn decode_request_body(
    headers: &mut HeaderMap,
    body_bytes: Bytes,
) -> Result<Bytes, ProxyError> {
    let Some(encoding) = get_content_encoding(headers) else {
        return Ok(body_bytes);
    };

    if !is_supported_content_encoding(&encoding) {
        return Err(ProxyError::InvalidRequest(format!(
            "Unsupported request content-encoding: {encoding}"
        )));
    }

    log::debug!("[Codex] decoding request body: content-encoding={encoding}");
    let decompressed = match decompress_body(&encoding, &body_bytes) {
        Ok(Some(decompressed)) => decompressed,
        // Supported encodings never return None; never forward compressed bytes as JSON.
        Ok(None) => {
            return Err(ProxyError::InvalidRequest(format!(
                "Unsupported request content-encoding: {encoding}"
            )));
        }
        Err(e) => {
            log::warn!("[Codex] request body decode failed ({encoding}): {e}");
            return Err(ProxyError::InvalidRequest(format!(
                "Failed to decompress request body ({encoding}): {e}"
            )));
        }
    };

    headers.remove(axum::http::header::CONTENT_ENCODING);
    headers.remove(axum::http::header::CONTENT_LENGTH);
    headers.remove(axum::http::header::TRANSFER_ENCODING);

    Ok(Bytes::from(decompressed))
}

/// `endpoint` plus the request's query string.
pub fn endpoint_with_query(uri: &Uri, endpoint: &str) -> String {
    match uri.query() {
        Some(query) => format!("{endpoint}?{query}"),
        None => endpoint.to_string(),
    }
}

/// Path and query of `uri` with `prefix` removed from the front of the path.
pub fn strip_route_prefix(uri: &Uri, prefix: Option<&str>) -> String {
    let raw = uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or(uri.path());
    match prefix.and_then(|prefix| raw.strip_prefix(prefix)) {
        Some(rest) if rest.starts_with('/') => rest.to_string(),
        _ => raw.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(body: &'static [u8], headers: &[(&str, &str)]) -> axum::extract::Request {
        let mut builder = http::Request::builder()
            .method("POST")
            .uri("/v1/messages?beta=true");
        for (k, v) in headers {
            builder = builder.header(*k, *v);
        }
        builder.body(axum::body::Body::from(body)).unwrap()
    }

    #[tokio::test]
    async fn parses_json_and_keeps_request_parts() {
        let inbound = read_inbound(request(br#"{"stream":true}"#, &[]), BodyPolicy::JSON)
            .await
            .unwrap();
        assert!(inbound.is_stream());
        assert_eq!(inbound.method, Method::POST);
        assert_eq!(
            inbound.endpoint_without_prefix(None),
            "/v1/messages?beta=true"
        );
    }

    #[tokio::test]
    async fn malformed_json_is_a_400_invalid_request() {
        let err = read_inbound(request(b"{", &[]), BodyPolicy::JSON)
            .await
            .err()
            .unwrap();
        assert!(matches!(err, ProxyError::InvalidRequest(_)));
    }

    #[tokio::test]
    async fn empty_body_is_null_only_where_allowed() {
        let inbound = read_inbound(request(b"", &[]), BodyPolicy::GEMINI)
            .await
            .unwrap();
        assert!(inbound.body.is_null());
        assert!(read_inbound(request(b"", &[]), BodyPolicy::JSON)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn codex_bodies_are_decompressed_and_entity_headers_dropped() {
        let compressed = zstd::encode_all(&br#"{"model":"m"}"#[..], 0).unwrap();
        let leaked: &'static [u8] = Box::leak(compressed.into_boxed_slice());
        let inbound = read_inbound(
            request(
                leaked,
                &[("content-encoding", "zstd"), ("content-length", "5")],
            ),
            BodyPolicy::CODEX,
        )
        .await
        .unwrap();
        assert_eq!(inbound.body["model"], "m");
        assert!(inbound.headers.get("content-encoding").is_none());
        assert!(inbound.headers.get("content-length").is_none());
    }

    #[test]
    fn route_prefixes_are_stripped_only_on_a_segment_boundary() {
        let uri: Uri = "/gemini/v1beta/models/g:generateContent?alt=sse"
            .parse()
            .unwrap();
        assert_eq!(
            strip_route_prefix(&uri, Some("/gemini")),
            "/v1beta/models/g:generateContent?alt=sse"
        );
        let uri: Uri = "/claude/v1/messages".parse().unwrap();
        assert_eq!(strip_route_prefix(&uri, Some("/claude")), "/v1/messages");
        let uri: Uri = "/claudex/v1/messages".parse().unwrap();
        assert_eq!(
            strip_route_prefix(&uri, Some("/claude")),
            "/claudex/v1/messages"
        );
        let uri: Uri = "/v1/messages".parse().unwrap();
        assert_eq!(strip_route_prefix(&uri, Some("/claude")), "/v1/messages");
        let uri: Uri = "/v1/responses".parse().unwrap();
        assert_eq!(endpoint_with_query(&uri, "/responses"), "/responses");
    }
}
