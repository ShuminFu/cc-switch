//! S3 entry points of the shared auto-sync worker.
//!
//! There is one worker and one suppression counter for every transport
//! ([`crate::services::auto_sync`], SVC-19): a WebDAV download now also
//! suppresses the S3 auto-upload. Notifying a change or starting the worker
//! through this module is the same as doing it through `webdav_auto_sync`
//! (signals are coalesced and the worker is a singleton).

pub(crate) use super::auto_sync::AutoSyncSuppressionGuard;
pub use super::auto_sync::{notify_db_changed, start_worker};
