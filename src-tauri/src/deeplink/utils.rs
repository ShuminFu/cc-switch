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

/// Candidate spellings of a Base64 URL parameter, most likely first:
/// spaces restored to `+` (form-style URL decoding), the value as-is, padded
/// variants, and finally a fully whitespace-trimmed variant.
fn base64_candidates(raw: &str) -> Vec<String> {
    let mut candidates: Vec<String> = Vec::new();
    // Keep spaces (to restore `+`), but remove newlines
    let trimmed = raw.trim_matches(|c| c == '\r' || c == '\n');

    // First try restoring spaces to "+"
    if trimmed.contains(' ') {
        let replaced = trimmed.replace(' ', "+");
        if !replaced.is_empty() && !candidates.contains(&replaced) {
            candidates.push(replaced);
        }
    }

    // Original value
    if !trimmed.is_empty() && !candidates.contains(&trimmed.to_string()) {
        candidates.push(trimmed.to_string());
    }

    // Add padding variants
    let existing = candidates.clone();
    for candidate in existing {
        let mut padded = candidate.clone();
        let remainder = padded.len() % 4;
        if remainder != 0 {
            padded.extend(std::iter::repeat_n('=', 4 - remainder));
        }
        if !candidates.contains(&padded) {
            candidates.push(padded);
        }
    }

    // Last resort, tried only after every variant above: treat surrounding
    // spaces as stray whitespace rather than URL-decoded '+'.
    let fully_trimmed = raw.trim().replace(' ', "+");
    if !fully_trimmed.is_empty() && !candidates.contains(&fully_trimmed) {
        candidates.push(fully_trimmed);
    }

    candidates
}

/// Try every candidate with every Base64 engine and return the first decoded
/// bytes that `accept` takes. `last_error` describes the last failure.
fn decode_base64_with<T>(
    field: &str,
    raw: &str,
    mut accept: impl FnMut(Vec<u8>) -> Result<T, String>,
) -> Result<T, AppError> {
    let mut last_error: Option<String> = None;
    for candidate in base64_candidates(raw) {
        for engine in [
            &BASE64_STANDARD,
            &BASE64_STANDARD_NO_PAD,
            &BASE64_URL_SAFE,
            &BASE64_URL_SAFE_NO_PAD,
        ] {
            match engine.decode(&candidate) {
                Ok(bytes) => match accept(bytes) {
                    Ok(value) => return Ok(value),
                    Err(err) => last_error = Some(err),
                },
                Err(err) => last_error = Some(err.to_string()),
            }
        }
    }

    Err(AppError::InvalidInput(format!(
        "{field} 参数 Base64 解码失败：{}。请确认链接参数已用 Base64 编码并经过 URL 转义（尤其是将 '+' 编码为 %2B，或使用 URL-safe Base64）。",
        last_error.unwrap_or_else(|| "未知错误".to_string())
    )))
}

/// Decode a Base64 deep link parameter that must be UTF-8 text (configs,
/// prompt content, usage scripts). This is the single decoder for both the
/// import path and the confirmation previews sent to the frontend.
///
/// Handles the usual damage Base64 suffers in URLs:
/// - `+` being decoded as space
/// - Missing padding `=`
/// - Both standard and URL-safe Base64 variants
/// - Stray surrounding whitespace
///
/// A candidate that decodes to non-UTF-8 bytes is skipped rather than
/// returned, so an ambiguous spelling (e.g. surrounding spaces read as `+`)
/// cannot shadow the correct one.
pub fn decode_base64_utf8_param(field: &str, raw: &str) -> Result<String, AppError> {
    decode_base64_with(field, raw, |bytes| {
        String::from_utf8(bytes).map_err(|e| format!("Invalid UTF-8 in {field}: {e}"))
    })
}

/// Infer homepage URL from API endpoint
///
/// Examples:
/// - https://api.anthropic.com/v1 → https://anthropic.com
/// - https://api.openai.com/v1 → https://openai.com
/// - https://api-test.company.com/v1 → https://company.com
/// - http://localhost:3000/v1 → http://localhost:3000
///
/// The scheme (http/https; anything else becomes https) and any non-default
/// port are preserved so local or self-hosted endpoints yield a reachable URL.
pub fn infer_homepage_from_endpoint(endpoint: &str) -> Option<String> {
    let url = Url::parse(endpoint).ok()?;
    let host = url.host_str()?;

    // Remove common API prefixes
    let clean_host = host
        .strip_prefix("api.")
        .or_else(|| host.strip_prefix("api-"))
        .unwrap_or(host);

    let scheme = if url.scheme() == "http" {
        "http"
    } else {
        "https"
    };
    let port = url.port().map(|p| format!(":{p}")).unwrap_or_default();

    Some(format!("{scheme}://{clean_host}{port}"))
}
