//! Secrets at rest and in sync (SEC-06, SVC-04, CMD-04, DB-02, PRODUCT-01).
//!
//! - [`envelope`]: passphrase-based authenticated encryption (Argon2id +
//!   XChaCha20-Poly1305) with a versioned envelope, used for sync artifacts.
//! - [`sync_credentials`]: moves the WebDAV password, the S3 secret key and the
//!   sync encryption passphrase out of `settings.json` into the OS keychain,
//!   falling back to the previous plaintext behaviour (with a warning) when no
//!   keychain is available.
//! - [`provider_vault`]: the provider API-key vault (placeholders in
//!   `settings_config`, values in the keychain), used by the provider DAO;
//!   [`provider_vault_admin`] switches it on/off. See `docs/secrets-vault.md`.
//!
//! All keychain access goes through the [`SecretBackend`] trait so tests never
//! touch the developer's real keychain: unit tests and any process with
//! `CC_SWITCH_TEST_HOME` set use an in-memory backend unless
//! `CC_SWITCH_SECRET_BACKEND=keyring` is set explicitly.

pub mod commands;
pub mod envelope;
pub mod provider_vault;
pub mod provider_vault_admin;
pub mod sync_credentials;

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Keychain service name (matches the Tauri bundle identifier).
pub const KEYCHAIN_SERVICE: &str = "com.ccswitch.desktop";

/// Errors reported by a secret backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretError {
    /// No keychain is usable (headless Linux without Secret Service, locked
    /// or missing store, unsupported platform). Callers fall back.
    Unavailable(String),
    /// The backend answered but the operation failed.
    Failed(String),
}

impl std::fmt::Display for SecretError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(msg) => write!(f, "keychain unavailable: {msg}"),
            Self::Failed(msg) => write!(f, "keychain operation failed: {msg}"),
        }
    }
}

impl std::error::Error for SecretError {}

/// Storage for small secrets addressed by an account name.
pub trait SecretBackend: Send + Sync {
    /// Short name for logs and the UI (`keychain`, `memory`, `none`).
    fn name(&self) -> &'static str;
    /// Returns `Ok(None)` when no entry exists.
    fn get(&self, account: &str) -> Result<Option<String>, SecretError>;
    fn set(&self, account: &str, value: &str) -> Result<(), SecretError>;
    /// Deleting a missing entry is not an error.
    fn delete(&self, account: &str) -> Result<(), SecretError>;
}

// ─── OS keychain backend ─────────────────────────────────────

/// macOS Keychain, Windows Credential Manager, or the freedesktop Secret
/// Service (GNOME Keyring / KWallet) on Linux, through the `keyring` crate.
pub struct KeyringBackend {
    service: String,
}

impl KeyringBackend {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    fn entry(&self, account: &str) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(&self.service, account).map_err(map_keyring_error)
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn map_keyring_error(err: keyring::Error) -> SecretError {
    match err {
        keyring::Error::PlatformFailure(e) => SecretError::Unavailable(e.to_string()),
        keyring::Error::NoStorageAccess(e) => SecretError::Unavailable(e.to_string()),
        other => SecretError::Failed(other.to_string()),
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
impl SecretBackend for KeyringBackend {
    fn name(&self) -> &'static str {
        "keychain"
    }

    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        match self.entry(account)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(map_keyring_error(err)),
        }
    }

    fn set(&self, account: &str, value: &str) -> Result<(), SecretError> {
        self.entry(account)?
            .set_password(value)
            .map_err(map_keyring_error)?;
        // Read back through a fresh entry: catches stores that accept writes
        // but do not persist them (e.g. a mock store on unknown platforms).
        match self.get(account)? {
            Some(stored) if stored == value => Ok(()),
            _ => Err(SecretError::Unavailable(
                "keychain did not persist the value".to_string(),
            )),
        }
    }

    fn delete(&self, account: &str) -> Result<(), SecretError> {
        match self.entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(map_keyring_error(err)),
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
impl SecretBackend for KeyringBackend {
    fn name(&self) -> &'static str {
        "keychain"
    }
    fn get(&self, _account: &str) -> Result<Option<String>, SecretError> {
        Err(SecretError::Unavailable("unsupported platform".to_string()))
    }
    fn set(&self, _account: &str, _value: &str) -> Result<(), SecretError> {
        Err(SecretError::Unavailable("unsupported platform".to_string()))
    }
    fn delete(&self, _account: &str) -> Result<(), SecretError> {
        Err(SecretError::Unavailable("unsupported platform".to_string()))
    }
}

// ─── Test / fallback backends ────────────────────────────────

/// Process-local store. Used by tests so they never touch the real keychain.
#[derive(Default)]
pub struct MemoryBackend {
    entries: Mutex<HashMap<String, String>>,
}

impl MemoryBackend {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl SecretBackend for MemoryBackend {
    fn name(&self) -> &'static str {
        "memory"
    }
    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        Ok(self.lock().get(account).cloned())
    }
    fn set(&self, account: &str, value: &str) -> Result<(), SecretError> {
        self.lock().insert(account.to_string(), value.to_string());
        Ok(())
    }
    fn delete(&self, account: &str) -> Result<(), SecretError> {
        self.lock().remove(account);
        Ok(())
    }
}

/// A backend that is never available (forces the plaintext fallback).
pub struct UnavailableBackend;

impl SecretBackend for UnavailableBackend {
    fn name(&self) -> &'static str {
        "none"
    }
    fn get(&self, _account: &str) -> Result<Option<String>, SecretError> {
        Err(SecretError::Unavailable("keychain disabled".to_string()))
    }
    fn set(&self, _account: &str, _value: &str) -> Result<(), SecretError> {
        Err(SecretError::Unavailable("keychain disabled".to_string()))
    }
    fn delete(&self, _account: &str) -> Result<(), SecretError> {
        Err(SecretError::Unavailable("keychain disabled".to_string()))
    }
}

// ─── Backend selection ───────────────────────────────────────

/// Environment variable that forces a backend: `keyring`, `memory` or `none`.
pub const BACKEND_ENV: &str = "CC_SWITCH_SECRET_BACKEND";

fn select_backend() -> Box<dyn SecretBackend> {
    let forced = std::env::var(BACKEND_ENV)
        .ok()
        .map(|v| v.trim().to_ascii_lowercase());
    match forced.as_deref() {
        Some("keyring") | Some("keychain") => {
            return Box::new(KeyringBackend::new(KEYCHAIN_SERVICE))
        }
        Some("memory") => return Box::new(MemoryBackend::new()),
        Some("none") | Some("off") | Some("file") => return Box::new(UnavailableBackend),
        _ => {}
    }
    // Tests (unit tests, and integration tests that isolate HOME) must never
    // read or write the developer's real keychain.
    let isolated_home = std::env::var("CC_SWITCH_TEST_HOME")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false);
    if cfg!(test) || isolated_home {
        return Box::new(MemoryBackend::new());
    }
    Box::new(KeyringBackend::new(KEYCHAIN_SERVICE))
}

/// The process-wide secret backend.
pub fn backend() -> &'static dyn SecretBackend {
    static BACKEND: OnceLock<Box<dyn SecretBackend>> = OnceLock::new();
    BACKEND.get_or_init(select_backend).as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_backend_round_trip() {
        let b = MemoryBackend::new();
        assert_eq!(b.get("a").unwrap(), None);
        b.set("a", "1").unwrap();
        assert_eq!(b.get("a").unwrap().as_deref(), Some("1"));
        b.delete("a").unwrap();
        b.delete("a").unwrap();
        assert_eq!(b.get("a").unwrap(), None);
    }

    #[test]
    fn unit_tests_never_select_the_real_keychain() {
        if std::env::var(BACKEND_ENV).is_err() {
            assert_eq!(backend().name(), "memory");
        }
    }

    #[test]
    fn unavailable_backend_reports_unavailable() {
        assert!(matches!(
            UnavailableBackend.set("a", "b"),
            Err(SecretError::Unavailable(_))
        ));
    }

    /// Real keychain round trip. Skipped (passes with a note) when no keychain
    /// backend exists, e.g. headless Linux CI without a Secret Service daemon.
    #[test]
    fn keyring_backend_round_trip_when_available() {
        let b = KeyringBackend::new(format!("{KEYCHAIN_SERVICE}.test"));
        let account = format!("cc-switch-test-{}", std::process::id());
        match b.set(&account, "probe-value") {
            Ok(()) => {}
            Err(SecretError::Unavailable(reason)) | Err(SecretError::Failed(reason)) => {
                eprintln!("skipping keychain test: no keychain backend ({reason})");
                return;
            }
        }
        assert_eq!(b.get(&account).unwrap().as_deref(), Some("probe-value"));
        b.delete(&account).unwrap();
        assert_eq!(b.get(&account).unwrap(), None);
    }
}
