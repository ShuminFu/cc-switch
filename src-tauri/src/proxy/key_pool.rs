//! API key pool rotation (PRODUCT-32, first increment).
//!
//! A provider may list extra keys in `meta.apiKeyPool`. Its own key (from
//! `settings_config`) is the `primary` entry. For each request the proxy
//! starts at the next key in round-robin order, skipping keys that recently
//! answered 429 or 401 (they move to the end for [`COOLDOWN`]). When an
//! attempt fails with 429/401 the same provider is retried with the next key
//! before the request fails over to another provider.
//!
//! Keys never appear in logs or traces: only their labels do.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::Value;

use super::ProxyError;
use crate::provider::Provider;

/// How long a key that answered 429/401 is tried last.
pub const COOLDOWN: Duration = Duration::from_secs(60);

/// Label used for the provider's own key.
pub const PRIMARY_LABEL: &str = "primary";

/// Key fields, in the order the proxy adapters read them.
const KEY_FIELDS: &[(&str, &str)] = &[
    ("env", "ANTHROPIC_AUTH_TOKEN"),
    ("env", "ANTHROPIC_API_KEY"),
    ("env", "OPENROUTER_API_KEY"),
    ("env", "OPENAI_API_KEY"),
    ("auth", "OPENAI_API_KEY"),
    ("env", "GEMINI_API_KEY"),
    ("", "apiKey"),
    ("", "api_key"),
];

/// One key the proxy may use for a provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PooledKey {
    pub label: String,
    pub key: String,
}

fn field<'a>(settings: &'a Value, section: &str, name: &str) -> Option<&'a str> {
    let holder = if section.is_empty() {
        Some(settings)
    } else {
        settings.get(section)
    };
    holder?
        .get(name)?
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// The provider's own key and where it lives.
fn primary(provider: &Provider) -> Option<(&'static str, &'static str, String)> {
    KEY_FIELDS.iter().find_map(|(section, name)| {
        field(&provider.settings_config, section, name).map(|k| (*section, *name, k.to_string()))
    })
}

/// All keys of `provider`: `primary` first, then the pool (deduplicated,
/// empty keys dropped). Empty when the provider has no pool.
pub fn keys_of(provider: &Provider) -> Vec<PooledKey> {
    let pool = provider
        .meta
        .as_ref()
        .map(|m| m.api_key_pool.as_slice())
        .unwrap_or_default();
    if pool.is_empty() {
        return Vec::new();
    }
    let mut keys: Vec<PooledKey> = Vec::new();
    if let Some((_, _, key)) = primary(provider) {
        keys.push(PooledKey {
            label: PRIMARY_LABEL.to_string(),
            key,
        });
    }
    for (i, entry) in pool.iter().enumerate() {
        let key = entry.key.trim();
        if key.is_empty() || keys.iter().any(|k| k.key == key) {
            continue;
        }
        let label = if entry.label.trim().is_empty() {
            format!("key{}", i + 1)
        } else {
            entry.label.trim().to_string()
        };
        keys.push(PooledKey {
            label,
            key: key.to_string(),
        });
    }
    keys
}

/// A copy of `provider` that authenticates with `key`.
pub fn with_key(provider: &Provider, key: &str) -> Provider {
    let mut p = provider.clone();
    let (section, name) = primary(provider)
        .map(|(s, n, _)| (s, n))
        .unwrap_or(("env", "ANTHROPIC_AUTH_TOKEN"));
    let target = if section.is_empty() {
        Some(&mut p.settings_config)
    } else {
        p.settings_config.get_mut(section)
    };
    if let Some(Value::Object(map)) = target {
        map.insert(name.to_string(), Value::String(key.to_string()));
    }
    p
}

/// Whether an attempt error should move to the provider's next key.
pub fn should_rotate(error: &ProxyError) -> bool {
    matches!(
        error,
        ProxyError::UpstreamError {
            status: 401 | 429,
            ..
        }
    )
}

#[derive(Default)]
struct PoolState {
    cursor: HashMap<String, usize>,
    cooling: HashMap<(String, String), Instant>,
}

fn state() -> &'static Mutex<PoolState> {
    static STATE: OnceLock<Mutex<PoolState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(PoolState::default()))
}

fn pool_key(app_type: &str, provider: &Provider) -> String {
    format!("{app_type}:{}", provider.id)
}

/// Keys in the order this request should try them.
pub fn ordered_keys(app_type: &str, provider: &Provider) -> Vec<PooledKey> {
    let keys = keys_of(provider);
    if keys.len() <= 1 {
        return keys;
    }
    let mut st = state().lock().unwrap_or_else(|e| e.into_inner());
    let id = pool_key(app_type, provider);
    let cursor = st.cursor.entry(id.clone()).or_insert(0);
    let start = *cursor % keys.len();
    *cursor = cursor.wrapping_add(1);
    let mut rotated: Vec<PooledKey> = keys[start..]
        .iter()
        .chain(&keys[..start])
        .cloned()
        .collect();
    let now = Instant::now();
    st.cooling.retain(|_, until| *until > now);
    let cooling = |k: &PooledKey| st.cooling.contains_key(&(id.clone(), k.label.clone()));
    rotated.sort_by_key(cooling);
    rotated
}

/// Put a key that answered 429/401 at the end for [`COOLDOWN`].
pub fn mark_failed(app_type: &str, provider: &Provider, label: &str) {
    let mut st = state().lock().unwrap_or_else(|e| e.into_inner());
    st.cooling.insert(
        (pool_key(app_type, provider), label.to_string()),
        Instant::now() + COOLDOWN,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ApiKeyPoolEntry, ProviderMeta};
    use serde_json::json;

    fn pooled(id: &str, settings: Value, pool: &[(&str, &str)]) -> Provider {
        let mut p = Provider::with_id(id.to_string(), id.to_string(), settings, None);
        p.meta = Some(ProviderMeta {
            api_key_pool: pool
                .iter()
                .map(|(label, key)| ApiKeyPoolEntry {
                    label: label.to_string(),
                    key: key.to_string(),
                })
                .collect(),
            ..Default::default()
        });
        p
    }

    #[test]
    fn no_pool_means_no_rotation() {
        let p = Provider::with_id(
            "x".into(),
            "x".into(),
            json!({"env": {"ANTHROPIC_AUTH_TOKEN": "a"}}),
            None,
        );
        assert!(keys_of(&p).is_empty());
        assert!(ordered_keys("claude", &p).is_empty());
    }

    #[test]
    fn primary_comes_first_and_duplicates_are_dropped() {
        let p = pooled(
            "kp-dedupe",
            json!({"env": {"ANTHROPIC_AUTH_TOKEN": "sk-a"}}),
            &[("B", "sk-b"), ("dup", "sk-a"), ("", "sk-c"), ("empty", " ")],
        );
        let keys = keys_of(&p);
        let labels: Vec<&str> = keys.iter().map(|k| k.label.as_str()).collect();
        assert_eq!(labels, ["primary", "B", "key3"]);
    }

    #[test]
    fn with_key_rewrites_the_field_the_adapter_reads() {
        let claude = pooled(
            "c",
            json!({"env": {"ANTHROPIC_API_KEY": "old", "ANTHROPIC_BASE_URL": "u"}}),
            &[],
        );
        let swapped = with_key(&claude, "new");
        assert_eq!(swapped.settings_config["env"]["ANTHROPIC_API_KEY"], "new");
        assert_eq!(swapped.settings_config["env"]["ANTHROPIC_BASE_URL"], "u");
        assert_eq!(claude.settings_config["env"]["ANTHROPIC_API_KEY"], "old");

        let codex = pooled("x", json!({"auth": {"OPENAI_API_KEY": "old"}}), &[]);
        assert_eq!(
            with_key(&codex, "new").settings_config["auth"]["OPENAI_API_KEY"],
            "new"
        );
        let gemini = pooled("g", json!({"env": {"GEMINI_API_KEY": "old"}}), &[]);
        assert_eq!(
            with_key(&gemini, "new").settings_config["env"]["GEMINI_API_KEY"],
            "new"
        );
    }

    #[test]
    fn rotation_is_round_robin_and_failed_keys_go_last() {
        let p = pooled(
            "kp-rr",
            json!({"env": {"ANTHROPIC_AUTH_TOKEN": "sk-a"}}),
            &[("B", "sk-b")],
        );
        let first: Vec<String> = (0..4)
            .map(|_| ordered_keys("claude", &p)[0].label.clone())
            .collect();
        assert_eq!(first, ["primary", "B", "primary", "B"]);
        mark_failed("claude", &p, "primary");
        for _ in 0..2 {
            let order = ordered_keys("claude", &p);
            assert_eq!(order[0].label, "B");
            assert_eq!(order[1].label, "primary");
        }
        // Other apps keep their own state.
        assert_eq!(ordered_keys("codex", &p)[0].label, "primary");
    }

    #[test]
    fn only_429_and_401_rotate() {
        let e = |status| ProxyError::UpstreamError { status, body: None };
        assert!(should_rotate(&e(429)));
        assert!(should_rotate(&e(401)));
        assert!(!should_rotate(&e(500)));
        assert!(!should_rotate(&e(400)));
    }
}
