//! Live-config writer testkit, used by `tests/config_writers.rs` and
//! `tests/app_adapter_conformance.rs`.
//!
//! Exposes the production entry points that rewrite users' CLI configs, the
//! OAuth token stores and the Codex state DB migration, so the integration
//! test can pin their on-disk output with snapshots. Nothing here adds
//! behaviour: every function forwards to the code path the app itself runs.

use std::path::Path;

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;

pub use crate::claude_mcp::set_mcp_servers_map as set_claude_mcp_servers_map;
pub use crate::claude_plugin::{
    clear_claude_config as clear_claude_plugin_config,
    write_claude_config as write_claude_plugin_config,
};
pub use crate::codex_config::CC_SWITCH_CODEX_MODEL_PROVIDER_ID;
pub use crate::codex_history_migration::maybe_migrate_codex_third_party_history_provider_bucket;
pub use crate::proxy::providers::codex_oauth_auth::CodexOAuthManager;
pub use crate::proxy::providers::copilot_auth::CopilotAuthManager;

/// The app adapter surface, used by `tests/app_adapter_conformance.rs` to run
/// one contract over every entry in [`APPS`] (and a test-only extra app).
pub use crate::apps::adapter::{
    AppAdapter, CommonSnippet, Credentials, McpProjector, ProxyTakeover, SessionSource,
};
pub use crate::apps::{adapter, AppCaps, AppDescriptor, SyncMode, APPS};

/// File name of the Codex state DB (`state_5.sqlite`).
pub const CODEX_STATE_DB_FILENAME: &str = crate::codex_state_db::CODEX_STATE_DB_FILENAME;

/// Write a provider to its app's live config the way a provider switch does
/// (common-config snippet merged in, then the per-app live writer).
pub fn write_provider_live(
    db: &Database,
    app_type: &AppType,
    provider: &Provider,
) -> Result<(), AppError> {
    crate::services::provider::write_live_with_common_config(db, app_type, provider)
}

/// Apply a provider to Claude Desktop config files rooted at explicit
/// directories, so the writer runs on every platform (the app itself only
/// resolves these directories on macOS and Windows).
pub fn apply_claude_desktop_provider_at(
    db: &Database,
    provider: &Provider,
    normal_dir: &Path,
    threep_dir: &Path,
) -> Result<(), AppError> {
    crate::claude_desktop_config::apply_provider_at_dirs(db, provider, normal_dir, threep_dir)
}
