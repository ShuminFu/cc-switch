//! Self-contained product features that sit beside the core services.
//!
//! Each module owns its own state (a JSON file under the app config dir or a
//! read-only file installed by an administrator) and exposes a few Tauri
//! commands, so the core services only need a one-line hook to use it.

pub mod audit_log;
pub mod config_history;
pub mod deeplink_nav;
pub mod file_dialog;
pub mod log_retention;
pub mod policy;
pub mod redact;
pub mod session_export;
