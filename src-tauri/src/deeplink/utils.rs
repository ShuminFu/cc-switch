//! Deep link utility functions
//!
//! Common helpers for URL validation, Base64 decoding, etc.

use crate::error::AppError;
use base64::prelude::*;
use url::Url;

/// Validate that a string is a valid HTTP(S) URL
pub fn validate_url(url_str: &str, field_name: &str) -> Result<(), AppError> {
    let url = Url::parse(url_str)
        .map_err(|e| AppError::InvalidInput(format!("Invalid URL for '{field_name}': {e}")))?;

    let scheme = url.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(AppError::InvalidInput(format!(
            "Invalid URL scheme for '{field_name}': must be http or https, got '{scheme}'"
        )));
    }

    Ok(())
}

/// Maximum decoded size of a base64 deep-link parameter (SESS-33).
pub const MAX_DECODED_PARAM_BYTES: usize = 256 * 1024;

/// Decode a Base64 parameter from deep link URL
///
/// One tolerant normalisation pass, then a single decode (SESS-33):
/// - `+` that URL decoding turned into a space is restored
/// - URL-safe `-`/`_` are mapped to the standard alphabet
/// - missing `=` padding is added
///
/// Payloads that would decode to more than [`MAX_DECODED_PARAM_BYTES`] are
/// rejected before decoding.
pub fn decode_base64_param(field: &str, raw: &str) -> Result<Vec<u8>, AppError> {
    let mut normalized: String = raw
        .trim_matches(|c| c == '\r' || c == '\n')
        .chars()
        .filter(|c| *c != '\r' && *c != '\n')
        .map(|c| match c {
            ' ' => '+',
            '-' => '+',
            '_' => '/',
            other => other,
        })
        .collect();
    let trimmed_len = normalized.trim_end_matches('=').len();
    normalized.truncate(trimmed_len);
    if normalized.len() / 4 * 3 > MAX_DECODED_PARAM_BYTES {
        return Err(AppError::InvalidInput(format!(
            "{field} parameter is too large (max {MAX_DECODED_PARAM_BYTES} bytes decoded)"
        )));
    }
    let remainder = normalized.len() % 4;
    if remainder != 0 {
        normalized.extend(std::iter::repeat_n('=', 4 - remainder));
    }

    BASE64_STANDARD.decode(normalized.as_bytes()).map_err(|err| {
        AppError::InvalidInput(format!(
            "{field} 参数 Base64 解码失败：{err}。请确认链接参数已用 Base64 编码并经过 URL 转义（尤其是将 '+' 编码为 %2B，或使用 URL-safe Base64）。"
        ))
    })
}

/// Infer homepage URL from API endpoint
///
/// Examples:
/// - https://api.anthropic.com/v1 → https://anthropic.com
/// - https://api.openai.com/v1 → https://openai.com
/// - https://api-test.company.com/v1 → https://company.com
pub fn infer_homepage_from_endpoint(endpoint: &str) -> Option<String> {
    let url = Url::parse(endpoint).ok()?;
    let host = url.host_str()?;

    // Remove common API prefixes
    let clean_host = host
        .strip_prefix("api.")
        .or_else(|| host.strip_prefix("api-"))
        .unwrap_or(host);

    Some(format!("https://{clean_host}"))
}
