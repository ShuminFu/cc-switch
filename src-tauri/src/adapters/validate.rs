//! Field-level validation of provider settings before they are written to a
//! live config (CFG-24, first increment).
//!
//! Issues carry a dotted `path` into the tool's config, an i18n `key` and
//! `params`, so the editor can render an inline, localized marker. They are
//! advisory: nothing here blocks a write yet.

use serde::Serialize;
use serde_json::{json, Value};

use crate::app_config::AppType;

/// One problem found in a provider's settings.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ValidationIssue {
    /// Dotted path in the tool's config, e.g. `model_providers.custom.wire_api`.
    pub path: String,
    /// i18n key, e.g. `validation.codex.wireApi.invalid`.
    pub key: String,
    /// Interpolation values for the message (`value`, `allowed`, `expected`).
    pub params: Value,
}

/// Codex `wire_api` values (see the Codex config reference).
const CODEX_WIRE_APIS: &[&str] = &["responses", "chat"];

/// Claude Code environment variables cc-switch writes; a key that matches one
/// of these only case-insensitively is a typo the CLI silently ignores.
const CLAUDE_ENV_KEYS: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_CUSTOM_HEADERS",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_MODEL",
    "ANTHROPIC_SMALL_FAST_MODEL",
    "API_TIMEOUT_MS",
    "CLAUDE_CODE_MAX_OUTPUT_TOKENS",
    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
];

/// Gemini CLI environment variables cc-switch writes to `.env`.
const GEMINI_ENV_KEYS: &[&str] = &["GEMINI_API_KEY", "GOOGLE_GEMINI_BASE_URL", "GEMINI_MODEL"];

/// Validate `settings` (a provider `settings_config`) for `app`.
pub fn validate_provider_settings(app: &AppType, settings: &Value) -> Vec<ValidationIssue> {
    match app {
        AppType::Codex => settings
            .get("config")
            .and_then(Value::as_str)
            .map(validate_codex_config_text)
            .unwrap_or_default(),
        AppType::Claude => env_key_case_issues(settings, CLAUDE_ENV_KEYS),
        AppType::Gemini => env_key_case_issues(settings, GEMINI_ENV_KEYS),
        _ => Vec::new(),
    }
}

fn env_key_case_issues(settings: &Value, known: &[&str]) -> Vec<ValidationIssue> {
    let Some(env) = settings.get("env").and_then(Value::as_object) else {
        return Vec::new();
    };
    env.keys()
        .filter_map(|key| {
            let expected = known
                .iter()
                .find(|known| known.eq_ignore_ascii_case(key) && **known != key.as_str())?;
            Some(ValidationIssue {
                path: format!("env.{key}"),
                key: "validation.env.keyCase".to_string(),
                params: json!({ "value": key, "expected": expected }),
            })
        })
        .collect()
}

/// Field checks for a Codex `config.toml` text. A TOML syntax error is one
/// issue at the root (the write path already rejects it).
pub fn validate_codex_config_text(text: &str) -> Vec<ValidationIssue> {
    if text.trim().is_empty() {
        return Vec::new();
    }
    let table = match toml::from_str::<toml::Table>(text) {
        Ok(table) => table,
        Err(e) => {
            return vec![ValidationIssue {
                path: String::new(),
                key: "validation.toml.syntax".to_string(),
                params: json!({ "message": e.message() }),
            }]
        }
    };

    let mut issues = Vec::new();
    let mut check_wire_api = |path: String, value: Option<&toml::Value>| {
        let Some(value) = value else {
            return;
        };
        let valid = value.as_str().is_some_and(|v| CODEX_WIRE_APIS.contains(&v));
        if !valid {
            issues.push(ValidationIssue {
                path,
                key: "validation.codex.wireApi.invalid".to_string(),
                params: json!({
                    "value": value.as_str().map(str::to_string).unwrap_or_else(|| value.to_string()),
                    "allowed": CODEX_WIRE_APIS,
                }),
            });
        }
    };

    check_wire_api("wire_api".to_string(), table.get("wire_api"));
    if let Some(providers) = table.get("model_providers").and_then(toml::Value::as_table) {
        for (id, provider) in providers {
            check_wire_api(
                format!("model_providers.{id}.wire_api"),
                provider.get("wire_api"),
            );
        }
    }

    if let Some(active) = table.get("model_provider").and_then(toml::Value::as_str) {
        let custom = crate::codex_config::is_custom_codex_model_provider_id(active);
        let defined = table
            .get("model_providers")
            .and_then(toml::Value::as_table)
            .is_some_and(|providers| providers.contains_key(active));
        if custom && !defined {
            issues.push(ValidationIssue {
                path: "model_provider".to_string(),
                key: "validation.codex.modelProvider.undefined".to_string(),
                params: json!({ "value": active }),
            });
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_wire_api_typo_is_reported_at_its_path() {
        let text = r#"
model_provider = "custom"

[model_providers.custom]
base_url = "https://example.test/v1"
wire_api = "response"
"#;
        let issues = validate_codex_config_text(text);
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert_eq!(issues[0].path, "model_providers.custom.wire_api");
        assert_eq!(issues[0].key, "validation.codex.wireApi.invalid");
        assert_eq!(issues[0].params["value"], "response");
    }

    #[test]
    fn valid_codex_config_has_no_issues() {
        let text = r#"
model_provider = "custom"
[model_providers.custom]
wire_api = "responses"
"#;
        assert!(validate_codex_config_text(text).is_empty());
        assert!(validate_codex_config_text("").is_empty());
        // built-in providers need no table
        assert!(validate_codex_config_text("model_provider = \"openai\"").is_empty());
    }

    #[test]
    fn codex_undefined_custom_provider_and_syntax_errors() {
        let issues = validate_codex_config_text("model_provider = \"custom\"");
        assert_eq!(issues[0].path, "model_provider");
        let issues = validate_codex_config_text("model_provider = ");
        assert_eq!(issues[0].key, "validation.toml.syntax");
    }

    #[test]
    fn claude_env_key_case_typo_is_reported() {
        let settings = json!({ "env": {
            "ANTHROPIC_BASE_URl": "https://x",
            "ANTHROPIC_AUTH_TOKEN": "t",
            "MY_OWN_VAR": "1"
        }});
        let issues = validate_provider_settings(&AppType::Claude, &settings);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].path, "env.ANTHROPIC_BASE_URl");
        assert_eq!(issues[0].params["expected"], "ANTHROPIC_BASE_URL");
    }
}
