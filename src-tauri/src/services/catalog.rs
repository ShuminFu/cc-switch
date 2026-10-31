//! Remote, signed preset & sponsor catalog (PRODUCT-07).
//!
//! Provider presets are compiled into the frontend. This module lets the
//! maintainers ship an *overlay* — add presets, patch preset links / endpoint
//! candidates / partner flags, hide presets and provide promotion copy —
//! without an app release:
//!
//! * a bundled snapshot (`catalog/v1/catalog.json`, empty overlay) is always
//!   available, so offline users keep today's preset list;
//! * a remote catalog is fetched at most once a day (opt-out), and only used
//!   after its minisign signature verifies against [`CATALOG_PUBLIC_KEY`]
//!   (the updater key) — the cached copy is re-verified on every load, so a
//!   file tampered with on disk is ignored;
//! * versions only move forward (no rollback to an older signed catalog);
//! * the catalog only changes the templates offered when *adding* a provider.
//!   It never touches providers the user already configured.
//!
//! [`verify_minisign`] is shared with signed deep links (SESS-10).

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Catalog format understood by this build.
pub const CATALOG_SCHEMA: u64 = 1;
/// Minisign public key that signs remote catalogs (same key as the updater).
pub const CATALOG_PUBLIC_KEY: &str = "RWTjKDlXmowCyC9Q/dOAftdyN/oC70kgS2Zbl5CRd63EFO5NZwtHjEVQ";
/// Published next to the release assets; `<url>.minisig` holds the signature.
pub const CATALOG_URL: &str =
    "https://github.com/farion1231/cc-switch/releases/latest/download/preset-catalog-v1.json";

const BUNDLED_CATALOG: &str = include_str!("../../catalog/v1/catalog.json");
const MAX_CATALOG_BYTES: usize = 2 * 1024 * 1024;
const MAX_SIGNATURE_BYTES: usize = 4 * 1024;
const CHECK_INTERVAL_SECS: i64 = 24 * 60 * 60;
const FETCH_TIMEOUT: Duration = Duration::from_secs(20);

/// Apps whose presets the catalog may overlay.
const CATALOG_APPS: &[&str] = &[
    "claude",
    "claudeDesktop",
    "codex",
    "gemini",
    "grokbuild",
    "opencode",
    "openclaw",
    "hermes",
    "universal",
];

/// Preset fields a catalog may patch. Anything else is dropped.
const PATCHABLE_FIELDS: &[&str] = &[
    "websiteUrl",
    "apiKeyUrl",
    "endpointCandidates",
    "isPartner",
    "primePartner",
    "partnerPromotionKey",
    "hidden",
    "category",
    "icon",
    "iconColor",
    "settingsConfig",
];

/// Verify a minisign signature (`signature_text` is the full `.minisig`
/// content) of `data` with a base64 minisign public key. Only prehashed
/// (current minisign default) signatures are accepted. Returns the trusted
/// comment on success.
pub fn verify_minisign(
    public_key_b64: &str,
    data: &[u8],
    signature_text: &str,
) -> Result<String, String> {
    let key = minisign_verify::PublicKey::from_base64(public_key_b64.trim())
        .map_err(|e| format!("invalid public key: {e}"))?;
    let signature = minisign_verify::Signature::decode(signature_text.trim())
        .map_err(|e| format!("invalid signature: {e}"))?;
    key.verify(data, &signature, false)
        .map_err(|e| format!("signature verification failed: {e}"))?;
    Ok(signature.trusted_comment().to_string())
}

/// Hex key id of a minisign public key (as printed by `minisign -G`).
pub fn minisign_key_id(public_key_b64: &str) -> Option<String> {
    use base64::Engine;
    let bin = base64::engine::general_purpose::STANDARD
        .decode(public_key_b64.trim())
        .ok()?;
    if bin.len() != 42 {
        return None;
    }
    Some(
        bin[2..10]
            .iter()
            .rev()
            .map(|b| format!("{b:02X}"))
            .collect(),
    )
}

/// The overlay document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PresetCatalog {
    pub schema: u64,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
    /// `{ "<app>": { "patch": { "<preset name>": {...} }, "add": [ {...} ] } }`
    #[serde(default)]
    pub apps: Map<String, Value>,
    /// `{ "<partnerPromotionKey>": { "<lang>": "text" } }`
    #[serde(default)]
    pub promotions: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogStatus {
    /// `"bundled"` or `"remote"`.
    pub source: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fetched_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_checked_at: Option<i64>,
    pub auto_update: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogResponse {
    pub status: CatalogStatus,
    pub catalog: PresetCatalog,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogState {
    #[serde(default = "default_true")]
    auto_update: bool,
    #[serde(default)]
    last_checked_at: Option<i64>,
    #[serde(default)]
    fetched_at: Option<i64>,
    #[serde(default)]
    last_error: Option<String>,
}

fn default_true() -> bool {
    true
}

/// Compare dotted numeric versions (`2026.11.02.1`); non-numeric parts
/// compare as 0.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let parse = |v: &str| -> Vec<u64> {
        v.split(['.', '-'])
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let (a, b) = (parse(a), parse(b));
    for i in 0..a.len().max(b.len()) {
        let ord = a.get(i).unwrap_or(&0).cmp(b.get(i).unwrap_or(&0));
        if ord != Ordering::Equal {
            return ord;
        }
    }
    Ordering::Equal
}

/// Parse and sanitize a catalog document. Unknown apps and non-whitelisted
/// patch fields are dropped; structural errors reject the whole catalog.
pub fn parse_catalog(bytes: &[u8]) -> Result<PresetCatalog, String> {
    let mut catalog: PresetCatalog =
        serde_json::from_slice(bytes).map_err(|e| format!("invalid catalog JSON: {e}"))?;
    if catalog.schema != CATALOG_SCHEMA {
        return Err(format!(
            "unsupported catalog schema {} (this app understands {CATALOG_SCHEMA})",
            catalog.schema
        ));
    }
    if catalog.version.trim().is_empty() {
        return Err("catalog version is empty".to_string());
    }

    let mut apps = Map::new();
    for (app, overlay) in std::mem::take(&mut catalog.apps) {
        if !CATALOG_APPS.contains(&app.as_str()) {
            log::warn!("[catalog] ignoring unknown app '{app}'");
            continue;
        }
        let Value::Object(overlay) = overlay else {
            return Err(format!("catalog entry for '{app}' must be an object"));
        };
        let mut clean = Map::new();
        if let Some(patch) = overlay.get("patch") {
            let Value::Object(patch) = patch else {
                return Err(format!("'{app}.patch' must be an object"));
            };
            let mut clean_patch = Map::new();
            for (name, fields) in patch {
                let Value::Object(fields) = fields else {
                    return Err(format!("'{app}.patch.{name}' must be an object"));
                };
                let kept: Map<String, Value> = fields
                    .iter()
                    .filter(|(k, _)| PATCHABLE_FIELDS.contains(&k.as_str()))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                clean_patch.insert(name.clone(), Value::Object(kept));
            }
            clean.insert("patch".into(), Value::Object(clean_patch));
        }
        if let Some(add) = overlay.get("add") {
            let Value::Array(items) = add else {
                return Err(format!("'{app}.add' must be an array"));
            };
            for item in items {
                let ok = item.get("name").and_then(Value::as_str).is_some()
                    && item.get("settingsConfig").is_some_and(Value::is_object);
                if !ok {
                    return Err(format!(
                        "every '{app}.add' preset needs a name and a settingsConfig object"
                    ));
                }
            }
            clean.insert("add".into(), Value::Array(items.clone()));
        }
        apps.insert(app, Value::Object(clean));
    }
    catalog.apps = apps;

    for (key, texts) in &catalog.promotions {
        let valid = texts
            .as_object()
            .is_some_and(|m| m.values().all(Value::is_string));
        if !valid {
            return Err(format!("promotion '{key}' must map languages to strings"));
        }
    }
    Ok(catalog)
}

pub fn bundled_catalog() -> PresetCatalog {
    parse_catalog(BUNDLED_CATALOG.as_bytes()).expect("bundled catalog is valid")
}

/// On-disk cache of the last verified remote catalog plus update settings.
pub struct CatalogStore {
    dir: PathBuf,
    public_key: String,
}

impl CatalogStore {
    pub fn new(dir: PathBuf, public_key: &str) -> Self {
        Self {
            dir,
            public_key: public_key.to_string(),
        }
    }

    fn catalog_path(&self) -> PathBuf {
        self.dir.join("catalog.json")
    }
    fn signature_path(&self) -> PathBuf {
        self.dir.join("catalog.json.minisig")
    }
    fn state_path(&self) -> PathBuf {
        self.dir.join("state.json")
    }

    fn state(&self) -> CatalogState {
        std::fs::read(self.state_path())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or(CatalogState {
                auto_update: true,
                ..CatalogState::default()
            })
    }

    fn save_state(&self, state: &CatalogState) {
        if let Ok(bytes) = serde_json::to_vec_pretty(state) {
            if let Err(e) = write_atomic(&self.state_path(), &bytes) {
                log::warn!("[catalog] failed to save state: {e}");
            }
        }
    }

    /// The cached remote catalog, only if its signature still verifies.
    fn cached(&self) -> Option<PresetCatalog> {
        let bytes = std::fs::read(self.catalog_path()).ok()?;
        let signature = std::fs::read_to_string(self.signature_path()).ok()?;
        if let Err(err) = verify_minisign(&self.public_key, &bytes, &signature) {
            log::warn!("[catalog] cached catalog rejected: {err}");
            return None;
        }
        parse_catalog(&bytes).ok()
    }

    /// The catalog in effect: a verified remote catalog newer than the
    /// bundled one, otherwise the bundled snapshot.
    pub fn load(&self) -> CatalogResponse {
        let state = self.state();
        let bundled = bundled_catalog();
        let (catalog, source) = match self.cached() {
            Some(remote) if compare_versions(&remote.version, &bundled.version).is_gt() => {
                (remote, "remote")
            }
            _ => (bundled, "bundled"),
        };
        CatalogResponse {
            status: CatalogStatus {
                source: source.to_string(),
                version: catalog.version.clone(),
                published_at: catalog.published_at.clone(),
                fetched_at: (source == "remote").then_some(state.fetched_at).flatten(),
                last_checked_at: state.last_checked_at,
                auto_update: state.auto_update,
                last_error: state.last_error,
            },
            catalog,
        }
    }

    /// Verify and install a downloaded catalog. Rejects bad signatures,
    /// malformed documents and versions not newer than the current one.
    pub fn install(&self, bytes: &[u8], signature: &str) -> Result<PresetCatalog, String> {
        if bytes.len() > MAX_CATALOG_BYTES {
            return Err("catalog too large".to_string());
        }
        verify_minisign(&self.public_key, bytes, signature)?;
        let catalog = parse_catalog(bytes)?;
        let current = self.load().catalog;
        match compare_versions(&catalog.version, &current.version) {
            Ordering::Greater => {}
            Ordering::Equal => return Ok(current),
            Ordering::Less => {
                return Err(format!(
                    "catalog {} is older than the installed {}; refusing to roll back",
                    catalog.version, current.version
                ))
            }
        }
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        // Signature first: a crash between the writes leaves a pair that fails
        // verification and is ignored, never an unsigned catalog.
        write_atomic(&self.signature_path(), signature.as_bytes())?;
        write_atomic(&self.catalog_path(), bytes)?;
        Ok(catalog)
    }

    pub fn set_auto_update(&self, enabled: bool) -> CatalogStatus {
        let mut state = self.state();
        state.auto_update = enabled;
        self.save_state(&state);
        self.load().status
    }

    /// Whether a scheduled (non-forced) check is due.
    fn check_due(&self, now: i64) -> bool {
        let state = self.state();
        state.auto_update
            && state
                .last_checked_at
                .is_none_or(|last| now - last >= CHECK_INTERVAL_SECS)
    }

    fn record_check(&self, now: i64, result: &Result<bool, String>) {
        let mut state = self.state();
        state.last_checked_at = Some(now);
        match result {
            Ok(installed) => {
                if *installed {
                    state.fetched_at = Some(now);
                }
                state.last_error = None;
            }
            Err(err) => state.last_error = Some(err.clone()),
        }
        self.save_state(&state);
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    crate::config::atomic_write(path, bytes).map_err(|e| e.to_string())
}

static STORE_LOCK: Mutex<()> = Mutex::new(());

fn global_store() -> CatalogStore {
    CatalogStore::new(
        crate::config::get_app_config_dir().join("catalog"),
        CATALOG_PUBLIC_KEY,
    )
}

pub fn get_catalog() -> CatalogResponse {
    let _guard = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    global_store().load()
}

pub fn set_auto_update(enabled: bool) -> CatalogStatus {
    let _guard = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    global_store().set_auto_update(enabled)
}

async fn fetch_limited(url: &str, limit: usize) -> Result<Vec<u8>, String> {
    let client = crate::proxy::http_client::get();
    let response = client
        .get(url)
        .timeout(FETCH_TIMEOUT)
        .send()
        .await
        .map_err(|e| format!("download failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("download failed: HTTP {}", response.status()));
    }
    if response
        .content_length()
        .is_some_and(|len| len as usize > limit)
    {
        return Err("download too large".to_string());
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("download failed: {e}"))?;
    if bytes.len() > limit {
        return Err("download too large".to_string());
    }
    Ok(bytes.to_vec())
}

/// Check for a newer signed catalog. Without `force` this is a no-op unless
/// auto-update is on and the last check is more than a day old.
pub async fn refresh_catalog(force: bool) -> CatalogStatus {
    let now = chrono::Utc::now().timestamp();
    {
        let _guard = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let store = global_store();
        if !force && !store.check_due(now) {
            return store.load().status;
        }
    }

    let result: Result<bool, String> = async {
        let bytes = fetch_limited(CATALOG_URL, MAX_CATALOG_BYTES).await?;
        let signature =
            fetch_limited(&format!("{CATALOG_URL}.minisig"), MAX_SIGNATURE_BYTES).await?;
        let signature = String::from_utf8(signature).map_err(|_| "invalid signature file")?;
        let _guard = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let store = global_store();
        let before = store.load().status.version;
        let installed = store.install(&bytes, &signature)?;
        Ok(installed.version != before)
    }
    .await;

    if let Err(err) = &result {
        log::warn!("[catalog] update check failed: {err}");
    }
    let _guard = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let store = global_store();
    store.record_check(now, &result);
    store.load().status
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test-only key pair generated with `minisign -G -W` (secret key not kept).
    const TEST_PUBLIC_KEY: &str = "RWSGmZsfeFIsiSCs/JSRfD8/Lfp1OFnCiEfSB6W7e0kULAVKwBncgFFT";
    const OTHER_PUBLIC_KEY: &str = "RWSBmqQn+8wPmpm6ewuBfJ2qgYRwIiUEfQMw4Flc8vh7FpcCxN7tzQqt";
    const FIXTURE: &str = r#"{"schema":1,"version":"2026.11.01.1","apps":{"claude":{"patch":{"DeepSeek":{"websiteUrl":"https://example.com/new"}}}}}"#;
    const FIXTURE_SIG: &str = "untrusted comment: signature from minisign secret key
RUSGmZsfeFIsiXFmOwvqJCKlsyzevdBfyTSvexZApL/ARq7WTlnEi8dX6wWDYpp3x+CoAVyterEn7GNLqEOvFBAZ9GaBehTVHQY=
trusted comment: cc-switch preset catalog test fixture
sE05aZZYa9x34t1SBumBDz3CO0JeUfK1h3dTHW/a3E6us6F+Jnx1j0NpiH7KfMAp2G0xZ0oQ7xdJMAoiA9RzDA==
";

    fn store() -> (tempfile::TempDir, CatalogStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = CatalogStore::new(dir.path().join("catalog"), TEST_PUBLIC_KEY);
        (dir, store)
    }

    #[test]
    fn valid_signature_verifies() {
        let comment = verify_minisign(TEST_PUBLIC_KEY, FIXTURE.as_bytes(), FIXTURE_SIG).unwrap();
        assert_eq!(comment, "cc-switch preset catalog test fixture");
    }

    #[test]
    fn tampered_payload_is_rejected() {
        let tampered = FIXTURE.replace("example.com/new", "evil.example/new");
        assert!(verify_minisign(TEST_PUBLIC_KEY, tampered.as_bytes(), FIXTURE_SIG).is_err());
        // Tampered trusted comment breaks the global signature.
        let sig = FIXTURE_SIG.replace("test fixture", "test fixturE");
        assert!(verify_minisign(TEST_PUBLIC_KEY, FIXTURE.as_bytes(), &sig).is_err());
    }

    #[test]
    fn wrong_key_is_rejected() {
        assert!(verify_minisign(OTHER_PUBLIC_KEY, FIXTURE.as_bytes(), FIXTURE_SIG).is_err());
        assert!(verify_minisign("not a key", FIXTURE.as_bytes(), FIXTURE_SIG).is_err());
    }

    #[test]
    fn key_id_matches_minisign_output() {
        assert_eq!(
            minisign_key_id(TEST_PUBLIC_KEY).as_deref(),
            Some("892C52781F9B9986")
        );
        assert_eq!(
            minisign_key_id(CATALOG_PUBLIC_KEY).as_deref(),
            Some("C8028C9A573928E3")
        );
    }

    #[test]
    fn falls_back_to_bundled_without_cache_or_when_tampered() {
        let (_dir, store) = store();
        let loaded = store.load();
        assert_eq!(loaded.status.source, "bundled");
        assert_eq!(loaded.catalog, bundled_catalog());
        assert!(loaded.catalog.apps.is_empty(), "bundled overlay is empty");

        store.install(FIXTURE.as_bytes(), FIXTURE_SIG).unwrap();
        let loaded = store.load();
        assert_eq!(loaded.status.source, "remote");
        assert_eq!(loaded.status.version, "2026.11.01.1");

        // Tampering with the cached file on disk drops back to the bundle.
        std::fs::write(
            store.catalog_path(),
            FIXTURE.replace("example.com", "evil.example"),
        )
        .unwrap();
        assert_eq!(store.load().status.source, "bundled");
    }

    #[test]
    fn install_rejects_bad_signature_and_rollback() {
        let (_dir, store) = store();
        let tampered = FIXTURE.replace("2026.11.01.1", "2026.11.09.1");
        assert!(store.install(tampered.as_bytes(), FIXTURE_SIG).is_err());
        assert_eq!(store.load().status.source, "bundled");

        store.install(FIXTURE.as_bytes(), FIXTURE_SIG).unwrap();
        // Same version again is a no-op.
        assert!(store.install(FIXTURE.as_bytes(), FIXTURE_SIG).is_ok());
    }

    #[test]
    fn parse_drops_unknown_fields_and_rejects_bad_shapes() {
        let catalog = parse_catalog(
            br#"{"schema":1,"version":"1","apps":{"claude":{"patch":{"X":{"websiteUrl":"u","evil":1}}},"nope":{}}}"#,
        )
        .unwrap();
        assert!(catalog.apps.get("nope").is_none());
        let patch = &catalog.apps["claude"]["patch"]["X"];
        assert_eq!(patch.get("websiteUrl").and_then(Value::as_str), Some("u"));
        assert!(patch.get("evil").is_none());

        assert!(parse_catalog(br#"{"schema":2,"version":"1"}"#).is_err());
        assert!(parse_catalog(
            br#"{"schema":1,"version":"1","apps":{"claude":{"add":[{"name":"x"}]}}}"#
        )
        .is_err());
        assert!(
            parse_catalog(br#"{"schema":1,"version":"1","promotions":{"k":{"en":1}}}"#).is_err()
        );
    }

    #[test]
    fn auto_update_schedule_and_opt_out() {
        let (_dir, store) = store();
        assert!(store.check_due(1_000));
        store.record_check(1_000, &Ok(false));
        assert!(!store.check_due(1_000 + 60));
        assert!(store.check_due(1_000 + CHECK_INTERVAL_SECS));
        let status = store.set_auto_update(false);
        assert!(!status.auto_update);
        assert!(!store.check_due(1_000 + 10 * CHECK_INTERVAL_SECS));
        store.record_check(2_000, &Err("offline".into()));
        assert_eq!(store.load().status.last_error.as_deref(), Some("offline"));
    }

    #[test]
    fn versions_compare_numerically() {
        assert!(compare_versions("2026.11.10", "2026.11.9").is_gt());
        assert!(compare_versions("0.0.0", "2026.1.1").is_lt());
        assert!(compare_versions("1.0", "1.0.0").is_eq());
    }
}
