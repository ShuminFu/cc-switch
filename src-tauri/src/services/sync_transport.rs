//! Transport-agnostic cloud-sync engine (SVC-19).
//!
//! [`SyncTransport`] is the small set of object-store operations the sync
//! protocol needs (get / put with an `If-Match` precondition / head). The
//! protocol itself — snapshot building, the upload precondition, encryption
//! checks, artifact verification and applying a download — lives once in
//! [`upload`] and [`download`] and is shared by every transport. S3 runs on
//! it; tests drive it through [`InMemoryTransport`] so the protocol is
//! covered without network access.

use std::collections::BTreeMap;
use std::future::Future;

use crate::error::AppError;
use crate::services::operation_log::{Operation, StepOutcome};
use crate::settings::WebDavSyncStatus;

use super::sync_protocol::{
    apply_snapshot, build_snapshot, capture_local_fingerprint, download_passphrase,
    effective_db_compat_version, ensure_local_unchanged, ensure_no_encryption_downgrade,
    evaluate_upload_precondition, local_unchanged_since_sync, open_artifact, remote_changed_error,
    remote_encryption_info, sha256_hex, unlock_remote, upload_key, upload_passphrase,
    validate_artifact_size_limit, validate_manifest_compat, verify_artifact, ArtifactMeta,
    RemoteLayout, RemoteManifestView, SyncBaseline, SyncManifest, MAX_MANIFEST_BYTES,
    MAX_SYNC_ARTIFACT_BYTES, REMOTE_DB_SQL, REMOTE_MANIFEST, REMOTE_SKILLS_ZIP,
};

/// An object's bytes and ETag, or `None` when it does not exist.
pub(crate) type Fetched = Option<(Vec<u8>, Option<String>)>;

/// Result of a conditional put.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PutResult {
    Stored,
    /// The precondition (`If-Match`) did not hold.
    PreconditionFailed,
}

/// Localization keys a transport reports its errors under (kept per
/// transport so existing frontend translations keep working).
pub(crate) struct TransportKeys {
    pub remote_empty: &'static str,
    pub manifest_missing_artifact: &'static str,
    pub remote_missing_artifact: &'static str,
}

/// Object-store operations used by the sync protocol. `name` is an artifact
/// name (`manifest.json`, `db.sql`, `skills.zip`) relative to the transport's
/// configured remote directory.
pub(crate) trait SyncTransport: Sync {
    /// Short name for logs ("S3", "WebDAV").
    fn label(&self) -> &'static str;
    fn keys(&self) -> &'static TransportKeys;
    /// Create the remote directory structure if needed.
    fn ensure_root(&self) -> impl Future<Output = Result<(), AppError>> + Send;
    /// Fetch an object (at most `max_bytes`) and its ETag.
    fn get(
        &self,
        name: &str,
        max_bytes: usize,
    ) -> impl Future<Output = Result<Fetched, AppError>> + Send;
    /// Store an object; with `if_match`, only if its current ETag matches.
    fn put(
        &self,
        name: &str,
        bytes: Vec<u8>,
        content_type: &str,
        if_match: Option<&str>,
    ) -> impl Future<Output = Result<PutResult, AppError>> + Send;
    /// Current ETag of an object.
    fn head(&self, name: &str) -> impl Future<Output = Result<Option<String>, AppError>> + Send;
    /// Whether `put` honours `if_match` (some S3-compatible stores do not).
    fn supports_conditional_put(&self) -> bool {
        true
    }
    /// Remote directory layout this transport reads and writes (WebDAV can
    /// also read the pre-`db-v*` legacy layout).
    fn layout(&self) -> RemoteLayout {
        RemoteLayout::Current
    }
    /// Human-readable remote location for status payloads.
    fn remote_path(&self) -> String;
    /// Whether the remote is reached over plain HTTP.
    fn insecure(&self) -> bool;
}

/// A remote manifest with its raw bytes and ETag.
pub(crate) struct RemoteSnapshot {
    pub layout: RemoteLayout,
    pub manifest: SyncManifest,
    pub manifest_bytes: Vec<u8>,
    pub manifest_etag: Option<String>,
}

/// Read and parse the remote manifest, or `None` when there is none.
pub(crate) async fn fetch_manifest<T: SyncTransport>(
    transport: &T,
) -> Result<Option<RemoteSnapshot>, AppError> {
    let Some((manifest_bytes, manifest_etag)) =
        transport.get(REMOTE_MANIFEST, MAX_MANIFEST_BYTES).await?
    else {
        return Ok(None);
    };
    let manifest: SyncManifest =
        serde_json::from_slice(&manifest_bytes).map_err(|e| AppError::Json {
            path: REMOTE_MANIFEST.to_string(),
            source: e,
        })?;
    Ok(Some(RemoteSnapshot {
        layout: transport.layout(),
        manifest,
        manifest_bytes,
        manifest_etag,
    }))
}

/// Status payload describing a remote snapshot without downloading it
/// (shared by every transport's `fetch_remote_info`).
pub(crate) fn remote_info<T: SyncTransport>(
    transport: &T,
    status: &WebDavSyncStatus,
    snapshot: &RemoteSnapshot,
) -> serde_json::Value {
    let manifest = &snapshot.manifest;
    let compatible = validate_manifest_compat(manifest, snapshot.layout).is_ok();
    let db_compat_version = effective_db_compat_version(manifest, snapshot.layout);
    let remote_changed = evaluate_upload_precondition(
        status,
        Some(&RemoteManifestView {
            bytes: &snapshot.manifest_bytes,
            etag: snapshot.manifest_etag.as_deref(),
            manifest: None,
        }),
        false,
    )
    .is_err();
    let encryption = remote_encryption_info(manifest);
    serde_json::json!({
        "remoteChangedSinceLastSync": remote_changed,
        "insecureTransport": transport.insecure(),
        "encrypted": encryption.encrypted,
        "passphraseMatches": encryption.passphrase_matches,
        "deviceName": manifest.device_name,
        "createdAt": manifest.created_at,
        "snapshotId": manifest.snapshot_id,
        "version": manifest.version,
        "protocolVersion": manifest.version,
        "dbCompatVersion": db_compat_version,
        "compatible": compatible,
        "artifacts": manifest.artifacts.keys().collect::<Vec<_>>(),
        "layout": snapshot.layout.as_str(),
        "remotePath": transport.remote_path(),
    })
}

/// What an [`upload`] did.
#[derive(Debug)]
pub(crate) enum UploadOutcome {
    /// Local content unchanged since the last sync and `skip_if_unchanged`.
    Unchanged,
    /// Uploaded; persist this baseline as the new sync status.
    Uploaded(SyncBaseline),
}

/// Upload the local snapshot through `transport`.
///
/// Unless `force`, refuses with a "remote changed" conflict when the remote
/// manifest differs from the one recorded in `status`; the manifest is
/// written last with `If-Match` on the ETag that was checked.
pub(crate) async fn upload<T: SyncTransport>(
    transport: &T,
    db: &crate::database::Database,
    status: &WebDavSyncStatus,
    force: bool,
    skip_if_unchanged: bool,
) -> Result<UploadOutcome, AppError> {
    // SVC-23: one traced operation per sync upload.
    let op = Operation::start_async(
        "sync",
        &[
            ("transport", transport.label()),
            ("direction", "upload"),
            ("force", if force { "true" } else { "false" }),
        ],
    );
    let result = tracing::Instrument::instrument(
        upload_inner(transport, db, status, force, skip_if_unchanged, &op),
        op.span(),
    )
    .await;
    op.finish(&result);
    result
}

async fn upload_inner<T: SyncTransport>(
    transport: &T,
    db: &crate::database::Database,
    status: &WebDavSyncStatus,
    force: bool,
    skip_if_unchanged: bool,
    op: &Operation,
) -> Result<UploadOutcome, AppError> {
    transport.ensure_root().await?;

    // SEC-06 / SVC-04: encrypt the artifacts when sync encryption is enabled.
    let encryption = upload_passphrase()?
        .map(|passphrase| upload_key(&passphrase))
        .transpose()?;
    let snapshot = build_snapshot(db, encryption.as_ref())?;
    let local_fingerprint = snapshot.local_fingerprint.clone();
    op.step("snapshot", StepOutcome::Ok, None);
    if skip_if_unchanged && local_unchanged_since_sync(status, &local_fingerprint) {
        op.step(
            "upload",
            StepOutcome::Skipped,
            Some("local content unchanged"),
        );
        log::debug!(
            "[{}] Local content unchanged since last sync; skipping upload",
            transport.label()
        );
        return Ok(UploadOutcome::Unchanged);
    }

    // Precondition: the remote must still be what this device last synced.
    let remote = transport.get(REMOTE_MANIFEST, MAX_MANIFEST_BYTES).await?;
    let if_match = {
        let parsed = remote
            .as_ref()
            .and_then(|(bytes, _)| serde_json::from_slice::<SyncManifest>(bytes).ok());
        let view = remote.as_ref().map(|(bytes, etag)| RemoteManifestView {
            bytes,
            etag: etag.as_deref(),
            manifest: parsed.as_ref(),
        });
        ensure_no_encryption_downgrade(parsed.as_ref(), encryption.is_some(), force)?;
        let precondition = evaluate_upload_precondition(status, view.as_ref(), force);
        op.step_result("remote_precondition", &precondition);
        precondition?
    };
    let if_match = if_match.filter(|_| transport.supports_conditional_put());

    // Upload order: artifacts first, manifest last (best-effort consistency)
    expect_stored(
        transport
            .put(REMOTE_DB_SQL, snapshot.db_sql, "application/sql", None)
            .await?,
    )?;
    expect_stored(
        transport
            .put(
                REMOTE_SKILLS_ZIP,
                snapshot.skills_zip,
                "application/zip",
                None,
            )
            .await?,
    )?;
    let outcome = transport
        .put(
            REMOTE_MANIFEST,
            snapshot.manifest_bytes,
            "application/json",
            if_match.as_deref(),
        )
        .await?;
    op.step(
        "manifest_put",
        if outcome == PutResult::Stored {
            StepOutcome::Ok
        } else {
            StepOutcome::Error
        },
        (outcome == PutResult::PreconditionFailed).then_some("precondition failed"),
    );
    if outcome == PutResult::PreconditionFailed {
        log::warn!(
            "[{}] Manifest changed during upload (412); refusing to overwrite",
            transport.label()
        );
        return Err(remote_changed_error(None));
    }

    // Fetch etag (best-effort, don't fail the upload)
    let etag = match transport.head(REMOTE_MANIFEST).await {
        Ok(e) => e,
        Err(e) => {
            log::debug!(
                "[{}] Failed to fetch ETag after upload: {e}",
                transport.label()
            );
            None
        }
    };

    Ok(UploadOutcome::Uploaded(SyncBaseline {
        manifest_hash: snapshot.manifest_hash,
        etag,
        local_fingerprint: Some(local_fingerprint),
    }))
}

fn expect_stored(result: PutResult) -> Result<(), AppError> {
    match result {
        PutResult::Stored => Ok(()),
        // Unconditional puts never fail a precondition; treat it as a conflict.
        PutResult::PreconditionFailed => Err(remote_changed_error(None)),
    }
}

/// Download the remote snapshot through `transport` and apply it.
///
/// Unless `force`, refuses with a "local changed" conflict when local data
/// changed since the last sync. Returns the baseline to persist.
pub(crate) async fn download<T: SyncTransport>(
    transport: &T,
    db: &crate::database::Database,
    status: &WebDavSyncStatus,
    force: bool,
) -> Result<SyncBaseline, AppError> {
    ensure_local_unchanged(db, status, force)?;
    let snapshot = fetch_manifest(transport)
        .await?
        .ok_or_else(|| remote_empty_error(transport))?;
    download_snapshot(transport, db, snapshot).await
}

/// What downloading the remote snapshot would change locally (SVC-27).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncPreview {
    pub device_name: String,
    pub created_at: String,
    pub snapshot_id: String,
    /// Per-table differences (only tables that differ); rows and key paths
    /// only, never values.
    pub tables: Vec<crate::database::sync_diff::TableDiff>,
}

impl SyncPreview {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
    }
}

/// Dry-run a download: fetch and verify the remote `db.sql`, load it into a
/// scratch database and diff it against the local data. Nothing local is
/// changed. `None` when the remote has no snapshot.
pub(crate) async fn preview_snapshot<T: SyncTransport>(
    transport: &T,
    db: &crate::database::Database,
    snapshot: &RemoteSnapshot,
) -> Result<SyncPreview, AppError> {
    let manifest = &snapshot.manifest;
    validate_manifest_compat(manifest, snapshot.layout)?;
    let key = unlock_remote(manifest, download_passphrase().as_deref())?;
    let db_sql = download_and_verify(transport, REMOTE_DB_SQL, &manifest.artifacts).await?;
    let db_sql = open_artifact(key.as_ref(), REMOTE_DB_SQL, db_sql)?;
    let sql = String::from_utf8(db_sql).map_err(|e| {
        AppError::localized(
            "sync.preview_invalid_sql",
            format!("远端 db.sql 不是有效的 UTF-8: {e}"),
            format!("Remote db.sql is not valid UTF-8: {e}"),
        )
    })?;
    let remote = crate::database::Database::from_sync_export(&sql)?;
    let local = db.sync_view()?;
    let tables = {
        let local_conn = local
            .conn
            .lock()
            .map_err(|e| AppError::Database(e.to_string()))?;
        let remote_conn = remote
            .conn
            .lock()
            .map_err(|e| AppError::Database(e.to_string()))?;
        crate::database::sync_diff::diff_databases(&local_conn, &remote_conn)?
    };
    Ok(SyncPreview {
        device_name: manifest.device_name.clone(),
        created_at: manifest.created_at.clone(),
        snapshot_id: manifest.snapshot_id.clone(),
        tables,
    })
}

/// [`preview_snapshot`] of the transport's current manifest.
pub(crate) async fn preview<T: SyncTransport>(
    transport: &T,
    db: &crate::database::Database,
) -> Result<Option<SyncPreview>, AppError> {
    match fetch_manifest(transport).await? {
        Some(snapshot) => preview_snapshot(transport, db, &snapshot).await.map(Some),
        None => Ok(None),
    }
}

/// "Nothing to download" in the transport's localization namespace.
pub(crate) fn remote_empty_error<T: SyncTransport>(transport: &T) -> AppError {
    AppError::localized(
        transport.keys().remote_empty,
        "远端没有可下载的同步数据",
        "No downloadable sync data found on the remote.",
    )
}

/// Verify, download and apply the artifacts of an already fetched manifest.
///
/// The caller has checked the local-changes precondition; `transport` must
/// address the layout the manifest was read from.
pub(crate) async fn download_snapshot<T: SyncTransport>(
    transport: &T,
    db: &crate::database::Database,
    snapshot: RemoteSnapshot,
) -> Result<SyncBaseline, AppError> {
    // SVC-23: one traced operation per sync download.
    let op = Operation::start_async(
        "sync",
        &[
            ("transport", transport.label()),
            ("direction", "download"),
            ("layout", snapshot.layout.as_str()),
        ],
    );
    let result = tracing::Instrument::instrument(
        download_snapshot_inner(transport, db, snapshot, &op),
        op.span(),
    )
    .await;
    op.finish(&result);
    result
}

async fn download_snapshot_inner<T: SyncTransport>(
    transport: &T,
    db: &crate::database::Database,
    snapshot: RemoteSnapshot,
    op: &Operation,
) -> Result<SyncBaseline, AppError> {
    let RemoteSnapshot {
        layout,
        manifest,
        manifest_bytes,
        manifest_etag,
    } = snapshot;
    validate_manifest_compat(&manifest, layout)?;
    // Encrypted snapshot: check the passphrase before downloading artifacts.
    let key = unlock_remote(&manifest, download_passphrase().as_deref())?;

    let db_sql = download_and_verify(transport, REMOTE_DB_SQL, &manifest.artifacts).await?;
    let skills_zip = download_and_verify(transport, REMOTE_SKILLS_ZIP, &manifest.artifacts).await?;
    op.step("artifacts_verified", StepOutcome::Ok, None);
    let db_sql = open_artifact(key.as_ref(), REMOTE_DB_SQL, db_sql)?;
    let skills_zip = open_artifact(key.as_ref(), REMOTE_SKILLS_ZIP, skills_zip)?;

    let applied = apply_snapshot(db, &db_sql, &skills_zip);
    op.step_result("apply_snapshot", &applied);
    applied?;

    Ok(SyncBaseline {
        manifest_hash: sha256_hex(&manifest_bytes),
        etag: manifest_etag,
        local_fingerprint: capture_local_fingerprint(db),
    })
}

async fn download_and_verify<T: SyncTransport>(
    transport: &T,
    artifact_name: &str,
    artifacts: &BTreeMap<String, ArtifactMeta>,
) -> Result<Vec<u8>, AppError> {
    let keys = transport.keys();
    let meta = artifacts.get(artifact_name).ok_or_else(|| {
        AppError::localized(
            keys.manifest_missing_artifact,
            format!("manifest 中缺少 artifact: {artifact_name}"),
            format!("Manifest missing artifact: {artifact_name}"),
        )
    })?;
    validate_artifact_size_limit(artifact_name, meta.size)?;

    let (bytes, _) = transport
        .get(artifact_name, MAX_SYNC_ARTIFACT_BYTES as usize)
        .await?
        .ok_or_else(|| {
            AppError::localized(
                keys.remote_missing_artifact,
                format!("远端缺少 artifact 文件: {artifact_name}"),
                format!("Remote artifact file missing: {artifact_name}"),
            )
        })?;

    verify_artifact(&bytes, artifact_name, meta)?;
    Ok(bytes)
}

// ─── In-memory transport (tests) ─────────────────────────────

/// A transport backed by a map, with ETags that change on every write.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct InMemoryTransport {
    objects: std::sync::Mutex<BTreeMap<String, (Vec<u8>, u64)>>,
    next_etag: std::sync::atomic::AtomicU64,
}

#[cfg(test)]
static IN_MEMORY_KEYS: TransportKeys = TransportKeys {
    remote_empty: "memory.sync.remote_empty",
    manifest_missing_artifact: "memory.sync.manifest_missing_artifact",
    remote_missing_artifact: "memory.sync.remote_missing_artifact",
};

#[cfg(test)]
impl InMemoryTransport {
    pub(crate) fn object(&self, name: &str) -> Option<Vec<u8>> {
        self.objects
            .lock()
            .unwrap()
            .get(name)
            .map(|(b, _)| b.clone())
    }

    pub(crate) fn replace(&self, name: &str, bytes: Vec<u8>) {
        let etag = self
            .next_etag
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.objects
            .lock()
            .unwrap()
            .insert(name.to_string(), (bytes, etag + 1000));
    }
}

#[cfg(test)]
impl SyncTransport for InMemoryTransport {
    fn label(&self) -> &'static str {
        "Memory"
    }
    fn keys(&self) -> &'static TransportKeys {
        &IN_MEMORY_KEYS
    }
    async fn ensure_root(&self) -> Result<(), AppError> {
        Ok(())
    }
    async fn get(&self, name: &str, _max_bytes: usize) -> Result<Fetched, AppError> {
        Ok(self
            .objects
            .lock()
            .unwrap()
            .get(name)
            .map(|(bytes, etag)| (bytes.clone(), Some(format!("\"{etag}\"")))))
    }
    async fn put(
        &self,
        name: &str,
        bytes: Vec<u8>,
        _content_type: &str,
        if_match: Option<&str>,
    ) -> Result<PutResult, AppError> {
        let mut objects = self.objects.lock().unwrap();
        if let Some(expected) = if_match {
            let current = objects.get(name).map(|(_, etag)| format!("\"{etag}\""));
            if current.as_deref() != Some(expected) {
                return Ok(PutResult::PreconditionFailed);
            }
        }
        let etag = self
            .next_etag
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        objects.insert(name.to_string(), (bytes, etag + 1));
        Ok(PutResult::Stored)
    }
    async fn head(&self, name: &str) -> Result<Option<String>, AppError> {
        Ok(self
            .objects
            .lock()
            .unwrap()
            .get(name)
            .map(|(_, etag)| format!("\"{etag}\"")))
    }
    fn remote_path(&self) -> String {
        "/memory".to_string()
    }
    fn insecure(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::sync_protocol::{is_sync_conflict, sync_status_from_baseline};

    /// Points the app config dir (skills SSOT) at a temp dir for the test.
    struct TestHome {
        _dir: tempfile::TempDir,
        previous: Option<std::ffi::OsString>,
    }

    impl TestHome {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("tempdir");
            let previous = std::env::var_os("CC_SWITCH_TEST_HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            Self {
                _dir: dir,
                previous,
            }
        }
    }

    impl Drop for TestHome {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(v) => std::env::set_var("CC_SWITCH_TEST_HOME", v),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    fn block<F: Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(f)
    }

    #[test]
    #[serial_test::serial]
    fn upload_then_skip_unchanged_and_detect_remote_change() {
        let _home = TestHome::new();
        let db = crate::database::Database::memory().expect("memory db");
        let remote = InMemoryTransport::default();
        let status = WebDavSyncStatus::default();

        let baseline = match block(upload(&remote, &db, &status, false, false)).unwrap() {
            UploadOutcome::Uploaded(baseline) => baseline,
            other => panic!("expected an upload, got {other:?}"),
        };
        assert!(remote.object(REMOTE_MANIFEST).is_some());
        assert!(remote.object(REMOTE_DB_SQL).is_some());
        assert!(remote.object(REMOTE_SKILLS_ZIP).is_some());
        let status = sync_status_from_baseline(baseline);

        // Nothing changed locally: an auto-sync upload is skipped.
        assert!(matches!(
            block(upload(&remote, &db, &status, false, true)).unwrap(),
            UploadOutcome::Unchanged
        ));

        // Another device replaced the manifest: a non-forced upload conflicts.
        let mut manifest = remote.object(REMOTE_MANIFEST).unwrap();
        manifest.extend_from_slice(b" ");
        remote.replace(REMOTE_MANIFEST, manifest);
        let err = block(upload(&remote, &db, &status, false, false)).unwrap_err();
        assert!(is_sync_conflict(&err), "remote change is a conflict: {err}");
    }

    #[test]
    #[serial_test::serial]
    fn download_rejects_tampered_artifact() {
        let _home = TestHome::new();
        let db = crate::database::Database::memory().expect("memory db");
        let remote = InMemoryTransport::default();
        block(upload(
            &remote,
            &db,
            &WebDavSyncStatus::default(),
            false,
            false,
        ))
        .unwrap();

        let mut sql = remote.object(REMOTE_DB_SQL).unwrap();
        sql.extend_from_slice(b"\n-- tampered");
        remote.replace(REMOTE_DB_SQL, sql);

        let err = block(download(&remote, &db, &WebDavSyncStatus::default(), true))
            .expect_err("hash mismatch must be rejected before apply");
        assert!(!is_sync_conflict(&err));
    }

    /// SVC-19: remote info is computed once for every transport.
    #[test]
    #[serial_test::serial]
    fn remote_info_reports_manifest_and_remote_change() {
        let _home = TestHome::new();
        let db = crate::database::Database::memory().expect("memory db");
        let remote = InMemoryTransport::default();
        assert!(block(fetch_manifest(&remote)).unwrap().is_none());

        let baseline = match block(upload(
            &remote,
            &db,
            &WebDavSyncStatus::default(),
            false,
            false,
        ))
        .unwrap()
        {
            UploadOutcome::Uploaded(baseline) => baseline,
            other => panic!("expected an upload, got {other:?}"),
        };
        let status = sync_status_from_baseline(baseline);
        let snapshot = block(fetch_manifest(&remote)).unwrap().expect("manifest");
        let info = remote_info(&remote, &status, &snapshot);
        assert_eq!(info["remoteChangedSinceLastSync"], false);
        assert_eq!(info["compatible"], true);
        assert_eq!(info["layout"], RemoteLayout::Current.as_str());
        assert_eq!(info["remotePath"], "/memory");
        assert_eq!(info["insecureTransport"], false);

        // Another device replaced the manifest since this device's last sync.
        let mut manifest = remote.object(REMOTE_MANIFEST).unwrap();
        manifest.extend_from_slice(b" ");
        remote.replace(REMOTE_MANIFEST, manifest);
        let snapshot = block(fetch_manifest(&remote)).unwrap().expect("manifest");
        let info = remote_info(&remote, &status, &snapshot);
        assert_eq!(info["remoteChangedSinceLastSync"], true);
    }

    /// SVC-27: a preview of an identical snapshot is empty; a local edit shows
    /// up as one changed key path; nothing local is modified.
    #[test]
    #[serial_test::serial]
    fn preview_diffs_remote_against_local_without_applying() {
        let _home = TestHome::new();
        let db = crate::database::Database::memory().expect("memory db");
        let remote = InMemoryTransport::default();
        assert!(block(preview(&remote, &db)).unwrap().is_none());

        let mut provider = crate::provider::Provider::with_id(
            "p1".to_string(),
            "P1".to_string(),
            serde_json::json!({ "env": { "ANTHROPIC_BASE_URL": "https://a.example" } }),
            None,
        );
        db.save_provider("claude", &provider).unwrap();
        block(upload(
            &remote,
            &db,
            &WebDavSyncStatus::default(),
            false,
            false,
        ))
        .unwrap();
        let unchanged = block(preview(&remote, &db)).unwrap().expect("preview");
        assert!(unchanged.is_empty(), "{:?}", unchanged.tables);

        provider.settings_config =
            serde_json::json!({ "env": { "ANTHROPIC_BASE_URL": "https://b.example" } });
        db.save_provider("claude", &provider).unwrap();
        let changed = block(preview(&remote, &db)).unwrap().expect("preview");
        assert_eq!(changed.tables.len(), 1);
        assert_eq!(
            changed.tables[0].changed[0].fields,
            vec!["settings_config.env.ANTHROPIC_BASE_URL".to_string()]
        );
        // The local edit is still there: a preview never applies.
        let local = db.get_provider_by_id("p1", "claude").unwrap().unwrap();
        assert_eq!(
            local.settings_config["env"]["ANTHROPIC_BASE_URL"],
            "https://b.example"
        );
    }

    #[test]
    #[serial_test::serial]
    fn download_from_empty_remote_reports_transport_key() {
        let _home = TestHome::new();
        let db = crate::database::Database::memory().expect("memory db");
        let remote = InMemoryTransport::default();
        let err = block(download(&remote, &db, &WebDavSyncStatus::default(), true)).unwrap_err();
        assert!(matches!(
            err,
            AppError::Localized {
                key: "memory.sync.remote_empty",
                ..
            }
        ));
    }
}
