//! One auto-sync worker for every cloud-sync transport (SVC-19, SVC-52).
//!
//! Database changes to synced tables enqueue a "dirty" signal. A single
//! worker debounces the signals and then uploads to each enabled transport
//! (WebDAV, S3) in turn. Previously each transport had its own copy of this
//! module, its own channel and its own suppression counter, so a WebDAV
//! download did not suppress the S3 auto-upload (and vice versa).
//!
//! Suppression (SVC-52): a download creates an [`AutoSyncSuppressionGuard`].
//! Every guard bumps a global epoch, and every signal carries the epoch it was
//! enqueued under. A batch whose newest signal predates the current epoch, or
//! that is due while a guard is alive, is dropped instead of re-uploading the
//! snapshot that was just downloaded under this device's name. In addition,
//! a transport whose local content is unchanged since its last sync is not
//! re-uploaded.
//!
//! Deferred signals (SEC-36): a change reported while a guard is alive is not
//! discarded. It sets a "deferred" flag, and when the last guard drops exactly
//! one signal is enqueued under the new epoch. If the only writes during the
//! window were the import's own, the upload finds the content unchanged since
//! the download and is skipped; a user edit made during the download is
//! uploaded.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde_json::json;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::mpsc::{channel, Receiver, Sender};

use crate::error::AppError;
use crate::services::sync_protocol::is_sync_conflict;
use crate::settings::{self, WebDavSyncStatus};

const AUTO_SYNC_DEBOUNCE_MS: u64 = 1000;
pub(crate) const MAX_AUTO_SYNC_WAIT_MS: u64 = 10_000;

/// A change signal: the table that changed and the suppression epoch at the
/// time it was enqueued.
pub(crate) type ChangeSignal = (String, usize);

static DB_CHANGE_TX: OnceLock<Sender<ChangeSignal>> = OnceLock::new();
static AUTO_SYNC_SUPPRESS_DEPTH: AtomicUsize = AtomicUsize::new(0);
static AUTO_SYNC_SUPPRESS_EPOCH: AtomicUsize = AtomicUsize::new(0);
/// A synced table changed while a guard was alive (SEC-36).
static AUTO_SYNC_DEFERRED_SIGNAL: AtomicBool = AtomicBool::new(false);

/// Suppresses auto-sync uploads while alive (used around downloads/imports).
pub(crate) struct AutoSyncSuppressionGuard {
    /// Test hook: where the deferred signal goes instead of the global worker.
    #[cfg(test)]
    sender: Option<Sender<ChangeSignal>>,
}

impl AutoSyncSuppressionGuard {
    pub fn new() -> Self {
        AUTO_SYNC_SUPPRESS_EPOCH.fetch_add(1, Ordering::SeqCst);
        AUTO_SYNC_SUPPRESS_DEPTH.fetch_add(1, Ordering::SeqCst);
        Self {
            #[cfg(test)]
            sender: None,
        }
    }

    #[cfg(test)]
    fn with_sender(sender: Sender<ChangeSignal>) -> Self {
        let mut guard = Self::new();
        guard.sender = Some(sender);
        guard
    }
}

impl Drop for AutoSyncSuppressionGuard {
    fn drop(&mut self) {
        let previous =
            AUTO_SYNC_SUPPRESS_DEPTH.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
                Some(value.saturating_sub(1))
            });
        if previous == Ok(1) {
            #[cfg(test)]
            let sender = self.sender.as_ref().or_else(|| DB_CHANGE_TX.get());
            #[cfg(not(test))]
            let sender = DB_CHANGE_TX.get();
            flush_deferred_signal(sender);
        }
    }
}

/// Enqueue the one signal deferred while suppressed, if any (SEC-36).
fn flush_deferred_signal(tx: Option<&Sender<ChangeSignal>>) -> bool {
    if !AUTO_SYNC_DEFERRED_SIGNAL.swap(false, Ordering::SeqCst) {
        return false;
    }
    match tx {
        Some(tx) => enqueue_change_signal(tx, "deferred"),
        None => false,
    }
}

pub(crate) fn is_auto_sync_suppressed() -> bool {
    AUTO_SYNC_SUPPRESS_DEPTH.load(Ordering::SeqCst) > 0
}

fn current_epoch() -> usize {
    AUTO_SYNC_SUPPRESS_EPOCH.load(Ordering::SeqCst)
}

/// Whether a batch whose newest signal was stamped `epoch` may still upload.
pub(crate) fn batch_still_valid(epoch: usize) -> bool {
    !is_auto_sync_suppressed() && epoch == current_epoch()
}

pub fn should_trigger_for_table(table: &str) -> bool {
    let normalized = table.trim().to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "providers"
            | "provider_endpoints"
            | "mcp_servers"
            | "prompts"
            | "skills"
            | "skill_repos"
            | "settings"
            | "proxy_config"
    )
}

pub(crate) fn enqueue_change_signal(tx: &Sender<ChangeSignal>, table: &str) -> bool {
    match tx.try_send((table.to_string(), current_epoch())) {
        Ok(()) => true,
        Err(TrySendError::Full(_)) | Err(TrySendError::Closed(_)) => false,
    }
}

pub(crate) fn auto_sync_wait_duration(started_at: Instant, now: Instant) -> Option<Duration> {
    let max_wait = Duration::from_millis(MAX_AUTO_SYNC_WAIT_MS);
    let debounce = Duration::from_millis(AUTO_SYNC_DEBOUNCE_MS);
    let elapsed = now.saturating_duration_since(started_at);
    if elapsed >= max_wait {
        return None;
    }
    Some(debounce.min(max_wait - elapsed))
}

pub(crate) fn should_run_auto_sync(enabled: bool, auto_sync: bool) -> bool {
    enabled && auto_sync
}

pub fn notify_db_changed(table: &str) {
    notify_db_changed_with(DB_CHANGE_TX.get(), table);
}

fn notify_db_changed_with(tx: Option<&Sender<ChangeSignal>>, table: &str) {
    if !should_trigger_for_table(table) {
        return;
    }
    if is_auto_sync_suppressed() {
        // Defer instead of dropping: the guard enqueues one signal on release.
        AUTO_SYNC_DEFERRED_SIGNAL.store(true, Ordering::SeqCst);
        return;
    }
    let Some(tx) = tx else {
        return;
    };
    let _ = enqueue_change_signal(tx, table);
}

// ─── Transports ──────────────────────────────────────────────

type UploadFuture<'a> = Pin<Box<dyn Future<Output = Result<(), AppError>> + Send + 'a>>;

/// What the worker needs from one transport.
pub(crate) trait AutoSyncTarget: Send + Sync {
    /// Display name used in logs and notifications ("WebDAV", "S3").
    fn label(&self) -> &'static str;
    /// Tauri event the frontend listens to for this transport.
    fn status_event(&self) -> &'static str;
    /// Enabled with auto-sync on.
    fn is_active(&self) -> bool;
    /// Upload under the transport's sync lock, skipping when the local
    /// content is unchanged since the last sync. `still_valid` is checked
    /// after the lock is acquired (a download may have held it).
    fn upload<'a>(
        &'a self,
        db: &'a crate::database::Database,
        still_valid: &'a (dyn Fn() -> bool + Send + Sync),
    ) -> UploadFuture<'a>;
    /// Record an auto-sync failure in the transport's status.
    fn persist_error(&self, error: &AppError);
}

pub(crate) fn record_auto_sync_error(status: &mut WebDavSyncStatus, error: &AppError) {
    status.last_error = Some(error.to_string());
    status.last_error_source = Some("auto".to_string());
}

struct WebDavTarget;

impl AutoSyncTarget for WebDavTarget {
    fn label(&self) -> &'static str {
        "WebDAV"
    }
    fn status_event(&self) -> &'static str {
        crate::events::WEBDAV_SYNC_STATUS_UPDATED
    }
    fn is_active(&self) -> bool {
        settings::get_webdav_sync_settings()
            .is_some_and(|s| should_run_auto_sync(s.enabled, s.auto_sync))
    }
    fn upload<'a>(
        &'a self,
        db: &'a crate::database::Database,
        still_valid: &'a (dyn Fn() -> bool + Send + Sync),
    ) -> UploadFuture<'a> {
        Box::pin(async move {
            use crate::services::webdav_sync;
            webdav_sync::run_with_sync_lock(async {
                if !still_valid() {
                    return Ok(());
                }
                let Some(mut settings) = settings::get_webdav_sync_settings() else {
                    return Ok(());
                };
                if !should_run_auto_sync(settings.enabled, settings.auto_sync) {
                    return Ok(());
                }
                // Auto-sync never forces: a remote change since the last sync
                // is a conflict, not something to overwrite.
                webdav_sync::upload_if_changed(db, &mut settings)
                    .await
                    .map(|_| ())
            })
            .await
        })
    }
    fn persist_error(&self, error: &AppError) {
        if let Some(settings) = settings::get_webdav_sync_settings() {
            let mut status = settings.status;
            record_auto_sync_error(&mut status, error);
            let _ = settings::update_webdav_sync_status(status);
        }
    }
}

struct S3Target;

impl AutoSyncTarget for S3Target {
    fn label(&self) -> &'static str {
        "S3"
    }
    fn status_event(&self) -> &'static str {
        crate::events::S3_SYNC_STATUS_UPDATED
    }
    fn is_active(&self) -> bool {
        settings::get_s3_sync_settings()
            .is_some_and(|s| should_run_auto_sync(s.enabled, s.auto_sync))
    }
    fn upload<'a>(
        &'a self,
        db: &'a crate::database::Database,
        still_valid: &'a (dyn Fn() -> bool + Send + Sync),
    ) -> UploadFuture<'a> {
        Box::pin(async move {
            use crate::services::s3_sync;
            s3_sync::run_with_sync_lock(async {
                if !still_valid() {
                    return Ok(());
                }
                let Some(mut settings) = settings::get_s3_sync_settings() else {
                    return Ok(());
                };
                if !should_run_auto_sync(settings.enabled, settings.auto_sync) {
                    return Ok(());
                }
                s3_sync::upload_if_changed(db, &mut settings)
                    .await
                    .map(|_| ())
            })
            .await
        })
    }
    fn persist_error(&self, error: &AppError) {
        if let Some(settings) = settings::get_s3_sync_settings() {
            let mut status = settings.status;
            record_auto_sync_error(&mut status, error);
            let _ = settings::update_s3_sync_status(status);
        }
    }
}

fn targets() -> [&'static dyn AutoSyncTarget; 2] {
    static WEBDAV: WebDavTarget = WebDavTarget;
    static S3: S3Target = S3Target;
    [&WEBDAV, &S3]
}

fn emit_auto_sync_status_updated(
    app: &AppHandle,
    event: &str,
    status: &str,
    error: Option<&AppError>,
) {
    let payload = match error {
        Some(err) => json!({
            "source": "auto",
            "status": status,
            "error": err.to_string(),
            "conflict": is_sync_conflict(err),
        }),
        None => json!({
            "source": "auto",
            "status": status,
        }),
    };

    if let Err(err) = app.emit(event, payload) {
        log::debug!("[AutoSync] failed to emit {event}: {err}");
    }
}

// ─── Worker ──────────────────────────────────────────────────

pub fn start_worker(db: Arc<crate::database::Database>, app: tauri::AppHandle) {
    if DB_CHANGE_TX.get().is_some() {
        return;
    }

    // Buffer size 1 is enough: we only need "dirty" signals, not every event.
    let (tx, rx) = channel::<ChangeSignal>(1);
    if DB_CHANGE_TX.set(tx).is_err() {
        return;
    }

    tauri::async_runtime::spawn(async move {
        run_worker_loop(rx, |epoch| {
            let db = db.clone();
            let app = app.clone();
            async move { upload_to_active_targets(&db, &app, epoch).await }
        })
        .await;
    });
}

/// Debounce signals into batches and hand each batch's newest epoch to
/// `on_batch`. Generic so tests can drive it without Tauri.
pub(crate) async fn run_worker_loop<F, Fut>(mut rx: Receiver<ChangeSignal>, mut on_batch: F)
where
    F: FnMut(usize) -> Fut,
    Fut: Future<Output = ()>,
{
    while let Some((first_table, mut epoch)) = rx.recv().await {
        let started_at = Instant::now();
        let mut merged_count = 1usize;

        while let Some(wait_for) = auto_sync_wait_duration(started_at, Instant::now()) {
            match tokio::time::timeout(wait_for, rx.recv()).await {
                Ok(Some((_, signal_epoch))) => {
                    merged_count += 1;
                    epoch = signal_epoch;
                }
                Ok(None) => return,
                Err(_) => break,
            }
        }

        log::debug!("[AutoSync] Triggered by table={first_table}, merged_changes={merged_count}");

        if !batch_still_valid(epoch) {
            log::debug!("[AutoSync] Skipping batch queued before a sync download");
            continue;
        }
        on_batch(epoch).await;
    }
}

async fn upload_to_active_targets(db: &crate::database::Database, app: &AppHandle, epoch: usize) {
    let still_valid = move || batch_still_valid(epoch);
    for target in targets() {
        if !target.is_active() {
            continue;
        }
        match target.upload(db, &still_valid).await {
            Ok(()) => emit_auto_sync_status_updated(app, target.status_event(), "success", None),
            Err(err) => {
                log::warn!("[{}][AutoSync] Upload failed: {err}", target.label());
                target.persist_error(&err);
                emit_auto_sync_status_updated(app, target.status_event(), "error", Some(&err));
                crate::services::notifications::notify_sync_error(
                    app,
                    target.label(),
                    &err.to_string(),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::sync::atomic::AtomicUsize;
    use tokio::sync::mpsc::channel;

    #[test]
    fn should_trigger_sync_for_config_tables_only() {
        assert!(should_trigger_for_table("providers"));
        assert!(should_trigger_for_table("settings"));
        assert!(!should_trigger_for_table("proxy_request_logs"));
        assert!(!should_trigger_for_table("provider_health"));
    }

    #[test]
    #[serial(auto_sync)]
    fn suppression_guard_enables_and_restores_state() {
        assert!(!is_auto_sync_suppressed());
        {
            let _guard = AutoSyncSuppressionGuard::new();
            assert!(is_auto_sync_suppressed());
        }
        assert!(!is_auto_sync_suppressed());
    }

    #[test]
    fn max_wait_caps_flush_latency_for_continuous_events() {
        let started = Instant::now();
        let later = started + Duration::from_millis(MAX_AUTO_SYNC_WAIT_MS + 1);
        assert!(auto_sync_wait_duration(started, later).is_none());
    }

    #[tokio::test]
    async fn enqueue_change_signal_drops_when_channel_is_full() {
        let (tx, _rx) = channel::<ChangeSignal>(1);
        assert!(enqueue_change_signal(&tx, "providers"));
        assert!(!enqueue_change_signal(&tx, "providers"));
    }

    #[test]
    fn should_run_auto_sync_requires_enabled_and_auto_sync_flag() {
        assert!(!should_run_auto_sync(false, true));
        assert!(!should_run_auto_sync(true, false));
        assert!(should_run_auto_sync(true, true));
    }

    #[tokio::test]
    #[serial(auto_sync)]
    async fn signal_queued_before_a_download_does_not_upload() {
        let (tx, rx) = channel::<ChangeSignal>(1);
        let uploads = Arc::new(AtomicUsize::new(0));
        let counter = uploads.clone();
        let worker = tokio::spawn(run_worker_loop(rx, move |_| {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        }));

        // A provider edit enqueues a signal, then the user starts a download.
        assert!(enqueue_change_signal(&tx, "providers"));
        let guard = AutoSyncSuppressionGuard::new();
        tokio::time::sleep(Duration::from_millis(AUTO_SYNC_DEBOUNCE_MS + 500)).await;
        drop(guard);
        tokio::time::sleep(Duration::from_millis(AUTO_SYNC_DEBOUNCE_MS + 500)).await;
        assert_eq!(
            uploads.load(Ordering::SeqCst),
            0,
            "no upload after download"
        );

        // A change made after the download still uploads.
        assert!(enqueue_change_signal(&tx, "providers"));
        tokio::time::sleep(Duration::from_millis(AUTO_SYNC_DEBOUNCE_MS + 500)).await;
        assert_eq!(uploads.load(Ordering::SeqCst), 1);

        drop(tx);
        worker.await.unwrap();
    }

    #[tokio::test]
    #[serial(auto_sync)]
    async fn change_during_suppression_uploads_once_after_guard_drops() {
        let (tx, rx) = channel::<ChangeSignal>(1);
        let uploads = Arc::new(AtomicUsize::new(0));
        let counter = uploads.clone();
        let worker = tokio::spawn(run_worker_loop(rx, move |_| {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        }));

        let guard = AutoSyncSuppressionGuard::with_sender(tx.clone());
        // Several edits while the download runs: none enqueued yet.
        notify_db_changed_with(Some(&tx), "providers");
        notify_db_changed_with(Some(&tx), "mcp_servers");
        tokio::time::sleep(Duration::from_millis(AUTO_SYNC_DEBOUNCE_MS + 500)).await;
        assert_eq!(uploads.load(Ordering::SeqCst), 0, "no upload while held");

        drop(guard);
        tokio::time::sleep(Duration::from_millis(AUTO_SYNC_DEBOUNCE_MS + 500)).await;
        assert_eq!(
            uploads.load(Ordering::SeqCst),
            1,
            "exactly one upload once the guard drops"
        );

        // Nothing deferred: releasing another guard uploads nothing.
        drop(AutoSyncSuppressionGuard::with_sender(tx.clone()));
        tokio::time::sleep(Duration::from_millis(AUTO_SYNC_DEBOUNCE_MS + 500)).await;
        assert_eq!(uploads.load(Ordering::SeqCst), 1);

        drop(tx);
        worker.await.unwrap();
    }

    #[test]
    fn service_layer_does_not_depend_on_commands_layer() {
        let source = include_str!("auto_sync.rs");
        let needle = ["crate", "commands", ""].join("::");
        assert!(
            !source.contains(&needle),
            "services layer should not depend on commands layer"
        );
    }
}
