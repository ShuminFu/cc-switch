//! Per-app live-config adapters (CFG-06, CFG-09, SVC-18).
//!
//! * [`live_file`] — the shared live-file layer (lenient reads, style
//!   preserving JSON, backup-before-write, atomic writes, optimistic
//!   read-modify-write);
//! * [`json5_doc`] — comment/order preserving JSON5/JSONC documents.

pub mod json5_doc;
pub mod live_file;
