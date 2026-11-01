//! Publisher key registry for signed deep links (SESS-10).
//!
//! A signed v2 link carries the publisher's minisign public key inside the
//! signed envelope. The key is then classified:
//!
//! * `builtin` — shipped with the app ([`BUILTIN_PUBLISHERS`]); the display
//!   name comes from the registry, never from the link;
//! * `trusted` — the user trusted this key earlier (trust on first use);
//! * `new` — valid signature by a key the app has not seen; the link's
//!   claimed name is shown as unverified and the user may trust it;
//! * `mismatch` — the link claims the name of a known publisher but is signed
//!   with a different key (likely impersonation).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// `(publisher name, minisign public key)` pairs shipped with the app.
/// Publishers request inclusion through a pull request.
pub const BUILTIN_PUBLISHERS: &[(&str, &str)] = &[];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PublisherTrust {
    Builtin,
    Trusted,
    New,
    Mismatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustedPublisher {
    pub name: String,
    pub public_key: String,
    #[serde(default)]
    pub added_at: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct TrustStore {
    #[serde(default)]
    publishers: Vec<TrustedPublisher>,
}

fn store_path() -> PathBuf {
    crate::config::get_app_config_dir().join("deeplink-publishers.json")
}

fn read_store() -> TrustStore {
    std::fs::read(store_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn trusted_publishers() -> Vec<TrustedPublisher> {
    read_store().publishers
}

/// Remember `public_key` as `name` (trust on first use).
pub fn trust_publisher(name: &str, public_key: &str) -> Result<(), String> {
    let mut store = read_store();
    store.publishers.retain(|p| p.public_key != public_key);
    store.publishers.push(TrustedPublisher {
        name: name.trim().chars().take(80).collect(),
        public_key: public_key.trim().to_string(),
        added_at: chrono::Utc::now().timestamp(),
    });
    let bytes = serde_json::to_vec_pretty(&store).map_err(|e| e.to_string())?;
    crate::config::atomic_write(&store_path(), &bytes).map_err(|e| e.to_string())
}

/// Classify `public_key` given the claimed publisher name.
pub fn resolve(public_key: &str, claimed: Option<&str>) -> (String, PublisherTrust) {
    resolve_with(
        BUILTIN_PUBLISHERS,
        &trusted_publishers(),
        public_key,
        claimed,
    )
}

pub fn resolve_with(
    builtin: &[(&str, &str)],
    trusted: &[TrustedPublisher],
    public_key: &str,
    claimed: Option<&str>,
) -> (String, PublisherTrust) {
    let key = public_key.trim();
    if let Some((name, _)) = builtin.iter().find(|(_, k)| *k == key) {
        return (name.to_string(), PublisherTrust::Builtin);
    }
    if let Some(p) = trusted.iter().find(|p| p.public_key == key) {
        return (p.name.clone(), PublisherTrust::Trusted);
    }
    let claimed = claimed
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Unknown publisher")
        .chars()
        .take(80)
        .collect::<String>();
    let impersonates = builtin
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case(&claimed))
        || trusted
            .iter()
            .any(|p| p.name.eq_ignore_ascii_case(&claimed));
    if impersonates {
        (claimed, PublisherTrust::Mismatch)
    } else {
        (claimed, PublisherTrust::New)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUILTIN: &[(&str, &str)] = &[("Acme", "KEY_ACME")];

    #[test]
    fn classifies_keys() {
        let trusted = vec![TrustedPublisher {
            name: "Friend".into(),
            public_key: "KEY_FRIEND".into(),
            added_at: 0,
        }];
        assert_eq!(
            resolve_with(BUILTIN, &trusted, "KEY_ACME", Some("Anything")),
            ("Acme".into(), PublisherTrust::Builtin),
            "builtin name wins over the claimed one"
        );
        assert_eq!(
            resolve_with(BUILTIN, &trusted, "KEY_FRIEND", None),
            ("Friend".into(), PublisherTrust::Trusted)
        );
        assert_eq!(
            resolve_with(BUILTIN, &trusted, "KEY_NEW", Some("Newco")),
            ("Newco".into(), PublisherTrust::New)
        );
        assert_eq!(
            resolve_with(BUILTIN, &trusted, "KEY_EVIL", Some("acme")),
            ("acme".into(), PublisherTrust::Mismatch)
        );
    }
}
