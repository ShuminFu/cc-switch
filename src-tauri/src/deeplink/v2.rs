//! `ccswitch://v2/import` — bundled, optionally signed deep links (SESS-10, SESS-30).
//!
//! ```text
//! ccswitch://v2/import?p=<base64url(JSON envelope)>[&sig=<base64url(minisign signature)>]
//! ```
//!
//! The envelope is `{ "v": 2, "items": [ {resource, ...v1 fields}, ... ],
//! "publisher": "Name", "publisherKey": "<minisign public key>", "minApp": "3.18.0" }`.
//! Each item uses the v1 parameter names; `config` may be an inline JSON
//! object instead of base64. The signature (if any) covers the exact decoded
//! `p` bytes and must verify with `publisherKey`; a bad signature rejects the
//! whole link. See `docs/deeplink-spec.md`.

use std::collections::HashMap;

use base64::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::parser::parse_params;
use super::trust::{self, PublisherTrust};
use super::DeepLinkImportRequest;
use crate::error::AppError;

/// Maximum decoded envelope size.
pub const MAX_V2_PAYLOAD_BYTES: usize = 32 * 1024;
/// Maximum number of resources in one link.
pub const MAX_BUNDLE_ITEMS: usize = 16;
const MAX_SIGNATURE_BYTES: usize = 2 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SignatureStatus {
    #[default]
    Unsigned,
    Verified,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureInfo {
    pub status: SignatureStatus,
    /// Display name: the registry name for known keys, otherwise the name the
    /// link claims (shown as unverified).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trust: Option<PublisherTrust>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_key: Option<String>,
}

/// Every item of a v2 link plus who signed it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepLinkBundle {
    pub version: u32,
    pub items: Vec<DeepLinkImportRequest>,
    pub signature: SignatureInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_app: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Envelope {
    v: u32,
    items: Vec<Value>,
    #[serde(default)]
    publisher: Option<String>,
    #[serde(default)]
    publisher_key: Option<String>,
    #[serde(default)]
    min_app: Option<String>,
}

/// Strict-ish base64url decoding with a size cap: URL-safe and standard
/// alphabets and missing padding are accepted, nothing else.
fn decode_b64url(field: &str, raw: &str, limit: usize) -> Result<Vec<u8>, AppError> {
    let normalized: String = raw
        .trim()
        .trim_end_matches('=')
        .chars()
        .map(|c| match c {
            '+' | ' ' => '-',
            '/' => '_',
            other => other,
        })
        .collect();
    if normalized.len() > limit.div_ceil(3) * 4 + 4 {
        return Err(AppError::InvalidInput(format!("'{field}' is too large")));
    }
    let bytes = BASE64_URL_SAFE_NO_PAD
        .decode(normalized.as_bytes())
        .map_err(|e| AppError::InvalidInput(format!("'{field}' is not valid base64url: {e}")))?;
    if bytes.len() > limit {
        return Err(AppError::InvalidInput(format!("'{field}' is too large")));
    }
    Ok(bytes)
}

/// Convert one envelope item into v1-style query parameters.
fn item_to_params(item: &Value) -> Result<HashMap<String, String>, AppError> {
    let obj = item
        .as_object()
        .ok_or_else(|| AppError::InvalidInput("Each v2 item must be an object".to_string()))?;
    let mut params = HashMap::new();
    for (key, value) in obj {
        let text = match value {
            Value::Null => continue,
            Value::String(s) => s.clone(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => n.to_string(),
            // Inline JSON config / content is base64-encoded so the v1 parsers
            // and importers handle it unchanged.
            Value::Object(_) | Value::Array(_) => {
                let json = serde_json::to_string(value)
                    .map_err(|e| AppError::InvalidInput(e.to_string()))?;
                BASE64_STANDARD.encode(json)
            }
        };
        params.insert(key.clone(), text);
    }
    Ok(params)
}

/// Parse the query of a `ccswitch://v2/import` link.
pub fn parse_v2(params: &HashMap<String, String>) -> Result<DeepLinkImportRequest, AppError> {
    let payload_raw = params
        .get("p")
        .ok_or_else(|| AppError::InvalidInput("Missing 'p' parameter for v2 link".to_string()))?;
    let payload = decode_b64url("p", payload_raw, MAX_V2_PAYLOAD_BYTES)?;

    let signature = match params.get("sig") {
        None => SignatureInfo::default(),
        Some(sig_raw) => {
            let sig_bytes = decode_b64url("sig", sig_raw, MAX_SIGNATURE_BYTES)?;
            let sig_text = String::from_utf8(sig_bytes)
                .map_err(|_| AppError::InvalidInput("'sig' is not UTF-8".to_string()))?;
            verify_envelope_signature(&payload, &sig_text)?
        }
    };

    let envelope: Envelope = serde_json::from_slice(&payload)
        .map_err(|e| AppError::InvalidInput(format!("Invalid v2 envelope: {e}")))?;
    if envelope.v != 2 {
        return Err(AppError::InvalidInput(format!(
            "Envelope version {} does not match the v2 link",
            envelope.v
        )));
    }
    if let Some(min_app) = envelope.min_app.as_deref() {
        let current = env!("CARGO_PKG_VERSION");
        if crate::services::catalog::compare_versions(min_app, current).is_gt() {
            return Err(AppError::InvalidInput(format!(
                "This link requires CC Switch {min_app} or newer (installed: {current}). Please update CC Switch."
            )));
        }
    }
    if envelope.items.is_empty() {
        return Err(AppError::InvalidInput("v2 link has no items".to_string()));
    }
    if envelope.items.len() > MAX_BUNDLE_ITEMS {
        return Err(AppError::InvalidInput(format!(
            "v2 link has too many items (max {MAX_BUNDLE_ITEMS})"
        )));
    }

    // A signed link's identity comes from the signed envelope; the unsigned
    // `publisher` field is never shown as verified.
    let signature = if signature.status == SignatureStatus::Verified {
        let key = envelope.publisher_key.clone().unwrap_or_default();
        if signature.public_key.as_deref() != Some(key.as_str()) {
            return Err(AppError::InvalidInput(
                "Signature key does not match the envelope publisherKey".to_string(),
            ));
        }
        let (name, trust) = trust::resolve(&key, envelope.publisher.as_deref());
        SignatureInfo {
            publisher: Some(name),
            trust: Some(trust),
            ..signature
        }
    } else {
        SignatureInfo {
            publisher: envelope.publisher.clone(),
            ..SignatureInfo::default()
        }
    };

    let mut items = Vec::with_capacity(envelope.items.len());
    for (index, item) in envelope.items.iter().enumerate() {
        let item_params = item_to_params(item)?;
        let request = parse_params(&item_params, "v2".to_string())
            .map_err(|e| AppError::InvalidInput(format!("Item {}: {e}", index + 1)))?;
        items.push(request);
    }

    let mut primary = items[0].clone();
    primary.bundle = Some(Box::new(DeepLinkBundle {
        version: 2,
        items,
        signature,
        min_app: envelope.min_app,
    }));
    Ok(primary)
}

/// Verify `sig_text` over the envelope bytes with the envelope's own
/// `publisherKey`. Any failure rejects the link.
fn verify_envelope_signature(payload: &[u8], sig_text: &str) -> Result<SignatureInfo, AppError> {
    let key = serde_json::from_slice::<Value>(payload)
        .ok()
        .and_then(|v| {
            v.get("publisherKey")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .ok_or_else(|| {
            AppError::InvalidInput("Signed link is missing 'publisherKey'".to_string())
        })?;
    crate::services::catalog::verify_minisign(&key, payload, sig_text).map_err(|e| {
        AppError::InvalidInput(format!(
            "Deep link signature is invalid ({e}); the link was modified or not signed by its publisher"
        ))
    })?;
    Ok(SignatureInfo {
        status: SignatureStatus::Verified,
        key_id: crate::services::catalog::minisign_key_id(&key),
        public_key: Some(key),
        ..SignatureInfo::default()
    })
}

/// Build an unsigned v2 link from items (used by "share as link").
pub fn build_v2_link(items: Vec<Value>) -> String {
    let envelope = serde_json::json!({ "v": 2, "items": items });
    let payload = serde_json::to_vec(&envelope).unwrap_or_default();
    format!(
        "ccswitch://v2/import?p={}",
        BASE64_URL_SAFE_NO_PAD.encode(payload)
    )
}

#[cfg(test)]
mod tests {
    use super::super::parser::parse_deeplink_url;
    use super::*;

    /// Test-only minisign key (secret key not kept); see services::catalog tests.
    const TEST_PUBLIC_KEY: &str = "RWSGmZsfeFIsiSCs/JSRfD8/Lfp1OFnCiEfSB6W7e0kULAVKwBncgFFT";
    pub(crate) const SIGNED_ENVELOPE: &str = include_str!("testdata/signed_envelope.json");
    pub(crate) const SIGNED_ENVELOPE_SIG: &str =
        include_str!("testdata/signed_envelope.json.minisig");

    fn link(payload: &[u8], sig: Option<&str>) -> String {
        let mut url = format!(
            "ccswitch://v2/import?p={}",
            BASE64_URL_SAFE_NO_PAD.encode(payload)
        );
        if let Some(sig) = sig {
            url.push_str("&sig=");
            url.push_str(&BASE64_URL_SAFE_NO_PAD.encode(sig));
        }
        url
    }

    #[test]
    fn signed_bundle_verifies_and_lists_every_item() {
        let request =
            parse_deeplink_url(&link(SIGNED_ENVELOPE.as_bytes(), Some(SIGNED_ENVELOPE_SIG)))
                .unwrap();
        let bundle = request.bundle.as_ref().expect("bundle");
        assert_eq!(bundle.items.len(), 3);
        assert_eq!(bundle.items[0].resource, "provider");
        assert_eq!(bundle.items[1].resource, "mcp");
        assert_eq!(bundle.items[2].resource, "prompt");
        assert_eq!(bundle.signature.status, SignatureStatus::Verified);
        assert_eq!(
            bundle.signature.publisher.as_deref(),
            Some("Example Vendor")
        );
        assert_eq!(bundle.signature.key_id.as_deref(), Some("892C52781F9B9986"));
        assert_eq!(
            bundle.signature.public_key.as_deref(),
            Some(TEST_PUBLIC_KEY)
        );
        assert!(matches!(
            bundle.signature.trust,
            Some(PublisherTrust::New) | Some(PublisherTrust::Trusted)
        ));
        assert_eq!(request.version, "v2");
    }

    #[test]
    fn tampered_bundle_is_rejected() {
        let tampered = SIGNED_ENVELOPE.replace("api.example.com", "api.evil.test");
        let err = parse_deeplink_url(&link(tampered.as_bytes(), Some(SIGNED_ENVELOPE_SIG)))
            .unwrap_err()
            .to_string();
        assert!(err.contains("signature is invalid"), "{err}");
    }

    #[test]
    fn bundle_signed_with_another_key_is_rejected() {
        // Swap in a different publisherKey: the signature no longer matches.
        let other = SIGNED_ENVELOPE.replace(
            TEST_PUBLIC_KEY,
            "RWSBmqQn+8wPmpm6ewuBfJ2qgYRwIiUEfQMw4Flc8vh7FpcCxN7tzQqt",
        );
        assert!(parse_deeplink_url(&link(other.as_bytes(), Some(SIGNED_ENVELOPE_SIG))).is_err());
    }

    #[test]
    fn unsigned_bundle_is_marked_unsigned() {
        let payload = br#"{"v":2,"publisher":"Claims To Be Acme","items":[{"resource":"skill","repo":"owner/repo"}]}"#;
        let request = parse_deeplink_url(&link(payload, None)).unwrap();
        let bundle = request.bundle.unwrap();
        assert_eq!(bundle.signature.status, SignatureStatus::Unsigned);
        assert_eq!(bundle.signature.trust, None);
        assert_eq!(bundle.items[0].repo.as_deref(), Some("owner/repo"));
    }

    #[test]
    fn inline_json_config_and_limits() {
        let payload = br#"{"v":2,"items":[{"resource":"mcp","apps":"claude","config":{"mcpServers":{"x":{"command":"echo"}}}}]}"#;
        let request = parse_deeplink_url(&link(payload, None)).unwrap();
        let config = BASE64_STANDARD.decode(request.config.unwrap()).unwrap();
        assert!(String::from_utf8(config).unwrap().contains("mcpServers"));

        let items: Vec<Value> = (0..=MAX_BUNDLE_ITEMS)
            .map(|_| serde_json::json!({"resource":"skill","repo":"o/r"}))
            .collect();
        let too_many = serde_json::to_vec(&serde_json::json!({"v":2,"items":items})).unwrap();
        assert!(parse_deeplink_url(&link(&too_many, None)).is_err());

        let big = vec![b' '; MAX_V2_PAYLOAD_BYTES + 10];
        let err = parse_deeplink_url(&link(&big, None))
            .unwrap_err()
            .to_string();
        assert!(err.contains("too large"), "{err}");
    }

    #[test]
    fn min_app_newer_than_installed_asks_to_update() {
        let payload = br#"{"v":2,"minApp":"999.0.0","items":[{"resource":"skill","repo":"o/r"}]}"#;
        let err = parse_deeplink_url(&link(payload, None))
            .unwrap_err()
            .to_string();
        assert!(err.contains("999.0.0 or newer"), "{err}");
    }

    #[test]
    fn share_link_round_trips() {
        let url = build_v2_link(vec![serde_json::json!({
            "resource": "provider", "app": "claude", "name": "Shared",
            "endpoint": "https://api.example.com", "homepage": "https://example.com"
        })]);
        let request = parse_deeplink_url(&url).unwrap();
        assert_eq!(request.name.as_deref(), Some("Shared"));
        assert_eq!(request.endpoint.as_deref(), Some("https://api.example.com"));
    }
}
