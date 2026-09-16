//! Error value crossing the IPC boundary.
//!
//! Today every backend command rejects with a plain string; skill commands
//! smuggle a JSON object (`{ code, context, suggestion }`) inside it. This
//! type accepts both shapes so the frontend has one error to handle.

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct IpcError {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

impl IpcError {
    pub fn message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            ..Self::default()
        }
    }

    /// Builds an error from the raw string a command rejected with. A string
    /// that is itself a JSON object with a `code` field is parsed into the
    /// structured form.
    pub fn from_raw(raw: &str) -> Self {
        let trimmed = raw.trim();
        if trimmed.starts_with('{') {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
                if let Some(obj) = value.as_object() {
                    let get = |k: &str| obj.get(k).and_then(|v| v.as_str()).map(str::to_string);
                    if obj.contains_key("code") || obj.contains_key("message") {
                        return Self {
                            code: get("code"),
                            message: get("message")
                                .or_else(|| get("context"))
                                .unwrap_or_else(|| trimmed.to_string()),
                            context: get("context"),
                            suggestion: get("suggestion"),
                        };
                    }
                }
            }
        }
        Self::message(raw)
    }
}

impl fmt::Display for IpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.code {
            Some(code) => write!(f, "[{code}] {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for IpcError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_and_structured_errors() {
        assert_eq!(IpcError::from_raw("boom"), IpcError::message("boom"));
        let structured = IpcError::from_raw(
            r#"{"code":"SKILL_EXISTS","context":"already installed","suggestion":"remove it"}"#,
        );
        assert_eq!(structured.code.as_deref(), Some("SKILL_EXISTS"));
        assert_eq!(structured.message, "already installed");
        assert_eq!(structured.suggestion.as_deref(), Some("remove it"));
        assert_eq!(structured.to_string(), "[SKILL_EXISTS] already installed");
        // Arbitrary JSON without code/message stays a plain message.
        assert_eq!(IpcError::from_raw("{\"x\":1}").message, "{\"x\":1}");
    }
}
