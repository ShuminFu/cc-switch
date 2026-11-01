//! End-to-end tests for the S3 sync protocol against an in-process fake S3
//! server (SVC-24, S3 half; the WebDAV half lives in `webdav_sync.rs`).
//!
//! The fake speaks just enough of the path-style S3 REST API used by
//! `services::s3` (PUT/GET/HEAD object, HEAD bucket) and ignores SigV4, so the
//! tests exercise the real upload/download pipeline: snapshot build, manifest
//! precondition, artifact hash verification and `apply_snapshot`.

use super::*;

mod fake_s3 {
    use axum::body::Bytes;
    use axum::extract::State;
    use axum::http::{Method, StatusCode, Uri};
    use axum::response::{IntoResponse, Response};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    /// `/{bucket}/{key}` -> (body, version). ETag is `"v{version}"`.
    pub type Store = Arc<Mutex<HashMap<String, (Vec<u8>, u64)>>>;

    async fn handle(State(store): State<Store>, method: Method, uri: Uri, body: Bytes) -> Response {
        let path = uri.path().to_string();
        let mut objects = store.lock().unwrap();
        match method.as_str() {
            "PUT" => {
                let next = objects.get(&path).map(|(_, v)| v + 1).unwrap_or(1);
                objects.insert(path, (body.to_vec(), next));
                StatusCode::OK.into_response()
            }
            "GET" | "HEAD" => match objects.get(&path) {
                Some((bytes, v)) => {
                    let body = if method == Method::HEAD {
                        Vec::new()
                    } else {
                        bytes.clone()
                    };
                    (StatusCode::OK, [("etag", format!("\"v{v}\""))], body).into_response()
                }
                // HEAD bucket (test connection) succeeds for any bucket.
                None if method == Method::HEAD
                    && path.trim_end_matches('/').matches('/').count() == 1 =>
                {
                    StatusCode::OK.into_response()
                }
                None => StatusCode::NOT_FOUND.into_response(),
            },
            _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
        }
    }

    /// Returns the endpoint (`http://127.0.0.1:{port}`) and the object store.
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
        (format!("http://{addr}"), store)
    }
}

const BUCKET: &str = "test-bucket";

fn device_settings(endpoint: &str) -> S3SyncSettings {
    S3SyncSettings {
        enabled: true,
        region: "us-east-1".to_string(),
        bucket: BUCKET.to_string(),
        access_key_id: "AKIDTEST".to_string(),
        secret_access_key: "secret".to_string(),
        endpoint: endpoint.to_string(),
        ..S3SyncSettings::default()
    }
}

fn object_path(settings: &S3SyncSettings, artifact: &str) -> String {
    format!("/{BUCKET}/{}", s3_key(settings, artifact))
}

/// Per-test isolated home so `persist_sync_success` writes settings there.
struct TestHome {
    _dir: tempfile::TempDir,
    previous: Option<std::ffi::OsString>,
}

impl TestHome {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("create test home");
        let previous = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
        crate::settings::update_settings(crate::settings::AppSettings::default())
            .expect("reset settings");
        Self {
            _dir: dir,
            previous,
        }
    }
}

impl Drop for TestHome {
    fn drop(&mut self) {
        let _ = crate::settings::update_settings(crate::settings::AppSettings::default());
        match self.previous.take() {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
    }
}

fn seeded_db(provider_id: &str) -> crate::database::Database {
    let db = crate::database::Database::memory().expect("memory db");
    let provider = crate::provider::Provider::with_id(
        provider_id.to_string(),
        provider_id.to_string(),
        serde_json::json!({ "env": { "ANTHROPIC_AUTH_TOKEN": "k" } }),
        None,
    );
    db.save_provider("claude", &provider)
        .expect("seed provider");
    db
}

fn provider_ids(db: &crate::database::Database) -> Vec<String> {
    let mut ids: Vec<String> = db
        .get_all_providers("claude")
        .expect("list providers")
        .keys()
        .cloned()
        .collect();
    ids.sort();
    ids
}

#[tokio::test]
#[serial_test::serial]
async fn upload_then_download_round_trips_providers_to_a_fresh_device() {
    let _home = TestHome::new();
    let (endpoint, store) = fake_s3::start().await;

    let source = seeded_db("from-device-a");
    let mut device_a = device_settings(&endpoint);
    let result = upload(&source, &mut device_a, false).await.expect("upload");
    assert_eq!(result["status"], "uploaded");
    assert_eq!(
        result["insecureTransport"], false,
        "plain http to a loopback endpoint is not flagged as insecure"
    );
    assert!(device_a.status.last_remote_manifest_hash.is_some());

    {
        let objects = store.lock().unwrap();
        for artifact in [REMOTE_MANIFEST, REMOTE_DB_SQL, REMOTE_SKILLS_ZIP] {
            assert!(
                objects.contains_key(&object_path(&device_a, artifact)),
                "missing remote {artifact}: {:?}",
                objects.keys().collect::<Vec<_>>()
            );
        }
    }

    // Remote info is visible before downloading.
    let info = fetch_remote_info(&device_a)
        .await
        .expect("fetch remote info")
        .expect("remote has a snapshot");
    assert_eq!(info["compatible"], true);
    assert_eq!(info["remoteChangedSinceLastSync"], false);

    // A fresh device (its own local data, no baseline) downloads A's snapshot.
    let target = seeded_db("local-only");
    let mut device_b = device_settings(&endpoint);
    let result = download(&target, &mut device_b, false)
        .await
        .expect("download");
    assert_eq!(result["status"], "downloaded");
    assert_eq!(provider_ids(&target), vec!["from-device-a"]);
    assert!(device_b.status.last_local_manifest_hash.is_some());
}

#[tokio::test]
#[serial_test::serial]
async fn tampered_artifact_is_rejected_before_the_local_db_is_touched() {
    let _home = TestHome::new();
    let (endpoint, store) = fake_s3::start().await;

    let mut uploader = device_settings(&endpoint);
    upload(&seeded_db("remote"), &mut uploader, false)
        .await
        .expect("upload");

    // Corrupt the uploaded SQL dump (hash no longer matches the manifest).
    {
        let mut objects = store.lock().unwrap();
        let entry = objects
            .get_mut(&object_path(&uploader, REMOTE_DB_SQL))
            .expect("db.sql uploaded");
        entry.0.extend_from_slice(b"\n-- tampered\n");
    }

    let local = seeded_db("keep-me");
    let mut device = device_settings(&endpoint);
    let err = download(&local, &mut device, false)
        .await
        .expect_err("hash mismatch must be rejected");
    assert!(!err.to_string().is_empty());
    assert_eq!(
        provider_ids(&local),
        vec!["keep-me"],
        "a rejected download must not modify the local database"
    );
    assert!(device.status.last_remote_manifest_hash.is_none());
}

#[tokio::test]
#[serial_test::serial]
async fn missing_remote_snapshot_reports_remote_empty() {
    let _home = TestHome::new();
    let (endpoint, _store) = fake_s3::start().await;

    let mut device = device_settings(&endpoint);
    assert!(fetch_remote_info(&device)
        .await
        .expect("fetch info")
        .is_none());
    let local = seeded_db("only");
    download(&local, &mut device, false)
        .await
        .expect_err("nothing to download");
    assert_eq!(provider_ids(&local), vec!["only"]);
}

#[tokio::test]
#[serial_test::serial]
async fn stale_device_upload_is_refused_unless_forced() {
    let _home = TestHome::new();
    let (endpoint, store) = fake_s3::start().await;
    let db = seeded_db("p");

    let mut device_a = device_settings(&endpoint);
    upload(&db, &mut device_a, false)
        .await
        .expect("first upload allowed");
    let manifest_path = object_path(&device_a, REMOTE_MANIFEST);
    let a_manifest = store.lock().unwrap()[&manifest_path].0.clone();

    // Device B last synced some other snapshot.
    let mut device_b = device_settings(&endpoint);
    device_b.status.last_remote_manifest_hash = Some(sha256_hex(b"older manifest"));
    let err = upload(&db, &mut device_b, false)
        .await
        .expect_err("stale upload must be refused");
    assert!(super::super::sync_protocol::is_sync_conflict(&err), "{err}");
    assert_eq!(store.lock().unwrap()[&manifest_path].0, a_manifest);

    upload(&db, &mut device_b, true)
        .await
        .expect("forced upload allowed");
    assert_ne!(store.lock().unwrap()[&manifest_path].0, a_manifest);
}
