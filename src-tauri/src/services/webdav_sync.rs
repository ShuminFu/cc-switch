//! WebDAV v2 sync protocol layer with DB compatibility subdirectories.
//!
//! Implements manifest-based synchronization on top of the HTTP transport
//! primitives in [`super::webdav`]. Artifact set: `db.sql` + `skills.zip`.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::OnceLock;

use serde_json::Value;

use crate::error::AppError;
use crate::services::webdav::{
    auth_from_credentials, build_remote_url, ensure_remote_directories, get_bytes, head_etag,
    path_segments, put_bytes, put_bytes_if_match, test_connection, PutOutcome, WebDavAuth,
};
use crate::settings::{update_webdav_sync_status, WebDavSyncSettings};

use super::sync_protocol::{
    apply_snapshot, build_snapshot, capture_local_fingerprint, download_passphrase,
    effective_db_compat_version, ensure_local_unchanged, ensure_no_encryption_downgrade,
    evaluate_upload_precondition, is_insecure_remote_url, localized, open_artifact,
    persist_sync_success_best_effort, remote_changed_error, remote_encryption_info, sha256_hex,
    sync_status_from_baseline, unlock_remote, upload_key, upload_passphrase,
    validate_artifact_size_limit, validate_manifest_compat, verify_artifact, warn_if_insecure,
    ArtifactMeta, RemoteLayout, RemoteManifestView, SyncBaseline, SyncManifest, DB_COMPAT_VERSION,
    MAX_MANIFEST_BYTES, MAX_SYNC_ARTIFACT_BYTES, PROTOCOL_VERSION, REMOTE_DB_SQL, REMOTE_MANIFEST,
    REMOTE_SKILLS_ZIP,
};

pub(crate) mod archive;

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

struct RemoteSnapshot {
    layout: RemoteLayout,
    manifest: SyncManifest,
    manifest_bytes: Vec<u8>,
    manifest_etag: Option<String>,
}
// ─── Public API ──────────────────────────────────────────────

/// Check WebDAV connectivity and ensure remote directory structure.
pub async fn check_connection(settings: &WebDavSyncSettings) -> Result<(), AppError> {
    settings.validate()?;
    let auth = auth_for(settings);
    test_connection(&settings.base_url, &auth).await?;
    let dir_segs = remote_dir_segments(settings, RemoteLayout::Current);
    ensure_remote_directories(&settings.base_url, &dir_segs, &auth).await?;
    Ok(())
}

/// Upload local snapshot (db + skills) to remote.
///
/// Unless `force`, the upload is refused with a distinct "remote changed"
/// conflict error (see [`super::sync_protocol::is_sync_conflict`]) when the
/// remote manifest differs from the one seen at the last successful sync.
/// The manifest PUT carries `If-Match` with the ETag that was just checked.
pub async fn upload(
    db: &crate::database::Database,
    settings: &mut WebDavSyncSettings,
    force: bool,
) -> Result<Value, AppError> {
    settings.validate()?;
    let insecure = warn_if_insecure("WebDAV", &settings.base_url);
    let auth = auth_for(settings);
    let dir_segs = remote_dir_segments(settings, RemoteLayout::Current);
    ensure_remote_directories(&settings.base_url, &dir_segs, &auth).await?;

    // SEC-06 / SVC-04: encrypt the artifacts when sync encryption is enabled.
    let encryption = upload_passphrase()?
        .map(|passphrase| upload_key(&passphrase))
        .transpose()?;
    let snapshot = build_snapshot(db, encryption.as_ref())?;
    let local_fingerprint = snapshot.local_fingerprint.clone();

    // Precondition: the remote must still be what this device last synced.
    let manifest_url = remote_file_url(settings, RemoteLayout::Current, REMOTE_MANIFEST)?;
    let remote = get_bytes(&manifest_url, &auth, MAX_MANIFEST_BYTES).await?;
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
        evaluate_upload_precondition(&settings.status, view.as_ref(), force)?
    };

    // Upload order: artifacts first, manifest last (best-effort consistency)
    let db_url = remote_file_url(settings, RemoteLayout::Current, REMOTE_DB_SQL)?;
    put_bytes(&db_url, &auth, snapshot.db_sql, "application/sql").await?;

    let skills_url = remote_file_url(settings, RemoteLayout::Current, REMOTE_SKILLS_ZIP)?;
    put_bytes(&skills_url, &auth, snapshot.skills_zip, "application/zip").await?;

    let outcome = put_bytes_if_match(
        &manifest_url,
        &auth,
        snapshot.manifest_bytes,
        "application/json",
        if_match.as_deref(),
    )
    .await?;
    if outcome == PutOutcome::PreconditionFailed {
        log::warn!("[WebDAV] Manifest changed during upload (412); refusing to overwrite");
        return Err(remote_changed_error(None));
    }

    // Fetch etag (best-effort, don't fail the upload)
    let etag = match head_etag(&manifest_url, &auth).await {
        Ok(e) => e,
        Err(e) => {
            log::debug!("[WebDAV] Failed to fetch ETag after upload: {e}");
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
    settings: &mut WebDavSyncSettings,
    force: bool,
) -> Result<Value, AppError> {
    settings.validate()?;
    let insecure = warn_if_insecure("WebDAV", &settings.base_url);
    ensure_local_unchanged(db, &settings.status, force)?;
    let auth = auth_for(settings);
    let snapshot = find_remote_snapshot(settings, &auth)
        .await?
        .ok_or_else(|| {
            localized(
                "webdav.sync.remote_empty",
                "远端没有可下载的同步数据",
                "No downloadable sync data found on the remote.",
            )
        })?;

    validate_manifest_compat(&snapshot.manifest, snapshot.layout)?;
    // Encrypted snapshot: check the passphrase before downloading artifacts.
    let key = unlock_remote(&snapshot.manifest, download_passphrase().as_deref())?;

    // Download and verify artifacts
    let db_sql = download_and_verify(
        settings,
        &auth,
        snapshot.layout,
        REMOTE_DB_SQL,
        &snapshot.manifest.artifacts,
    )
    .await?;
    let skills_zip = download_and_verify(
        settings,
        &auth,
        snapshot.layout,
        REMOTE_SKILLS_ZIP,
        &snapshot.manifest.artifacts,
    )
    .await?;
    let db_sql = open_artifact(key.as_ref(), REMOTE_DB_SQL, db_sql)?;
    let skills_zip = open_artifact(key.as_ref(), REMOTE_SKILLS_ZIP, skills_zip)?;

    // Apply snapshot
    apply_snapshot(db, &db_sql, &skills_zip)?;

    let manifest_hash = sha256_hex(&snapshot.manifest_bytes);
    let _persisted = persist_sync_success_best_effort(
        settings,
        SyncBaseline {
            manifest_hash,
            etag: snapshot.manifest_etag,
            local_fingerprint: capture_local_fingerprint(db),
        },
        persist_sync_success,
    );
    Ok(serde_json::json!({
        "status": "downloaded",
        "insecureTransport": insecure,
        "sourceLayout": snapshot.layout.as_str(),
        "sourcePath": remote_dir_display(settings, snapshot.layout),
    }))
}

/// Fetch remote manifest info without downloading artifacts.
pub async fn fetch_remote_info(settings: &WebDavSyncSettings) -> Result<Option<Value>, AppError> {
    settings.validate()?;
    let auth = auth_for(settings);
    let Some(snapshot) = find_remote_snapshot(settings, &auth).await? else {
        return Ok(None);
    };
    let compatible = validate_manifest_compat(&snapshot.manifest, snapshot.layout).is_ok();
    let db_compat_version = effective_db_compat_version(&snapshot.manifest, snapshot.layout);
    let remote_changed = evaluate_upload_precondition(
        &settings.status,
        Some(&RemoteManifestView {
            bytes: &snapshot.manifest_bytes,
            etag: snapshot.manifest_etag.as_deref(),
            manifest: None,
        }),
        false,
    )
    .is_err();
    let encryption = remote_encryption_info(&snapshot.manifest);

    let payload = serde_json::json!({
        "remoteChangedSinceLastSync": remote_changed,
        "insecureTransport": is_insecure_remote_url(&settings.base_url),
        "encrypted": encryption.encrypted,
        "passphraseMatches": encryption.passphrase_matches,
        "deviceName": snapshot.manifest.device_name,
        "createdAt": snapshot.manifest.created_at,
        "snapshotId": snapshot.manifest.snapshot_id,
        "version": snapshot.manifest.version,
        "protocolVersion": snapshot.manifest.version,
        "dbCompatVersion": db_compat_version,
        "compatible": compatible,
        "artifacts": snapshot.manifest.artifacts.keys().collect::<Vec<_>>(),
        "layout": snapshot.layout.as_str(),
        "remotePath": remote_dir_display(settings, snapshot.layout),
    });

    Ok(Some(payload))
}

// ─── Sync status persistence ─────────────────────────────────

fn persist_sync_success(
    settings: &mut WebDavSyncSettings,
    baseline: SyncBaseline,
) -> Result<(), AppError> {
    let status = sync_status_from_baseline(baseline);
    settings.status = status.clone();
    update_webdav_sync_status(status)
}

async fn find_remote_snapshot(
    settings: &WebDavSyncSettings,
    auth: &WebDavAuth,
) -> Result<Option<RemoteSnapshot>, AppError> {
    if let Some(snapshot) = fetch_remote_snapshot(settings, auth, RemoteLayout::Current).await? {
        return Ok(Some(snapshot));
    }
    fetch_remote_snapshot(settings, auth, RemoteLayout::Legacy).await
}

async fn fetch_remote_snapshot(
    settings: &WebDavSyncSettings,
    auth: &WebDavAuth,
    layout: RemoteLayout,
) -> Result<Option<RemoteSnapshot>, AppError> {
    let manifest_url = remote_file_url(settings, layout, REMOTE_MANIFEST)?;
    let Some((manifest_bytes, manifest_etag)) =
        get_bytes(&manifest_url, auth, MAX_MANIFEST_BYTES).await?
    else {
        return Ok(None);
    };

    let manifest: SyncManifest =
        serde_json::from_slice(&manifest_bytes).map_err(|e| AppError::Json {
            path: REMOTE_MANIFEST.to_string(),
            source: e,
        })?;

    Ok(Some(RemoteSnapshot {
        layout,
        manifest,
        manifest_bytes,
        manifest_etag,
    }))
}
// ─── Download & verify ───────────────────────────────────────

async fn download_and_verify(
    settings: &WebDavSyncSettings,
    auth: &WebDavAuth,
    layout: RemoteLayout,
    artifact_name: &str,
    artifacts: &BTreeMap<String, ArtifactMeta>,
) -> Result<Vec<u8>, AppError> {
    let meta = artifacts.get(artifact_name).ok_or_else(|| {
        localized(
            "webdav.sync.manifest_missing_artifact",
            format!("manifest 中缺少 artifact: {artifact_name}"),
            format!("Manifest missing artifact: {artifact_name}"),
        )
    })?;
    validate_artifact_size_limit(artifact_name, meta.size)?;

    let url = remote_file_url(settings, layout, artifact_name)?;
    let (bytes, _) = get_bytes(&url, auth, MAX_SYNC_ARTIFACT_BYTES as usize)
        .await?
        .ok_or_else(|| {
            localized(
                "webdav.sync.remote_missing_artifact",
                format!("远端缺少 artifact 文件: {artifact_name}"),
                format!("Remote artifact file missing: {artifact_name}"),
            )
        })?;

    verify_artifact(&bytes, artifact_name, meta)?;
    Ok(bytes)
}

// ─── Remote path helpers ─────────────────────────────────────

fn remote_dir_segments(settings: &WebDavSyncSettings, layout: RemoteLayout) -> Vec<String> {
    let mut segs = Vec::new();
    segs.extend(path_segments(&settings.remote_root).map(str::to_string));
    segs.push(format!("v{PROTOCOL_VERSION}"));
    if layout == RemoteLayout::Current {
        segs.push(format!("db-v{DB_COMPAT_VERSION}"));
    }
    segs.extend(path_segments(&settings.profile).map(str::to_string));
    segs
}

fn remote_file_url(
    settings: &WebDavSyncSettings,
    layout: RemoteLayout,
    file_name: &str,
) -> Result<String, AppError> {
    let mut segs = remote_dir_segments(settings, layout);
    segs.extend(path_segments(file_name).map(str::to_string));
    build_remote_url(&settings.base_url, &segs)
}

fn remote_dir_display(settings: &WebDavSyncSettings, layout: RemoteLayout) -> String {
    let segs = remote_dir_segments(settings, layout);
    format!("/{}", segs.join("/"))
}

fn auth_for(settings: &WebDavSyncSettings) -> WebDavAuth {
    auth_from_credentials(&settings.username, &settings.password)
}

// ─── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_dir_segments_uses_current_layout() {
        let settings = WebDavSyncSettings {
            remote_root: "cc-switch-sync".to_string(),
            profile: "default".to_string(),
            ..WebDavSyncSettings::default()
        };
        let segs = remote_dir_segments(&settings, RemoteLayout::Current);
        assert_eq!(segs, vec!["cc-switch-sync", "v2", "db-v6", "default"]);
    }

    #[test]
    fn remote_dir_segments_uses_legacy_layout() {
        let settings = WebDavSyncSettings {
            remote_root: "cc-switch-sync".to_string(),
            profile: "default".to_string(),
            ..WebDavSyncSettings::default()
        };
        let segs = remote_dir_segments(&settings, RemoteLayout::Legacy);
        assert_eq!(segs, vec!["cc-switch-sync", "v2", "default"]);
    }

    // ── Fake WebDAV server: conflict safety end-to-end (SEC-28 / SVC-02) ──

    mod fake_server {
        use axum::body::Bytes;
        use axum::extract::State;
        use axum::http::{HeaderMap, Method, StatusCode, Uri};
        use axum::response::{IntoResponse, Response};
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};

        /// path -> (body, version). ETag is `"v{version}"`.
        pub type Store = Arc<Mutex<HashMap<String, (Vec<u8>, u64)>>>;

        async fn handle(
            State(store): State<Store>,
            method: Method,
            uri: Uri,
            headers: HeaderMap,
            body: Bytes,
        ) -> Response {
            let path = uri.path().to_string();
            let mut files = store.lock().unwrap();
            match method.as_str() {
                "MKCOL" => StatusCode::CREATED.into_response(),
                "PROPFIND" => StatusCode::MULTI_STATUS.into_response(),
                "PUT" => {
                    let current = files.get(&path).map(|(_, v)| format!("\"v{v}\""));
                    if let Some(expected) = headers.get("if-match").and_then(|v| v.to_str().ok()) {
                        if current.as_deref() != Some(expected) {
                            return StatusCode::PRECONDITION_FAILED.into_response();
                        }
                    }
                    let next = files.get(&path).map(|(_, v)| v + 1).unwrap_or(1);
                    files.insert(path, (body.to_vec(), next));
                    StatusCode::CREATED.into_response()
                }
                "GET" | "HEAD" => match files.get(&path) {
                    Some((bytes, v)) => {
                        let body = if method == Method::HEAD {
                            Vec::new()
                        } else {
                            bytes.clone()
                        };
                        (StatusCode::OK, [("etag", format!("\"v{v}\""))], body).into_response()
                    }
                    None => StatusCode::NOT_FOUND.into_response(),
                },
                _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
            }
        }

        pub async fn start() -> (String, Store) {
            let store: Store = Arc::default();
            let app = axum::Router::new()
                .fallback(handle)
                .with_state(store.clone());
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            (format!("http://{addr}/dav/"), store)
        }
    }

    fn device_settings(base_url: &str) -> WebDavSyncSettings {
        WebDavSyncSettings {
            enabled: true,
            base_url: base_url.to_string(),
            username: "alice".to_string(),
            password: "secret".to_string(),
            remote_root: "cc-switch-sync".to_string(),
            profile: "default".to_string(),
            ..WebDavSyncSettings::default()
        }
    }

    fn remote_manifest(store: &fake_server::Store) -> Vec<u8> {
        store
            .lock()
            .unwrap()
            .get("/dav/cc-switch-sync/v2/db-v6/default/manifest.json")
            .map(|(bytes, _)| bytes.clone())
            .expect("remote manifest present")
    }

    fn isolate_test_home(name: &str) {
        let test_home = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&test_home);
        std::fs::create_dir_all(&test_home).expect("create test home");
        std::env::set_var("CC_SWITCH_TEST_HOME", &test_home);
        crate::settings::update_settings(crate::settings::AppSettings::default())
            .expect("reset settings");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn stale_device_upload_is_refused_and_remote_keeps_other_snapshot() {
        isolate_test_home("cc-switch-webdav-conflict-upload-test");
        let (base_url, store) = fake_server::start().await;
        let db = crate::database::Database::memory().expect("memory db");

        // Device A uploads first (no baseline yet: first sync is allowed).
        let mut device_a = device_settings(&base_url);
        upload(&db, &mut device_a, false)
            .await
            .expect("first upload allowed");
        let a_manifest = remote_manifest(&store);
        assert!(device_a.status.last_remote_manifest_hash.is_some());

        // Device B last synced an older snapshot, so its baseline is stale.
        let mut device_b = device_settings(&base_url);
        device_b.status.last_remote_manifest_hash = Some(sha256_hex(b"older manifest"));
        device_b.status.last_remote_etag = Some("\"v0\"".to_string());
        let err = upload(&db, &mut device_b, false)
            .await
            .expect_err("stale upload must be refused");
        assert!(super::super::sync_protocol::is_sync_conflict(&err), "{err}");
        assert_eq!(remote_manifest(&store), a_manifest, "remote must keep A");

        // An explicit overwrite still works...
        upload(&db, &mut device_b, true)
            .await
            .expect("forced upload allowed");
        assert_ne!(remote_manifest(&store), a_manifest);

        // ...and now A is the stale one.
        let err = upload(&db, &mut device_a, false)
            .await
            .expect_err("A must see B's newer snapshot as a conflict");
        assert!(super::super::sync_protocol::is_sync_conflict(&err));

        // B is up to date and may keep uploading.
        upload(&db, &mut device_b, false)
            .await
            .expect("up-to-date device may upload");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn manifest_put_with_stale_if_match_is_rejected() {
        let (base_url, store) = fake_server::start().await;
        let url = format!("{base_url}file.json");
        let auth = auth_from_credentials("alice", "secret");
        put_bytes(&url, &auth, b"one".to_vec(), "application/json")
            .await
            .unwrap();
        let outcome = put_bytes_if_match(
            &url,
            &auth,
            b"two".to_vec(),
            "application/json",
            Some("\"v0\""),
        )
        .await
        .unwrap();
        assert_eq!(outcome, PutOutcome::PreconditionFailed);
        let outcome = put_bytes_if_match(
            &url,
            &auth,
            b"two".to_vec(),
            "application/json",
            Some("\"v1\""),
        )
        .await
        .unwrap();
        assert_eq!(outcome, PutOutcome::Stored);
        assert_eq!(
            store.lock().unwrap().get("/dav/file.json").unwrap().0,
            b"two".to_vec()
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn download_refuses_to_overwrite_local_changes_unless_forced() {
        isolate_test_home("cc-switch-webdav-conflict-download-test");
        let (base_url, _store) = fake_server::start().await;
        let db = crate::database::Database::memory().expect("memory db");
        // Import validation requires at least one provider or MCP server.
        let provider = crate::provider::Provider::with_id(
            "p1".to_string(),
            "Provider".to_string(),
            serde_json::json!({ "env": { "API_KEY": "k" } }),
            None,
        );
        db.save_provider("claude", &provider)
            .expect("seed provider");

        let mut uploader = device_settings(&base_url);
        upload(&db, &mut uploader, false).await.expect("upload");

        // Fresh device: no local baseline, download allowed.
        let mut device = device_settings(&base_url);
        download(&db, &mut device, false)
            .await
            .expect("first download allowed");
        assert!(device
            .status
            .last_local_manifest_hash
            .as_deref()
            .is_some_and(|v| v.starts_with(super::super::sync_protocol::LOCAL_FINGERPRINT_PREFIX)));

        // Unchanged local data: re-download is allowed (no false positive).
        download(&db, &mut device, false)
            .await
            .expect("clean local may download again");

        // Local edit since the last sync: refused unless forced.
        db.set_setting("sync_conflict_test_key", "local edit")
            .expect("local edit");
        let err = download(&db, &mut device, false)
            .await
            .expect_err("dirty local must be refused");
        assert!(super::super::sync_protocol::is_sync_conflict(&err), "{err}");
        download(&db, &mut device, true)
            .await
            .expect("forced download allowed");
    }

    // ── End-to-end encryption (SEC-06 / SVC-04) ──

    fn set_passphrase(passphrase: Option<&str>) {
        crate::settings::set_sync_encryption_settings(passphrase.map(|p| {
            crate::settings::SyncEncryptionSettings {
                enabled: true,
                passphrase: p.to_string(),
                passphrase_in_keychain: false,
            }
        }))
        .expect("set sync encryption");
    }

    fn remote_file(store: &fake_server::Store, name: &str) -> Vec<u8> {
        store
            .lock()
            .unwrap()
            .get(&format!("/dav/cc-switch-sync/v2/db-v6/default/{name}"))
            .map(|(bytes, _)| bytes.clone())
            .unwrap_or_else(|| panic!("remote {name} present"))
    }

    fn error_key(err: &AppError) -> Option<&'static str> {
        match err {
            AppError::Localized { key, .. } => Some(*key),
            _ => None,
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn encrypted_snapshot_round_trip_and_passphrase_errors() {
        use super::super::sync_protocol::{
            ENCRYPTED_PROTOCOL_VERSION, ENCRYPTION_DOWNGRADE_KEY,
            ENCRYPTION_PASSPHRASE_REQUIRED_KEY, ENCRYPTION_WRONG_PASSPHRASE_KEY,
        };
        isolate_test_home("cc-switch-webdav-encrypted-test");
        let (base_url, store) = fake_server::start().await;
        let db = crate::database::Database::memory().expect("memory db");
        let provider = crate::provider::Provider::with_id(
            "p1".to_string(),
            "Provider".to_string(),
            serde_json::json!({ "env": { "ANTHROPIC_AUTH_TOKEN": "sk-very-secret-key" } }),
            None,
        );
        db.save_provider("claude", &provider)
            .expect("seed provider");

        set_passphrase(Some("correct horse battery staple"));
        let mut uploader = device_settings(&base_url);
        upload(&db, &mut uploader, false)
            .await
            .expect("encrypted upload");

        // The share holds ciphertext only.
        let db_sql = remote_file(&store, REMOTE_DB_SQL);
        assert!(crate::secrets::envelope::is_sealed(&db_sql));
        assert!(!db_sql.windows(18).any(|w| w == b"sk-very-secret-key"));
        let manifest: SyncManifest =
            serde_json::from_slice(&remote_manifest(&store)).expect("manifest");
        assert_eq!(manifest.version, ENCRYPTED_PROTOCOL_VERSION);
        assert!(manifest.encryption.is_some());

        let info = fetch_remote_info(&uploader).await.unwrap().unwrap();
        assert_eq!(info["encrypted"], serde_json::json!(true));
        assert_eq!(info["passphraseMatches"], serde_json::json!(true));
        assert_eq!(info["compatible"], serde_json::json!(true));

        // Same passphrase on another device: downloads and decrypts.
        let mut device = device_settings(&base_url);
        download(&db, &mut device, true)
            .await
            .expect("download with passphrase");
        assert!(db
            .get_provider_by_id("p1", "claude")
            .expect("query")
            .is_some());

        // Wrong passphrase: clear, distinct error.
        set_passphrase(Some("wrong passphrase!!"));
        let err = download(&db, &mut device, true).await.unwrap_err();
        assert_eq!(
            error_key(&err),
            Some(ENCRYPTION_WRONG_PASSPHRASE_KEY),
            "{err}"
        );
        let info = fetch_remote_info(&device).await.unwrap().unwrap();
        assert_eq!(info["passphraseMatches"], serde_json::json!(false));

        // No passphrase at all.
        set_passphrase(None);
        let err = download(&db, &mut device, true).await.unwrap_err();
        assert_eq!(
            error_key(&err),
            Some(ENCRYPTION_PASSPHRASE_REQUIRED_KEY),
            "{err}"
        );

        // Encryption off: an automatic (non-forced) plaintext upload must not
        // replace the encrypted snapshot; a forced manual upload may.
        let mut plain = device_settings(&base_url);
        plain.status = uploader.status.clone();
        let err = upload(&db, &mut plain, false).await.unwrap_err();
        assert_eq!(error_key(&err), Some(ENCRYPTION_DOWNGRADE_KEY), "{err}");
        upload(&db, &mut plain, true)
            .await
            .expect("forced plaintext upload");
        assert!(!crate::secrets::envelope::is_sealed(&remote_file(
            &store,
            REMOTE_DB_SQL
        )));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn plaintext_snapshot_still_downloads_with_encryption_enabled() {
        isolate_test_home("cc-switch-webdav-plaintext-compat-test");
        let (base_url, _store) = fake_server::start().await;
        let db = crate::database::Database::memory().expect("memory db");
        let provider = crate::provider::Provider::with_id(
            "p1".to_string(),
            "Provider".to_string(),
            serde_json::json!({ "env": { "API_KEY": "k" } }),
            None,
        );
        db.save_provider("claude", &provider)
            .expect("seed provider");

        // Snapshot uploaded as by an older version (no encryption).
        let mut old_device = device_settings(&base_url);
        upload(&db, &mut old_device, false)
            .await
            .expect("plain upload");

        set_passphrase(Some("a new passphrase"));
        let mut device = device_settings(&base_url);
        let info = fetch_remote_info(&device).await.unwrap().unwrap();
        assert_eq!(info["encrypted"], serde_json::json!(false));
        download(&db, &mut device, true)
            .await
            .expect("plaintext snapshots stay readable");
        set_passphrase(None);
    }
}
