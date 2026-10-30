//! Transport-agnostic sync protocol layer.
//!
//! Shared by WebDAV, S3, and future transports. Artifact set: `db.sql` + `skills.zip`.

use std::collections::BTreeMap;
use std::fs;
use std::process::Command;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::tempdir;

use crate::error::AppError;
use crate::secrets::envelope::{self, DerivedKey, EncryptionHeader, EnvelopeError};
use crate::settings::WebDavSyncStatus;

// Re-export archive functions for use by transport layers.
pub(crate) use super::webdav_sync::archive::{
    backup_current_skills, restore_skills_from_backup, restore_skills_zip, zip_skills_ssot,
};

// ─── Protocol constants ──────────────────────────────────────

/// Wire-format identifier stored in remote manifests.
/// Retains historic "webdav" naming for backward compatibility with existing remotes.
pub(crate) const PROTOCOL_FORMAT: &str = "cc-switch-webdav-sync";
pub(crate) const PROTOCOL_VERSION: u32 = 2;
/// Derived from the schema migration registry (`database::schema::MIGRATIONS`): it only
/// changes when a migration flagged `breaks_sync` is added. Do not edit it by hand.
pub(crate) const DB_COMPAT_VERSION: u32 = crate::database::SYNC_DB_COMPAT_VERSION;
/// Manifest `version` of an end-to-end encrypted snapshot (SEC-06 / SVC-04).
///
/// Encrypted snapshots live in the same `v2/` remote directory, but their
/// manifest says `version: 3`, so older clients stop with "protocol version
/// is incompatible" instead of trying to import ciphertext. Plaintext
/// snapshots keep `version: 2` and stay readable by every client.
pub(crate) const ENCRYPTED_PROTOCOL_VERSION: u32 = 3;
pub(crate) const LEGACY_DB_COMPAT_VERSION: u32 = 5;
pub(crate) const REMOTE_DB_SQL: &str = "db.sql";
pub(crate) const REMOTE_SKILLS_ZIP: &str = "skills.zip";
pub(crate) const REMOTE_MANIFEST: &str = "manifest.json";
pub(crate) const MAX_DEVICE_NAME_LEN: usize = 64;
pub(crate) const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_SYNC_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;

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
    /// Present when the artifacts are encrypted; holds the public KDF salt and
    /// parameters. Artifact hashes and sizes then describe the ciphertext.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encryption: Option<EncryptionHeader>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ArtifactMeta {
    pub sha256: String,
    pub size: u64,
}

pub(crate) struct LocalSnapshot {
    /// `db.sql` as uploaded (ciphertext when encrypted).
    pub db_sql: Vec<u8>,
    /// `skills.zip` as uploaded (ciphertext when encrypted).
    pub skills_zip: Vec<u8>,
    pub manifest_bytes: Vec<u8>,
    pub manifest_hash: String,
    /// Fingerprint of the plaintext content (see [`local_content_fingerprint`]).
    pub local_fingerprint: String,
}

/// Key and public header used to encrypt one upload.
#[derive(Clone)]
pub(crate) struct SnapshotKey {
    pub header: EncryptionHeader,
    pub key: DerivedKey,
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

pub(crate) fn build_local_snapshot(
    db: &crate::database::Database,
) -> Result<LocalSnapshot, AppError> {
    build_snapshot(db, None)
}

/// Build the upload snapshot, encrypting the artifacts when `encryption` is set.
pub(crate) fn build_snapshot(
    db: &crate::database::Database,
    encryption: Option<&SnapshotKey>,
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
    let local_fingerprint = local_content_fingerprint(&db_sql, &skills_zip);

    let (db_sql, skills_zip) = match encryption {
        Some(enc) => (
            seal_artifact(enc, REMOTE_DB_SQL, &db_sql)?,
            seal_artifact(enc, REMOTE_SKILLS_ZIP, &skills_zip)?,
        ),
        None => (db_sql, skills_zip),
    };

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
    let manifest = SyncManifest {
        format: PROTOCOL_FORMAT.to_string(),
        version: if encryption.is_some() {
            ENCRYPTED_PROTOCOL_VERSION
        } else {
            PROTOCOL_VERSION
        },
        db_compat_version: Some(DB_COMPAT_VERSION),
        device_name: detect_system_device_name().unwrap_or_else(|| "Unknown Device".to_string()),
        created_at: Utc::now().to_rfc3339(),
        artifacts,
        snapshot_id,
        encryption: encryption.map(|enc| enc.header.clone()),
    };
    let manifest_bytes =
        serde_json::to_vec_pretty(&manifest).map_err(|e| AppError::JsonSerialize { source: e })?;
    let manifest_hash = sha256_hex(&manifest_bytes);

    Ok(LocalSnapshot {
        db_sql,
        skills_zip,
        manifest_bytes,
        manifest_hash,
        local_fingerprint,
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
    let expected_version = if manifest.encryption.is_some() {
        ENCRYPTED_PROTOCOL_VERSION
    } else {
        PROTOCOL_VERSION
    };
    if manifest.version != expected_version {
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
    if let Some(header) = &manifest.encryption {
        header.check_supported().map_err(encryption_error)?;
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

// ─── Conflict detection (SEC-28 / SVC-02) ────────────────────
//
// Sync is whole-snapshot, so every write must prove it is based on the remote
// state this device last saw. The baseline lives in `WebDavSyncStatus`:
//
// - `last_remote_manifest_hash` / `last_remote_etag`: the remote manifest seen
//   at the last successful upload/download. Before uploading, the current
//   remote manifest is compared with it; a mismatch is a "remote changed"
//   conflict. The manifest PUT additionally carries `If-Match: <etag>` of the
//   manifest that was just checked, closing the read-then-write race on
//   servers that support conditional writes.
// - `last_local_manifest_hash`: a content fingerprint of the local snapshot
//   (prefixed with [`LOCAL_FINGERPRINT_PREFIX`]) taken right after the last
//   successful sync. Before downloading, the current local fingerprint is
//   compared with it; a mismatch means local edits would be overwritten.
//
// No baseline (fresh install, or status written by an older version) means
// "unknown" and the operation is allowed, so existing users' first sync keeps
// working. `force = true` skips both checks for explicit user overwrites.

/// Error key of the distinct "remote changed since last sync" conflict.
pub(crate) const SYNC_REMOTE_CHANGED_KEY: &str = "sync.conflict.remote_changed";
/// Error key of the distinct "local data changed since last sync" conflict.
pub(crate) const SYNC_LOCAL_CHANGED_KEY: &str = "sync.conflict.local_changed";
/// Marks a `last_local_manifest_hash` value as a content fingerprint.
/// Values without this prefix were written by older versions (they held the
/// manifest hash) and are treated as "no baseline".
pub(crate) const LOCAL_FINGERPRINT_PREFIX: &str = "content-v1:";

/// The remote manifest as observed right before an upload.
pub(crate) struct RemoteManifestView<'a> {
    pub bytes: &'a [u8],
    pub etag: Option<&'a str>,
    /// Parsed manifest, used only to describe the conflict to the user.
    pub manifest: Option<&'a SyncManifest>,
}

/// Returns `true` when the error is a sync conflict (remote or local changed).
pub(crate) fn is_sync_conflict(err: &AppError) -> bool {
    matches!(
        err,
        AppError::Localized { key, .. }
            if *key == SYNC_REMOTE_CHANGED_KEY || *key == SYNC_LOCAL_CHANGED_KEY
    )
}

pub(crate) fn remote_changed_error(manifest: Option<&SyncManifest>) -> AppError {
    let (zh_detail, en_detail) = match manifest {
        Some(m) => (
            format!("（来自设备「{}」，时间 {}）", m.device_name, m.created_at),
            format!(" (from device \"{}\" at {})", m.device_name, m.created_at),
        ),
        None => (String::new(), String::new()),
    };
    localized(
        SYNC_REMOTE_CHANGED_KEY,
        format!(
            "同步冲突：远端数据自上次同步后已被其他设备更新{zh_detail}。为避免覆盖，已取消上传。请先下载远端数据，或在手动上传时确认覆盖。"
        ),
        format!(
            "Sync conflict: the remote data changed since the last sync{en_detail}. Upload was cancelled to avoid overwriting it. Download the remote data first, or confirm an overwrite with a manual upload."
        ),
    )
}

pub(crate) fn local_changed_error() -> AppError {
    localized(
        SYNC_LOCAL_CHANGED_KEY,
        "同步冲突：本地数据自上次同步后已修改，下载将覆盖这些修改。请先上传本地数据，或确认强制下载。",
        "Sync conflict: local data changed since the last sync and downloading would overwrite it. Upload first, or confirm a forced download.",
    )
}

/// Weak ETags (`W/"..."`) never match `If-Match` (strong comparison), so they
/// must not be used as a write precondition.
fn strong_etag(etag: Option<&str>) -> Option<String> {
    let etag = etag?.trim();
    if etag.is_empty() || etag.starts_with("W/") {
        return None;
    }
    Some(etag.to_string())
}

/// Decide whether an upload may overwrite the remote snapshot.
///
/// Returns the ETag to send as `If-Match` on the manifest PUT (if any), or the
/// distinct remote-changed conflict error.
pub(crate) fn evaluate_upload_precondition(
    status: &WebDavSyncStatus,
    remote: Option<&RemoteManifestView<'_>>,
    force: bool,
) -> Result<Option<String>, AppError> {
    // Nothing on the remote yet: nothing to protect.
    let Some(remote) = remote else {
        return Ok(None);
    };
    let if_match = strong_etag(remote.etag);
    if force {
        return Ok(if_match);
    }

    let stored_hash = status.last_remote_manifest_hash.as_deref();
    let stored_etag = status.last_remote_etag.as_deref();
    if stored_hash.is_none() && stored_etag.is_none() {
        // First sync from this device (or status from an older version).
        return Ok(if_match);
    }

    let hash_matches = stored_hash.is_some_and(|h| h == sha256_hex(remote.bytes));
    let etag_matches = matches!((stored_etag, remote.etag), (Some(a), Some(b)) if a == b);
    if hash_matches || etag_matches {
        Ok(if_match)
    } else {
        Err(remote_changed_error(remote.manifest))
    }
}

/// Fingerprint of local sync content, independent of export time.
///
/// The SQL dump starts with `--` comment lines that include the export
/// timestamp; they are skipped so unchanged data yields the same fingerprint.
pub(crate) fn local_content_fingerprint(db_sql: &[u8], skills_zip: &[u8]) -> String {
    let mut body = db_sql;
    while body.starts_with(b"--") {
        match body.iter().position(|b| *b == b'\n') {
            Some(pos) => body = &body[pos + 1..],
            None => {
                body = &[];
                break;
            }
        }
    }
    let mut hasher = Sha256::new();
    hasher.update(sha256_hex(body).as_bytes());
    hasher.update(b"|");
    hasher.update(sha256_hex(skills_zip).as_bytes());
    format!("{LOCAL_FINGERPRINT_PREFIX}{:x}", hasher.finalize())
}

/// Stored fingerprint of the local snapshot at the last sync, if any.
fn stored_local_fingerprint(status: &WebDavSyncStatus) -> Option<&str> {
    status
        .last_local_manifest_hash
        .as_deref()
        .filter(|v| v.starts_with(LOCAL_FINGERPRINT_PREFIX))
}

/// Decide whether a download may overwrite local data.
pub(crate) fn evaluate_download_precondition(
    status: &WebDavSyncStatus,
    current_fingerprint: Option<&str>,
    force: bool,
) -> Result<(), AppError> {
    if force {
        return Ok(());
    }
    match (stored_local_fingerprint(status), current_fingerprint) {
        (Some(stored), Some(current)) if stored != current => Err(local_changed_error()),
        _ => Ok(()),
    }
}

/// Refuse a download that would overwrite local changes made since the last
/// sync, unless `force`. Only builds a local snapshot when a baseline exists.
pub(crate) fn ensure_local_unchanged(
    db: &crate::database::Database,
    status: &WebDavSyncStatus,
    force: bool,
) -> Result<(), AppError> {
    if force || stored_local_fingerprint(status).is_none() {
        return Ok(());
    }
    let current = capture_local_fingerprint(db);
    evaluate_download_precondition(status, current.as_deref(), force)
}

/// Best-effort fingerprint of the current local state (e.g. after a download).
pub(crate) fn capture_local_fingerprint(db: &crate::database::Database) -> Option<String> {
    match build_local_snapshot(db) {
        Ok(snapshot) => Some(snapshot.local_fingerprint),
        Err(err) => {
            log::warn!("[Sync] Failed to fingerprint local snapshot: {err}");
            None
        }
    }
}

// ─── End-to-end encryption (SEC-06 / SVC-04) ─────────────────
//
// Opt-in: with `syncEncryption.enabled`, uploads seal `db.sql` and
// `skills.zip` with a key derived from the user's passphrase (see
// `crate::secrets::envelope`) and put the public header in the manifest.
// Downloads of encrypted snapshots need the passphrase; plaintext snapshots
// (older versions, or encryption off) still download unchanged.

pub(crate) const ENCRYPTION_PASSPHRASE_REQUIRED_KEY: &str = "sync.encryption.passphrase_required";
pub(crate) const ENCRYPTION_WRONG_PASSPHRASE_KEY: &str = "sync.encryption.wrong_passphrase";
pub(crate) const ENCRYPTION_PASSPHRASE_UNAVAILABLE_KEY: &str =
    "sync.encryption.passphrase_unavailable";
pub(crate) const ENCRYPTION_DOWNGRADE_KEY: &str = "sync.encryption.downgrade_refused";

fn encryption_error(err: EnvelopeError) -> AppError {
    match err {
        EnvelopeError::Decrypt => localized(
            ENCRYPTION_WRONG_PASSPHRASE_KEY,
            "同步加密密码错误，或远端同步数据已被修改。",
            "Wrong sync encryption passphrase, or the remote sync data was modified.",
        ),
        EnvelopeError::Unsupported(detail) => localized(
            "sync.encryption.unsupported",
            format!("远端同步数据使用了不支持的加密方式（{detail}），请升级 CC Switch。"),
            format!(
                "The remote sync data uses an unsupported encryption format ({detail}). Update CC Switch."
            ),
        ),
        EnvelopeError::NotEncrypted | EnvelopeError::Malformed(_) => localized(
            "sync.encryption.malformed",
            format!("远端加密同步数据格式无效: {err}"),
            format!("The encrypted remote sync data is invalid: {err}"),
        ),
    }
}

fn passphrase_required_error() -> AppError {
    localized(
        ENCRYPTION_PASSPHRASE_REQUIRED_KEY,
        "远端同步数据已加密。请先在同步设置中填写加密密码。",
        "The remote sync data is encrypted. Enter the sync encryption passphrase in the sync settings first.",
    )
}

/// The passphrase to encrypt uploads with, `Ok(None)` when encryption is off.
///
/// When encryption is enabled but the passphrase cannot be read (keychain
/// locked or unavailable), the upload fails instead of going out in plaintext.
pub(crate) fn upload_passphrase() -> Result<Option<String>, AppError> {
    match crate::settings::get_sync_encryption_settings() {
        Some(enc) if enc.enabled => {
            if enc.passphrase.is_empty() {
                Err(localized(
                    ENCRYPTION_PASSPHRASE_UNAVAILABLE_KEY,
                    "已启用同步加密，但无法读取加密密码（系统钥匙串不可用或已锁定）。为避免明文上传，已取消上传。",
                    "Sync encryption is enabled but the passphrase is unavailable (the system keychain is locked or missing). The upload was cancelled so nothing is sent unencrypted.",
                ))
            } else {
                Ok(Some(enc.passphrase))
            }
        }
        _ => Ok(None),
    }
}

/// The configured passphrase for decrypting downloads, if any.
pub(crate) fn download_passphrase() -> Option<String> {
    crate::settings::get_sync_encryption_settings()
        .map(|enc| enc.passphrase)
        .filter(|p| !p.is_empty())
}

struct CachedKey {
    passphrase_digest: [u8; 32],
    header: EncryptionHeader,
    key: DerivedKey,
}

/// Argon2id is deliberately slow; keep the last derived key so auto-sync does
/// not re-derive it on every upload. Keyed by the passphrase digest, so a
/// changed passphrase always derives a new key.
fn key_cache() -> &'static std::sync::Mutex<Option<CachedKey>> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<Option<CachedKey>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(None))
}

fn passphrase_digest(passphrase: &str) -> [u8; 32] {
    Sha256::digest(passphrase.as_bytes()).into()
}

/// Key for encrypting an upload with `passphrase`.
pub(crate) fn upload_key(passphrase: &str) -> Result<SnapshotKey, AppError> {
    let digest = passphrase_digest(passphrase);
    let mut cache = key_cache().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(cached) = cache.as_ref().filter(|c| c.passphrase_digest == digest) {
        return Ok(SnapshotKey {
            header: cached.header.clone(),
            key: cached.key.clone(),
        });
    }
    let (header, key) = envelope::new_header(passphrase, envelope::KdfParams::recommended())
        .map_err(encryption_error)?;
    *cache = Some(CachedKey {
        passphrase_digest: digest,
        header: header.clone(),
        key: key.clone(),
    });
    Ok(SnapshotKey { header, key })
}

/// Unlock an encrypted remote snapshot. `Ok(None)` for plaintext snapshots.
pub(crate) fn unlock_remote(
    manifest: &SyncManifest,
    passphrase: Option<&str>,
) -> Result<Option<DerivedKey>, AppError> {
    let Some(header) = &manifest.encryption else {
        return Ok(None);
    };
    let passphrase = passphrase.ok_or_else(passphrase_required_error)?;
    let digest = passphrase_digest(passphrase);
    let mut cache = key_cache().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(cached) = cache
        .as_ref()
        .filter(|c| c.passphrase_digest == digest && &c.header == header)
    {
        return Ok(Some(cached.key.clone()));
    }
    let key = header.unlock(passphrase).map_err(encryption_error)?;
    *cache = Some(CachedKey {
        passphrase_digest: digest,
        header: header.clone(),
        key: key.clone(),
    });
    Ok(Some(key))
}

fn seal_artifact(enc: &SnapshotKey, name: &str, plain: &[u8]) -> Result<Vec<u8>, AppError> {
    envelope::seal(&enc.key, name.as_bytes(), plain).map_err(encryption_error)
}

/// Decrypt a downloaded (and hash-verified) artifact. Plaintext artifacts of
/// unencrypted snapshots are returned unchanged.
pub(crate) fn open_artifact(
    key: Option<&DerivedKey>,
    name: &str,
    bytes: Vec<u8>,
) -> Result<Vec<u8>, AppError> {
    match key {
        Some(key) => envelope::open(key, name.as_bytes(), &bytes).map_err(encryption_error),
        None if envelope::is_sealed(&bytes) => Err(encryption_error(EnvelopeError::Malformed(
            "encrypted artifact without an encryption header".to_string(),
        ))),
        None => Ok(bytes),
    }
}

/// Refuse to replace an encrypted remote snapshot with a plaintext one unless
/// the user explicitly forces the overwrite (e.g. after turning encryption off).
pub(crate) fn ensure_no_encryption_downgrade(
    remote: Option<&SyncManifest>,
    encrypting: bool,
    force: bool,
) -> Result<(), AppError> {
    let remote_encrypted = remote.and_then(|m| m.encryption.as_ref()).is_some();
    if force || encrypting || !remote_encrypted {
        return Ok(());
    }
    Err(localized(
        ENCRYPTION_DOWNGRADE_KEY,
        "远端同步数据已加密，但本机未启用同步加密。为避免以明文覆盖，已取消上传。请启用同步加密，或手动上传并确认覆盖。",
        "The remote sync data is encrypted but sync encryption is off on this device. The upload was cancelled so the encrypted snapshot is not replaced with plaintext. Enable sync encryption, or confirm an overwrite with a manual upload.",
    ))
}

/// Encryption fields reported by `fetch_remote_info`.
pub(crate) struct RemoteEncryptionInfo {
    pub encrypted: bool,
    /// `None` when not encrypted or no passphrase is configured.
    pub passphrase_matches: Option<bool>,
}

pub(crate) fn remote_encryption_info(manifest: &SyncManifest) -> RemoteEncryptionInfo {
    let encrypted = manifest.encryption.is_some();
    let passphrase_matches = if encrypted {
        download_passphrase().map(|p| unlock_remote(manifest, Some(&p)).is_ok())
    } else {
        None
    };
    RemoteEncryptionInfo {
        encrypted,
        passphrase_matches,
    }
}

// ─── Plain-HTTP transport warning (SEC-17) ───────────────────

/// Returns `true` for `http://` URLs that leave the local network.
///
/// Credentials and the full config snapshot (including API keys) travel in
/// clear text over such endpoints. Loopback, private (RFC 1918 / ULA),
/// link-local, CGNAT/Tailscale ranges, single-label hosts (`nas`, `minio`)
/// and `.local`/`.lan`/`.home.arpa`/`.internal`/`localhost` names are treated
/// as local and not flagged. Sync is not refused (that would break existing
/// setups); callers log a warning and return an `insecureTransport` flag.
pub(crate) fn is_insecure_remote_url(raw: &str) -> bool {
    let trimmed = raw.trim();
    if !trimmed
        .get(..7)
        .is_some_and(|p| p.eq_ignore_ascii_case("http://"))
    {
        return false;
    }
    let Ok(url) = url::Url::parse(trimmed) else {
        return true;
    };
    match url.host() {
        Some(url::Host::Ipv4(ip)) => {
            let [a, b, ..] = ip.octets();
            let cgnat = a == 100 && (64..128).contains(&b);
            !(ip.is_loopback() || ip.is_private() || ip.is_link_local() || cgnat)
        }
        Some(url::Host::Ipv6(ip)) => {
            let first = ip.segments()[0];
            let unique_local = (first & 0xfe00) == 0xfc00;
            let link_local = (first & 0xffc0) == 0xfe80;
            !(ip.is_loopback() || unique_local || link_local)
        }
        Some(url::Host::Domain(domain)) => {
            let d = domain.trim_end_matches('.').to_ascii_lowercase();
            let local = !d.contains('.')
                || d == "localhost"
                || [".localhost", ".local", ".lan", ".home.arpa", ".internal"]
                    .iter()
                    .any(|suffix| d.ends_with(suffix));
            !local
        }
        None => true,
    }
}

/// Log a warning when syncing over plain HTTP to a non-local host.
/// Returns the `insecureTransport` flag reported back to the caller.
pub(crate) fn warn_if_insecure(transport: &str, raw_url: &str) -> bool {
    let insecure = is_insecure_remote_url(raw_url);
    if insecure {
        log::warn!(
            "[{transport}] Sync endpoint uses plain http:// to a non-local host; credentials and synced data (including API keys) are sent unencrypted. Use https://."
        );
    }
    insecure
}

// ─── Sync status persistence ─────────────────────────────────

/// What a successful sync records as the baseline for the next one.
#[derive(Debug, Clone)]
pub(crate) struct SyncBaseline {
    /// SHA-256 of the manifest bytes now on the remote.
    pub manifest_hash: String,
    /// ETag of the remote manifest, when the server returned one.
    pub etag: Option<String>,
    /// Fingerprint of local content right after the sync.
    pub local_fingerprint: Option<String>,
}

pub(crate) fn sync_status_from_baseline(baseline: SyncBaseline) -> WebDavSyncStatus {
    WebDavSyncStatus {
        last_sync_at: Some(Utc::now().timestamp()),
        last_error: None,
        last_error_source: None,
        last_local_manifest_hash: baseline.local_fingerprint,
        last_remote_manifest_hash: Some(baseline.manifest_hash),
        last_remote_etag: baseline.etag,
    }
}

pub(crate) fn persist_sync_success_best_effort<S, F>(
    settings: &mut S,
    baseline: SyncBaseline,
    persist_fn: F,
) -> bool
where
    F: FnOnce(&mut S, SyncBaseline) -> Result<(), AppError>,
{
    match persist_fn(settings, baseline) {
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
            baseline("hash", Some("etag")),
            |_settings, _baseline| Ok(()),
        );
        assert!(ok);
    }

    #[test]
    fn persist_best_effort_returns_false_on_error() {
        let mut dummy = ();
        let ok = persist_sync_success_best_effort(
            &mut dummy,
            baseline("hash", None),
            |_settings, _baseline| Err(AppError::Config("boom".to_string())),
        );
        assert!(!ok);
    }

    fn baseline(hash: &str, etag: Option<&str>) -> SyncBaseline {
        SyncBaseline {
            manifest_hash: hash.to_string(),
            etag: etag.map(str::to_string),
            local_fingerprint: None,
        }
    }

    // ── Upload precondition (SEC-28 / SVC-02) ──

    fn status_with(hash: Option<&str>, etag: Option<&str>) -> WebDavSyncStatus {
        WebDavSyncStatus {
            last_remote_manifest_hash: hash.map(str::to_string),
            last_remote_etag: etag.map(str::to_string),
            ..WebDavSyncStatus::default()
        }
    }

    fn view<'a>(bytes: &'a [u8], etag: Option<&'a str>) -> RemoteManifestView<'a> {
        RemoteManifestView {
            bytes,
            etag,
            manifest: None,
        }
    }

    #[test]
    fn upload_allowed_when_remote_is_empty() {
        let status = status_with(Some("stale"), Some("\"e1\""));
        assert_eq!(
            evaluate_upload_precondition(&status, None, false).unwrap(),
            None
        );
    }

    #[test]
    fn upload_allowed_on_first_sync_without_baseline() {
        let status = WebDavSyncStatus::default();
        let remote = view(b"remote manifest", Some("\"e1\""));
        assert_eq!(
            evaluate_upload_precondition(&status, Some(&remote), false).unwrap(),
            Some("\"e1\"".to_string())
        );
    }

    #[test]
    fn upload_allowed_when_remote_hash_matches_baseline() {
        let bytes = b"manifest A";
        let status = status_with(Some(&sha256_hex(bytes)), Some("\"old-etag\""));
        let remote = view(bytes, Some("\"new-etag\""));
        assert_eq!(
            evaluate_upload_precondition(&status, Some(&remote), false).unwrap(),
            Some("\"new-etag\"".to_string())
        );
    }

    #[test]
    fn upload_allowed_when_remote_etag_matches_baseline() {
        let status = status_with(Some("other-hash"), Some("\"e1\""));
        let remote = view(b"manifest A", Some("\"e1\""));
        assert!(evaluate_upload_precondition(&status, Some(&remote), false).is_ok());
    }

    #[test]
    fn upload_refused_with_conflict_when_remote_changed() {
        let status = status_with(Some(&sha256_hex(b"manifest A")), Some("\"e1\""));
        let mut manifest =
            manifest_with(PROTOCOL_FORMAT, PROTOCOL_VERSION, Some(DB_COMPAT_VERSION));
        manifest.device_name = "Laptop B".to_string();
        let remote = RemoteManifestView {
            bytes: b"manifest B",
            etag: Some("\"e2\""),
            manifest: Some(&manifest),
        };
        let err = evaluate_upload_precondition(&status, Some(&remote), false)
            .expect_err("changed remote must be a conflict");
        assert!(is_sync_conflict(&err));
        assert!(err.to_string().contains("Laptop B"), "{err}");
    }

    #[test]
    fn forced_upload_ignores_baseline_but_keeps_if_match() {
        let status = status_with(Some("stale"), Some("\"e1\""));
        let remote = view(b"manifest B", Some("\"e2\""));
        assert_eq!(
            evaluate_upload_precondition(&status, Some(&remote), true).unwrap(),
            Some("\"e2\"".to_string())
        );
    }

    #[test]
    fn weak_etag_is_not_used_for_if_match() {
        let status = WebDavSyncStatus::default();
        let remote = view(b"m", Some("W/\"weak\""));
        assert_eq!(
            evaluate_upload_precondition(&status, Some(&remote), false).unwrap(),
            None
        );
    }

    #[test]
    fn non_conflict_errors_are_not_reported_as_conflicts() {
        assert!(!is_sync_conflict(&AppError::Config("boom".to_string())));
        assert!(is_sync_conflict(&local_changed_error()));
        assert!(is_sync_conflict(&remote_changed_error(None)));
    }

    // ── Download precondition ──

    #[test]
    fn local_fingerprint_ignores_export_header_comments() {
        let a = local_content_fingerprint(
            b"-- CC Switch\n-- time: 1\nINSERT INTO t VALUES (1);\n",
            b"zip",
        );
        let b = local_content_fingerprint(
            b"-- CC Switch\n-- time: 2\nINSERT INTO t VALUES (1);\n",
            b"zip",
        );
        let c = local_content_fingerprint(
            b"-- CC Switch\n-- time: 2\nINSERT INTO t VALUES (2);\n",
            b"zip",
        );
        let d = local_content_fingerprint(
            b"-- CC Switch\n-- time: 2\nINSERT INTO t VALUES (1);\n",
            b"zip2",
        );
        assert!(a.starts_with(LOCAL_FINGERPRINT_PREFIX));
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }

    #[test]
    fn download_refused_when_local_changed_since_last_sync() {
        let stored = local_content_fingerprint(b"A", b"z");
        let current = local_content_fingerprint(b"B", b"z");
        let status = WebDavSyncStatus {
            last_local_manifest_hash: Some(stored.clone()),
            ..WebDavSyncStatus::default()
        };
        let err = evaluate_download_precondition(&status, Some(&current), false)
            .expect_err("dirty local must be refused");
        assert!(is_sync_conflict(&err));
        assert!(evaluate_download_precondition(&status, Some(&current), true).is_ok());
        assert!(evaluate_download_precondition(&status, Some(&stored), false).is_ok());
    }

    #[test]
    fn download_allowed_without_fingerprint_baseline() {
        // Older versions stored the manifest hash here (no prefix): unknown, allow.
        let status = WebDavSyncStatus {
            last_local_manifest_hash: Some(sha256_hex(b"legacy manifest")),
            ..WebDavSyncStatus::default()
        };
        let current = local_content_fingerprint(b"B", b"z");
        assert!(evaluate_download_precondition(&status, Some(&current), false).is_ok());
        assert!(evaluate_download_precondition(&WebDavSyncStatus::default(), None, false).is_ok());
    }

    // ── Plain-HTTP detection (SEC-17) ──

    #[test]
    fn insecure_url_flags_public_http_only() {
        for url in [
            "http://dav.example.com/dav",
            "http://8.8.8.8:5005/",
            "HTTP://storage.example.org",
            "http://[2001:db8::1]/",
        ] {
            assert!(is_insecure_remote_url(url), "{url} should be insecure");
        }
        for url in [
            "https://dav.example.com/dav",
            "minio.example.com:9000",
            "",
            "http://localhost:8080/dav",
            "http://127.0.0.1:9000",
            "http://[::1]:9000",
            "http://192.168.1.10/dav",
            "http://10.0.0.5",
            "http://172.20.1.1",
            "http://169.254.1.1",
            "http://100.101.102.103",
            "http://[fd00::1]/",
            "http://nas:5005/",
            "http://minio:9000",
            "http://nas.local/dav",
            "http://box.lan",
            "http://dav.home.arpa",
        ] {
            assert!(!is_insecure_remote_url(url), "{url} should not be flagged");
        }
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
            encryption: None,
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

    // ── Encryption (SEC-06 / SVC-04) ──

    fn encrypted_manifest(passphrase: &str) -> (SyncManifest, SnapshotKey) {
        let key = upload_key(passphrase).expect("key");
        let mut m = manifest_with(
            PROTOCOL_FORMAT,
            ENCRYPTED_PROTOCOL_VERSION,
            Some(DB_COMPAT_VERSION),
        );
        m.encryption = Some(key.header.clone());
        (m, key)
    }

    #[test]
    fn encrypted_manifest_versioning_is_enforced() {
        let (m, _key) = encrypted_manifest("versioning passphrase");
        assert!(validate_manifest_compat(&m, RemoteLayout::Current).is_ok());

        // version 3 needs an encryption header, and a header needs version 3.
        let plain_v3 = manifest_with(
            PROTOCOL_FORMAT,
            ENCRYPTED_PROTOCOL_VERSION,
            Some(DB_COMPAT_VERSION),
        );
        assert!(validate_manifest_compat(&plain_v3, RemoteLayout::Current).is_err());
        let mut header_v2 = m.clone();
        header_v2.version = PROTOCOL_VERSION;
        assert!(validate_manifest_compat(&header_v2, RemoteLayout::Current).is_err());

        // Unknown scheme: incompatible rather than a confusing decrypt error.
        let mut unknown = m.clone();
        unknown.encryption.as_mut().unwrap().scheme = "future-scheme".to_string();
        let err = validate_manifest_compat(&unknown, RemoteLayout::Current).unwrap_err();
        assert!(err.to_string().contains("unsupported"), "{err}");

        // Old manifests without the field still parse.
        let json = serde_json::to_value(manifest_with(
            PROTOCOL_FORMAT,
            PROTOCOL_VERSION,
            Some(DB_COMPAT_VERSION),
        ))
        .unwrap();
        assert!(json.get("encryption").is_none());
        let parsed: SyncManifest = serde_json::from_value(json).unwrap();
        assert!(parsed.encryption.is_none());
    }

    #[test]
    fn unlock_and_open_artifacts() {
        let (m, key) = encrypted_manifest("artifact passphrase");
        let sealed = seal_artifact(&key, REMOTE_DB_SQL, b"-- sql").unwrap();

        let unlocked = unlock_remote(&m, Some("artifact passphrase"))
            .unwrap()
            .expect("key for encrypted manifest");
        assert_eq!(
            open_artifact(Some(&unlocked), REMOTE_DB_SQL, sealed.clone()).unwrap(),
            b"-- sql"
        );
        // Bound to the artifact name.
        assert!(open_artifact(Some(&unlocked), REMOTE_SKILLS_ZIP, sealed.clone()).is_err());

        let err = unlock_remote(&m, None).err().unwrap();
        assert!(
            matches!(err, AppError::Localized { key, .. } if key == ENCRYPTION_PASSPHRASE_REQUIRED_KEY)
        );
        let err = unlock_remote(&m, Some("nope nope nope")).err().unwrap();
        assert!(
            matches!(err, AppError::Localized { key, .. } if key == ENCRYPTION_WRONG_PASSPHRASE_KEY)
        );

        // Plaintext manifests need no key and pass artifacts through...
        let plain = manifest_with(PROTOCOL_FORMAT, PROTOCOL_VERSION, Some(DB_COMPAT_VERSION));
        assert!(unlock_remote(&plain, None).unwrap().is_none());
        assert_eq!(
            open_artifact(None, REMOTE_DB_SQL, b"-- sql".to_vec()).unwrap(),
            b"-- sql"
        );
        // ...but sealed bytes without a header are rejected, never imported.
        assert!(open_artifact(None, REMOTE_DB_SQL, sealed).is_err());
    }

    #[test]
    fn plaintext_upload_over_encrypted_remote_needs_force() {
        let (encrypted, _key) = encrypted_manifest("downgrade passphrase");
        let plain = manifest_with(PROTOCOL_FORMAT, PROTOCOL_VERSION, Some(DB_COMPAT_VERSION));
        assert!(ensure_no_encryption_downgrade(Some(&encrypted), false, false).is_err());
        assert!(ensure_no_encryption_downgrade(Some(&encrypted), false, true).is_ok());
        assert!(ensure_no_encryption_downgrade(Some(&encrypted), true, false).is_ok());
        assert!(ensure_no_encryption_downgrade(Some(&plain), false, false).is_ok());
        assert!(ensure_no_encryption_downgrade(None, false, false).is_ok());
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
