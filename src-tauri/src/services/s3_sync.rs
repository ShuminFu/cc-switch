//! S3 v2 sync protocol layer.
//!
//! Implements manifest-based synchronization on top of the S3 transport
//! primitives in [`super::s3`]. Artifact set: `db.sql` + `skills.zip`.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::OnceLock;

use serde_json::Value;

use crate::error::AppError;
use crate::services::s3::{self, S3Credentials};
use crate::settings::{update_s3_sync_status, S3SyncSettings};

use super::sync_protocol::{
    apply_snapshot, build_local_snapshot, capture_local_fingerprint, ensure_local_unchanged,
    evaluate_upload_precondition, is_insecure_remote_url, local_content_fingerprint, localized,
    persist_sync_success_best_effort, remote_changed_error, sha256_hex, sync_status_from_baseline,
    validate_artifact_size_limit, validate_manifest_compat, verify_artifact, warn_if_insecure,
    ArtifactMeta, RemoteLayout, RemoteManifestView, SyncBaseline, SyncManifest, DB_COMPAT_VERSION,
    MAX_MANIFEST_BYTES, MAX_SYNC_ARTIFACT_BYTES, PROTOCOL_VERSION, REMOTE_DB_SQL, REMOTE_MANIFEST,
    REMOTE_SKILLS_ZIP,
};

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
    settings.validate()?;
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
    settings.validate()?;
    let insecure = warn_if_insecure("S3", &settings.endpoint);
    let creds = creds_for(settings);

    let snapshot = build_local_snapshot(db)?;
    let local_fingerprint = local_content_fingerprint(&snapshot.db_sql, &snapshot.skills_zip);

    // Precondition: the remote must still be what this device last synced.
    let manifest_key = s3_key(settings, REMOTE_MANIFEST);
    let remote = s3::get_object(&creds, &manifest_key, MAX_MANIFEST_BYTES).await?;
    let if_match = {
        let parsed = remote
            .as_ref()
            .and_then(|(bytes, _)| serde_json::from_slice::<SyncManifest>(bytes).ok());
        let view = remote.as_ref().map(|(bytes, etag)| RemoteManifestView {
            bytes,
            etag: etag.as_deref(),
            manifest: parsed.as_ref(),
        });
        evaluate_upload_precondition(&settings.status, view.as_ref(), force)?
    };
    let if_match = if_match.filter(|_| s3::supports_conditional_put(&creds));

    // Upload order: artifacts first, manifest last (best-effort consistency)
    let db_key = s3_key(settings, REMOTE_DB_SQL);
    s3::put_object(&creds, &db_key, snapshot.db_sql, "application/sql").await?;

    let skills_key = s3_key(settings, REMOTE_SKILLS_ZIP);
    s3::put_object(&creds, &skills_key, snapshot.skills_zip, "application/zip").await?;

    let outcome = s3::put_object_if_match(
        &creds,
        &manifest_key,
        snapshot.manifest_bytes,
        "application/json",
        if_match.as_deref(),
    )
    .await?;
    if outcome == s3::PutOutcome::PreconditionFailed {
        log::warn!("[S3] Manifest changed during upload (412); refusing to overwrite");
        return Err(remote_changed_error(None));
    }

    // Fetch etag (best-effort, don't fail the upload)
    let etag = match s3::head_object(&creds, &manifest_key).await {
        Ok(e) => e,
        Err(e) => {
            log::debug!("[S3] Failed to fetch ETag after upload: {e}");
            None
        }
    };

    let _persisted = persist_sync_success_best_effort(
        settings,
        SyncBaseline {
            manifest_hash: snapshot.manifest_hash,
            etag,
            local_fingerprint: Some(local_fingerprint),
        },
        persist_sync_success,
    );
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
    settings.validate()?;
    let insecure = warn_if_insecure("S3", &settings.endpoint);
    ensure_local_unchanged(db, &settings.status, force)?;
    let creds = creds_for(settings);

    let manifest_key = s3_key(settings, REMOTE_MANIFEST);
    let (manifest_bytes, etag) = s3::get_object(&creds, &manifest_key, MAX_MANIFEST_BYTES)
        .await?
        .ok_or_else(|| {
            localized(
                "s3.sync.remote_empty",
                "远端没有可下载的同步数据",
                "No downloadable sync data found on the remote.",
            )
        })?;

    let manifest: SyncManifest =
        serde_json::from_slice(&manifest_bytes).map_err(|e| AppError::Json {
            path: REMOTE_MANIFEST.to_string(),
            source: e,
        })?;

    validate_manifest_compat(&manifest, RemoteLayout::Current)?;

    // Download and verify artifacts
    let db_sql = download_and_verify(settings, &creds, REMOTE_DB_SQL, &manifest.artifacts).await?;
    let skills_zip =
        download_and_verify(settings, &creds, REMOTE_SKILLS_ZIP, &manifest.artifacts).await?;

    // Apply snapshot
    apply_snapshot(db, &db_sql, &skills_zip)?;

    let manifest_hash = sha256_hex(&manifest_bytes);
    let _persisted = persist_sync_success_best_effort(
        settings,
        SyncBaseline {
            manifest_hash,
            etag,
            local_fingerprint: capture_local_fingerprint(db),
        },
        persist_sync_success,
    );
    Ok(serde_json::json!({ "status": "downloaded", "insecureTransport": insecure }))
}

/// Fetch remote manifest info without downloading artifacts.
pub async fn fetch_remote_info(settings: &S3SyncSettings) -> Result<Option<Value>, AppError> {
    settings.validate()?;
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

    let payload = serde_json::json!({
        "remoteChangedSinceLastSync": remote_changed,
        "insecureTransport": is_insecure_remote_url(&settings.endpoint),
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

// ─── Download & verify ───────────────────────────────────────

async fn download_and_verify(
    settings: &S3SyncSettings,
    creds: &S3Credentials,
    artifact_name: &str,
    artifacts: &BTreeMap<String, ArtifactMeta>,
) -> Result<Vec<u8>, AppError> {
    let meta = artifacts.get(artifact_name).ok_or_else(|| {
        localized(
            "s3.sync.manifest_missing_artifact",
            format!("manifest 中缺少 artifact: {artifact_name}"),
            format!("Manifest missing artifact: {artifact_name}"),
        )
    })?;
    validate_artifact_size_limit(artifact_name, meta.size)?;

    let key = s3_key(settings, artifact_name);
    let (bytes, _) = s3::get_object(creds, &key, MAX_SYNC_ARTIFACT_BYTES as usize)
        .await?
        .ok_or_else(|| {
            localized(
                "s3.sync.remote_missing_artifact",
                format!("远端缺少 artifact 文件: {artifact_name}"),
                format!("Remote artifact file missing: {artifact_name}"),
            )
        })?;

    verify_artifact(&bytes, artifact_name, meta)?;
    Ok(bytes)
}

// ─── S3 key helpers ──────────────────────────────────────────

/// Build the S3 object key for a given artifact.
///
/// Format: `{remote_root}/v{PROTOCOL_VERSION}/db-v{DB_COMPAT_VERSION}/{profile}/{artifact}`
/// Example: `cc-switch-sync/v2/db-v6/default/manifest.json`
fn s3_key(settings: &S3SyncSettings, artifact: &str) -> String {
    format!(
        "{}/v{}/db-v{}/{}/{}",
        settings.remote_root, PROTOCOL_VERSION, DB_COMPAT_VERSION, settings.profile, artifact
    )
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
