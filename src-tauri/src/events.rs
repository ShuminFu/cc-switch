//! Names of the events the backend emits to the webview (CMD-14).
//!
//! Emit with these constants instead of string literals so a rename or a typo
//! is a compile error, and so `rg events::` lists every emitter. The frontend
//! listens with the same strings (`listen("provider-switched", ...)`).
//!
//! Payload notes:
//! - [`PROVIDER_SWITCHED`]: `{ appType, providerId, source? , proxyEnabled?, autoFailoverEnabled? }`.
//!   `appType` and `providerId` are always present; the frontend only relies on those.
//! - [`PROXY_RESTORE_FAILED`]: `{ appType, error }`, emitted when the startup
//!   takeover restore fails even after a retry (CMD-46).

/// A provider became current for an app (UI, tray, failover, profile apply).
pub const PROVIDER_SWITCHED: &str = "provider-switched";
/// Proxy / auto-failover flags changed outside the main window (tray).
pub const PROXY_FLAGS_CHANGED: &str = "proxy-flags-changed";
/// The usage cache has a new entry for a provider.
pub const USAGE_CACHE_UPDATED: &str = "usage-cache-updated";
/// A `ccswitch://` import request is ready for the frontend.
pub const DEEPLINK_IMPORT: &str = "deeplink-import";
/// A `ccswitch://` link could not be parsed.
pub const DEEPLINK_ERROR: &str = "deeplink-error";
/// A profile was applied (tray or command).
pub const PROFILE_APPLIED: &str = "profile-applied";
/// Providers were added or changed in the background; refetch provider lists.
pub const UNIVERSAL_PROVIDER_SYNCED: &str = "universal-provider-synced";
/// Updater download progress.
pub const UPDATE_DOWNLOAD_PROGRESS: &str = "update-download-progress";
/// Restoring a proxy takeover at startup failed (after one retry).
pub const PROXY_RESTORE_FAILED: &str = "proxy-restore-failed";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_names_are_unique_kebab_case() {
        let names = [
            PROVIDER_SWITCHED,
            PROXY_FLAGS_CHANGED,
            USAGE_CACHE_UPDATED,
            DEEPLINK_IMPORT,
            DEEPLINK_ERROR,
            PROFILE_APPLIED,
            UNIVERSAL_PROVIDER_SYNCED,
            UPDATE_DOWNLOAD_PROGRESS,
            PROXY_RESTORE_FAILED,
        ];
        let mut seen = std::collections::HashSet::new();
        for name in names {
            assert!(seen.insert(name), "duplicate event name {name}");
            assert!(name.chars().all(|c| c.is_ascii_lowercase() || c == '-'));
        }
    }
}
