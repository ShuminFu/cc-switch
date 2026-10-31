//! Team policy ("managed mode") file (PRODUCT-24).
//!
//! An administrator installs a `policy.json` that restricts what this machine's
//! CC Switch may configure. The file is looked up, first match wins, at:
//!
//! 1. the path in `CC_SWITCH_POLICY_FILE`;
//! 2. the system-wide location (`/etc/cc-switch/policy.json` on Linux,
//!    `/Library/Application Support/cc-switch/policy.json` on macOS,
//!    `%ProgramData%\cc-switch\policy.json` on Windows), which only an
//!    administrator can write;
//! 3. `<app config dir>/policy.json` (`~/.cc-switch/policy.json`).
//!
//! When a trusted minisign public key is configured (`CC_SWITCH_POLICY_PUBKEY`,
//! or a `policy.pub` file in the system-wide directory) the policy must carry a
//! valid `policy.json.minisig` signature; a missing or failing signature makes
//! the app ignore the file and report why.
//!
//! ```json
//! {
//!   "version": 1,
//!   "name": "Acme IT",
//!   "message": "Questions: it@acme.example",
//!   "endpoints": { "allow": ["*.anthropic.com"], "deny": ["*.example-relay.com"] },
//!   "disabledFeatures": ["deeplinkImport", "providerEdits"]
//! }
//! ```
//!
//! Enforced today: endpoint allow/deny globs on every provider add/update
//! (including deep-link imports and universal providers), `deeplinkImport`
//! (deep links are rejected) and `providerEdits` (providers are read-only).
//! Loopback endpoints (the local routing proxy) are always allowed.

use std::path::{Path, PathBuf};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::AppError;

pub const POLICY_FILE_ENV: &str = "CC_SWITCH_POLICY_FILE";
pub const POLICY_PUBKEY_ENV: &str = "CC_SWITCH_POLICY_PUBKEY";
pub const POLICY_FILE_NAME: &str = "policy.json";

/// Feature flag: reject every `ccswitch://` deep link.
pub const FEATURE_DEEPLINK_IMPORT: &str = "deeplinkImport";
/// Feature flag: providers cannot be added or edited.
pub const FEATURE_PROVIDER_EDITS: &str = "providerEdits";

const KNOWN_FEATURES: &[&str] = &[FEATURE_DEEPLINK_IMPORT, FEATURE_PROVIDER_EDITS];

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EndpointRules {
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedPolicy {
    #[serde(default)]
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Free text shown next to every policy block (e.g. who to contact).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default)]
    pub endpoints: EndpointRules,
    #[serde(default)]
    pub disabled_features: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PolicyState {
    /// No policy file found.
    None,
    /// A policy is loaded and enforced.
    Active,
    /// A file exists but could not be read or parsed; it is ignored.
    Invalid,
    /// A trusted key is configured and the signature is missing or wrong; ignored.
    SignatureInvalid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PolicyStatus {
    pub state: PolicyState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub signed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy: Option<ManagedPolicy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// `disabledFeatures` entries this version does not know (not enforced).
    #[serde(default)]
    pub unknown_features: Vec<String>,
}

impl PolicyStatus {
    fn none() -> Self {
        Self {
            state: PolicyState::None,
            source: None,
            signed: false,
            policy: None,
            error: None,
            unknown_features: Vec::new(),
        }
    }

    fn ignored(state: PolicyState, source: &Path, error: String) -> Self {
        log::warn!(
            "Ignoring managed policy {}: {error}",
            source.to_string_lossy()
        );
        Self {
            state,
            source: Some(source.to_string_lossy().to_string()),
            signed: false,
            policy: None,
            error: Some(error),
            unknown_features: Vec::new(),
        }
    }

    /// The enforced policy, if any.
    pub fn active(&self) -> Option<&ManagedPolicy> {
        match self.state {
            PolicyState::Active => self.policy.as_ref(),
            _ => None,
        }
    }
}

fn system_policy_dir() -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        Some(PathBuf::from("/etc/cc-switch"))
    }
    #[cfg(target_os = "macos")]
    {
        Some(PathBuf::from("/Library/Application Support/cc-switch"))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("ProgramData").map(|p| PathBuf::from(p).join("cc-switch"))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Candidate policy files, highest precedence first.
fn candidate_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(p) = non_empty_env(POLICY_FILE_ENV) {
        out.push(PathBuf::from(p));
    }
    if let Some(dir) = system_policy_dir() {
        out.push(dir.join(POLICY_FILE_NAME));
    }
    out.push(crate::config::get_app_config_dir().join(POLICY_FILE_NAME));
    out
}

fn trusted_public_key() -> Option<String> {
    if let Some(key) = non_empty_env(POLICY_PUBKEY_ENV) {
        return Some(key);
    }
    let path = system_policy_dir()?.join("policy.pub");
    let text = std::fs::read_to_string(path).ok()?;
    // Accept either a bare base64 key or minisign's two-line `.pub` format.
    text.lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty() && !l.starts_with("untrusted comment:"))
        .map(str::to_string)
}

fn signature_path(policy_path: &Path) -> PathBuf {
    let mut name = policy_path.as_os_str().to_os_string();
    name.push(".minisig");
    PathBuf::from(name)
}

/// Load and validate one policy file. `public_key` makes a signature mandatory.
pub fn load_policy_file(path: &Path, public_key: Option<&str>) -> PolicyStatus {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return PolicyStatus::none(),
        Err(e) => return PolicyStatus::ignored(PolicyState::Invalid, path, e.to_string()),
    };

    let mut signed = false;
    if let Some(key) = public_key {
        let sig_path = signature_path(path);
        let signature = match std::fs::read_to_string(&sig_path) {
            Ok(s) => s,
            Err(_) => {
                return PolicyStatus::ignored(
                    PolicyState::SignatureInvalid,
                    path,
                    format!(
                        "A trusted policy key is configured but {} is missing",
                        sig_path.to_string_lossy()
                    ),
                )
            }
        };
        if let Err(e) = crate::services::catalog::verify_minisign(key, &bytes, &signature) {
            return PolicyStatus::ignored(PolicyState::SignatureInvalid, path, e);
        }
        signed = true;
    }

    let policy: ManagedPolicy = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(e) => return PolicyStatus::ignored(PolicyState::Invalid, path, e.to_string()),
    };
    if let Err(e) = compile_rules(&policy.endpoints) {
        return PolicyStatus::ignored(PolicyState::Invalid, path, e);
    }

    let unknown_features = policy
        .disabled_features
        .iter()
        .filter(|f| !KNOWN_FEATURES.contains(&f.as_str()))
        .cloned()
        .collect();

    PolicyStatus {
        state: PolicyState::Active,
        source: Some(path.to_string_lossy().to_string()),
        signed,
        policy: Some(policy),
        error: None,
        unknown_features,
    }
}

/// Resolve the effective policy: the first existing candidate file decides
/// (an ignored file does not fall through to a lower-precedence one).
pub fn current_status() -> PolicyStatus {
    let key = trusted_public_key();
    for path in candidate_paths() {
        if path.exists() {
            return load_policy_file(&path, key.as_deref());
        }
    }
    PolicyStatus::none()
}

// ---------------------------------------------------------------------------
// Matching
// ---------------------------------------------------------------------------

/// Reduce a pattern such as `https://*.relay.com:443/v1` to `*.relay.com`.
fn pattern_host(pattern: &str) -> String {
    let p = pattern.trim().to_ascii_lowercase();
    let p = p.split_once("://").map(|(_, rest)| rest).unwrap_or(&p);
    let p = p.split('/').next().unwrap_or("");
    let p = p.rsplit_once('@').map(|(_, h)| h).unwrap_or(p);
    match p.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) => host.to_string(),
        _ => p.to_string(),
    }
}

fn compile_pattern(pattern: &str) -> Result<Regex, String> {
    let host = pattern_host(pattern);
    if host.is_empty() {
        return Err(format!("Empty endpoint pattern '{pattern}'"));
    }
    // `*.example.com` covers the apex domain as well as every subdomain.
    let body = if let Some(rest) = host.strip_prefix("*.") {
        format!(r"(?:.*\.)?{}", regex::escape(rest).replace(r"\*", ".*"))
    } else {
        regex::escape(&host).replace(r"\*", ".*")
    };
    Regex::new(&format!("^{body}$"))
        .map_err(|e| format!("Invalid endpoint pattern '{pattern}': {e}"))
}

struct CompiledRules {
    allow: Vec<(String, Regex)>,
    deny: Vec<(String, Regex)>,
}

fn compile_rules(rules: &EndpointRules) -> Result<CompiledRules, String> {
    let compile = |list: &[String]| -> Result<Vec<(String, Regex)>, String> {
        list.iter()
            .map(|p| compile_pattern(p).map(|r| (p.clone(), r)))
            .collect()
    };
    Ok(CompiledRules {
        allow: compile(&rules.allow)?,
        deny: compile(&rules.deny)?,
    })
}

fn is_loopback_host(host: &str) -> bool {
    let h = host.trim_start_matches('[').trim_end_matches(']');
    h == "localhost"
        || h.ends_with(".localhost")
        || h.parse::<std::net::IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false)
}

/// Why an endpoint host is not allowed, or `None` when it is.
pub fn endpoint_violation(rules: &EndpointRules, host: &str) -> Option<String> {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() || is_loopback_host(&host) {
        return None;
    }
    let compiled = compile_rules(rules).ok()?;
    if let Some((pattern, _)) = compiled.deny.iter().find(|(_, r)| r.is_match(&host)) {
        return Some(format!("{host} matches the denied endpoint '{pattern}'"));
    }
    if !compiled.allow.is_empty() && !compiled.allow.iter().any(|(_, r)| r.is_match(&host)) {
        return Some(format!("{host} is not in the list of allowed endpoints"));
    }
    None
}

/// Every http(s) host referenced anywhere in a JSON value (including URLs
/// embedded in TOML/YAML text such as Codex's `config` string).
pub fn collect_endpoint_hosts(value: &Value) -> Vec<String> {
    static URL_RE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
        Regex::new(r#"(?i)https?://[^\s"'<>\\,;)\]}]+"#).expect("valid url regex")
    });
    let mut hosts = Vec::new();
    let mut stack = vec![value];
    while let Some(v) = stack.pop() {
        match v {
            Value::String(s) => {
                for m in URL_RE.find_iter(s) {
                    if let Some(host) = url::Url::parse(m.as_str())
                        .ok()
                        .and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase()))
                    {
                        if !hosts.contains(&host) {
                            hosts.push(host);
                        }
                    }
                }
            }
            Value::Array(items) => stack.extend(items.iter()),
            Value::Object(map) => stack.extend(map.values()),
            _ => {}
        }
    }
    hosts
}

fn policy_label(policy: &ManagedPolicy) -> String {
    let name = policy
        .name
        .as_deref()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or("organization policy");
    match policy.message.as_deref().filter(|m| !m.trim().is_empty()) {
        Some(msg) => format!("{name} — {msg}"),
        None => name.to_string(),
    }
}

/// Check a provider's configuration against a policy.
pub fn check_config_against(policy: &ManagedPolicy, config: &Value) -> Result<(), AppError> {
    if policy
        .disabled_features
        .iter()
        .any(|f| f == FEATURE_PROVIDER_EDITS)
    {
        let label = policy_label(policy);
        return Err(AppError::localized(
            "policy.providerEditsDisabled",
            format!("供应商已被管理策略锁定，无法添加或修改（{label}）"),
            format!(
                "Providers are locked by a managed policy and cannot be added or changed ({label})"
            ),
        ));
    }
    for host in collect_endpoint_hosts(config) {
        if let Some(reason) = endpoint_violation(&policy.endpoints, &host) {
            let label = policy_label(policy);
            return Err(AppError::localized(
                "policy.endpointBlocked",
                format!("管理策略禁止该端点：{reason}（{label}）"),
                format!("Blocked by managed policy: {reason} ({label})"),
            ));
        }
    }
    Ok(())
}

/// Enforce the active policy (if any) on a provider-like value about to be saved.
pub fn enforce_provider_config<T: Serialize>(item: &T) -> Result<(), AppError> {
    let status = current_status();
    let Some(policy) = status.active() else {
        return Ok(());
    };
    let value = serde_json::to_value(item).map_err(|source| AppError::JsonSerialize { source })?;
    check_config_against(policy, &value)
}

/// Whether the active policy disables `feature`.
pub fn is_feature_disabled(feature: &str) -> bool {
    current_status()
        .active()
        .is_some_and(|p| p.disabled_features.iter().any(|f| f == feature))
}

/// Reject deep links when the policy disables them.
pub fn enforce_deeplink_allowed() -> Result<(), AppError> {
    if is_feature_disabled(FEATURE_DEEPLINK_IMPORT) {
        return Err(AppError::InvalidInput(
            "Deep-link imports are disabled by a managed policy".to_string(),
        ));
    }
    Ok(())
}

/// Report the managed policy state to the UI.
#[tauri::command]
pub fn get_managed_policy_status() -> PolicyStatus {
    current_status()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    fn rules(allow: &[&str], deny: &[&str]) -> EndpointRules {
        EndpointRules {
            allow: allow.iter().map(|s| s.to_string()).collect(),
            deny: deny.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn deny_glob_blocks_subdomains_and_apex() {
        let r = rules(&[], &["*.example-relay.com"]);
        assert!(endpoint_violation(&r, "api.example-relay.com").is_some());
        assert!(endpoint_violation(&r, "example-relay.com").is_some());
        assert!(endpoint_violation(&r, "API.Example-Relay.com").is_some());
        assert!(endpoint_violation(&r, "notexample-relay.com").is_none());
        assert!(endpoint_violation(&r, "api.anthropic.com").is_none());
    }

    #[test]
    fn allow_list_blocks_everything_else_but_loopback() {
        let r = rules(&["*.anthropic.com", "https://api.openai.com/v1"], &[]);
        assert!(endpoint_violation(&r, "api.anthropic.com").is_none());
        assert!(endpoint_violation(&r, "api.openai.com").is_none());
        assert!(endpoint_violation(&r, "relay.example.com").is_some());
        assert!(endpoint_violation(&r, "127.0.0.1").is_none());
        assert!(endpoint_violation(&r, "localhost").is_none());
        assert!(endpoint_violation(&r, "[::1]").is_none());
    }

    #[test]
    fn deny_wins_over_allow() {
        let r = rules(&["*.example.com"], &["bad.example.com"]);
        assert!(endpoint_violation(&r, "good.example.com").is_none());
        assert!(endpoint_violation(&r, "bad.example.com").is_some());
    }

    #[test]
    fn collects_hosts_from_env_json_and_embedded_toml() {
        let config = json!({
            "env": { "ANTHROPIC_BASE_URL": "https://api.example-relay.com/v1" },
            "config": "model_provider = \"x\"\n[model_providers.x]\nbase_url = \"https://codex.relay.io/v1\"\n",
            "nested": [{ "baseURL": "http://10.0.0.5:8080" }],
            "note": "no url here",
        });
        let mut hosts = collect_endpoint_hosts(&config);
        hosts.sort();
        assert_eq!(
            hosts,
            vec!["10.0.0.5", "api.example-relay.com", "codex.relay.io"]
        );
    }

    #[test]
    fn provider_with_denied_endpoint_is_blocked_with_reason() {
        let policy = ManagedPolicy {
            name: Some("Acme IT".into()),
            message: Some("ask it@acme.example".into()),
            endpoints: rules(&[], &["*.example-relay.com"]),
            ..Default::default()
        };
        let provider = json!({
            "id": "p1",
            "settingsConfig": { "env": { "ANTHROPIC_BASE_URL": "https://api.example-relay.com" } }
        });
        let err = check_config_against(&policy, &provider).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("api.example-relay.com"), "{text}");
        assert!(text.contains("Acme IT"), "{text}");

        let ok = json!({ "settingsConfig": { "env": { "ANTHROPIC_BASE_URL": "https://api.anthropic.com" } } });
        assert!(check_config_against(&policy, &ok).is_ok());
    }

    #[test]
    fn provider_edits_feature_locks_providers() {
        let policy = ManagedPolicy {
            disabled_features: vec![FEATURE_PROVIDER_EDITS.into()],
            ..Default::default()
        };
        assert!(check_config_against(&policy, &json!({})).is_err());
    }

    #[test]
    fn missing_file_means_no_policy() {
        let dir = TempDir::new().unwrap();
        let status = load_policy_file(&dir.path().join("policy.json"), None);
        assert_eq!(status.state, PolicyState::None);
        assert!(status.active().is_none());
    }

    #[test]
    fn unsigned_file_loads_when_no_key_is_configured() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("policy.json");
        std::fs::write(
            &path,
            r#"{"version":1,"endpoints":{"deny":["*.x.com"]},"disabledFeatures":["deeplinkImport","futureThing"]}"#,
        )
        .unwrap();
        let status = load_policy_file(&path, None);
        assert_eq!(status.state, PolicyState::Active);
        assert!(!status.signed);
        assert_eq!(status.unknown_features, vec!["futureThing".to_string()]);
        assert!(status.active().is_some());
    }

    #[test]
    fn malformed_file_is_ignored() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("policy.json");
        std::fs::write(&path, "{ not json").unwrap();
        let status = load_policy_file(&path, None);
        assert_eq!(status.state, PolicyState::Invalid);
        assert!(status.active().is_none());
    }

    /// Test-only key from services/catalog.rs tests (secret key not kept).
    const TEST_PUBLIC_KEY: &str = "RWSGmZsfeFIsiSCs/JSRfD8/Lfp1OFnCiEfSB6W7e0kULAVKwBncgFFT";

    #[test]
    fn signature_required_when_key_configured() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("policy.json");
        std::fs::write(&path, r#"{"endpoints":{"deny":["*.x.com"]}}"#).unwrap();

        // No signature file at all.
        let status = load_policy_file(&path, Some(TEST_PUBLIC_KEY));
        assert_eq!(status.state, PolicyState::SignatureInvalid);
        assert!(status.active().is_none());

        // A signature that does not verify.
        std::fs::write(
            signature_path(&path),
            "untrusted comment: bogus\nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\ntrusted comment: x\nAAAA\n",
        )
        .unwrap();
        let status = load_policy_file(&path, Some(TEST_PUBLIC_KEY));
        assert_eq!(status.state, PolicyState::SignatureInvalid);
        assert!(status.active().is_none());
    }

    #[test]
    fn invalid_pattern_makes_file_invalid() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("policy.json");
        std::fs::write(&path, r#"{"endpoints":{"deny":["   "]}}"#).unwrap();
        assert_eq!(load_policy_file(&path, None).state, PolicyState::Invalid);
    }
}
