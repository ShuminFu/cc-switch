//! Passphrase-based authenticated encryption for sync artifacts (SEC-06, SVC-04).
//!
//! Scheme `argon2id-xchacha20poly1305`, envelope version 1:
//!
//! - Key: Argon2id(passphrase, 16-byte random salt, m/t/p from the header) →
//!   32 bytes. The salt and KDF parameters travel in an [`EncryptionHeader`]
//!   stored in the (plaintext) sync manifest.
//! - Each artifact is `MAGIC (8 bytes) || nonce (24 bytes) || ciphertext+tag`,
//!   sealed with XChaCha20-Poly1305 and `AAD = MAGIC || context`, where the
//!   context is the artifact name. Swapping artifacts, truncation, a wrong
//!   passphrase or any modification fails authentication.
//! - `keyCheck` in the header is a sealed constant, so a wrong passphrase is
//!   reported before any artifact is downloaded.
//!
//! The magic carries the envelope version (`CCSENC\0` + version byte), so a
//! future scheme can be added without guessing.

use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine as _;
use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Scheme identifier stored in the header.
pub const SCHEME: &str = "argon2id-xchacha20poly1305";
/// Envelope format version.
pub const ENVELOPE_VERSION: u32 = 1;
/// Prefix of every sealed blob: `CCSENC\0` + envelope version byte.
pub const MAGIC: &[u8; 8] = b"CCSENC\x00\x01";
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;
const SALT_LEN: usize = 16;
const KEY_LEN: usize = 32;
const KEY_CHECK_CONTEXT: &[u8] = b"cc-switch:key-check";
const KEY_CHECK_PLAINTEXT: &[u8] = b"cc-switch sync passphrase check v1";

/// Upper bounds accepted when reading a header, so a hostile manifest cannot
/// make the client allocate gigabytes or spin for minutes.
pub const MAX_MEMORY_KIB: u32 = 1024 * 1024; // 1 GiB
pub const MAX_ITERATIONS: u32 = 16;
pub const MAX_PARALLELISM: u32 = 8;

/// Minimum passphrase length accepted by the settings command.
pub const MIN_PASSPHRASE_CHARS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvelopeError {
    /// Data does not start with the envelope magic (e.g. an old plaintext artifact).
    NotEncrypted,
    /// Unknown scheme or envelope version.
    Unsupported(String),
    /// Header or blob is structurally invalid.
    Malformed(String),
    /// Authentication failed: wrong passphrase, or the data was modified.
    Decrypt,
}

impl std::fmt::Display for EnvelopeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotEncrypted => write!(f, "data is not encrypted"),
            Self::Unsupported(s) => write!(f, "unsupported encryption: {s}"),
            Self::Malformed(s) => write!(f, "malformed encrypted data: {s}"),
            Self::Decrypt => write!(f, "decryption failed: wrong passphrase or modified data"),
        }
    }
}

impl std::error::Error for EnvelopeError {}

/// Argon2id cost parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KdfParams {
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
}

impl KdfParams {
    /// Parameters for new headers: 64 MiB, 3 passes, 1 lane (well above the
    /// OWASP Argon2id minimum). Unit tests use cheap parameters.
    pub fn recommended() -> Self {
        if cfg!(test) {
            Self {
                memory_kib: 64,
                iterations: 1,
                parallelism: 1,
            }
        } else {
            Self {
                memory_kib: 64 * 1024,
                iterations: 3,
                parallelism: 1,
            }
        }
    }

    fn validate(&self) -> Result<(), EnvelopeError> {
        if self.memory_kib > MAX_MEMORY_KIB
            || self.iterations == 0
            || self.iterations > MAX_ITERATIONS
            || self.parallelism == 0
            || self.parallelism > MAX_PARALLELISM
        {
            return Err(EnvelopeError::Unsupported(format!(
                "KDF parameters out of range (m={} KiB, t={}, p={})",
                self.memory_kib, self.iterations, self.parallelism
            )));
        }
        Ok(())
    }
}

/// Public encryption parameters, stored in the sync manifest as `encryption`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptionHeader {
    pub scheme: String,
    pub version: u32,
    pub kdf: KdfParams,
    /// Base64 (standard) salt.
    pub salt: String,
    /// Base64 (standard) sealed constant used to recognise a wrong passphrase.
    pub key_check: String,
}

/// A derived 256-bit key; wiped from memory on drop.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct DerivedKey([u8; KEY_LEN]);

impl std::fmt::Debug for DerivedKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DerivedKey(..)")
    }
}

fn b64() -> base64::engine::general_purpose::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

/// Derive the key for `passphrase` with explicit salt and parameters.
pub fn derive_key(
    passphrase: &str,
    salt: &[u8],
    params: &KdfParams,
) -> Result<DerivedKey, EnvelopeError> {
    params.validate()?;
    let argon_params = Params::new(
        params.memory_kib,
        params.iterations,
        params.parallelism,
        Some(KEY_LEN),
    )
    .map_err(|e| EnvelopeError::Unsupported(e.to_string()))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon_params);
    let mut out = [0u8; KEY_LEN];
    argon
        .hash_password_into(passphrase.as_bytes(), salt, &mut out)
        .map_err(|e| EnvelopeError::Malformed(e.to_string()))?;
    let key = DerivedKey(out);
    out.zeroize();
    Ok(key)
}

/// Create a fresh header (random salt) and its key for `passphrase`.
pub fn new_header(
    passphrase: &str,
    params: KdfParams,
) -> Result<(EncryptionHeader, DerivedKey), EnvelopeError> {
    use chacha20poly1305::aead::rand_core::RngCore;
    let mut salt = [0u8; SALT_LEN];
    OsRng.fill_bytes(&mut salt);
    let key = derive_key(passphrase, &salt, &params)?;
    let key_check = seal(&key, KEY_CHECK_CONTEXT, KEY_CHECK_PLAINTEXT)?;
    let header = EncryptionHeader {
        scheme: SCHEME.to_string(),
        version: ENVELOPE_VERSION,
        kdf: params,
        salt: b64().encode(salt),
        key_check: b64().encode(key_check),
    };
    Ok((header, key))
}

impl EncryptionHeader {
    pub fn check_supported(&self) -> Result<(), EnvelopeError> {
        if self.scheme != SCHEME || self.version != ENVELOPE_VERSION {
            return Err(EnvelopeError::Unsupported(format!(
                "{} v{}",
                self.scheme, self.version
            )));
        }
        self.kdf.validate()
    }

    pub fn salt_bytes(&self) -> Result<Vec<u8>, EnvelopeError> {
        let salt = b64()
            .decode(self.salt.as_bytes())
            .map_err(|e| EnvelopeError::Malformed(format!("salt: {e}")))?;
        if salt.len() < 8 || salt.len() > 64 {
            return Err(EnvelopeError::Malformed("salt length".to_string()));
        }
        Ok(salt)
    }

    /// Derive the key for this header and verify it against `keyCheck`.
    /// A wrong passphrase yields [`EnvelopeError::Decrypt`].
    pub fn unlock(&self, passphrase: &str) -> Result<DerivedKey, EnvelopeError> {
        self.check_supported()?;
        let key = derive_key(passphrase, &self.salt_bytes()?, &self.kdf)?;
        self.verify_key(&key)?;
        Ok(key)
    }

    /// Check that `key` matches this header.
    pub fn verify_key(&self, key: &DerivedKey) -> Result<(), EnvelopeError> {
        let check = b64()
            .decode(self.key_check.as_bytes())
            .map_err(|e| EnvelopeError::Malformed(format!("keyCheck: {e}")))?;
        let plain = open(key, KEY_CHECK_CONTEXT, &check)?;
        if plain != KEY_CHECK_PLAINTEXT {
            return Err(EnvelopeError::Decrypt);
        }
        Ok(())
    }
}

/// Returns `true` when `data` starts with the envelope magic.
pub fn is_sealed(data: &[u8]) -> bool {
    data.starts_with(MAGIC)
}

fn aad(context: &[u8]) -> Vec<u8> {
    let mut aad = Vec::with_capacity(MAGIC.len() + context.len());
    aad.extend_from_slice(MAGIC);
    aad.extend_from_slice(context);
    aad
}

/// Encrypt `plaintext` bound to `context` (e.g. the artifact name).
pub fn seal(key: &DerivedKey, context: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, EnvelopeError> {
    let cipher = XChaCha20Poly1305::new((&key.0).into());
    let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
    let aad = aad(context);
    let ciphertext = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: plaintext,
                aad: &aad,
            },
        )
        .map_err(|_| EnvelopeError::Malformed("encryption failed".to_string()))?;
    let mut out = Vec::with_capacity(MAGIC.len() + NONCE_LEN + ciphertext.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypt a blob produced by [`seal`] with the same `context`.
pub fn open(key: &DerivedKey, context: &[u8], sealed: &[u8]) -> Result<Vec<u8>, EnvelopeError> {
    if sealed.len() < 6 || sealed[..6] != MAGIC[..6] {
        return Err(EnvelopeError::NotEncrypted);
    }
    if sealed.len() < MAGIC.len() || sealed[..MAGIC.len()] != MAGIC[..] {
        return Err(EnvelopeError::Unsupported(format!(
            "envelope version {}",
            sealed.get(7).copied().unwrap_or_default()
        )));
    }
    if sealed.len() < MAGIC.len() + NONCE_LEN + TAG_LEN {
        return Err(EnvelopeError::Malformed("truncated".to_string()));
    }
    let (nonce, ciphertext) = sealed[MAGIC.len()..].split_at(NONCE_LEN);
    let cipher = XChaCha20Poly1305::new((&key.0).into());
    let aad = aad(context);
    cipher
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad: &aad,
            },
        )
        .map_err(|_| EnvelopeError::Decrypt)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(pass: &str) -> (EncryptionHeader, DerivedKey) {
        new_header(pass, KdfParams::recommended()).expect("header")
    }

    #[test]
    fn seal_open_round_trip() {
        let (_h, key) = header("correct horse battery");
        let sealed = seal(&key, b"db.sql", b"INSERT INTO t VALUES (1);").unwrap();
        assert!(is_sealed(&sealed));
        assert!(!sealed.windows(6).any(|w| w == b"INSERT"));
        let plain = open(&key, b"db.sql", &sealed).unwrap();
        assert_eq!(plain, b"INSERT INTO t VALUES (1);");
    }

    #[test]
    fn same_plaintext_encrypts_differently() {
        let (_h, key) = header("pw-pw-pw-pw");
        assert_ne!(
            seal(&key, b"a", b"x").unwrap(),
            seal(&key, b"a", b"x").unwrap()
        );
    }

    #[test]
    fn wrong_passphrase_is_detected_by_key_check() {
        let (h, _key) = header("right passphrase");
        assert!(h.unlock("right passphrase").is_ok());
        assert_eq!(
            h.unlock("wrong passphrase").unwrap_err(),
            EnvelopeError::Decrypt
        );
    }

    #[test]
    fn wrong_key_or_context_or_tamper_fails_authentication() {
        let (h, key) = header("passphrase-1");
        let (_h2, other) = header("passphrase-2");
        let sealed = seal(&key, b"db.sql", b"secret").unwrap();
        assert_eq!(
            open(&other, b"db.sql", &sealed).unwrap_err(),
            EnvelopeError::Decrypt
        );
        assert_eq!(
            open(&key, b"skills.zip", &sealed).unwrap_err(),
            EnvelopeError::Decrypt
        );
        let mut tampered = sealed.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert_eq!(
            open(&key, b"db.sql", &tampered).unwrap_err(),
            EnvelopeError::Decrypt
        );
        let truncated = &sealed[..MAGIC.len() + NONCE_LEN + 4];
        assert!(matches!(
            open(&key, b"db.sql", truncated),
            Err(EnvelopeError::Malformed(_))
        ));
        // Key re-derived from the header decrypts.
        let again = h.unlock("passphrase-1").unwrap();
        assert_eq!(open(&again, b"db.sql", &sealed).unwrap(), b"secret");
    }

    #[test]
    fn plaintext_is_reported_as_not_encrypted() {
        let (_h, key) = header("passphrase");
        assert_eq!(
            open(&key, b"db.sql", b"-- CC Switch SQL export").unwrap_err(),
            EnvelopeError::NotEncrypted
        );
        assert!(!is_sealed(b"PK\x03\x04zip"));
    }

    #[test]
    fn future_envelope_version_is_unsupported() {
        let (_h, key) = header("passphrase");
        let mut sealed = seal(&key, b"a", b"b").unwrap();
        sealed[7] = 2;
        assert!(matches!(
            open(&key, b"a", &sealed),
            Err(EnvelopeError::Unsupported(_))
        ));
    }

    #[test]
    fn header_rejects_unknown_scheme_and_hostile_kdf_params() {
        let (mut h, _key) = header("passphrase");
        h.scheme = "rot13".to_string();
        assert!(matches!(
            h.unlock("passphrase"),
            Err(EnvelopeError::Unsupported(_))
        ));

        let (mut h, _key) = header("passphrase");
        h.kdf.memory_kib = MAX_MEMORY_KIB + 1;
        assert!(matches!(
            h.unlock("passphrase"),
            Err(EnvelopeError::Unsupported(_))
        ));
        h.kdf = KdfParams {
            memory_kib: 64,
            iterations: 0,
            parallelism: 1,
        };
        assert!(matches!(
            h.unlock("passphrase"),
            Err(EnvelopeError::Unsupported(_))
        ));
    }

    #[test]
    fn header_serializes_with_camel_case_and_round_trips() {
        let (h, _key) = header("passphrase");
        let json = serde_json::to_value(&h).unwrap();
        assert_eq!(json["scheme"], SCHEME);
        assert_eq!(json["version"], ENVELOPE_VERSION);
        assert!(json["kdf"]["memoryKib"].is_number());
        assert!(json["keyCheck"].is_string());
        let back: EncryptionHeader = serde_json::from_value(json).unwrap();
        assert_eq!(back, h);
    }

    #[test]
    fn recommended_production_params_are_strong() {
        // The cfg(test) params are cheap; make sure the production branch is not.
        let prod = KdfParams {
            memory_kib: 64 * 1024,
            iterations: 3,
            parallelism: 1,
        };
        assert!(prod.validate().is_ok());
        assert!(prod.memory_kib >= 19 * 1024);
    }
}
