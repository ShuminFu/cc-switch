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
//! Integration (see `docs/secrets-vault.md`):
//! - the provider DAO calls [`for_read`] on every `settings_config` it loads,
//!   so the rest of the app (live-config writers, proxy, UI) keeps seeing real
//!   values, and [`for_storage`] before it persists one;
//! - the flag (`AppSettings::provider_secret_vault`, default OFF) is changed
//!   only by `provider_vault_admin`, which migrates the existing rows;
//! - SQL export / sync / import adapt placeholders through
//!   [`redact_for_export`] and [`adapt_imported`].

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

use super::{SecretBackend, SecretError};

/// Prefix of every placeholder string.
pub const PLACEHOLDER_PREFIX: &str = "cc-switch-secret:v1:";

/// App name used in references of universal providers (`universal/<id>/apiKey`).
pub const UNIVERSAL_APP: &str = "universal";

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
#[cfg_attr(not(test), allow(dead_code))]
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
#[cfg_attr(not(test), allow(dead_code))]
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

/// Result of [`migrate_rows_from_vault`].
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RestoreOutcome {
    /// References that were inlined; the caller deletes their keychain
    /// entries (with [`delete_refs`]) once the rows are persisted.
    pub restored: Vec<String>,
    /// `(app, provider id, reference)` of placeholders that stay because the
    /// entry is missing or the keychain cannot be read.
    pub unresolved: Vec<(String, String, String)>,
}

/// Migrate every row back to inline secrets (feature switched OFF). Unlike
/// [`restore`], one missing entry does not block the other rows or fields:
/// every resolvable placeholder is inlined, the rest is reported. Keychain
/// entries are not deleted here, so nothing is lost if persisting fails.
pub fn migrate_rows_from_vault(
    backend: &dyn SecretBackend,
    rows: &mut [ProviderRow],
) -> RestoreOutcome {
    let mut outcome = RestoreOutcome::default();
    for row in rows.iter_mut() {
        let before = placeholder_refs(&row.settings_config);
        if before.is_empty() {
            continue;
        }
        let (resolved, unresolved) = resolve_lenient(backend, &row.settings_config);
        row.settings_config = resolved;
        let prefix = owned_prefix(&row.app, &row.id);
        for r in before {
            if unresolved.contains(&r) {
                continue;
            }
            if r.starts_with(&prefix) && !outcome.restored.contains(&r) {
                outcome.restored.push(r);
            }
        }
        for r in unresolved {
            outcome
                .unresolved
                .push((row.app.clone(), row.id.clone(), r));
        }
    }
    outcome
}

/// Delete the keychain entries of the given references.
pub fn delete_refs(backend: &dyn SecretBackend, refs: &[String]) {
    for r in refs {
        if let Err(e) = backend.delete(&account_for(r)) {
            log::warn!("[Vault] Failed to delete keychain entry for {r}: {e}");
        }
    }
}

/// True when the keychain holds the entry of `reference`.
pub fn ref_exists(backend: &dyn SecretBackend, reference: &str) -> bool {
    matches!(backend.get(&account_for(reference)), Ok(Some(_)))
}

// ─── Runtime integration ─────────────────────────────────────

#[cfg(test)]
thread_local! {
    static ENABLED_OVERRIDE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Force the flag for the current test thread (`None` = read the setting).
#[cfg(test)]
pub(crate) fn set_enabled_for_test(value: Option<bool>) {
    ENABLED_OVERRIDE.with(|c| c.set(value));
}

/// Whether new writes move secrets into the vault.
pub fn enabled() -> bool {
    #[cfg(test)]
    if let Some(v) = ENABLED_OVERRIDE.with(|c| c.get()) {
        return v;
    }
    crate::settings::provider_secret_vault_enabled()
}

/// The process secret backend with a read-through cache, so resolving
/// placeholders on every DAO read does not hit the keychain each time.
struct CachedBackend {
    cache: Mutex<HashMap<String, String>>,
}

impl CachedBackend {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.cache.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl SecretBackend for CachedBackend {
    fn name(&self) -> &'static str {
        super::backend().name()
    }
    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        if let Some(v) = self.lock().get(account) {
            return Ok(Some(v.clone()));
        }
        let value = super::backend().get(account)?;
        if let Some(v) = &value {
            self.lock().insert(account.to_string(), v.clone());
        }
        Ok(value)
    }
    fn set(&self, account: &str, value: &str) -> Result<(), SecretError> {
        if self.lock().get(account).map(String::as_str) == Some(value) {
            return Ok(());
        }
        super::backend().set(account, value)?;
        self.lock().insert(account.to_string(), value.to_string());
        Ok(())
    }
    fn delete(&self, account: &str) -> Result<(), SecretError> {
        self.lock().remove(account);
        super::backend().delete(account)
    }
}

/// Backend used by the DAO, the migrations and export/import.
pub fn vault_backend() -> &'static dyn SecretBackend {
    static CACHED: OnceLock<CachedBackend> = OnceLock::new();
    CACHED.get_or_init(|| CachedBackend {
        cache: Mutex::new(HashMap::new()),
    })
}

/// Probe account written and removed by [`check_available`].
const PROBE_ACCOUNT: &str = "provider/__cc_switch_probe__";

/// Check that the backend can store and return a value. Used before the
/// vault is switched on: without a keychain (headless Linux without a
/// Secret Service) it must refuse instead of losing keys.
pub fn check_available(backend: &dyn SecretBackend) -> Result<(), SecretError> {
    let probe = format!("probe-{}", std::process::id());
    backend.set(PROBE_ACCOUNT, &probe)?;
    let read = backend.get(PROBE_ACCOUNT);
    let _ = backend.delete(PROBE_ACCOUNT);
    match read? {
        Some(v) if v == probe => Ok(()),
        _ => Err(SecretError::Unavailable(
            "the keychain did not return the stored value".to_string(),
        )),
    }
}

/// True when any string in `value` is a placeholder.
pub fn contains_placeholder(value: &Value) -> bool {
    match value {
        Value::String(s) => placeholder_ref(s).is_some(),
        Value::Object(map) => map.values().any(contains_placeholder),
        Value::Array(items) => items.iter().any(contains_placeholder),
        _ => false,
    }
}

/// Every placeholder reference in `value`.
pub fn placeholder_refs(value: &Value) -> Vec<String> {
    let mut out = Vec::new();
    collect_placeholders(value, &mut out);
    out
}

/// Number of inline (not yet vaulted) secret values in `value`.
pub fn inline_secret_count(value: &Value) -> usize {
    let mut out = Vec::new();
    collect_secrets(value, "", &mut out);
    out.len()
}

/// Reference prefix owned by one provider (`<app>/<id>/`).
pub fn owned_prefix(app: &str, provider_id: &str) -> String {
    format!("{}/", secret_ref(app, provider_id, ""))
}

/// Replace every placeholder that can be resolved; leave the others in place
/// and return their references. Never fails as a whole.
pub fn resolve_lenient(backend: &dyn SecretBackend, config: &Value) -> (Value, Vec<String>) {
    let mut out = config.clone();
    let mut unresolved = Vec::new();
    resolve_lenient_in_place(backend, &mut out, &mut unresolved);
    (out, unresolved)
}

fn resolve_lenient_in_place(
    backend: &dyn SecretBackend,
    value: &mut Value,
    unresolved: &mut Vec<String>,
) {
    match value {
        Value::String(s) => {
            if let Some(r) = placeholder_ref(s) {
                match backend.get(&account_for(r)) {
                    Ok(Some(secret)) => *s = secret,
                    _ => unresolved.push(r.to_string()),
                }
            }
        }
        Value::Object(map) => map
            .values_mut()
            .for_each(|v| resolve_lenient_in_place(backend, v, unresolved)),
        Value::Array(items) => items
            .iter_mut()
            .for_each(|v| resolve_lenient_in_place(backend, v, unresolved)),
        _ => {}
    }
}

/// DAO read path: replace placeholders with the real values. Unresolvable
/// placeholders (missing entry on this device, locked keychain) are kept, so
/// saving the provider back never loses the reference, and a placeholder that
/// reaches a client config is an obviously invalid key.
pub fn for_read(app: &str, provider_id: &str, config: Value) -> Value {
    for_read_with(vault_backend(), app, provider_id, config)
}

pub(crate) fn for_read_with(
    backend: &dyn SecretBackend,
    app: &str,
    provider_id: &str,
    config: Value,
) -> Value {
    if !contains_placeholder(&config) {
        return config;
    }
    let (resolved, unresolved) = resolve_lenient(backend, &config);
    if !unresolved.is_empty() {
        log::warn!(
            "[Vault] {} secret(s) of provider {app}/{provider_id} are not in this device's keychain; re-enter them",
            unresolved.len()
        );
    }
    resolved
}

/// DAO write path: when the vault is on, return a copy with the secrets moved
/// into the keychain. If the keychain fails, the values are stored inline (as
/// with the vault off) and a warning is logged; the settings panel reports
/// them and switching the vault on again retries.
pub fn for_storage(app: &str, provider_id: &str, config: &Value) -> Value {
    for_storage_with(enabled(), vault_backend(), app, provider_id, config)
}

pub(crate) fn for_storage_with(
    enabled: bool,
    backend: &dyn SecretBackend,
    app: &str,
    provider_id: &str,
    config: &Value,
) -> Value {
    if !enabled {
        return config.clone();
    }
    let mut out = config.clone();
    if let Err(e) = extract(backend, app, provider_id, &mut out) {
        log::warn!(
            "[Vault] Could not move secrets of provider {app}/{provider_id} to the keychain ({e}); stored inline"
        );
        return config.clone();
    }
    out
}

/// Delete the entries this provider owns that `previous` referenced but
/// `next` (the value now stored) no longer does. `next = None` (provider
/// deleted) removes all of them. Entries of other providers are never touched.
pub fn forget_stale(app: &str, provider_id: &str, previous: &Value, next: Option<&Value>) {
    forget_stale_with(vault_backend(), app, provider_id, previous, next)
}

pub(crate) fn forget_stale_with(
    backend: &dyn SecretBackend,
    app: &str,
    provider_id: &str,
    previous: &Value,
    next: Option<&Value>,
) {
    let prefix = owned_prefix(app, provider_id);
    let keep = next.map(placeholder_refs).unwrap_or_default();
    let stale: Vec<String> = placeholder_refs(previous)
        .into_iter()
        .filter(|r| r.starts_with(&prefix) && !keep.contains(r))
        .collect();
    delete_refs(backend, &stale);
}

fn pointer_join(pointer: &str, seg: &str) -> String {
    format!("{pointer}/{}", escape_pointer_segment(seg))
}

/// Replace placeholders that cannot be resolved here with the value this
/// device already has at the same position of `local` (the row being
/// replaced by the import).
fn fill_from_local(backend: &dyn SecretBackend, value: &mut Value, local: &Value, pointer: &str) {
    match value {
        Value::String(s) => {
            let Some(r) = placeholder_ref(s) else {
                return;
            };
            if ref_exists(backend, r) {
                return;
            }
            let candidate = match local.pointer(pointer) {
                Some(Value::String(l)) if !l.is_empty() => match placeholder_ref(l) {
                    Some(lr) => backend.get(&account_for(lr)).ok().flatten(),
                    None => Some(l.clone()),
                },
                _ => None,
            };
            if let Some(c) = candidate {
                *s = c;
            }
        }
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                fill_from_local(backend, v, local, &pointer_join(pointer, k));
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter_mut().enumerate() {
                fill_from_local(backend, v, local, &format!("{pointer}/{i}"));
            }
        }
        _ => {}
    }
}

/// Bring a provider config that arrives from an SQL import, a sync download
/// or a backup restore in line with this device:
/// - a placeholder whose keychain entry is missing here takes the value this
///   device already holds for the same provider and field, if any (so a sync
///   download never wipes a key this device has);
/// - with the vault on, inline secrets are moved into the keychain;
/// - with the vault off, resolvable placeholders become inline values again.
///
/// Returns the adapted config and the references that stay unresolved (the
/// user must re-enter those keys).
pub fn adapt_imported(
    enabled: bool,
    backend: &dyn SecretBackend,
    app: &str,
    provider_id: &str,
    incoming: &Value,
    local: Option<&Value>,
) -> (Value, Vec<String>) {
    let mut value = incoming.clone();
    if !contains_placeholder(&value) && !enabled {
        return (value, Vec::new());
    }
    if let Some(local) = local {
        fill_from_local(backend, &mut value, local, "");
    }
    let (mut value, _) = resolve_lenient(backend, &value);
    if enabled {
        if let Err(e) = extract(backend, app, provider_id, &mut value) {
            log::warn!(
                "[Vault] Could not move imported secrets of {app}/{provider_id} to the keychain: {e}"
            );
        }
    }
    let unresolved = placeholder_refs(&value)
        .into_iter()
        .filter(|r| !ref_exists(backend, r))
        .collect();
    (value, unresolved)
}

/// Prepare a config for an SQL export or a sync snapshot while the vault is
/// on: placeholders stay, and any secret still stored inline (its keychain
/// write failed earlier) is blanked, so the result never contains a resolved
/// secret. Export has no keychain side effects. Returns the config and the
/// number of values blanked.
pub fn redact_for_export(config: &Value) -> (Value, usize) {
    let mut value = config.clone();
    let mut found = Vec::new();
    collect_secrets(&value, "", &mut found);
    for (pointer, _) in &found {
        if let Some(slot) = value.pointer_mut(pointer) {
            *slot = Value::String(String::new());
        }
    }
    (value, found.len())
}

/// SQL comment lines added to an export that carries placeholders.
pub fn export_warning_comment(placeholders: usize, blanked: usize) -> String {
    let mut out = format!(
        "-- cc-switch-vault: {placeholders} provider secret(s) live in the OS keychain of the exporting device; this file holds only `{PLACEHOLDER_PREFIX}...` placeholders.\n\
         -- cc-switch-vault: on another device these API keys must be re-entered (a device that already has the provider keeps its own key).\n"
    );
    if blanked > 0 {
        out.push_str(&format!(
            "-- cc-switch-vault: {blanked} secret(s) could not be moved to the keychain and were left out of this export.\n"
        ));
    }
    out
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

        let outcome = migrate_rows_from_vault(&backend, &mut rows);
        assert!(outcome.unresolved.is_empty());
        assert_eq!(outcome.restored.len(), 5);
        let after: Vec<Value> = rows.iter().map(|r| r.settings_config.clone()).collect();
        assert_eq!(after, before);
        // Entries survive until the caller has persisted the rows.
        assert!(ref_exists(&backend, "codex/b_.._x/auth/OPENAI_API_KEY"));
        delete_refs(&backend, &outcome.restored);
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

    #[test]
    fn restore_from_vault_continues_past_missing_entries() {
        let backend = MemoryBackend::new();
        let mut rows = vec![
            ProviderRow {
                app: "claude".into(),
                id: "lost".into(),
                settings_config: json!({ "env": { "ANTHROPIC_API_KEY": "k1", "OTHER_API_KEY": "k2" } }),
            },
            ProviderRow {
                app: "claude".into(),
                id: "ok".into(),
                settings_config: json!({ "env": { "ANTHROPIC_API_KEY": "k3" } }),
            },
        ];
        migrate_rows_to_vault(&backend, &mut rows);
        backend
            .delete("provider/claude/lost/env/OTHER_API_KEY")
            .unwrap();
        let outcome = migrate_rows_from_vault(&backend, &mut rows);
        assert_eq!(
            outcome.unresolved,
            vec![(
                "claude".to_string(),
                "lost".to_string(),
                "claude/lost/env/OTHER_API_KEY".to_string()
            )]
        );
        assert_eq!(
            rows[0].settings_config["env"]["ANTHROPIC_API_KEY"],
            json!("k1")
        );
        assert!(placeholder_ref(
            rows[0].settings_config["env"]["OTHER_API_KEY"]
                .as_str()
                .unwrap()
        )
        .is_some());
        assert_eq!(
            rows[1].settings_config["env"]["ANTHROPIC_API_KEY"],
            json!("k3")
        );
    }

    #[test]
    fn storage_and_read_paths_round_trip() {
        let backend = MemoryBackend::new();
        let config = claude_config();
        assert_eq!(
            for_storage_with(false, &backend, "claude", "p", &config),
            config
        );
        let stored = for_storage_with(true, &backend, "claude", "p", &config);
        assert!(!stored.to_string().contains("sk-ant-secret"));
        assert_eq!(
            for_read_with(&backend, "claude", "p", stored.clone()),
            config
        );
        // Keychain failure: stored inline rather than losing the key.
        assert_eq!(
            for_storage_with(true, &UnavailableBackend, "claude", "p", &config),
            config
        );
        // Unresolvable placeholders are kept, not blanked.
        let other = MemoryBackend::new();
        assert_eq!(for_read_with(&other, "claude", "p", stored.clone()), stored);
    }

    #[test]
    fn forget_stale_only_touches_own_entries() {
        let backend = MemoryBackend::new();
        let mut mine = json!({ "env": { "A_API_KEY": "a", "B_API_KEY": "b" } });
        extract(&backend, "claude", "p", &mut mine).unwrap();
        let mut foreign = json!({ "env": { "A_API_KEY": "x" } });
        extract(&backend, "claude", "p2", &mut foreign).unwrap();
        let mut previous = mine.clone();
        previous["env"]["C_API_KEY"] = foreign["env"]["A_API_KEY"].clone();
        let mut next = mine.clone();
        next["env"].as_object_mut().unwrap().remove("B_API_KEY");
        forget_stale_with(&backend, "claude", "p", &previous, Some(&next));
        assert!(ref_exists(&backend, "claude/p/env/A_API_KEY"));
        assert!(!ref_exists(&backend, "claude/p/env/B_API_KEY"));
        assert!(ref_exists(&backend, "claude/p2/env/A_API_KEY"));
        forget_stale_with(&backend, "claude", "p", &previous, None);
        assert!(!ref_exists(&backend, "claude/p/env/A_API_KEY"));
        assert!(ref_exists(&backend, "claude/p2/env/A_API_KEY"));
    }

    #[test]
    fn imported_placeholders_keep_the_local_key() {
        // Device A (vault on) exported placeholders; device B has no entries.
        let device_a = MemoryBackend::new();
        let mut exported = json!({ "env": { "ANTHROPIC_API_KEY": "key-a" } });
        extract(&device_a, "claude", "p", &mut exported).unwrap();

        let device_b = MemoryBackend::new();
        let local = json!({ "env": { "ANTHROPIC_API_KEY": "key-b" } });
        let (adapted, unresolved) =
            adapt_imported(false, &device_b, "claude", "p", &exported, Some(&local));
        assert_eq!(adapted, local);
        assert!(unresolved.is_empty());

        // Vault on at B: the local key moves to B's keychain.
        let (adapted, unresolved) =
            adapt_imported(true, &device_b, "claude", "p", &exported, Some(&local));
        assert!(unresolved.is_empty());
        assert_eq!(adapted, exported);
        assert_eq!(for_read_with(&device_b, "claude", "p", adapted), local);

        // A new provider B never had: the key must be re-entered.
        let fresh = MemoryBackend::new();
        let (adapted, unresolved) = adapt_imported(false, &fresh, "claude", "p", &exported, None);
        assert_eq!(adapted, exported);
        assert_eq!(
            unresolved,
            vec!["claude/p/env/ANTHROPIC_API_KEY".to_string()]
        );

        // Plaintext import onto a device with the vault on is vaulted.
        let plain = json!({ "env": { "ANTHROPIC_API_KEY": "key-c" } });
        let (adapted, _) = adapt_imported(true, &fresh, "claude", "q", &plain, None);
        assert!(!adapted.to_string().contains("key-c"));
    }

    #[test]
    fn export_redaction_never_leaks_inline_secrets() {
        let backend = MemoryBackend::new();
        let mut vaulted = claude_config();
        extract(&backend, "claude", "p", &mut vaulted).unwrap();
        assert_eq!(redact_for_export(&vaulted), (vaulted.clone(), 0));
        let config = claude_config();
        let (redacted, blanked) = redact_for_export(&config);
        assert_eq!(blanked, 4);
        assert!(!redacted.to_string().contains("sk-ant-secret"));
        assert_eq!(redacted["env"]["ANTHROPIC_AUTH_TOKEN"], json!(""));
    }

    #[test]
    fn availability_probe() {
        assert!(check_available(&MemoryBackend::new()).is_ok());
        assert!(matches!(
            check_available(&UnavailableBackend),
            Err(SecretError::Unavailable(_))
        ));
    }
}
