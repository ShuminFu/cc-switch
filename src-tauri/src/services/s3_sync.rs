//! S3 v2 sync protocol layer.
//!
//! Implements manifest-based synchronization on top of the S3 transport
//! primitives in [`super::s3`]. Artifact set: `db.sql` + `skills.zip`.

use std::future::Future;
use std::sync::OnceLock;

use serde_json::Value;

use crate::error::AppError;
use crate::services::s3::{self, S3Credentials};
use crate::settings::{update_s3_sync_status, S3SyncSettings};

use super::sync_transport::{self, PutResult, SyncTransport, TransportKeys, UploadOutcome};

use super::sync_protocol::{
    evaluate_upload_precondition, is_insecure_remote_url, persist_sync_success_best_effort,
    remote_encryption_info, sync_status_from_baseline, validate_manifest_compat, warn_if_insecure,
    RemoteLayout, RemoteManifestView, SyncBaseline, SyncManifest, DB_COMPAT_VERSION,
    MAX_MANIFEST_BYTES, PROTOCOL_VERSION, REMOTE_MANIFEST,
};
#[cfg(test)]
use super::sync_protocol::{sha256_hex, REMOTE_DB_SQL, REMOTE_SKILLS_ZIP};

// ─── Sync lock ───────────────────────────────────────────────

pub fn sync_mutex() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

pub async fn run_with_sync_lock<T, Fut>(operation: Fut) -> Result<T, AppError>
where
    Fut: Future<Output = Result<T, AppError>>,
{
    let _guard = sync_mutex().lock().await;
    operation.await
}

// ─── Public API ──────────────────────────────────────────────

/// Check S3 connectivity by issuing a HEAD request against the bucket.
pub async fn check_connection(settings: &S3SyncSettings) -> Result<(), AppError> {
    validate_settings(settings)?;
    let creds = creds_for(settings);
    s3::test_connection(&creds).await
}

/// Upload local snapshot (db + skills) to remote S3.
///
/// Unless `force`, the upload is refused with a distinct "remote changed"
/// conflict error when the remote manifest differs from the one seen at the
/// last successful sync. On AWS the manifest PUT also carries `If-Match`.
pub async fn upload(
    db: &crate::database::Database,
    settings: &mut S3SyncSettings,
    force: bool,
) -> Result<Value, AppError> {
    upload_inner(db, settings, force, false).await
}

/// Auto-sync upload: like `upload(.., force = false)` but a no-op when the
/// local content is unchanged since the last successful sync (SVC-52), so a
/// stale change signal never republishes the same snapshot under this
/// device's name.
pub(crate) async fn upload_if_changed(
    db: &crate::database::Database,
    settings: &mut S3SyncSettings,
) -> Result<Value, AppError> {
    upload_inner(db, settings, false, true).await
}

async fn upload_inner(
    db: &crate::database::Database,
    settings: &mut S3SyncSettings,
    force: bool,
    skip_if_unchanged: bool,
) -> Result<Value, AppError> {
    validate_settings(settings)?;
    let insecure = warn_if_insecure("S3", &settings.endpoint);
    let transport = S3Transport::new(settings);
    let outcome =
        sync_transport::upload(&transport, db, &settings.status, force, skip_if_unchanged).await?;
    let baseline = match outcome {
        UploadOutcome::Unchanged => return Ok(serde_json::json!({ "status": "unchanged" })),
        UploadOutcome::Uploaded(baseline) => baseline,
    };
    let _persisted = persist_sync_success_best_effort(settings, baseline, persist_sync_success);
    Ok(serde_json::json!({ "status": "uploaded", "insecureTransport": insecure }))
}

/// Download remote snapshot and apply to local database + skills.
///
/// Unless `force`, the download is refused with a distinct "local changed"
/// conflict error when local data changed since the last successful sync.
pub async fn download(
    db: &crate::database::Database,
    settings: &mut S3SyncSettings,
    force: bool,
) -> Result<Value, AppError> {
    validate_settings(settings)?;
    let insecure = warn_if_insecure("S3", &settings.endpoint);
    let transport = S3Transport::new(settings);
    let baseline = sync_transport::download(&transport, db, &settings.status, force).await?;
    let _persisted = persist_sync_success_best_effort(settings, baseline, persist_sync_success);
    Ok(serde_json::json!({ "status": "downloaded", "insecureTransport": insecure }))
}

/// Fetch remote manifest info without downloading artifacts.
pub async fn fetch_remote_info(settings: &S3SyncSettings) -> Result<Option<Value>, AppError> {
    validate_settings(settings)?;
    let creds = creds_for(settings);
    let manifest_key = s3_key(settings, REMOTE_MANIFEST);

    let Some((bytes, etag)) = s3::get_object(&creds, &manifest_key, MAX_MANIFEST_BYTES).await?
    else {
        return Ok(None);
    };

    let manifest: SyncManifest = serde_json::from_slice(&bytes).map_err(|e| AppError::Json {
        path: REMOTE_MANIFEST.to_string(),
        source: e,
    })?;

    let compatible = validate_manifest_compat(&manifest, RemoteLayout::Current).is_ok();
    let remote_changed = evaluate_upload_precondition(
        &settings.status,
        Some(&RemoteManifestView {
            bytes: &bytes,
            etag: etag.as_deref(),
            manifest: None,
        }),
        false,
    )
    .is_err();
    let encryption = remote_encryption_info(&manifest);

    let payload = serde_json::json!({
        "remoteChangedSinceLastSync": remote_changed,
        "insecureTransport": is_insecure_remote_url(&settings.endpoint),
        "encrypted": encryption.encrypted,
        "passphraseMatches": encryption.passphrase_matches,
        "deviceName": manifest.device_name,
        "createdAt": manifest.created_at,
        "snapshotId": manifest.snapshot_id,
        "version": manifest.version,
        "protocolVersion": manifest.version,
        "dbCompatVersion": manifest.db_compat_version,
        "compatible": compatible,
        "artifacts": manifest.artifacts.keys().collect::<Vec<_>>(),
        "layout": RemoteLayout::Current.as_str(),
        "remotePath": s3_dir_display(settings),
    });

    Ok(Some(payload))
}

// ─── Sync status persistence ─────────────────────────────────

fn persist_sync_success(
    settings: &mut S3SyncSettings,
    baseline: SyncBaseline,
) -> Result<(), AppError> {
    let status = sync_status_from_baseline(baseline);
    settings.status = status.clone();
    update_s3_sync_status(status)
}

// ─── Transport ───────────────────────────────────────────────

static S3_KEYS: TransportKeys = TransportKeys {
    remote_empty: "s3.sync.remote_empty",
    manifest_missing_artifact: "s3.sync.manifest_missing_artifact",
    remote_missing_artifact: "s3.sync.remote_missing_artifact",
};

/// S3 implementation of the shared sync engine's transport (SVC-19).
struct S3Transport<'a> {
    settings: &'a S3SyncSettings,
    creds: S3Credentials,
}

impl<'a> S3Transport<'a> {
    fn new(settings: &'a S3SyncSettings) -> Self {
        Self {
            settings,
            creds: creds_for(settings),
        }
    }
}

impl SyncTransport for S3Transport<'_> {
    fn label(&self) -> &'static str {
        "S3"
    }
    fn keys(&self) -> &'static TransportKeys {
        &S3_KEYS
    }
    async fn ensure_root(&self) -> Result<(), AppError> {
        // Object stores have no directories.
        Ok(())
    }
    async fn get(&self, name: &str, max_bytes: usize) -> Result<sync_transport::Fetched, AppError> {
        s3::get_object(&self.creds, &s3_key(self.settings, name), max_bytes).await
    }
    async fn put(
        &self,
        name: &str,
        bytes: Vec<u8>,
        content_type: &str,
        if_match: Option<&str>,
    ) -> Result<PutResult, AppError> {
        let key = s3_key(self.settings, name);
        Ok(
            match s3::put_object_if_match(&self.creds, &key, bytes, content_type, if_match).await? {
                s3::PutOutcome::Stored => PutResult::Stored,
                s3::PutOutcome::PreconditionFailed => PutResult::PreconditionFailed,
            },
        )
    }
    async fn head(&self, name: &str) -> Result<Option<String>, AppError> {
        s3::head_object(&self.creds, &s3_key(self.settings, name)).await
    }
    fn supports_conditional_put(&self) -> bool {
        s3::supports_conditional_put(&self.creds)
    }
}

// ─── S3 key helpers ──────────────────────────────────────────

/// Build the S3 object key for a given artifact.
///
/// Format: `{remote_root}/v{PROTOCOL_VERSION}/db-v{DB_COMPAT_VERSION}/{profile}/{artifact}`
/// Example: `cc-switch-sync/v2/db-v6/default/manifest.json`
///
/// The raw layout is kept (existing buckets depend on it); unsafe segments are
/// rejected up front by [`validate_settings`].
fn s3_key(settings: &S3SyncSettings, artifact: &str) -> String {
    format!(
        "{}/v{}/db-v{}/{}/{}",
        settings.remote_root, PROTOCOL_VERSION, DB_COMPAT_VERSION, settings.profile, artifact
    )
}

/// `settings.validate()` plus remote-layout validation (SVC-15).
fn validate_settings(settings: &S3SyncSettings) -> Result<(), AppError> {
    settings.validate()?;
    crate::services::sync_protocol::validate_remote_layout(&settings.remote_root, &settings.profile)
}

fn s3_dir_display(settings: &S3SyncSettings) -> String {
    format!(
        "{}/v{}/db-v{}/{}",
        settings.remote_root, PROTOCOL_VERSION, DB_COMPAT_VERSION, settings.profile
    )
}

fn creds_for(settings: &S3SyncSettings) -> S3Credentials {
    S3Credentials {
        access_key_id: settings.access_key_id.clone(),
        secret_access_key: settings.secret_access_key.clone(),
        region: settings.region.clone(),
        bucket: settings.bucket.clone(),
        endpoint: settings.endpoint.clone(),
    }
}

// ─── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_settings() -> S3SyncSettings {
        S3SyncSettings {
            remote_root: "cc-switch-sync".to_string(),
            profile: "default".to_string(),
            ..S3SyncSettings::default()
        }
    }

    #[test]
    fn s3_key_uses_v2_and_correct_format() {
        let settings = test_settings();
        let key = s3_key(&settings, "manifest.json");
        assert_eq!(key, "cc-switch-sync/v2/db-v6/default/manifest.json");
    }

    #[test]
    fn invalid_profile_and_root_are_rejected() {
        let mut settings = test_settings();
        settings.bucket = "b".into();
        settings.region = "r".into();
        settings.access_key_id = "a".into();
        settings.secret_access_key = "s".into();
        assert!(validate_settings(&settings).is_ok());
        settings.profile = "../x".into();
        assert!(validate_settings(&settings).is_err());
        settings.profile = "default".into();
        settings.remote_root = "root/../../etc".into();
        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn s3_key_with_custom_profile() {
        let settings = S3SyncSettings {
            remote_root: "my-root".to_string(),
            profile: "work".to_string(),
            ..S3SyncSettings::default()
        };
        assert_eq!(s3_key(&settings, "db.sql"), "my-root/v2/db-v6/work/db.sql");
    }

    #[test]
    fn s3_key_matches_expected_pattern() {
        let settings = test_settings();
        let key = s3_key(&settings, "skills.zip");
        // Should follow {remote_root}/v{version}/db-v{db}/{profile}/{artifact}
        let parts: Vec<&str> = key.splitn(5, '/').collect();
        assert_eq!(parts.len(), 5);
        assert_eq!(parts[0], "cc-switch-sync");
        assert_eq!(parts[1], "v2");
        assert_eq!(parts[2], "db-v6");
        assert_eq!(parts[3], "default");
        assert_eq!(parts[4], "skills.zip");
    }

    #[test]
    fn sync_mutex_is_singleton() {
        let m1 = sync_mutex();
        let m2 = sync_mutex();
        assert!(
            std::ptr::eq(m1, m2),
            "sync_mutex must return the same instance"
        );
    }

    #[test]
    fn creds_for_maps_all_fields() {
        let settings = S3SyncSettings {
            access_key_id: "AKID".to_string(),
            secret_access_key: "SECRET".to_string(),
            region: "us-west-2".to_string(),
            bucket: "my-bucket".to_string(),
            endpoint: "minio.local:9000".to_string(),
            ..S3SyncSettings::default()
        };
        let creds = creds_for(&settings);
        assert_eq!(creds.access_key_id, "AKID");
        assert_eq!(creds.secret_access_key, "SECRET");
        assert_eq!(creds.region, "us-west-2");
        assert_eq!(creds.bucket, "my-bucket");
        assert_eq!(creds.endpoint, "minio.local:9000");
    }
}

// End-to-end tests against a fake S3 server (SVC-24).
#[cfg(test)]
#[path = "s3_sync_e2e_tests.rs"]
mod e2e_tests;
