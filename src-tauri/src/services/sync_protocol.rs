//! Transport-agnostic sync protocol layer.
//!
//! Shared by WebDAV, S3, and future transports. Artifact set: `db.sql` + `skills.zip`.

use std::collections::BTreeMap;
use std::fs;
use std::process::Command;

use chrono::Utc;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::tempdir;

use crate::error::AppError;

// Re-export archive functions for use by transport layers.
pub(crate) use super::webdav_sync::archive::{
    backup_current_skills, restore_skills_from_backup, restore_skills_zip, zip_skills_ssot,
};

// ─── Protocol constants ──────────────────────────────────────

/// Wire-format identifier stored in remote manifests.
/// Retains historic "webdav" naming for backward compatibility with existing remotes.
pub(crate) const PROTOCOL_FORMAT: &str = "cc-switch-webdav-sync";
pub(crate) const PROTOCOL_VERSION: u32 = 2;
/// Generation of the synced `db.sql`. Bump only for a change an older app cannot
/// import (see docs/dev/DB_COMPATIBILITY.md); additive schema changes keep it.
pub(crate) const DB_COMPAT_VERSION: u32 = 6;
/// Generation assumed for legacy-layout remotes whose manifest predates
/// `dbCompatVersion`. Frozen: never bump.
pub(crate) const LEGACY_DB_COMPAT_VERSION: u32 = 5;
pub(crate) const REMOTE_DB_SQL: &str = "db.sql";
pub(crate) const REMOTE_SKILLS_ZIP: &str = "skills.zip";
pub(crate) const REMOTE_MANIFEST: &str = "manifest.json";
pub(crate) const MAX_DEVICE_NAME_LEN: usize = 64;
pub(crate) const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_SYNC_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;
/// Manifest signature algorithm (HMAC over [`manifest_signing_bytes`]).
pub(crate) const SIGNATURE_ALG: &str = "HMAC-SHA256";
/// Domain tag prepended to the signed manifest bytes.
const SIGNATURE_DOMAIN: &[u8] = b"cc-switch-sync-manifest-v1\n";
/// PBKDF2 salt for deriving the sync key from the passphrase. Fixed so every
/// device derives the same key from the same passphrase.
const SYNC_KEY_SALT: &[u8] = b"cc-switch-sync-snapshot-key-v1";
/// PBKDF2-HMAC-SHA256 rounds. The derived key is stored, so this runs only
/// when the passphrase is entered.
pub(crate) const SYNC_KEY_PBKDF2_ROUNDS: u32 = 600_000;
pub(crate) const MIN_SYNC_PASSPHRASE_CHARS: usize = 8;

// ─── Error helpers ───────────────────────────────────────────

pub(crate) fn localized(
    key: &'static str,
    zh: impl Into<String>,
    en: impl Into<String>,
) -> AppError {
    AppError::localized(key, zh, en)
}

pub(crate) fn io_context_localized(
    _key: &'static str,
    zh: impl Into<String>,
    en: impl Into<String>,
    source: std::io::Error,
) -> AppError {
    let zh_msg = zh.into();
    let en_msg = en.into();
    AppError::IoContext {
        context: format!("{zh_msg} ({en_msg})"),
        source,
    }
}

// ─── Types ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncManifest {
    pub format: String,
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub db_compat_version: Option<u32>,
    pub device_name: String,
    pub created_at: String,
    pub artifacts: BTreeMap<String, ArtifactMeta>,
    pub snapshot_id: String,
    /// HMAC of the other fields with the user's sync key. Older apps ignore it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<ManifestSignature>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ManifestSignature {
    pub alg: String,
    /// Fingerprint of the signing key, to tell a different key from tampering.
    pub key_id: String,
    pub mac: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ArtifactMeta {
    pub sha256: String,
    pub size: u64,
}

pub(crate) struct LocalSnapshot {
    pub db_sql: Vec<u8>,
    pub skills_zip: Vec<u8>,
    pub manifest_bytes: Vec<u8>,
    pub manifest_hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemoteLayout {
    Current,
    Legacy,
}

impl RemoteLayout {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Legacy => "legacy",
        }
    }
}

// ─── Snapshot building ───────────────────────────────────────

/// Build the local snapshot; the manifest is signed when a sync key is configured.
pub(crate) fn build_local_snapshot(
    db: &crate::database::Database,
    key: Option<&SyncKey>,
) -> Result<LocalSnapshot, AppError> {
    // Export database to SQL string
    let sql_string = db.export_sql_string_for_sync()?;
    let db_sql = sql_string.into_bytes();

    // Pack skills into deterministic ZIP
    let tmp = tempdir().map_err(|e| {
        io_context_localized(
            "sync.snapshot_tmpdir_failed",
            "创建快照临时目录失败",
            "Failed to create temporary directory for snapshot",
            e,
        )
    })?;
    let skills_zip_path = tmp.path().join(REMOTE_SKILLS_ZIP);
    zip_skills_ssot(&skills_zip_path)?;
    let skills_zip = fs::read(&skills_zip_path).map_err(|e| AppError::io(&skills_zip_path, e))?;

    // Build artifact map and compute hashes
    let mut artifacts = BTreeMap::new();
    artifacts.insert(
        REMOTE_DB_SQL.to_string(),
        ArtifactMeta {
            sha256: sha256_hex(&db_sql),
            size: db_sql.len() as u64,
        },
    );
    artifacts.insert(
        REMOTE_SKILLS_ZIP.to_string(),
        ArtifactMeta {
            sha256: sha256_hex(&skills_zip),
            size: skills_zip.len() as u64,
        },
    );

    let snapshot_id = compute_snapshot_id(&artifacts);
    let mut manifest = SyncManifest {
        format: PROTOCOL_FORMAT.to_string(),
        version: PROTOCOL_VERSION,
        db_compat_version: Some(DB_COMPAT_VERSION),
        device_name: detect_system_device_name().unwrap_or_else(|| "Unknown Device".to_string()),
        created_at: Utc::now().to_rfc3339(),
        artifacts,
        snapshot_id,
        signature: None,
    };
    if let Some(key) = key {
        sign_manifest(&mut manifest, key)?;
    }
    let manifest_bytes =
        serde_json::to_vec_pretty(&manifest).map_err(|e| AppError::JsonSerialize { source: e })?;
    let manifest_hash = sha256_hex(&manifest_bytes);

    Ok(LocalSnapshot {
        db_sql,
        skills_zip,
        manifest_bytes,
        manifest_hash,
    })
}

// ─── Manifest handling ───────────────────────────────────────

/// Compute a deterministic snapshot identity from artifact hashes.
///
/// BTreeMap iteration order is sorted by key, ensuring stability.
pub(crate) fn compute_snapshot_id(artifacts: &BTreeMap<String, ArtifactMeta>) -> String {
    let parts: Vec<String> = artifacts
        .iter()
        .map(|(name, meta)| format!("{}:{}", name, meta.sha256))
        .collect();
    sha256_hex(parts.join("|").as_bytes())
}

pub(crate) fn effective_db_compat_version(
    manifest: &SyncManifest,
    layout: RemoteLayout,
) -> Option<u32> {
    manifest
        .db_compat_version
        .or_else(|| (layout == RemoteLayout::Legacy).then_some(LEGACY_DB_COMPAT_VERSION))
}

pub(crate) fn validate_manifest_compat(
    manifest: &SyncManifest,
    layout: RemoteLayout,
) -> Result<(), AppError> {
    if manifest.format != PROTOCOL_FORMAT {
        return Err(localized(
            "sync.manifest_format_incompatible",
            format!("远端 manifest 格式不兼容: {}", manifest.format),
            format!(
                "Remote manifest format is incompatible: {}",
                manifest.format
            ),
        ));
    }
    if manifest.version != PROTOCOL_VERSION {
        return Err(localized(
            "sync.manifest_version_incompatible",
            format!(
                "远端 manifest 协议版本不兼容: v{} (本地 v{PROTOCOL_VERSION})",
                manifest.version
            ),
            format!(
                "Remote manifest protocol version is incompatible: v{} (local v{PROTOCOL_VERSION})",
                manifest.version
            ),
        ));
    }
    let Some(db_compat_version) = effective_db_compat_version(manifest, layout) else {
        return Err(localized(
            "sync.manifest_db_version_missing",
            "远端 manifest 缺少数据库兼容版本",
            "Remote manifest is missing the database compatibility version.",
        ));
    };
    match layout {
        RemoteLayout::Current if db_compat_version != DB_COMPAT_VERSION => {
            return Err(localized(
                "sync.manifest_db_version_incompatible",
                format!(
                    "远端数据库快照版本不兼容: db-v{db_compat_version} (本地 db-v{DB_COMPAT_VERSION})"
                ),
                format!(
                    "Remote database snapshot version is incompatible: db-v{db_compat_version} (local db-v{DB_COMPAT_VERSION})"
                ),
            ));
        }
        RemoteLayout::Legacy if db_compat_version > DB_COMPAT_VERSION => {
            return Err(localized(
                "sync.manifest_db_version_incompatible",
                format!(
                    "远端数据库快照版本不兼容: db-v{db_compat_version} (本地最高支持 db-v{DB_COMPAT_VERSION})"
                ),
                format!(
                    "Remote database snapshot version is incompatible: db-v{db_compat_version} (local supports up to db-v{DB_COMPAT_VERSION})"
                ),
            ));
        }
        _ => {}
    }
    Ok(())
}

// ─── Manifest authentication ─────────────────────────────────

/// Symmetric key that signs and verifies manifests, derived from the
/// passphrase the user enters on each device. Stored in the SecretStore as hex.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct SyncKey([u8; 32]);

impl std::fmt::Debug for SyncKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SyncKey({})", self.key_id())
    }
}

impl SyncKey {
    pub(crate) fn from_passphrase(passphrase: &str) -> Result<Self, AppError> {
        if passphrase.trim().chars().count() < MIN_SYNC_PASSPHRASE_CHARS {
            return Err(localized(
                "sync.key_too_short",
                format!("同步密钥至少需要 {MIN_SYNC_PASSPHRASE_CHARS} 个字符"),
                format!("The sync key must be at least {MIN_SYNC_PASSPHRASE_CHARS} characters."),
            ));
        }
        let mut key = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<Sha256>(
            passphrase.as_bytes(),
            SYNC_KEY_SALT,
            SYNC_KEY_PBKDF2_ROUNDS,
            &mut key,
        );
        Ok(Self(key))
    }

    pub(crate) fn from_hex(value: &str) -> Option<Self> {
        let bytes = decode_hex(value.trim())?;
        Some(Self(bytes.try_into().ok()?))
    }

    pub(crate) fn to_hex(&self) -> String {
        encode_hex(&self.0)
    }

    /// Short public fingerprint (not secret) shown in the UI and the manifest.
    pub(crate) fn key_id(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"cc-switch-sync-key-id\0");
        hasher.update(self.0);
        encode_hex(&hasher.finalize()[..8])
    }

    fn mac(&self) -> Hmac<Sha256> {
        <Hmac<Sha256> as Mac>::new_from_slice(&self.0).expect("HMAC accepts any key length")
    }
}

/// Bytes covered by the signature: a domain tag plus the manifest without its
/// `signature` field. Remote manifests are re-serialized from the parsed struct,
/// so unknown or reordered JSON fields cannot change what is verified.
pub(crate) fn manifest_signing_bytes(manifest: &SyncManifest) -> Result<Vec<u8>, AppError> {
    let mut unsigned = manifest.clone();
    unsigned.signature = None;
    let body = serde_json::to_vec(&unsigned).map_err(|e| AppError::JsonSerialize { source: e })?;
    let mut bytes = Vec::with_capacity(SIGNATURE_DOMAIN.len() + body.len());
    bytes.extend_from_slice(SIGNATURE_DOMAIN);
    bytes.extend_from_slice(&body);
    Ok(bytes)
}

pub(crate) fn sign_manifest(manifest: &mut SyncManifest, key: &SyncKey) -> Result<(), AppError> {
    let mut mac = key.mac();
    mac.update(&manifest_signing_bytes(manifest)?);
    manifest.signature = Some(ManifestSignature {
        alg: SIGNATURE_ALG.to_string(),
        key_id: key.key_id(),
        mac: encode_hex(&mac.finalize().into_bytes()),
    });
    Ok(())
}

/// Verify the manifest signature with `key`. An unsigned manifest is an error here;
/// callers decide about unsigned remotes with [`authorize_manifest`].
pub(crate) fn verify_manifest_signature(
    manifest: &SyncManifest,
    key: &SyncKey,
) -> Result<(), AppError> {
    let Some(signature) = manifest.signature.as_ref() else {
        return Err(unsigned_manifest_error());
    };
    if signature.alg != SIGNATURE_ALG {
        return Err(localized(
            "sync.manifest_signature_unsupported",
            format!("远端 manifest 签名算法不受支持: {}", signature.alg),
            format!(
                "Remote manifest signature algorithm is not supported: {}",
                signature.alg
            ),
        ));
    }
    let local_key_id = key.key_id();
    if signature.key_id != local_key_id {
        return Err(localized(
            "sync.manifest_key_mismatch",
            format!(
                "远端快照由其他同步密钥签名 (远端: {}, 本地: {local_key_id})，请在本设备输入相同的同步密钥",
                signature.key_id
            ),
            format!(
                "Remote snapshot was signed with a different sync key (remote: {}, local: {local_key_id}). Enter the same sync key on this device.",
                signature.key_id
            ),
        ));
    }
    let invalid = || {
        localized(
            "sync.manifest_signature_invalid",
            "远端 manifest 签名校验失败，快照可能被篡改，已拒绝导入",
            "Remote manifest signature is invalid; the snapshot may have been tampered with and was not imported.",
        )
    };
    let expected = decode_hex(&signature.mac).ok_or_else(invalid)?;
    let mut mac = key.mac();
    mac.update(&manifest_signing_bytes(manifest)?);
    mac.verify_slice(&expected).map_err(|_| invalid())
}

/// How a remote manifest was trusted by [`authorize_manifest`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ManifestTrust {
    /// Signature verified with the local sync key.
    Signed,
    /// Unsigned manifest accepted: no sync key configured yet (legacy), or the
    /// user confirmed the one-time migration import.
    Unsigned,
}

/// Device-side inputs for [`authorize_manifest`].
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SignaturePolicy {
    /// This device already synced a signed snapshot for this remote; unsigned
    /// manifests are a downgrade from then on and are always refused.
    pub signed_sync: bool,
    /// The user confirmed the one-time import of an unsigned (pre-signing) remote.
    pub allow_unsigned: bool,
}

/// Decide whether a remote manifest may be imported.
///
/// - signed: verified with the local key; missing key, other key or a bad MAC is refused;
/// - unsigned, after a signed sync: refused (no override);
/// - unsigned, sync key configured: refused with `sync.manifest_unsigned` unless the
///   user confirmed the one-time migration (`allow_unsigned`);
/// - unsigned, no sync key and never signed: accepted as before signing existed.
pub(crate) fn authorize_manifest(
    manifest: &SyncManifest,
    key: Option<&SyncKey>,
    policy: SignaturePolicy,
) -> Result<ManifestTrust, AppError> {
    if manifest.signature.is_some() {
        let key = key.ok_or_else(sync_key_missing_error)?;
        verify_manifest_signature(manifest, key)?;
        return Ok(ManifestTrust::Signed);
    }
    match key {
        None if policy.signed_sync => Err(sync_key_missing_error()),
        None => {
            log::warn!("[Sync] Importing an unsigned snapshot: no sync key is configured");
            Ok(ManifestTrust::Unsigned)
        }
        Some(_) if policy.signed_sync => Err(localized(
            "sync.manifest_unsigned_rejected",
            "远端快照未签名，而本设备此前已同步过签名快照，可能遭到降级攻击，已拒绝导入",
            "Remote snapshot is unsigned but this device already synced signed snapshots; it may be a downgrade attack and was not imported.",
        )),
        Some(_) if policy.allow_unsigned => {
            log::warn!("[Sync] Importing an unsigned snapshot after one-time migration confirmation");
            Ok(ManifestTrust::Unsigned)
        }
        Some(_) => Err(unsigned_manifest_error()),
    }
}

/// Key to sign an upload with. Without a key the upload stays unsigned, except
/// after a signed sync: then an unsigned upload would overwrite a signed remote
/// that other devices refuse, so it is rejected.
pub(crate) fn upload_signing_key(
    key: Option<SyncKey>,
    signed_sync: bool,
) -> Result<Option<SyncKey>, AppError> {
    match key {
        None if signed_sync => Err(sync_key_missing_error()),
        key => Ok(key),
    }
}

/// Short status of a remote manifest's signature, for remote-info payloads.
pub(crate) fn manifest_signature_status(
    manifest: &SyncManifest,
    key: Option<&SyncKey>,
) -> &'static str {
    match (manifest.signature.as_ref(), key) {
        (None, _) => "unsigned",
        (Some(_), None) => "keyMissing",
        (Some(_), Some(key)) => match verify_manifest_signature(manifest, key) {
            Ok(()) => "valid",
            Err(_) => "invalid",
        },
    }
}

fn unsigned_manifest_error() -> AppError {
    localized(
        "sync.manifest_unsigned",
        "远端快照未签名（由旧版本上传）。确认来源可信后可一次性导入，之后上传将带签名",
        "Remote snapshot is unsigned (uploaded by an older version). Confirm it is trusted to import it once; later uploads will be signed.",
    )
}

pub(crate) fn sync_key_missing_error() -> AppError {
    localized(
        "sync.key_missing",
        "未配置同步密钥，无法校验或签名快照；请在每台设备上输入相同的同步密钥",
        "No sync key is configured, so snapshots cannot be verified or signed. Enter the same sync key on each device.",
    )
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if !value.len().is_multiple_of(2) || !value.is_ascii() {
        return None;
    }
    (0..value.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&value[i..i + 2], 16).ok())
        .collect()
}

// ─── Artifact verification ───────────────────────────────────

pub(crate) fn validate_artifact_size_limit(artifact_name: &str, size: u64) -> Result<(), AppError> {
    if size > MAX_SYNC_ARTIFACT_BYTES {
        let max_mb = MAX_SYNC_ARTIFACT_BYTES / 1024 / 1024;
        return Err(localized(
            "sync.artifact_too_large",
            format!("artifact {artifact_name} 超过下载上限（{} MB）", max_mb),
            format!(
                "Artifact {artifact_name} exceeds download limit ({} MB)",
                max_mb
            ),
        ));
    }
    Ok(())
}

/// Verify that downloaded artifact bytes match the expected size and SHA-256 hash.
pub(crate) fn verify_artifact(
    bytes: &[u8],
    artifact_name: &str,
    meta: &ArtifactMeta,
) -> Result<(), AppError> {
    // Quick size check before expensive hash
    if bytes.len() as u64 != meta.size {
        return Err(localized(
            "sync.artifact_size_mismatch",
            format!(
                "artifact {artifact_name} 大小不匹配 (expected: {}, got: {})",
                meta.size,
                bytes.len(),
            ),
            format!(
                "Artifact {artifact_name} size mismatch (expected: {}, got: {})",
                meta.size,
                bytes.len(),
            ),
        ));
    }

    let actual_hash = sha256_hex(bytes);
    if actual_hash != meta.sha256 {
        return Err(localized(
            "sync.artifact_hash_mismatch",
            format!(
                "artifact {artifact_name} SHA256 校验失败 (expected: {}..., got: {}...)",
                meta.sha256.get(..8).unwrap_or(&meta.sha256),
                actual_hash.get(..8).unwrap_or(&actual_hash),
            ),
            format!(
                "Artifact {artifact_name} SHA256 verification failed (expected: {}..., got: {}...)",
                meta.sha256.get(..8).unwrap_or(&meta.sha256),
                actual_hash.get(..8).unwrap_or(&actual_hash),
            ),
        ));
    }
    Ok(())
}

// ─── Snapshot application ────────────────────────────────────

pub(crate) fn apply_snapshot(
    db: &crate::database::Database,
    db_sql: &[u8],
    skills_zip: &[u8],
) -> Result<(), AppError> {
    let sql_str = std::str::from_utf8(db_sql).map_err(|e| {
        localized(
            "sync.sql_not_utf8",
            format!("SQL 非 UTF-8: {e}"),
            format!("SQL is not valid UTF-8: {e}"),
        )
    })?;
    // A snapshot from a newer schema is rejected before skills or the DB are
    // touched, so it is never partially applied (see docs/dev/DB_COMPATIBILITY.md).
    crate::database::Database::ensure_sql_dump_version_supported(sql_str)?;
    let skills_backup = backup_current_skills()?;

    // Replace skills first, then import database; roll back skills on DB failure.
    restore_skills_zip(skills_zip)?;

    if let Err(db_err) = db.import_sql_string_for_sync(sql_str) {
        if let Err(rollback_err) = restore_skills_from_backup(&skills_backup) {
            return Err(localized(
                "sync.db_import_and_rollback_failed",
                format!("导入数据库失败: {db_err}; 同时回滚 Skills 失败: {rollback_err}"),
                format!(
                    "Database import failed: {db_err}; skills rollback also failed: {rollback_err}"
                ),
            ));
        }
        return Err(db_err);
    }

    Ok(())
}

// ─── Utilities ───────────────────────────────────────────────

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

pub(crate) fn detect_system_device_name() -> Option<String> {
    let env_name = ["CC_SWITCH_DEVICE_NAME", "COMPUTERNAME", "HOSTNAME"]
        .iter()
        .filter_map(|key| std::env::var(key).ok())
        .find_map(|value| normalize_device_name(&value));

    if env_name.is_some() {
        return env_name;
    }

    let output = Command::new("hostname").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let hostname = String::from_utf8(output.stdout).ok()?;
    normalize_device_name(&hostname)
}

pub(crate) fn normalize_device_name(raw: &str) -> Option<String> {
    let compact = raw
        .chars()
        .fold(String::with_capacity(raw.len()), |mut acc, ch| {
            if ch.is_whitespace() {
                acc.push(' ');
            } else if !ch.is_control() {
                acc.push(ch);
            }
            acc
        });
    let normalized = compact.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return None;
    }

    let limited = trimmed
        .chars()
        .take(MAX_DEVICE_NAME_LEN)
        .collect::<String>();
    if limited.is_empty() {
        None
    } else {
        Some(limited)
    }
}

// ─── Sync status persistence ─────────────────────────────────

pub(crate) fn persist_sync_success_best_effort<S, F>(
    settings: &mut S,
    manifest_hash: String,
    etag: Option<String>,
    persist_fn: F,
) -> bool
where
    F: FnOnce(&mut S, String, Option<String>) -> Result<(), AppError>,
{
    match persist_fn(settings, manifest_hash, etag) {
        Ok(()) => true,
        Err(err) => {
            log::warn!("[Sync] Persist sync status failed, keep operation success: {err}");
            false
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(sha256: &str, size: u64) -> ArtifactMeta {
        ArtifactMeta {
            sha256: sha256.to_string(),
            size,
        }
    }

    #[test]
    fn snapshot_id_is_stable() {
        let mut artifacts = BTreeMap::new();
        artifacts.insert("db.sql".to_string(), artifact("abc123", 100));
        artifacts.insert("skills.zip".to_string(), artifact("def456", 200));

        let id1 = compute_snapshot_id(&artifacts);
        let id2 = compute_snapshot_id(&artifacts);
        assert_eq!(id1, id2);
    }

    #[test]
    fn snapshot_id_changes_with_artifacts() {
        let mut a1 = BTreeMap::new();
        a1.insert("db.sql".to_string(), artifact("hash-a", 1));

        let mut a2 = BTreeMap::new();
        a2.insert("db.sql".to_string(), artifact("hash-b", 1));

        assert_ne!(compute_snapshot_id(&a1), compute_snapshot_id(&a2));
    }

    #[test]
    fn sha256_hex_is_correct() {
        let hash = sha256_hex(b"hello");
        assert_eq!(
            hash,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn persist_best_effort_returns_true_on_success() {
        let mut dummy = ();
        let ok = persist_sync_success_best_effort(
            &mut dummy,
            "hash".to_string(),
            Some("etag".to_string()),
            |_settings, _hash, _etag| Ok(()),
        );
        assert!(ok);
    }

    #[test]
    fn persist_best_effort_returns_false_on_error() {
        let mut dummy = ();
        let ok = persist_sync_success_best_effort(
            &mut dummy,
            "hash".to_string(),
            None,
            |_settings, _hash, _etag| Err(AppError::Config("boom".to_string())),
        );
        assert!(!ok);
    }

    fn manifest_with(format: &str, version: u32, db_compat_version: Option<u32>) -> SyncManifest {
        let mut artifacts = BTreeMap::new();
        artifacts.insert("db.sql".to_string(), artifact("abc", 1));
        artifacts.insert("skills.zip".to_string(), artifact("def", 2));
        SyncManifest {
            format: format.to_string(),
            version,
            db_compat_version,
            device_name: "My MacBook".to_string(),
            created_at: "2026-02-12T00:00:00Z".to_string(),
            artifacts,
            snapshot_id: "snap-1".to_string(),
            signature: None,
        }
    }

    #[test]
    fn validate_manifest_compat_accepts_supported_manifest() {
        let manifest = manifest_with(PROTOCOL_FORMAT, PROTOCOL_VERSION, Some(DB_COMPAT_VERSION));
        assert!(validate_manifest_compat(&manifest, RemoteLayout::Current).is_ok());
    }

    #[test]
    fn validate_manifest_compat_rejects_wrong_format() {
        let manifest = manifest_with("other-format", PROTOCOL_VERSION, Some(DB_COMPAT_VERSION));
        assert!(validate_manifest_compat(&manifest, RemoteLayout::Current).is_err());
    }

    #[test]
    fn validate_manifest_compat_rejects_wrong_version() {
        let manifest = manifest_with(
            PROTOCOL_FORMAT,
            PROTOCOL_VERSION + 1,
            Some(DB_COMPAT_VERSION),
        );
        assert!(validate_manifest_compat(&manifest, RemoteLayout::Current).is_err());
    }

    #[test]
    fn validate_manifest_compat_accepts_legacy_manifest_without_db_compat() {
        let manifest = manifest_with(PROTOCOL_FORMAT, PROTOCOL_VERSION, None);
        assert!(validate_manifest_compat(&manifest, RemoteLayout::Legacy).is_ok());
    }

    #[test]
    fn validate_manifest_compat_rejects_current_manifest_with_wrong_db_compat() {
        let manifest = manifest_with(
            PROTOCOL_FORMAT,
            PROTOCOL_VERSION,
            Some(LEGACY_DB_COMPAT_VERSION),
        );
        assert!(validate_manifest_compat(&manifest, RemoteLayout::Current).is_err());
    }

    #[test]
    fn validate_manifest_compat_rejects_legacy_manifest_from_newer_db_generation() {
        let manifest = manifest_with(
            PROTOCOL_FORMAT,
            PROTOCOL_VERSION,
            Some(DB_COMPAT_VERSION + 1),
        );
        assert!(validate_manifest_compat(&manifest, RemoteLayout::Legacy).is_err());
    }

    fn assert_localized(result: Result<(), AppError>, expected_key: &str) {
        match result {
            Err(AppError::Localized { key, zh, en }) => {
                assert_eq!(key, expected_key);
                assert!(!zh.is_empty() && !en.is_empty());
            }
            other => panic!("expected localized {expected_key}, got {other:?}"),
        }
    }

    #[test]
    fn validate_manifest_compat_rejects_next_db_compat_version_with_localized_error() {
        let manifest = manifest_with(
            PROTOCOL_FORMAT,
            PROTOCOL_VERSION,
            Some(DB_COMPAT_VERSION + 1),
        );
        for layout in [RemoteLayout::Current, RemoteLayout::Legacy] {
            assert_localized(
                validate_manifest_compat(&manifest, layout),
                "sync.manifest_db_version_incompatible",
            );
        }
    }

    #[test]
    fn apply_snapshot_rejects_newer_schema_dump_without_partial_import() {
        use crate::database::{Database, SCHEMA_VERSION};

        let home = tempfile::tempdir().expect("tempdir");
        let _paths = crate::paths::scoped(crate::paths::Paths::with_home(home.path()));
        let _settings = crate::settings::scoped_settings_store();

        let insert_provider = |db: &Database, id: &str| {
            let conn = db.conn.lock().expect("lock conn");
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta)
                 VALUES (?1, 'claude', 'Provider', '{}', '{}')",
                [id],
            )
            .expect("insert provider");
        };
        let local = Database::memory().expect("local db");
        insert_provider(&local, "local-provider");
        let remote = Database::memory().expect("remote db");
        insert_provider(&remote, "remote-provider");

        // A dump from a device one schema generation ahead.
        let newer_sql = remote
            .export_sql_string_for_sync()
            .expect("export")
            .lines()
            .map(|line| {
                if line.starts_with("PRAGMA user_version=") {
                    format!("PRAGMA user_version={};", SCHEMA_VERSION + 1)
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let skills_dir = crate::config::get_app_config_dir().join("skills");
        std::fs::create_dir_all(&skills_dir).expect("create skills dir");
        std::fs::write(skills_dir.join("marker.txt"), "local").expect("write skill marker");

        assert_localized(
            apply_snapshot(&local, newer_sql.as_bytes(), b"not a zip"),
            "backup.sql.newer_schema",
        );

        let ids: Vec<String> = {
            let conn = local.conn.lock().expect("lock conn");
            let mut stmt = conn
                .prepare("SELECT id FROM providers ORDER BY id")
                .expect("prepare");
            let rows = stmt
                .query_map([], |row| row.get(0))
                .expect("query")
                .collect::<Result<_, _>>()
                .expect("collect");
            rows
        };
        assert_eq!(ids, vec!["local-provider".to_string()]);
        assert_eq!(
            std::fs::read_to_string(skills_dir.join("marker.txt")).expect("skill marker"),
            "local",
            "skills must not be replaced by a rejected snapshot"
        );
    }

    #[test]
    fn effective_db_compat_version_defaults_legacy_layout_to_v5() {
        let manifest = manifest_with(PROTOCOL_FORMAT, PROTOCOL_VERSION, None);
        assert_eq!(
            effective_db_compat_version(&manifest, RemoteLayout::Legacy),
            Some(LEGACY_DB_COMPAT_VERSION)
        );
        assert_eq!(
            effective_db_compat_version(&manifest, RemoteLayout::Current),
            None
        );
    }

    #[test]
    fn normalize_device_name_returns_none_for_blank_input() {
        assert_eq!(normalize_device_name("   \n\t  "), None);
    }

    #[test]
    fn normalize_device_name_collapses_whitespace_and_drops_control_chars() {
        assert_eq!(
            normalize_device_name("  Mac\tBook \n Pro\u{0007} "),
            Some("Mac Book Pro".to_string())
        );
    }

    #[test]
    fn normalize_device_name_truncates_to_max_len() {
        let long = "a".repeat(80);
        assert_eq!(normalize_device_name(&long).map(|s| s.len()), Some(64));
    }

    #[test]
    fn manifest_serialization_uses_device_name_only() {
        let manifest = manifest_with(PROTOCOL_FORMAT, PROTOCOL_VERSION, Some(DB_COMPAT_VERSION));
        let value = serde_json::to_value(&manifest).expect("serialize manifest");
        assert!(
            value.get("deviceName").is_some(),
            "manifest should contain deviceName"
        );
        assert_eq!(
            value.get("dbCompatVersion").and_then(|v| v.as_u64()),
            Some(DB_COMPAT_VERSION as u64)
        );
        assert!(
            value.get("deviceId").is_none(),
            "manifest should not contain deviceId"
        );
    }

    #[test]
    fn validate_artifact_size_limit_rejects_oversized_artifacts() {
        let err = validate_artifact_size_limit("skills.zip", MAX_SYNC_ARTIFACT_BYTES + 1)
            .expect_err("artifact larger than limit should be rejected");
        assert!(
            err.to_string().contains("too large") || err.to_string().contains("超过"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_artifact_size_limit_accepts_limit_boundary() {
        assert!(validate_artifact_size_limit("skills.zip", MAX_SYNC_ARTIFACT_BYTES).is_ok());
    }

    #[test]
    fn verify_artifact_rejects_size_mismatch() {
        let meta = artifact("abc123", 100);
        let bytes = vec![0u8; 50];
        let err = verify_artifact(&bytes, "test.bin", &meta)
            .expect_err("size mismatch should be rejected");
        assert!(
            err.to_string().contains("mismatch") || err.to_string().contains("不匹配"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn verify_artifact_rejects_hash_mismatch() {
        let meta = ArtifactMeta {
            sha256: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            size: 5,
        };
        let bytes = b"hello";
        let err = verify_artifact(bytes, "test.bin", &meta)
            .expect_err("hash mismatch should be rejected");
        assert!(
            err.to_string().contains("verification failed") || err.to_string().contains("校验失败"),
            "unexpected error: {err}"
        );
    }

    // ─── Manifest authentication ─────────────────────────────

    fn test_key(byte: u8) -> SyncKey {
        SyncKey([byte; 32])
    }

    fn signed_manifest_bytes(key: &SyncKey) -> Vec<u8> {
        let mut manifest =
            manifest_with(PROTOCOL_FORMAT, PROTOCOL_VERSION, Some(DB_COMPAT_VERSION));
        sign_manifest(&mut manifest, key).expect("sign");
        serde_json::to_vec_pretty(&manifest).expect("serialize")
    }

    fn parse(bytes: &[u8]) -> SyncManifest {
        serde_json::from_slice(bytes).expect("parse manifest")
    }

    fn authorize_err_key(
        manifest: &SyncManifest,
        key: Option<&SyncKey>,
        policy: SignaturePolicy,
    ) -> &'static str {
        match authorize_manifest(manifest, key, policy) {
            Err(AppError::Localized { key, .. }) => key,
            other => panic!("expected localized rejection, got {other:?}"),
        }
    }

    #[test]
    fn signed_manifest_is_accepted_after_a_json_round_trip() {
        let key = test_key(7);
        let manifest = parse(&signed_manifest_bytes(&key));
        let signature = manifest.signature.as_ref().expect("signature");
        assert_eq!(signature.alg, SIGNATURE_ALG);
        assert_eq!(signature.key_id, key.key_id());
        assert_eq!(signature.mac.len(), 64);
        assert_eq!(
            authorize_manifest(&manifest, Some(&key), SignaturePolicy::default()).unwrap(),
            ManifestTrust::Signed
        );
        // Even after a signed sync and without the migration override.
        let strict = SignaturePolicy {
            signed_sync: true,
            allow_unsigned: false,
        };
        assert_eq!(
            authorize_manifest(&manifest, Some(&key), strict).unwrap(),
            ManifestTrust::Signed
        );
        assert_eq!(manifest_signature_status(&manifest, Some(&key)), "valid");
    }

    #[test]
    fn tampered_manifest_is_rejected() {
        let key = test_key(7);
        let original: serde_json::Value =
            serde_json::from_slice(&signed_manifest_bytes(&key)).unwrap();
        type Tamper = Box<dyn Fn(&mut serde_json::Value)>;
        let tamperings: Vec<(&str, Tamper)> = vec![
            (
                "artifact hash",
                Box::new(|m| {
                    m["artifacts"]["db.sql"]["sha256"] = "evil".into();
                }),
            ),
            (
                "artifact size",
                Box::new(|m| m["artifacts"]["skills.zip"]["size"] = 999.into()),
            ),
            (
                "added artifact",
                Box::new(|m| {
                    m["artifacts"]["extra.bin"] = serde_json::json!({"sha256": "x", "size": 1})
                }),
            ),
            (
                "device name",
                Box::new(|m| m["deviceName"] = "Attacker".into()),
            ),
            (
                "created at",
                Box::new(|m| m["createdAt"] = "2030-01-01T00:00:00Z".into()),
            ),
            (
                "snapshot id",
                Box::new(|m| m["snapshotId"] = "snap-2".into()),
            ),
            (
                "db compat version",
                Box::new(|m| m["dbCompatVersion"] = (DB_COMPAT_VERSION - 1).into()),
            ),
            (
                "mac",
                Box::new(|m| {
                    let mac = m["signature"]["mac"].as_str().unwrap().to_string();
                    let flipped = if mac.starts_with('0') { "1" } else { "0" };
                    m["signature"]["mac"] = format!("{flipped}{}", &mac[1..]).into();
                }),
            ),
            (
                "malformed mac",
                Box::new(|m| m["signature"]["mac"] = "zz".into()),
            ),
        ];
        for (what, tamper) in tamperings {
            let mut value = original.clone();
            tamper(&mut value);
            let manifest: SyncManifest = serde_json::from_value(value).expect(what);
            assert_eq!(
                authorize_err_key(&manifest, Some(&key), SignaturePolicy::default()),
                "sync.manifest_signature_invalid",
                "tampered {what} must be rejected"
            );
            assert_eq!(manifest_signature_status(&manifest, Some(&key)), "invalid");
        }
    }

    #[test]
    fn unknown_manifest_fields_do_not_affect_the_signature() {
        let key = test_key(7);
        let mut value: serde_json::Value =
            serde_json::from_slice(&signed_manifest_bytes(&key)).unwrap();
        value["futureField"] = "ignored".into();
        let manifest: SyncManifest = serde_json::from_value(value).unwrap();
        assert_eq!(
            authorize_manifest(&manifest, Some(&key), SignaturePolicy::default()).unwrap(),
            ManifestTrust::Signed
        );
    }

    #[test]
    fn manifest_signed_with_another_key_is_rejected() {
        let manifest = parse(&signed_manifest_bytes(&test_key(1)));
        assert_eq!(
            authorize_err_key(&manifest, Some(&test_key(2)), SignaturePolicy::default()),
            "sync.manifest_key_mismatch"
        );
    }

    #[test]
    fn forged_signature_claiming_the_local_key_id_is_rejected() {
        let local = test_key(1);
        let mut manifest = parse(&signed_manifest_bytes(&test_key(2)));
        manifest.signature.as_mut().unwrap().key_id = local.key_id();
        assert_eq!(
            authorize_err_key(&manifest, Some(&local), SignaturePolicy::default()),
            "sync.manifest_signature_invalid"
        );
    }

    #[test]
    fn unsupported_signature_algorithm_is_rejected() {
        let key = test_key(1);
        let mut manifest = parse(&signed_manifest_bytes(&key));
        manifest.signature.as_mut().unwrap().alg = "none".to_string();
        assert_eq!(
            authorize_err_key(&manifest, Some(&key), SignaturePolicy::default()),
            "sync.manifest_signature_unsupported"
        );
    }

    #[test]
    fn signed_manifest_without_local_key_is_rejected() {
        let manifest = parse(&signed_manifest_bytes(&test_key(1)));
        assert_eq!(
            authorize_err_key(&manifest, None, SignaturePolicy::default()),
            "sync.key_missing"
        );
        assert_eq!(manifest_signature_status(&manifest, None), "keyMissing");
    }

    #[test]
    fn unsigned_manifest_needs_one_time_migration_confirmation_when_key_is_set() {
        let key = test_key(1);
        let manifest = manifest_with(PROTOCOL_FORMAT, PROTOCOL_VERSION, Some(DB_COMPAT_VERSION));
        assert_eq!(manifest_signature_status(&manifest, Some(&key)), "unsigned");
        assert_eq!(
            authorize_err_key(&manifest, Some(&key), SignaturePolicy::default()),
            "sync.manifest_unsigned"
        );
        let confirmed = SignaturePolicy {
            signed_sync: false,
            allow_unsigned: true,
        };
        assert_eq!(
            authorize_manifest(&manifest, Some(&key), confirmed).unwrap(),
            ManifestTrust::Unsigned
        );
    }

    #[test]
    fn unsigned_manifest_is_refused_after_a_signed_sync_even_when_confirmed() {
        let key = test_key(1);
        // A signed manifest with its signature stripped (downgrade attempt).
        let mut manifest = parse(&signed_manifest_bytes(&key));
        manifest.signature = None;
        let policy = SignaturePolicy {
            signed_sync: true,
            allow_unsigned: true,
        };
        assert_eq!(
            authorize_err_key(&manifest, Some(&key), policy),
            "sync.manifest_unsigned_rejected"
        );
        assert_eq!(
            authorize_err_key(&manifest, None, policy),
            "sync.key_missing",
            "removing the key does not reopen unsigned imports"
        );
    }

    #[test]
    fn unsigned_manifest_without_key_keeps_legacy_behaviour() {
        let manifest = manifest_with(PROTOCOL_FORMAT, PROTOCOL_VERSION, None);
        assert_eq!(
            authorize_manifest(&manifest, None, SignaturePolicy::default()).unwrap(),
            ManifestTrust::Unsigned
        );
    }

    #[test]
    fn legacy_manifest_json_without_signature_still_parses() {
        let manifest: SyncManifest = serde_json::from_str(
            r#"{"format":"cc-switch-webdav-sync","version":2,"deviceName":"Old",
                "createdAt":"2025-01-01T00:00:00Z","artifacts":{},"snapshotId":"s"}"#,
        )
        .expect("legacy manifest");
        assert!(manifest.signature.is_none());
        let value = serde_json::to_value(&manifest).unwrap();
        assert!(value.get("signature").is_none());
    }

    #[test]
    fn upload_without_key_is_refused_only_after_a_signed_sync() {
        assert_eq!(upload_signing_key(None, false).unwrap(), None);
        assert_eq!(
            upload_signing_key(Some(test_key(3)), true).unwrap(),
            Some(test_key(3))
        );
        match upload_signing_key(None, true) {
            Err(AppError::Localized { key, .. }) => assert_eq!(key, "sync.key_missing"),
            other => panic!("expected key_missing, got {other:?}"),
        }
    }

    #[test]
    fn sync_key_derivation_is_deterministic_and_validated() {
        let a = SyncKey::from_passphrase("correct horse battery").unwrap();
        let b = SyncKey::from_passphrase("correct horse battery").unwrap();
        let c = SyncKey::from_passphrase("correct horse battery!").unwrap();
        assert_eq!(a, b, "every device derives the same key");
        assert_ne!(a, c);
        assert_eq!(SyncKey::from_hex(&a.to_hex()), Some(a.clone()));
        assert_eq!(a.key_id().len(), 16);
        assert!(
            !format!("{a:?}").contains(&a.to_hex()),
            "Debug hides the key"
        );
        assert!(SyncKey::from_hex("abcd").is_none());
        assert!(SyncKey::from_hex("zz").is_none());
        match SyncKey::from_passphrase("  short ") {
            Err(AppError::Localized { key, .. }) => assert_eq!(key, "sync.key_too_short"),
            other => panic!("expected key_too_short, got {other:?}"),
        }
    }

    #[test]
    fn build_local_snapshot_signs_manifest_when_key_is_configured() {
        use crate::database::Database;

        let home = tempfile::tempdir().expect("tempdir");
        let _paths = crate::paths::scoped(crate::paths::Paths::with_home(home.path()));
        let _settings = crate::settings::scoped_settings_store();
        let db = Database::memory().expect("db");
        let key = test_key(9);

        let signed = build_local_snapshot(&db, Some(&key)).expect("signed snapshot");
        assert_eq!(signed.manifest_hash, sha256_hex(&signed.manifest_bytes));
        let manifest = parse(&signed.manifest_bytes);
        assert_eq!(
            authorize_manifest(&manifest, Some(&key), SignaturePolicy::default()).unwrap(),
            ManifestTrust::Signed
        );
        verify_artifact(
            &signed.db_sql,
            REMOTE_DB_SQL,
            &manifest.artifacts[REMOTE_DB_SQL],
        )
        .expect("signed artifact hash");

        let unsigned = build_local_snapshot(&db, None).expect("unsigned snapshot");
        assert!(parse(&unsigned.manifest_bytes).signature.is_none());
    }

    #[test]
    fn verify_artifact_accepts_matching_data() {
        let data = b"hello";
        let meta = ArtifactMeta {
            sha256: sha256_hex(data),
            size: data.len() as u64,
        };
        assert!(verify_artifact(data, "test.bin", &meta).is_ok());
    }
}
