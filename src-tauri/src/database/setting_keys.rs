//! Registry of the keys stored in the `settings` table (DB-28, first increment).
//!
//! Every key the app writes is declared here once with its sync scope and
//! value kind. The sync exporter/importer derives the device-local key set
//! from this registry (instead of a hand-kept list next to it), and
//! [`Database::set_setting`](super::Database::set_setting) warns about keys
//! that are not registered, so a typo'd or undocumented key is visible.

/// Whether a key travels with WebDAV/S3 sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingScope {
    /// Exported and imported with the synced `settings` rows.
    Synced,
    /// Kept on this device: never exported, and an import keeps the local value.
    Device,
}

/// How the value is encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingKind {
    /// `"true"` / `"false"` (also `"1"`).
    Bool,
    /// Plain text or a number.
    Scalar,
    /// A JSON document.
    Json,
}

/// One registered key, or a family of keys sharing a prefix.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SettingKey {
    pub name: &'static str,
    /// `true`: `name` is a prefix (`common_config_<app>`, ...).
    pub prefix: bool,
    pub scope: SettingScope,
    // Documentation for readers of the registry; not consulted at runtime yet.
    #[allow(dead_code)]
    pub kind: SettingKind,
    #[allow(dead_code)]
    pub doc: &'static str,
}

const fn key(
    name: &'static str,
    scope: SettingScope,
    kind: SettingKind,
    doc: &'static str,
) -> SettingKey {
    SettingKey {
        name,
        prefix: false,
        scope,
        kind,
        doc,
    }
}

const fn family(
    name: &'static str,
    scope: SettingScope,
    kind: SettingKind,
    doc: &'static str,
) -> SettingKey {
    SettingKey {
        name,
        prefix: true,
        scope,
        kind,
        doc,
    }
}

use SettingKind::{Bool, Json, Scalar};
use SettingScope::{Device, Synced};

/// Every `settings` key the app uses.
pub(crate) const SETTING_KEYS: &[SettingKey] = &[
    // ── device-local ──────────────────────────────────────────
    key(
        "claude_desktop_gateway_token",
        Device,
        Scalar,
        "Per-device Claude Desktop gateway credential (SEC-20).",
    ),
    key(
        super::dao::proxy::PROXY_ACCESS_TOKEN_SETTING_KEY,
        Device,
        Scalar,
        "Access token of this device's local proxy; written into this device's live configs.",
    ),
    key(
        "global_proxy_url",
        Device,
        Scalar,
        "Outbound HTTP proxy of this device.",
    ),
    key(
        super::pricing_seed::SEED_VERSION_SETTING,
        Device,
        Scalar,
        "Built-in pricing list version applied on this device (DB-19).",
    ),
    key(
        super::sync_rows::USAGE_RETENTION_DAYS_KEY,
        Device,
        Scalar,
        "Usage history retention of this device (DB-25).",
    ),
    key(
        super::sync_rows::STREAM_CHECK_RETENTION_DAYS_KEY,
        Device,
        Scalar,
        "Stream-check log retention of this device (DB-25).",
    ),
    key(
        "project_bindings",
        Device,
        Json,
        "Project directory bindings (absolute local paths).",
    ),
    key(
        "skills_ssot_migration_pending",
        Device,
        Bool,
        "One-shot skills SSOT migration state.",
    ),
    key(
        "skills_ssot_migration_snapshot",
        Device,
        Json,
        "One-shot skills SSOT migration snapshot.",
    ),
    family(
        "current_profile_id_",
        Device,
        Scalar,
        "Active profile per scope on this device.",
    ),
    family(
        "profile_baseline_",
        Device,
        Json,
        "Profile baseline captured on this device.",
    ),
    family(
        "prompt_live_written_hash_",
        Device,
        Scalar,
        "Hash of the prompt file this device last wrote (external-edit detection).",
    ),
    family(
        "skill_repo_check:",
        Device,
        Json,
        "Cached skill repository reachability check.",
    ),
    // ── synced ────────────────────────────────────────────────
    key(
        "official_providers_seeded",
        Synced,
        Bool,
        "Official provider seeds were created.",
    ),
    key(
        "default_skill_repos_initialized",
        Synced,
        Bool,
        "Default skill repositories were added once.",
    ),
    key(
        "stream_check_config",
        Synced,
        Json,
        "Stream health check configuration.",
    ),
    key(
        "rectifier_config",
        Synced,
        Json,
        "Request rectifier configuration.",
    ),
    key(
        "pricing_fx_rates",
        Synced,
        Json,
        "Units of each currency per 1 USD, for model pricing rows not quoted in USD (PRODUCT-40).",
    ),
    key(
        "optimizer_config",
        Synced,
        Json,
        "Request optimizer configuration.",
    ),
    key(
        "copilot_optimizer_config",
        Synced,
        Json,
        "Copilot optimizer configuration.",
    ),
    key("log_config", Synced, Json, "Logging configuration."),
    key("universal_providers", Synced, Json, "Universal providers."),
    key(
        "budget_hard_limit_enforcement",
        Synced,
        Bool,
        "Enforce provider hard spending limits.",
    ),
    key(
        "common_config_legacy_migrated_v1",
        Synced,
        Bool,
        "Legacy common-config snippets were migrated.",
    ),
    family(
        "common_config_",
        Synced,
        Scalar,
        "Common config snippet per app (and its `_cleared` marker).",
    ),
    family(
        "proxy_takeover_",
        Synced,
        Bool,
        "Legacy per-app takeover flags (read only for migration).",
    ),
];

/// The registry entry for `key`: an exact match first, else the longest
/// matching prefix family.
pub(crate) fn lookup(key: &str) -> Option<&'static SettingKey> {
    SETTING_KEYS
        .iter()
        .find(|k| !k.prefix && k.name == key)
        .or_else(|| {
            SETTING_KEYS
                .iter()
                .filter(|k| k.prefix && key.starts_with(k.name))
                .max_by_key(|k| k.name.len())
        })
}

/// Exact device-local keys.
pub(crate) fn device_keys() -> impl Iterator<Item = &'static str> {
    SETTING_KEYS
        .iter()
        .filter(|k| k.scope == Device && !k.prefix)
        .map(|k| k.name)
}

/// Device-local key prefixes.
pub(crate) fn device_prefixes() -> impl Iterator<Item = &'static str> {
    SETTING_KEYS
        .iter()
        .filter(|k| k.scope == Device && k.prefix)
        .map(|k| k.name)
}

/// Whether `key` stays on this device.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn is_device_local(key: &str) -> bool {
    lookup(key).is_some_and(|k| k.scope == Device)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique_and_documented() {
        let mut seen = std::collections::HashSet::new();
        for key in SETTING_KEYS {
            assert!(
                seen.insert((key.name, key.prefix)),
                "duplicate {}",
                key.name
            );
            assert!(!key.doc.is_empty(), "{} is undocumented", key.name);
        }
    }

    #[test]
    fn lookup_prefers_exact_keys_then_longest_prefix() {
        assert_eq!(
            lookup("common_config_legacy_migrated_v1").map(|k| k.prefix),
            Some(false)
        );
        assert_eq!(
            lookup("common_config_claude").map(|k| k.name),
            Some("common_config_")
        );
        assert!(lookup("typo_key").is_none());
        assert!(is_device_local("prompt_live_written_hash_claude"));
        assert!(is_device_local("current_profile_id_global"));
        assert!(is_device_local("proxy_access_token"));
        assert!(!is_device_local("stream_check_config"));
    }

    #[test]
    fn keys_declared_as_constants_elsewhere_are_registered() {
        for key in [
            crate::services::budget::BUDGET_ENFORCEMENT_KEY,
            crate::services::project_binding::PROJECT_BINDINGS_KEY,
            super::super::dao::proxy::PROXY_ACCESS_TOKEN_SETTING_KEY,
        ] {
            assert!(lookup(key).is_some(), "{key} is not registered");
        }
    }
}
