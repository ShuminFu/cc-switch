//! Redaction helpers for deep link URLs
//!
//! ccswitch:// URLs may carry secrets (apiKey, usageApiKey, usageAccessToken, ...)
//! in their query string. Never log or emit the raw URL; use these helpers instead.

/// Redact a deep link URL for logging: keep scheme/host/path and query *keys* only.
pub(crate) fn redact_url_for_log(url_str: &str) -> String {
    match url::Url::parse(url_str) {
        Ok(url) => {
            let mut output = format!("{}://", url.scheme());
            if let Some(host) = url.host_str() {
                output.push_str(host);
            }
            output.push_str(url.path());

            let mut keys: Vec<String> = url.query_pairs().map(|(k, _)| k.to_string()).collect();
            keys.sort();
            keys.dedup();

            if !keys.is_empty() {
                output.push_str("?[keys:");
                output.push_str(&keys.join(","));
                output.push(']');
            }

            output
        }
        Err(_) => {
            let base = url_str.split('#').next().unwrap_or(url_str);
            match base.split_once('?') {
                Some((prefix, _)) => format!("{prefix}?[redacted]"),
                None => base.to_string(),
            }
        }
    }
}

/// Build the `deeplink-error` event payload sent to the frontend.
///
/// Only the redacted URL is included, so secrets never reach the webview.
pub(crate) fn deeplink_error_payload(url_str: &str, error: &str) -> serde_json::Value {
    serde_json::json!({
        "url": redact_url_for_log(url_str),
        "error": error
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET_URL: &str = "ccswitch://v1/import?resource=provider&app=claude&name=Test&endpoint=https%3A%2F%2Fapi.example.com&apiKey=sk-secret&usageApiKey=usage-secret&usageAccessToken=token-secret";

    #[test]
    fn redact_url_keeps_keys_but_drops_values() {
        let redacted = redact_url_for_log(SECRET_URL);
        assert!(redacted.starts_with("ccswitch://v1/import?[keys:"));
        assert!(redacted.contains("apiKey"));
        for secret in [
            "sk-secret",
            "usage-secret",
            "token-secret",
            "api.example.com",
        ] {
            assert!(!redacted.contains(secret), "leaked {secret}: {redacted}");
        }
    }

    #[test]
    fn redact_unparseable_url_drops_query() {
        let redacted = redact_url_for_log("not a url?apiKey=sk-secret#frag");
        assert_eq!(redacted, "not a url?[redacted]");
    }

    #[test]
    fn error_payload_does_not_contain_secret() {
        let url = "ccswitch://v1/import?resource=unknown&apiKey=sk-secret";
        let err = crate::deeplink::parse_deeplink_url(url).unwrap_err();
        let payload = deeplink_error_payload(url, &err.to_string());
        let serialized = payload.to_string();
        assert!(!serialized.contains("sk-secret"), "leaked: {serialized}");
        assert_eq!(payload["error"], err.to_string());
        assert!(payload["url"].as_str().unwrap().contains("apiKey"));
    }
}
