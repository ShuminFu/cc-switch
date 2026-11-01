//! Secret redaction shared by the history, audit and share-bundle features.
//!
//! Secrets are found by field name ([`is_secret_key`]) and,
//! inside embedded config text (Codex `config` TOML), by common token shapes
//! ([`crate::session_manager::index::redact_secrets`]). Two strategies:
//!
//! * [`mask`] replaces a secret with `<redacted:xxxxxxxx>` (a short SHA-256
//!   prefix), so a diff still shows *that* a key changed without revealing it;
//! * [`strip`] blanks it, for bundles that leave the machine.
//!
//! [`fill_masked`] restores masked positions from another value (the current
//! provider), which is how a history rollback keeps today's key.

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::secrets::provider_vault;

pub const MASK_PREFIX: &str = "<redacted:";

/// Field names whose string value is free-form config text that can embed
/// credentials (Codex stores its whole `config.toml` in `config`).
const EMBEDDED_TEXT_FIELDS: &[&str] = &["config"];

/// Field names whose string values are secrets: the vault's list plus the
/// names MCP server specs use for credentials (`GITHUB_TOKEN`,
/// `Authorization` headers, `*_PASSWORD`).
pub fn is_secret_key(name: &str) -> bool {
    let n = name.to_ascii_lowercase().replace('-', "_");
    provider_vault::is_secret_field(name)
        || n.ends_with("_token")
        || n.ends_with("_password")
        || n == "authorization"
        || n == "proxy_authorization"
        || n == "cookie"
}

fn short_hash(secret: &str) -> String {
    let digest = Sha256::digest(secret.as_bytes());
    digest[..4].iter().map(|b| format!("{b:02x}")).collect()
}

pub fn mask_secret(secret: &str) -> String {
    format!("{MASK_PREFIX}{}>", short_hash(secret))
}

pub fn is_masked(value: &str) -> bool {
    value.starts_with(MASK_PREFIX) && value.ends_with('>')
}

fn walk(value: &mut Value, replace: &dyn Fn(&str) -> String, count: &mut usize) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                match child {
                    Value::String(s) if !s.is_empty() && is_secret_key(key) => {
                        if !is_masked(s) && provider_vault::placeholder_ref(s).is_none() {
                            *s = replace(s);
                            *count += 1;
                        }
                    }
                    Value::String(s) if EMBEDDED_TEXT_FIELDS.contains(&key.as_str()) => {
                        let cleaned = crate::session_manager::index::redact_secrets(s);
                        if cleaned != *s {
                            *s = cleaned;
                            *count += 1;
                        }
                    }
                    _ => walk(child, replace, count),
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|v| walk(v, replace, count)),
        _ => {}
    }
}

/// Replace every secret with a short, non-reversible marker. Returns the
/// masked value and the number of secrets found.
pub fn mask(value: &Value) -> (Value, usize) {
    let mut out = value.clone();
    let mut count = 0;
    walk(&mut out, &mask_secret, &mut count);
    (out, count)
}

/// Blank every secret (vault placeholders are blanked too: they mean nothing
/// on another device). Returns the stripped value and the number of secrets.
pub fn strip(value: &Value) -> (Value, usize) {
    let mut out = value.clone();
    let mut count = 0;
    strip_placeholders(&mut out, &mut count);
    walk(&mut out, &|_| String::new(), &mut count);
    (out, count)
}

fn strip_placeholders(value: &mut Value, count: &mut usize) {
    match value {
        Value::String(s) if provider_vault::placeholder_ref(s).is_some() => {
            s.clear();
            *count += 1;
        }
        Value::Object(map) => map.values_mut().for_each(|v| strip_placeholders(v, count)),
        Value::Array(items) => items.iter_mut().for_each(|v| strip_placeholders(v, count)),
        _ => {}
    }
}

/// JSON pointers of blank secret fields (what a recipient must fill in).
pub fn missing_secret_pointers(value: &Value) -> Vec<String> {
    fn go(value: &Value, pointer: &str, out: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for (k, v) in map {
                    let child = format!("{pointer}/{}", k.replace('~', "~0").replace('/', "~1"));
                    match v {
                        Value::String(s) if s.is_empty() && is_secret_key(k) => out.push(child),
                        _ => go(v, &child, out),
                    }
                }
            }
            Value::Array(items) => {
                for (i, v) in items.iter().enumerate() {
                    go(v, &format!("{pointer}/{i}"), out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    go(value, "", &mut out);
    out
}

/// Replace masked markers in `target` with the value at the same position of
/// `source`; markers without a counterpart become empty strings. Returns the
/// pointers that could not be filled.
pub fn fill_masked(target: &mut Value, source: Option<&Value>) -> Vec<String> {
    fn go(target: &mut Value, source: Option<&Value>, pointer: &str, missing: &mut Vec<String>) {
        match target {
            Value::String(s) if is_masked(s) => match source.and_then(|src| src.pointer(pointer)) {
                Some(Value::String(v)) if !v.is_empty() => *s = v.clone(),
                _ => {
                    s.clear();
                    missing.push(pointer.to_string());
                }
            },
            Value::Object(map) => {
                for (k, v) in map.iter_mut() {
                    let child = format!("{pointer}/{}", k.replace('~', "~0").replace('/', "~1"));
                    go(v, source, &child, missing);
                }
            }
            Value::Array(items) => {
                for (i, v) in items.iter_mut().enumerate() {
                    go(v, source, &format!("{pointer}/{i}"), missing);
                }
            }
            _ => {}
        }
    }
    let mut missing = Vec::new();
    go(target, source, "", &mut missing);
    missing
}

/// SHA-256 of a value with object keys sorted, so equal documents hash equal
/// regardless of key order.
pub fn canonical_hash(value: &Value) -> String {
    fn canonical(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                let mut out = serde_json::Map::new();
                for k in keys {
                    out.insert(k.clone(), canonical(&map[k]));
                }
                Value::Object(out)
            }
            Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
            other => other.clone(),
        }
    }
    let text = serde_json::to_string(&canonical(value)).unwrap_or_default();
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Value {
        json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "sk-ant-very-secret-123456",
                "ANTHROPIC_BASE_URL": "https://relay.example.com"
            },
            "auth": { "OPENAI_API_KEY": "sk-openai-abcdef" },
            "config": "model = \"gpt\"\nexperimental_bearer_token = \"sk-proj-abcdefghijklmnopqrstuvwx\"\n"
        })
    }

    #[test]
    fn mask_hides_secrets_but_keeps_other_fields() {
        let (masked, n) = mask(&sample());
        let text = masked.to_string();
        assert!(n >= 2);
        assert!(!text.contains("sk-ant-very-secret"));
        assert!(!text.contains("sk-openai-abcdef"));
        assert!(text.contains("relay.example.com"));
        assert!(is_masked(
            masked["env"]["ANTHROPIC_AUTH_TOKEN"].as_str().unwrap()
        ));
        // Same secret, same marker: diffs stay stable.
        assert_eq!(mask(&sample()).0, masked);
    }

    #[test]
    fn strip_blanks_secrets_and_reports_them_missing() {
        let (stripped, _) = strip(&sample());
        assert_eq!(stripped["env"]["ANTHROPIC_AUTH_TOKEN"], "");
        assert_eq!(
            stripped["env"]["ANTHROPIC_BASE_URL"],
            "https://relay.example.com"
        );
        let missing = missing_secret_pointers(&stripped);
        assert!(missing.contains(&"/env/ANTHROPIC_AUTH_TOKEN".to_string()));
        assert!(missing.contains(&"/auth/OPENAI_API_KEY".to_string()));
    }

    #[test]
    fn fill_masked_takes_current_values() {
        let (mut masked, _) = mask(&sample());
        let current = json!({ "env": { "ANTHROPIC_AUTH_TOKEN": "sk-new" } });
        let missing = fill_masked(&mut masked, Some(&current));
        assert_eq!(masked["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-new");
        assert_eq!(masked["auth"]["OPENAI_API_KEY"], "");
        assert_eq!(missing, vec!["/auth/OPENAI_API_KEY".to_string()]);
    }

    #[test]
    fn mcp_credentials_are_secrets() {
        for k in ["GITHUB_TOKEN", "Authorization", "DB_PASSWORD", "apiKey"] {
            assert!(is_secret_key(k), "{k}");
        }
        for k in ["max_tokens", "command", "url", "token_limit"] {
            assert!(!is_secret_key(k), "{k}");
        }
    }

    #[test]
    fn canonical_hash_ignores_key_order() {
        let a = json!({"a": 1, "b": {"c": 2, "d": 3}});
        let b = json!({"b": {"d": 3, "c": 2}, "a": 1});
        assert_eq!(canonical_hash(&a), canonical_hash(&b));
        assert_ne!(canonical_hash(&a), canonical_hash(&json!({"a": 2})));
    }
}
