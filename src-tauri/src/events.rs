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
/// Proxy takeover was enabled while an official provider that cannot be
/// proxied is current: `{ appType, providerName }`.
pub const PROXY_OFFICIAL_WARNING: &str = "proxy-official-warning";
/// A WebDAV auto-sync run finished: `{ source: "auto", status, error?, conflict? }`.
pub const WEBDAV_SYNC_STATUS_UPDATED: &str = "webdav-sync-status-updated";
/// An S3 auto-sync run finished; same payload as [`WEBDAV_SYNC_STATUS_UPDATED`].
pub const S3_SYNC_STATUS_UPDATED: &str = "s3-sync-status-updated";

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
            PROXY_OFFICIAL_WARNING,
            WEBDAV_SYNC_STATUS_UPDATED,
            S3_SYNC_STATUS_UPDATED,
            crate::usage_events::EVENT_USAGE_LOG_RECORDED,
        ];
        let mut seen = std::collections::HashSet::new();
        for name in names {
            assert!(seen.insert(name), "duplicate event name {name}");
            assert!(name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
        }
    }

    /// No emitter passes an event-name string literal (CMD-14).
    #[test]
    fn no_emit_with_string_literal() {
        fn visit(dir: &std::path::Path, hits: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    visit(&path, hits);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    // built at runtime so this file does not match itself
                    let needle = [".emit", "("].concat();
                    for (idx, _) in text.match_indices(needle.as_str()) {
                        let rest = text[idx + needle.len()..].trim_start();
                        if rest.starts_with('"') {
                            hits.push(path.display().to_string());
                        }
                    }
                }
            }
        }
        let mut hits = Vec::new();
        visit(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut hits,
        );
        assert!(hits.is_empty(), "literal event names in: {hits:?}");
    }
}
