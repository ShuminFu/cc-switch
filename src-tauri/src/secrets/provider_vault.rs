//! Provider API-key vault (DB-02 / PRODUCT-01): abstraction and migrations.
//!
//! Secret fields of a provider's `settings_config` JSON (API keys, auth
//! tokens, OAuth tokens, passwords) are moved into the OS keychain and replaced
//! in the JSON by a string placeholder:
//!
//! ```text
//! "cc-switch-secret:v1:<app>/<provider id>/<JSON pointer>"
//! ```
//!
//! A string placeholder keeps the JSON shape intact, so code that expects a
//! string never panics, and a placeholder that leaks into a live config is an
//! obviously invalid key rather than a crash. The keychain account is the part
//! after the `cc-switch-secret:v1:` prefix, prefixed with `provider/`.
//!
//! This module is the building block only: it is not wired into the provider
//! DAO yet, and the feature flag defaults OFF. `docs/secrets-vault.md` lists
//! the remaining integration steps (DAO read/write, live-file writes, SQL
//! export/sync, backups, deletion) and why they are needed for a safe rollout.

// Not wired into the provider DAO yet (see docs/secrets-vault.md); only the
// unit tests use this module for now.
#![allow(dead_code)]

use serde_json::Value;

use super::{SecretBackend, SecretError};

/// Prefix of every placeholder string.
pub const PLACEHOLDER_PREFIX: &str = "cc-switch-secret:v1:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VaultError {
    /// The keychain could not be used; nothing was changed.
    Backend(SecretError),
    /// A placeholder refers to a keychain entry that does not exist.
    MissingSecret(String),
}

impl std::fmt::Display for VaultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Backend(e) => write!(f, "{e}"),
            Self::MissingSecret(r) => write!(f, "secret {r} is missing from the keychain"),
        }
    }
}

impl std::error::Error for VaultError {}

impl From<SecretError> for VaultError {
    fn from(e: SecretError) -> Self {
        Self::Backend(e)
    }
}

/// Field names whose string values are treated as secrets.
pub fn is_secret_field(name: &str) -> bool {
    let n = name.to_ascii_lowercase().replace('-', "_");
    n.ends_with("api_key")
        || n.ends_with("apikey")
        || n.ends_with("auth_token")
        || n.ends_with("access_token")
        || n.ends_with("refresh_token")
        || n.ends_with("id_token")
        || n.ends_with("bearer_token")
        || n.ends_with("_secret")
        || n.ends_with("secret_key")
        || n.ends_with("secret_access_key")
        || n == "token"
        || n == "secret"
        || n == "password"
}

/// Returns the reference inside a placeholder string, if `value` is one.
pub fn placeholder_ref(value: &str) -> Option<&str> {
    value.strip_prefix(PLACEHOLDER_PREFIX)
}

fn escape_pointer_segment(seg: &str) -> String {
    seg.replace('~', "~0").replace('/', "~1")
}

fn sanitize_ref_part(part: &str) -> String {
    part.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Placeholder reference for one field.
pub fn secret_ref(app: &str, provider_id: &str, pointer: &str) -> String {
    format!(
        "{}/{}{}",
        sanitize_ref_part(app),
        sanitize_ref_part(provider_id),
        pointer
    )
}

fn account_for(reference: &str) -> String {
    format!("provider/{reference}")
}

/// Collect `(JSON pointer, value)` for every non-empty secret string.
fn collect_secrets(value: &Value, pointer: &str, out: &mut Vec<(String, String)>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                let child = format!("{pointer}/{}", escape_pointer_segment(k));
                match v {
                    Value::String(s)
                        if is_secret_field(k) && !s.is_empty() && placeholder_ref(s).is_none() =>
                    {
                        out.push((child, s.clone()))
                    }
                    _ => collect_secrets(v, &child, out),
                }
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                collect_secrets(v, &format!("{pointer}/{i}"), out);
            }
        }
        _ => {}
    }
}

fn collect_placeholders(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(s) => {
            if let Some(r) = placeholder_ref(s) {
                out.push(r.to_string());
            }
        }
        Value::Object(map) => map.values().for_each(|v| collect_placeholders(v, out)),
        Value::Array(items) => items.iter().for_each(|v| collect_placeholders(v, out)),
        _ => {}
    }
}

fn replace_placeholders(
    value: &mut Value,
    resolve: &mut dyn FnMut(&str) -> Result<String, VaultError>,
) -> Result<usize, VaultError> {
    match value {
        Value::String(s) => match placeholder_ref(s) {
            Some(r) => {
                let secret = resolve(r)?;
                *s = secret;
                Ok(1)
            }
            None => Ok(0),
        },
        Value::Object(map) => {
            let mut n = 0;
            for v in map.values_mut() {
                n += replace_placeholders(v, resolve)?;
            }
            Ok(n)
        }
        Value::Array(items) => {
            let mut n = 0;
            for v in items.iter_mut() {
                n += replace_placeholders(v, resolve)?;
            }
            Ok(n)
        }
        _ => Ok(0),
    }
}

/// Move the secrets of one provider config into the vault.
///
/// All keychain writes happen before the JSON is changed: on any failure the
/// JSON is untouched (entries already written are removed again). Returns the
/// number of fields moved.
pub fn extract(
    backend: &dyn SecretBackend,
    app: &str,
    provider_id: &str,
    config: &mut Value,
) -> Result<usize, VaultError> {
    let mut found = Vec::new();
    collect_secrets(config, "", &mut found);
    let mut written: Vec<String> = Vec::new();
    for (pointer, secret) in &found {
        let reference = secret_ref(app, provider_id, pointer);
        if let Err(e) = backend.set(&account_for(&reference), secret) {
            for done in &written {
                let _ = backend.delete(&account_for(done));
            }
            return Err(e.into());
        }
        written.push(reference);
    }
    for ((pointer, _), reference) in found.iter().zip(&written) {
        if let Some(slot) = config.pointer_mut(pointer) {
            *slot = Value::String(format!("{PLACEHOLDER_PREFIX}{reference}"));
        }
    }
    Ok(found.len())
}

/// Return a copy of `config` with every placeholder replaced by its secret.
/// Fails without partial results when an entry is missing.
pub fn resolve(backend: &dyn SecretBackend, config: &Value) -> Result<Value, VaultError> {
    let mut out = config.clone();
    replace_placeholders(&mut out, &mut |r| {
        backend
            .get(&account_for(r))?
            .ok_or_else(|| VaultError::MissingSecret(r.to_string()))
    })?;
    Ok(out)
}

/// Move secrets back from the vault into `config` (the opposite migration).
/// Keychain entries are deleted only after every placeholder resolved.
pub fn restore(backend: &dyn SecretBackend, config: &mut Value) -> Result<usize, VaultError> {
    let resolved = resolve(backend, config)?;
    let mut refs = Vec::new();
    collect_placeholders(config, &mut refs);
    *config = resolved;
    for r in &refs {
        if let Err(e) = backend.delete(&account_for(r)) {
            log::warn!("[Vault] Failed to delete keychain entry for {r}: {e}");
        }
    }
    Ok(refs.len())
}

/// Delete every vault entry referenced by `config` (provider deleted).
pub fn forget(backend: &dyn SecretBackend, config: &Value) {
    let mut refs = Vec::new();
    collect_placeholders(config, &mut refs);
    for r in refs {
        let _ = backend.delete(&account_for(&r));
    }
}

/// One provider row for the bulk migrations.
pub struct ProviderRow {
    pub app: String,
    pub id: String,
    pub settings_config: Value,
}

/// Migrate every row into the vault (feature switched ON). Rows are changed
/// only after all writes for that row succeeded; stops at the first failure
/// and returns how many rows were migrated, so callers can persist exactly
/// those and retry later.
pub fn migrate_rows_to_vault(
    backend: &dyn SecretBackend,
    rows: &mut [ProviderRow],
) -> (usize, Option<VaultError>) {
    for (i, row) in rows.iter_mut().enumerate() {
        if let Err(e) = extract(backend, &row.app, &row.id, &mut row.settings_config) {
            return (i, Some(e));
        }
    }
    (rows.len(), None)
}

/// Migrate every row back to inline secrets (feature switched OFF). Same
/// stop-at-first-failure contract as [`migrate_rows_to_vault`].
pub fn migrate_rows_from_vault(
    backend: &dyn SecretBackend,
    rows: &mut [ProviderRow],
) -> (usize, Option<VaultError>) {
    for (i, row) in rows.iter_mut().enumerate() {
        if let Err(e) = restore(backend, &mut row.settings_config) {
            return (i, Some(e));
        }
    }
    (rows.len(), None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::{MemoryBackend, UnavailableBackend};
    use serde_json::json;

    fn claude_config() -> Value {
        json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "sk-ant-secret",
                "ANTHROPIC_BASE_URL": "https://api.example.com",
                "ANTHROPIC_MODEL": "claude"
            },
            "auth": { "OPENAI_API_KEY": "sk-openai", "tokens": { "refresh_token": "rt-1" } },
            "list": [{ "apiKey": "k-in-array" }],
            "empty": { "api_key": "" }
        })
    }

    #[test]
    fn detects_common_secret_field_names() {
        for name in [
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_API_KEY",
            "OPENAI_API_KEY",
            "GEMINI_API_KEY",
            "apiKey",
            "api-key",
            "refresh_token",
            "access_token",
            "client_secret",
            "password",
        ] {
            assert!(is_secret_field(name), "{name}");
        }
        for name in ["ANTHROPIC_BASE_URL", "model", "max_tokens", "token_limit"] {
            assert!(!is_secret_field(name), "{name}");
        }
    }

    #[test]
    fn extract_replaces_secrets_with_placeholders_and_resolve_restores_them() {
        let backend = MemoryBackend::new();
        let original = claude_config();
        let mut config = original.clone();
        let n = extract(&backend, "claude", "p1", &mut config).unwrap();
        assert_eq!(n, 4);
        let text = config.to_string();
        for secret in ["sk-ant-secret", "sk-openai", "rt-1", "k-in-array"] {
            assert!(!text.contains(secret), "{secret} still inline: {text}");
        }
        assert_eq!(
            config["env"]["ANTHROPIC_AUTH_TOKEN"],
            json!("cc-switch-secret:v1:claude/p1/env/ANTHROPIC_AUTH_TOKEN")
        );
        assert_eq!(
            config["env"]["ANTHROPIC_BASE_URL"],
            original["env"]["ANTHROPIC_BASE_URL"]
        );
        assert_eq!(config["empty"]["api_key"], json!(""));

        assert_eq!(resolve(&backend, &config).unwrap(), original);
        // Extracting again is a no-op.
        assert_eq!(extract(&backend, "claude", "p1", &mut config).unwrap(), 0);
    }

    #[test]
    fn round_trip_migration_in_both_directions() {
        let backend = MemoryBackend::new();
        let mut rows = vec![
            ProviderRow {
                app: "claude".into(),
                id: "a".into(),
                settings_config: claude_config(),
            },
            ProviderRow {
                app: "codex".into(),
                id: "b/../x".into(),
                settings_config: json!({ "auth": { "OPENAI_API_KEY": "sk-b" } }),
            },
        ];
        let before: Vec<Value> = rows.iter().map(|r| r.settings_config.clone()).collect();
        assert_eq!(migrate_rows_to_vault(&backend, &mut rows), (2, None));
        assert!(!rows[1].settings_config.to_string().contains("sk-b"));
        // Provider ids are sanitised in the reference.
        assert!(rows[1]
            .settings_config
            .to_string()
            .contains("codex/b_.._x/"));

        assert_eq!(migrate_rows_from_vault(&backend, &mut rows), (2, None));
        let after: Vec<Value> = rows.iter().map(|r| r.settings_config.clone()).collect();
        assert_eq!(after, before);
        // Entries were removed after restoring.
        assert_eq!(
            backend
                .get("provider/codex/b_.._x/auth/OPENAI_API_KEY")
                .unwrap(),
            None
        );
    }

    #[test]
    fn extract_without_keychain_leaves_config_untouched() {
        let mut config = claude_config();
        let err = extract(&UnavailableBackend, "claude", "p1", &mut config).unwrap_err();
        assert!(matches!(
            err,
            VaultError::Backend(SecretError::Unavailable(_))
        ));
        assert_eq!(config, claude_config());
    }

    #[test]
    fn missing_entry_fails_restore_without_partial_changes() {
        let backend = MemoryBackend::new();
        let mut config = claude_config();
        extract(&backend, "claude", "p1", &mut config).unwrap();
        backend
            .delete("provider/claude/p1/auth/OPENAI_API_KEY")
            .unwrap();
        let snapshot = config.clone();
        assert!(matches!(
            restore(&backend, &mut config),
            Err(VaultError::MissingSecret(_))
        ));
        assert_eq!(config, snapshot);
        // Remaining entries are kept for a later retry.
        assert!(backend
            .get("provider/claude/p1/env/ANTHROPIC_AUTH_TOKEN")
            .unwrap()
            .is_some());
    }

    #[test]
    fn forget_deletes_all_entries() {
        let backend = MemoryBackend::new();
        let mut config = claude_config();
        extract(&backend, "claude", "p1", &mut config).unwrap();
        forget(&backend, &config);
        assert!(matches!(
            resolve(&backend, &config),
            Err(VaultError::MissingSecret(_))
        ));
    }

    #[test]
    fn json_pointer_segments_are_escaped() {
        let backend = MemoryBackend::new();
        let mut config = json!({ "a/b~c": { "token": "t" } });
        extract(&backend, "x", "y", &mut config).unwrap();
        assert_eq!(
            config["a/b~c"]["token"],
            json!("cc-switch-secret:v1:x/y/a~1b~0c/token")
        );
        assert_eq!(
            resolve(&backend, &config).unwrap(),
            json!({ "a/b~c": { "token": "t" } })
        );
    }
}
