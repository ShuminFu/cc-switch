//! Sync credentials in the OS keychain (CMD-04).
//!
//! In memory, [`AppSettings`] always holds the real WebDAV password, S3 secret
//! access key and sync encryption passphrase, so the sync code is unchanged.
//! Only the copy written to `settings.json` differs:
//!
//! - **Save** ([`externalize_for_disk`]): each non-empty secret is written to
//!   the keychain and blanked in the file, which records `*InKeychain: true`.
//!   When the keychain is unavailable (headless Linux without Secret Service,
//!   unsupported platform) the secret stays in `settings.json` as before
//!   (0600 on Unix) and a warning is logged.
//! - **Load** ([`internalize_from_disk`]): secrets flagged as in the keychain
//!   are read back. Plaintext secrets left by older versions are migrated on
//!   the next save, which `AppSettings::load_from_file` triggers immediately.
//! - A secret whose keychain read failed (e.g. locked keychain) is
//!   "unresolved": saves keep the keychain flag and never delete the entry,
//!   so a transient failure cannot destroy the stored credential.
//! - Clearing a secret (or removing the sync config) deletes its entry.
//!
//! Keychain writes are skipped when the value is unchanged, so frequent
//! settings saves (sync status, current provider) do not touch the keychain.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use sha2::{Digest, Sha256};

use super::{backend, SecretBackend, SecretError};
use crate::settings::AppSettings;

/// Keychain account names.
pub const WEBDAV_PASSWORD_ACCOUNT: &str = "sync.webdav.password";
pub const S3_SECRET_ACCOUNT: &str = "sync.s3.secret-access-key";
pub const SYNC_PASSPHRASE_ACCOUNT: &str = "sync.encryption.passphrase";

/// Where a secret currently lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretLocation {
    Keychain,
    SettingsFile,
    /// No value configured.
    None,
}

impl SecretLocation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Keychain => "keychain",
            Self::SettingsFile => "settingsFile",
            Self::None => "none",
        }
    }
}

#[derive(Default)]
struct State {
    /// SHA-256 of the value last written to / read from the keychain.
    stored: HashMap<&'static str, [u8; 32]>,
    /// Accounts that may have a keychain entry (so clearing must delete it).
    maybe_present: HashSet<&'static str>,
    /// Accounts whose keychain read failed; never deleted or overwritten blank.
    unresolved: HashSet<&'static str>,
    /// Accounts whose last write fell back to settings.json.
    fallback: HashSet<&'static str>,
}

/// Keychain-backed credential store with per-account caching.
pub struct CredentialStore<'a> {
    backend: &'a dyn SecretBackend,
    state: Mutex<State>,
}

fn digest(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
}

type Slot<'s> = Option<(&'s mut String, &'s mut bool)>;

/// Visit the three credential slots of `settings`.
fn for_each_slot(settings: &mut AppSettings, mut f: impl FnMut(&'static str, Slot<'_>)) {
    f(
        WEBDAV_PASSWORD_ACCOUNT,
        settings
            .webdav_sync
            .as_mut()
            .map(|s| (&mut s.password, &mut s.password_in_keychain)),
    );
    f(
        S3_SECRET_ACCOUNT,
        settings
            .s3_sync
            .as_mut()
            .map(|s| (&mut s.secret_access_key, &mut s.secret_in_keychain)),
    );
    f(
        SYNC_PASSPHRASE_ACCOUNT,
        settings
            .sync_encryption
            .as_mut()
            .map(|s| (&mut s.passphrase, &mut s.passphrase_in_keychain)),
    );
}

impl<'a> CredentialStore<'a> {
    pub fn new(backend: &'a dyn SecretBackend) -> Self {
        Self {
            backend,
            state: Mutex::new(State::default()),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Write `value` to the keychain unless it is already there.
    fn store(&self, account: &'static str, value: &str) -> bool {
        let hash = digest(value);
        if self.state().stored.get(account) == Some(&hash) {
            return true;
        }
        match self.backend.set(account, value) {
            Ok(()) => {
                let mut st = self.state();
                st.stored.insert(account, hash);
                st.maybe_present.insert(account);
                st.unresolved.remove(account);
                st.fallback.remove(account);
                true
            }
            Err(err) => {
                let first = self.state().fallback.insert(account);
                if first {
                    log::warn!(
                        "[Secrets] Cannot store {account} in the OS keychain ({}: {err}); keeping it in settings.json (owner-only file permissions on Unix).",
                        self.backend.name()
                    );
                }
                false
            }
        }
    }

    /// Delete the keychain entry if one may exist.
    fn remove(&self, account: &'static str) {
        if !self.state().maybe_present.contains(account) {
            return;
        }
        match self.backend.delete(account) {
            Ok(()) => {
                let mut st = self.state();
                st.maybe_present.remove(account);
                st.stored.remove(account);
            }
            Err(err) => log::warn!("[Secrets] Failed to delete {account} from the keychain: {err}"),
        }
    }

    /// Prepare the on-disk copy of `settings`: move secrets to the keychain.
    pub fn externalize(&self, settings: &mut AppSettings) {
        for_each_slot(settings, |account, slot| match slot {
            Some((value, flag)) if !value.is_empty() => {
                if self.store(account, value) {
                    value.clear();
                    *flag = true;
                } else {
                    *flag = false;
                }
            }
            Some((_value, flag)) => {
                if self.state().unresolved.contains(account) {
                    // Keep pointing at the entry we could not read.
                    *flag = true;
                } else {
                    self.remove(account);
                    *flag = false;
                }
            }
            None => {
                self.state().unresolved.remove(account);
                self.remove(account);
            }
        });
    }

    /// Fill keychain-held secrets into freshly loaded `settings`.
    pub fn internalize(&self, settings: &mut AppSettings) {
        for_each_slot(settings, |account, slot| {
            let Some((value, flag)) = slot else {
                return;
            };
            if !*flag {
                return;
            }
            self.state().maybe_present.insert(account);
            if !value.is_empty() {
                // Both a file value and a keychain flag: the file value is
                // newer (written during a keychain outage); it wins.
                return;
            }
            match self.backend.get(account) {
                Ok(Some(secret)) => {
                    let mut st = self.state();
                    st.stored.insert(account, digest(&secret));
                    st.unresolved.remove(account);
                    drop(st);
                    *value = secret;
                }
                Ok(None) => {
                    log::warn!(
                        "[Secrets] {account} is marked as stored in the keychain but no entry was found; re-enter it in settings."
                    );
                    self.state().maybe_present.remove(account);
                    *flag = false;
                }
                Err(err) => {
                    let level = match err {
                        SecretError::Unavailable(_) => log::Level::Warn,
                        SecretError::Failed(_) => log::Level::Error,
                    };
                    log::log!(
                        level,
                        "[Secrets] Cannot read {account} from the keychain ({err}); sync will fail until it is available. The stored entry is kept."
                    );
                    self.state().unresolved.insert(account);
                }
            }
        });
    }

    /// Where the secret for `account` currently lives, given its in-memory value.
    pub fn location(&self, account: &'static str, value: &str) -> SecretLocation {
        let st = self.state();
        if st.unresolved.contains(account) {
            return SecretLocation::Keychain;
        }
        if value.is_empty() {
            return SecretLocation::None;
        }
        if st.stored.get(account) == Some(&digest(value)) {
            SecretLocation::Keychain
        } else {
            SecretLocation::SettingsFile
        }
    }
}

/// Plaintext credentials present in `settings` (as loaded from disk) that the
/// keychain migration should try to move.
pub fn has_plaintext_credentials(settings: &AppSettings) -> bool {
    let store = global();
    let webdav = settings.webdav_sync.as_ref().is_some_and(|s| {
        !s.password.is_empty()
            && store.location(WEBDAV_PASSWORD_ACCOUNT, &s.password) != SecretLocation::Keychain
    });
    let s3 = settings.s3_sync.as_ref().is_some_and(|s| {
        !s.secret_access_key.is_empty()
            && store.location(S3_SECRET_ACCOUNT, &s.secret_access_key) != SecretLocation::Keychain
    });
    let pass = settings.sync_encryption.as_ref().is_some_and(|s| {
        !s.passphrase.is_empty()
            && store.location(SYNC_PASSPHRASE_ACCOUNT, &s.passphrase) != SecretLocation::Keychain
    });
    webdav || s3 || pass
}

fn global() -> &'static CredentialStore<'static> {
    static STORE: OnceLock<CredentialStore<'static>> = OnceLock::new();
    STORE.get_or_init(|| CredentialStore::new(backend()))
}

/// See [`CredentialStore::externalize`]; uses the process-wide backend.
pub fn externalize_for_disk(settings: &mut AppSettings) {
    global().externalize(settings);
}

/// See [`CredentialStore::internalize`]; uses the process-wide backend.
pub fn internalize_from_disk(settings: &mut AppSettings) {
    global().internalize(settings);
}

/// Where the secret for `account` lives (process-wide store).
pub fn location(account: &'static str, value: &str) -> SecretLocation {
    global().location(account, value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::{MemoryBackend, UnavailableBackend};
    use crate::settings::{S3SyncSettings, SyncEncryptionSettings, WebDavSyncSettings};

    fn configured() -> AppSettings {
        AppSettings {
            webdav_sync: Some(WebDavSyncSettings {
                base_url: "https://dav.example.com".to_string(),
                username: "alice".to_string(),
                password: "dav-password".to_string(),
                ..WebDavSyncSettings::default()
            }),
            s3_sync: Some(S3SyncSettings {
                bucket: "b".to_string(),
                region: "r".to_string(),
                access_key_id: "AKID".to_string(),
                secret_access_key: "s3-secret".to_string(),
                ..S3SyncSettings::default()
            }),
            sync_encryption: Some(SyncEncryptionSettings {
                enabled: true,
                passphrase: "sync passphrase".to_string(),
                passphrase_in_keychain: false,
            }),
            ..AppSettings::default()
        }
    }

    fn disk_json(store: &CredentialStore<'_>, settings: &AppSettings) -> (AppSettings, String) {
        let mut disk = settings.clone();
        store.externalize(&mut disk);
        let json = serde_json::to_string(&disk).unwrap();
        (disk, json)
    }

    #[test]
    fn settings_file_has_no_credentials_when_keychain_available() {
        let backend = MemoryBackend::new();
        let store = CredentialStore::new(&backend);
        let (_disk, json) = disk_json(&store, &configured());
        for secret in ["dav-password", "s3-secret", "sync passphrase"] {
            assert!(
                !json.contains(secret),
                "{secret} leaked into settings.json: {json}"
            );
        }
        assert!(json.contains("\"passwordInKeychain\":true"));
        assert!(json.contains("\"secretInKeychain\":true"));
        assert!(json.contains("\"passphraseInKeychain\":true"));
        assert_eq!(
            backend.get(WEBDAV_PASSWORD_ACCOUNT).unwrap().as_deref(),
            Some("dav-password")
        );

        // Loading the file restores the real values in memory.
        let mut loaded: AppSettings = serde_json::from_str(&json).unwrap();
        let fresh = CredentialStore::new(&backend);
        fresh.internalize(&mut loaded);
        assert_eq!(loaded.webdav_sync.unwrap().password, "dav-password");
        assert_eq!(loaded.s3_sync.unwrap().secret_access_key, "s3-secret");
        assert_eq!(
            loaded.sync_encryption.unwrap().passphrase,
            "sync passphrase"
        );
    }

    #[test]
    fn legacy_plaintext_settings_are_migrated_on_save() {
        let backend = MemoryBackend::new();
        let store = CredentialStore::new(&backend);
        // A settings.json from an older version: plaintext, no flags.
        let legacy_json = serde_json::to_string(&configured()).unwrap();
        let mut loaded: AppSettings = serde_json::from_str(&legacy_json).unwrap();
        store.internalize(&mut loaded);
        assert_eq!(
            loaded.webdav_sync.as_ref().unwrap().password,
            "dav-password"
        );
        assert_eq!(
            store.location(WEBDAV_PASSWORD_ACCOUNT, "dav-password"),
            SecretLocation::SettingsFile
        );
        let (_disk, json) = disk_json(&store, &loaded);
        assert!(!json.contains("dav-password"));
        assert_eq!(
            store.location(WEBDAV_PASSWORD_ACCOUNT, "dav-password"),
            SecretLocation::Keychain
        );
    }

    #[test]
    fn falls_back_to_settings_file_without_keychain() {
        let store = CredentialStore::new(&UnavailableBackend);
        let (disk, json) = disk_json(&store, &configured());
        assert!(json.contains("dav-password"));
        assert!(json.contains("s3-secret"));
        assert!(!disk.webdav_sync.unwrap().password_in_keychain);
        assert_eq!(
            store.location(S3_SECRET_ACCOUNT, "s3-secret"),
            SecretLocation::SettingsFile
        );
        // And loads back unchanged.
        let mut loaded: AppSettings = serde_json::from_str(&json).unwrap();
        store.internalize(&mut loaded);
        assert_eq!(loaded.s3_sync.unwrap().secret_access_key, "s3-secret");
    }

    #[test]
    fn clearing_a_secret_deletes_the_keychain_entry() {
        let backend = MemoryBackend::new();
        let store = CredentialStore::new(&backend);
        let mut settings = configured();
        disk_json(&store, &settings);
        assert!(backend.get(WEBDAV_PASSWORD_ACCOUNT).unwrap().is_some());

        settings.webdav_sync.as_mut().unwrap().password.clear();
        settings.s3_sync = None;
        let (disk, _json) = disk_json(&store, &settings);
        assert!(!disk.webdav_sync.unwrap().password_in_keychain);
        assert_eq!(backend.get(WEBDAV_PASSWORD_ACCOUNT).unwrap(), None);
        assert_eq!(backend.get(S3_SECRET_ACCOUNT).unwrap(), None);
        assert!(backend.get(SYNC_PASSPHRASE_ACCOUNT).unwrap().is_some());
    }

    /// A backend whose reads fail, to simulate a locked keychain.
    struct ReadFails(MemoryBackend);
    impl SecretBackend for ReadFails {
        fn name(&self) -> &'static str {
            "read-fails"
        }
        fn get(&self, _a: &str) -> Result<Option<String>, SecretError> {
            Err(SecretError::Unavailable("locked".to_string()))
        }
        fn set(&self, a: &str, v: &str) -> Result<(), SecretError> {
            self.0.set(a, v)
        }
        fn delete(&self, a: &str) -> Result<(), SecretError> {
            self.0.delete(a)
        }
    }

    #[test]
    fn unreadable_keychain_entry_is_never_deleted_by_a_save() {
        let backend = ReadFails(MemoryBackend::new());
        backend
            .0
            .set(WEBDAV_PASSWORD_ACCOUNT, "dav-password")
            .unwrap();
        let json = r#"{"webdavSync":{"baseUrl":"https://d","username":"u","password":"","passwordInKeychain":true}}"#;
        let mut loaded: AppSettings = serde_json::from_str(json).unwrap();
        let store = CredentialStore::new(&backend);
        store.internalize(&mut loaded);
        assert_eq!(loaded.webdav_sync.as_ref().unwrap().password, "");

        let (disk, _json) = disk_json(&store, &loaded);
        assert!(disk.webdav_sync.unwrap().password_in_keychain);
        assert_eq!(
            backend.0.get(WEBDAV_PASSWORD_ACCOUNT).unwrap().as_deref(),
            Some("dav-password")
        );
    }

    #[test]
    fn unchanged_secret_is_not_rewritten() {
        struct CountingSets(MemoryBackend, Mutex<usize>);
        impl SecretBackend for CountingSets {
            fn name(&self) -> &'static str {
                "counting"
            }
            fn get(&self, a: &str) -> Result<Option<String>, SecretError> {
                self.0.get(a)
            }
            fn set(&self, a: &str, v: &str) -> Result<(), SecretError> {
                *self.1.lock().unwrap() += 1;
                self.0.set(a, v)
            }
            fn delete(&self, a: &str) -> Result<(), SecretError> {
                self.0.delete(a)
            }
        }
        let backend = CountingSets(MemoryBackend::new(), Mutex::new(0));
        let store = CredentialStore::new(&backend);
        let settings = configured();
        disk_json(&store, &settings);
        disk_json(&store, &settings);
        disk_json(&store, &settings);
        assert_eq!(*backend.1.lock().unwrap(), 3, "one write per secret");
    }

    #[test]
    fn missing_keychain_entry_clears_the_flag() {
        let backend = MemoryBackend::new();
        let json = r#"{"s3Sync":{"bucket":"b","region":"r","accessKeyId":"a","secretAccessKey":"","secretInKeychain":true}}"#;
        let mut loaded: AppSettings = serde_json::from_str(json).unwrap();
        let store = CredentialStore::new(&backend);
        store.internalize(&mut loaded);
        assert!(!loaded.s3_sync.as_ref().unwrap().secret_in_keychain);
    }
}
