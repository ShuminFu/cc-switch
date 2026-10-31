//! Per-app live-config adapters (CFG-06, CFG-09, SVC-18).
//!
//! Every managed tool (Claude Code, Claude Desktop, Codex, Gemini CLI, Grok
//! Build, OpenCode, OpenClaw, Hermes) implements [`AppAdapter`]. The services
//! (`ProviderService`, `McpService`, `ProxyService`) dispatch through
//! [`adapter_for`] instead of repeating an 8-arm `match app_type` for every
//! operation, so a behavioural fix (backups, BOM handling, validation, …)
//! lands once for all apps and a new tool is one module plus one registry
//! entry.
//!
//! Shared building blocks:
//! * [`live_file`] — lenient reads, style-preserving JSON, backup-before-write,
//!   atomic writes and an optimistic read-modify-write;
//! * [`json5_doc`] — comment/order preserving JSON5/JSONC documents.
//!
//! Ownership rules every adapter follows:
//! * an unparsable live file is never overwritten (the operation fails);
//! * the previous content is backed up under `~/.cc-switch/backups/<app>/`
//!   before it is replaced;
//! * keys the adapter does not own are preserved (exclusive-mode apps
//!   replace the provider-owned files; additive-mode apps only touch their
//!   provider/MCP entries).
//!
//! The per-app ownership matrix (files, owned keys, backfill stripping, env
//! overrides, backup locations) is in `docs/dev/config-adapters.md`; each
//! adapter module's header names its row there (CFG-26).

pub mod json5_doc;
pub mod live_file;
pub mod plan;
pub mod validate;

mod claude;
mod claude_desktop;
mod codex;
mod gemini;
mod grok;
mod hermes;
mod openclaw;
mod opencode;

use std::path::PathBuf;

use serde_json::Value;

use crate::app_config::{AppType, MultiAppConfig};
use crate::error::AppError;
use crate::provider::Provider;

/// How an app's live configuration relates to the provider list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    /// One provider at a time owns the live files (switching replaces them).
    Exclusive,
    /// All providers coexist in one live file; each write touches only its
    /// own entry.
    Additive,
}

/// Where a provider's "common config" snippet is merged into its settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommonConfigShape {
    /// The app has no common-config snippet.
    None,
    /// JSON snippet deep-merged into the settings root (Claude).
    JsonRoot,
    /// TOML snippet merged into the `config` string field (Codex).
    TomlConfigField,
    /// JSON snippet deep-merged into the `env` object (Gemini).
    JsonEnvField,
}

/// What an adapter supports; services consult this instead of hard-coding
/// lists of apps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdapterCapabilities {
    pub mode: WriteMode,
    /// [`AppAdapter::read_live`] returns a generic provider snapshot (used for
    /// backfill and "import current config").
    pub generic_live_read: bool,
    /// The app consumes MCP servers managed by cc-switch.
    pub mcp: bool,
    /// The local proxy can take over the app's live config.
    pub proxy_takeover: bool,
    pub common_config: CommonConfigShape,
}

/// One provider found in an app's live config that can be imported (CFG-23).
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportCandidate {
    /// Stable id derived from the live config (e.g. the Codex
    /// `model_providers` key); callers may use it as a provider id hint.
    pub id_hint: String,
    pub name: String,
    /// Provider `settings_config` to store.
    pub settings_config: Value,
    /// Hash of the endpoint and credential, for de-duplicating against
    /// providers that were already imported. Never contains the secret.
    pub fingerprint: String,
}

/// Fingerprint of an endpoint/credential pair (SHA-256, hex).
pub fn credential_fingerprint(endpoint: &str, credential: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(endpoint.trim().trim_end_matches('/').as_bytes());
    hasher.update([0u8]);
    hasher.update(credential.trim().as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// MCP projection for one app: DB (unified spec) → the app's native config.
pub trait McpProjector: Send + Sync {
    /// Insert or replace one server in the app's live config.
    fn upsert_server(&self, id: &str, spec: &Value) -> Result<(), AppError>;

    /// Remove one server from the app's live config (no-op when absent).
    fn remove_server(&self, id: &str) -> Result<(), AppError>;

    /// Import the app's servers into `config.mcp.servers` (enabling this app
    /// for servers that already exist). Returns how many entries changed.
    fn import_servers(&self, config: &mut MultiAppConfig) -> Result<usize, AppError>;

    /// The app's config directory when it does not exist. MCP sync never
    /// creates directories for apps that are not installed, so every write
    /// for this app is skipped; callers surface that as a warning instead of
    /// reporting the server as applied (CFG-46).
    fn missing_config_dir(&self) -> Option<PathBuf> {
        None
    }

    /// Project the desired state in one go: `enabled` servers are written,
    /// `disabled` ones removed. Adapters whose config is one file override
    /// this to rewrite the file at most once (CFG-41).
    fn project(&self, enabled: &[(&str, &Value)], disabled: &[&str]) -> Result<(), AppError> {
        for (id, spec) in enabled {
            self.upsert_server(id, spec)?;
        }
        for id in disabled {
            self.remove_server(id)?;
        }
        Ok(())
    }
}

/// Live-config adapter for one app.
pub trait AppAdapter: Send + Sync {
    fn app(&self) -> AppType;

    fn capabilities(&self) -> AdapterCapabilities;

    /// Files this adapter reads/writes (for diagnostics, backups and tests).
    fn live_paths(&self) -> Vec<PathBuf>;

    /// Read the live config as a provider `settings_config` snapshot.
    fn read_live(&self) -> Result<Value, AppError>;

    /// Write `provider` (with common config already applied) to the live files.
    fn write_live(&self, provider: &Provider) -> Result<(), AppError>;

    /// The files `write_live(provider)` would write, with their exact new
    /// content, without touching disk (CFG-22). `None` when the adapter
    /// cannot preview yet. Adapters that implement it write the planned
    /// bytes in `write_live`, so the preview equals the write.
    fn plan_live(&self, _provider: &Provider) -> Result<Option<Vec<plan::PlannedWrite>>, AppError> {
        Ok(None)
    }

    /// Turn a live snapshot back into what is stored for `provider`: strip
    /// projections cc-switch injected (MCP servers, context defaults, …) and
    /// re-attach DB-only fields. The default keeps the snapshot as-is.
    fn backfill(&self, _provider: &Provider, live: Value) -> Value {
        live
    }

    /// Snapshot used when importing the current live config as a provider.
    fn import_live(&self) -> Result<Value, AppError> {
        self.read_live()
    }

    /// Every provider the live config defines, for a repeatable "import from
    /// live config" (CFG-23). The default offers the current live snapshot
    /// as a single candidate.
    fn import_candidates(&self) -> Result<Vec<ImportCandidate>, AppError> {
        let settings = self.import_live()?;
        let fingerprint = credential_fingerprint(
            self.app().as_str(),
            &serde_json::to_string(&settings).unwrap_or_default(),
        );
        Ok(vec![ImportCandidate {
            id_hint: "default".to_string(),
            name: "default".to_string(),
            settings_config: settings,
            fingerprint,
        }])
    }

    /// Additive mode: whether the live file already contains `provider_id`.
    fn live_has_provider(&self, _provider_id: &str) -> Result<bool, AppError> {
        Ok(false)
    }

    /// Additive mode: remove one provider from the live file.
    fn remove_provider_from_live(&self, _provider_id: &str) -> Result<(), AppError> {
        Err(AppError::Message(format!(
            "App {} does not support remove from live config",
            self.app().as_str()
        )))
    }

    /// MCP projection, when the app consumes cc-switch MCP servers.
    fn mcp(&self) -> Option<&dyn McpProjector> {
        None
    }
}

/// The adapter for `app`. This is the only place that maps app types to
/// their implementation.
pub fn adapter_for(app: &AppType) -> &'static dyn AppAdapter {
    match app {
        AppType::Claude => &claude::ClaudeAdapter,
        AppType::ClaudeDesktop => &claude_desktop::ClaudeDesktopAdapter,
        AppType::Codex => &codex::CodexAdapter,
        AppType::Gemini => &gemini::GeminiAdapter,
        AppType::GrokBuild => &grok::GrokAdapter,
        AppType::OpenCode => &opencode::OpenCodeAdapter,
        AppType::OpenClaw => &openclaw::OpenClawAdapter,
        AppType::Hermes => &hermes::HermesAdapter,
    }
}

/// All adapters, in `AppType::all()` order.
pub fn all_adapters() -> impl Iterator<Item = &'static dyn AppAdapter> {
    AppType::all().map(|app| adapter_for(&app))
}

/// Adapters whose apps consume cc-switch MCP servers.
pub fn mcp_adapters() -> impl Iterator<Item = &'static dyn AppAdapter> {
    all_adapters().filter(|adapter| adapter.mcp().is_some())
}

/// An app whose MCP sync was skipped because its config directory is missing.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct McpSyncSkip {
    pub app: String,
    pub missing_dir: PathBuf,
}

/// For each of `apps` that consumes MCP, report the ones whose config
/// directory is missing, i.e. where a sync writes nothing (CFG-46).
pub fn mcp_sync_skips(apps: impl IntoIterator<Item = AppType>) -> Vec<McpSyncSkip> {
    apps.into_iter()
        .filter_map(|app| {
            let dir = adapter_for(&app).mcp()?.missing_config_dir()?;
            Some(McpSyncSkip {
                app: app.as_str().to_string(),
                missing_dir: dir,
            })
        })
        .collect()
}

/// Adapters whose live config can be taken over by the local proxy.
pub fn takeover_adapters() -> impl Iterator<Item = &'static dyn AppAdapter> {
    all_adapters().filter(|adapter| adapter.capabilities().proxy_takeover)
}

pub(crate) fn missing_live_error(
    key: &'static str,
    zh: &'static str,
    en: &'static str,
) -> AppError {
    AppError::localized(key, zh, en)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_covers_every_app_once() {
        for app in AppType::all() {
            let adapter = adapter_for(&app);
            assert_eq!(adapter.app(), app);
            let caps = adapter.capabilities();
            assert_eq!(
                caps.mode == WriteMode::Additive,
                app.is_additive_mode(),
                "{app:?} mode"
            );
            assert_eq!(caps.mcp, adapter.mcp().is_some(), "{app:?} mcp");
            if app != AppType::ClaudeDesktop {
                assert!(!adapter.live_paths().is_empty(), "{app:?} paths");
            }
        }
        let mcp: Vec<_> = mcp_adapters().map(|a| a.app()).collect();
        assert_eq!(
            mcp,
            vec![
                AppType::Claude,
                AppType::Codex,
                AppType::Gemini,
                AppType::GrokBuild,
                AppType::OpenCode,
                AppType::Hermes
            ]
        );
        let takeover: Vec<_> = takeover_adapters().map(|a| a.app()).collect();
        assert_eq!(
            takeover,
            vec![
                AppType::Claude,
                AppType::Codex,
                AppType::Gemini,
                AppType::GrokBuild
            ]
        );
    }
}
