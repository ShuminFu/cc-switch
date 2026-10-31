//! WebDAV entry points of the shared auto-sync worker.
//!
//! The worker, its debounce and the suppression guard live in
//! [`crate::services::auto_sync`] and serve every transport (SVC-19); this
//! module keeps the historical paths used by the database hook, the app
//! setup and the sync commands.

pub(crate) use super::auto_sync::AutoSyncSuppressionGuard;
pub use super::auto_sync::{notify_db_changed, start_worker};
