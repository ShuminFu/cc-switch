//! Tauri events emitted by the backend and consumed by the frontend.

pub const PROVIDER_SWITCHED: &str = "provider-switched";
pub const PROFILE_APPLIED: &str = "profile-applied";
pub const USAGE_CACHE_UPDATED: &str = "usage-cache-updated";
pub const USAGE_LOG_RECORDED: &str = "usage-log-recorded";
pub const DEEPLINK_IMPORT: &str = "deeplink-import";
pub const DEEPLINK_ERROR: &str = "deeplink-error";
pub const UNIVERSAL_PROVIDER_SYNCED: &str = "universal-provider-synced";
pub const UPDATE_DOWNLOAD_PROGRESS: &str = "update-download-progress";
pub const PROXY_OFFICIAL_WARNING: &str = "proxy-official-warning";
pub const WEBDAV_SYNC_STATUS_UPDATED: &str = "webdav-sync-status-updated";
pub const S3_SYNC_STATUS_UPDATED: &str = "s3-sync-status-updated";

/// Every event the frontend listens to.
pub const ALL: &[&str] = &[
    PROVIDER_SWITCHED,
    PROFILE_APPLIED,
    USAGE_CACHE_UPDATED,
    USAGE_LOG_RECORDED,
    DEEPLINK_IMPORT,
    DEEPLINK_ERROR,
    UNIVERSAL_PROVIDER_SYNCED,
    UPDATE_DOWNLOAD_PROGRESS,
    PROXY_OFFICIAL_WARNING,
    WEBDAV_SYNC_STATUS_UPDATED,
    S3_SYNC_STATUS_UPDATED,
];
