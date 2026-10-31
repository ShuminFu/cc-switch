//! WebDAV v2 sync protocol layer with DB compatibility subdirectories.
//!
//! Implements manifest-based synchronization on top of the HTTP transport
//! primitives in [`super::webdav`]. Artifact set: `db.sql` + `skills.zip`.

use std::future::Future;
use std::sync::OnceLock;

use serde_json::Value;

use crate::error::AppError;
use crate::services::webdav::{
    auth_from_credentials, build_remote_url, ensure_remote_directories, get_bytes, head_etag,
    path_segments, put_bytes_if_match, test_connection, PutOutcome, WebDavAuth,
};
use crate::settings::{update_webdav_sync_status, WebDavSyncSettings};

use super::sync_transport::{
    self, PutResult, RemoteSnapshot, SyncTransport, TransportKeys, UploadOutcome,
};

use super::sync_protocol::{
    ensure_local_unchanged, is_insecure_remote_url, persist_sync_success_best_effort,
    sync_status_from_baseline, warn_if_insecure, RemoteLayout, SyncBaseline, DB_COMPAT_VERSION,
    PROTOCOL_VERSION,
};
#[cfg(test)]
use super::sync_protocol::{sha256_hex, SyncManifest, REMOTE_DB_SQL};

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

// ─── Public API ──────────────────────────────────────────────

/// Check WebDAV connectivity and ensure remote directory structure.
pub async fn check_connection(settings: &WebDavSyncSettings) -> Result<(), AppError> {
    settings.validate()?;
    crate::services::sync_protocol::validate_remote_layout(
        &settings.remote_root,
        &settings.profile,
    )?;
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
    upload_inner(db, settings, force, false).await
}

/// Auto-sync upload: like `upload(.., force = false)` but a no-op when the
/// local content is unchanged since the last successful sync (SVC-52), so a
/// stale change signal never republishes the same snapshot under this
/// device's name.
pub(crate) async fn upload_if_changed(
    db: &crate::database::Database,
    settings: &mut WebDavSyncSettings,
) -> Result<Value, AppError> {
    upload_inner(db, settings, false, true).await
}

async fn upload_inner(
    db: &crate::database::Database,
    settings: &mut WebDavSyncSettings,
    force: bool,
    skip_if_unchanged: bool,
) -> Result<Value, AppError> {
    settings.validate()?;
    crate::services::sync_protocol::validate_remote_layout(
        &settings.remote_root,
        &settings.profile,
    )?;
    let insecure = warn_if_insecure("WebDAV", &settings.base_url);
    let transport = WebDavTransport::new(settings);
    let outcome =
        sync_transport::upload(&transport, db, &settings.status, force, skip_if_unchanged).await?;
    let baseline = match outcome {
        UploadOutcome::Unchanged => return Ok(serde_json::json!({ "status": "unchanged" })),
        UploadOutcome::Uploaded(baseline) => baseline,
    };
    let _persisted = persist_sync_success_best_effort(settings, baseline, persist_sync_success);
    Ok(serde_json::json!({ "status": "uploaded", "insecureTransport": insecure }))
}

// ─── Transport ───────────────────────────────────────────────

static WEBDAV_KEYS: TransportKeys = TransportKeys {
    remote_empty: "webdav.sync.remote_empty",
    manifest_missing_artifact: "webdav.sync.manifest_missing_artifact",
    remote_missing_artifact: "webdav.sync.remote_missing_artifact",
};

/// WebDAV implementation of the shared sync engine's transport (SVC-19).
/// Uploads always use the current layout; downloads and remote info also
/// read the legacy (pre-`db-v*`) layout through a second transport.
struct WebDavTransport<'a> {
    settings: &'a WebDavSyncSettings,
    auth: WebDavAuth,
    layout: RemoteLayout,
}

impl<'a> WebDavTransport<'a> {
    fn new(settings: &'a WebDavSyncSettings) -> Self {
        Self::with_layout(settings, RemoteLayout::Current)
    }

    fn with_layout(settings: &'a WebDavSyncSettings, layout: RemoteLayout) -> Self {
        Self {
            settings,
            auth: auth_for(settings),
            layout,
        }
    }

    fn url(&self, name: &str) -> Result<String, AppError> {
        remote_file_url(self.settings, self.layout, name)
    }
}

impl SyncTransport for WebDavTransport<'_> {
    fn label(&self) -> &'static str {
        "WebDAV"
    }
    fn keys(&self) -> &'static TransportKeys {
        &WEBDAV_KEYS
    }
    async fn ensure_root(&self) -> Result<(), AppError> {
        let dir_segs = remote_dir_segments(self.settings, self.layout);
        ensure_remote_directories(&self.settings.base_url, &dir_segs, &self.auth).await
    }
    async fn get(&self, name: &str, max_bytes: usize) -> Result<sync_transport::Fetched, AppError> {
        get_bytes(&self.url(name)?, &self.auth, max_bytes).await
    }
    async fn put(
        &self,
        name: &str,
        bytes: Vec<u8>,
        content_type: &str,
        if_match: Option<&str>,
    ) -> Result<PutResult, AppError> {
        Ok(
            match put_bytes_if_match(&self.url(name)?, &self.auth, bytes, content_type, if_match)
                .await?
            {
                PutOutcome::Stored => PutResult::Stored,
                PutOutcome::PreconditionFailed => PutResult::PreconditionFailed,
            },
        )
    }
    async fn head(&self, name: &str) -> Result<Option<String>, AppError> {
        head_etag(&self.url(name)?, &self.auth).await
    }
    fn layout(&self) -> RemoteLayout {
        self.layout
    }
    fn remote_path(&self) -> String {
        remote_dir_display(self.settings, self.layout)
    }
    fn insecure(&self) -> bool {
        is_insecure_remote_url(&self.settings.base_url)
    }
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
    crate::services::sync_protocol::validate_remote_layout(
        &settings.remote_root,
        &settings.profile,
    )?;
    let insecure = warn_if_insecure("WebDAV", &settings.base_url);
    ensure_local_unchanged(db, &settings.status, force)?;
    let (transport, snapshot) = find_remote_snapshot(settings)
        .await?
        .ok_or_else(|| sync_transport::remote_empty_error(&WebDavTransport::new(settings)))?;
    let layout = snapshot.layout;
    let baseline = sync_transport::download_snapshot(&transport, db, snapshot).await?;
    let source_path = transport.remote_path();
    drop(transport);

    let _persisted = persist_sync_success_best_effort(settings, baseline, persist_sync_success);
    Ok(serde_json::json!({
        "status": "downloaded",
        "insecureTransport": insecure,
        "sourceLayout": layout.as_str(),
        "sourcePath": source_path,
    }))
}

/// Dry-run a download (SVC-27): what the remote snapshot would change
/// locally, without applying it. `None` when the remote has no snapshot.
/// Exposed as the `preview_sync_download` command.
pub(crate) async fn preview(
    db: &crate::database::Database,
    settings: &WebDavSyncSettings,
) -> Result<Option<sync_transport::SyncPreview>, AppError> {
    settings.validate()?;
    crate::services::sync_protocol::validate_remote_layout(
        &settings.remote_root,
        &settings.profile,
    )?;
    let Some((transport, snapshot)) = find_remote_snapshot(settings).await? else {
        return Ok(None);
    };
    sync_transport::preview_snapshot(&transport, db, &snapshot)
        .await
        .map(Some)
}

/// Fetch remote manifest info without downloading artifacts.
pub async fn fetch_remote_info(settings: &WebDavSyncSettings) -> Result<Option<Value>, AppError> {
    settings.validate()?;
    crate::services::sync_protocol::validate_remote_layout(
        &settings.remote_root,
        &settings.profile,
    )?;
    let Some((transport, snapshot)) = find_remote_snapshot(settings).await? else {
        return Ok(None);
    };
    Ok(Some(sync_transport::remote_info(
        &transport,
        &settings.status,
        &snapshot,
    )))
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

/// The manifest of the current layout, else of the legacy layout, with the
/// transport that addresses it.
async fn find_remote_snapshot(
    settings: &WebDavSyncSettings,
) -> Result<Option<(WebDavTransport<'_>, RemoteSnapshot)>, AppError> {
    for layout in [RemoteLayout::Current, RemoteLayout::Legacy] {
        let transport = WebDavTransport::with_layout(settings, layout);
        if let Some(snapshot) = sync_transport::fetch_manifest(&transport).await? {
            return Ok(Some((transport, snapshot)));
        }
    }
    Ok(None)
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
        put_bytes_if_match(&url, &auth, b"one".to_vec(), "application/json", None)
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
