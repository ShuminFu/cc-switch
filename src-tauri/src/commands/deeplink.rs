use crate::deeplink::{
    import_mcp_from_deeplink, import_prompt_from_deeplink, import_provider_from_deeplink,
    import_skill_from_deeplink, parse_deeplink_url, DeepLinkImportRequest,
};
use crate::store::AppState;
use serde::Serialize;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::State;

// ============================================================
// Deep-link inbox: buffer links until the webview is listening
// ============================================================

/// A deep-link event waiting to be delivered to the frontend.
///
/// Serialized as `{ "kind": "import", "request": {...} }` or
/// `{ "kind": "error", "url": "<redacted>", "error": "..." }`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PendingDeepLink {
    Import { request: Box<DeepLinkImportRequest> },
    Error { url: String, error: String },
}

/// The same URL delivered again within this window is treated as a duplicate
/// (e.g. `get_current()` replaying a link the listener already handled).
const DUPLICATE_WINDOW: Duration = Duration::from_secs(2);

struct DeepLinkInbox {
    /// Whether the current webview has registered its `deeplink-*` listeners.
    frontend_ready: bool,
    /// Latest event received while the frontend was not ready (latest wins).
    pending: Option<PendingDeepLink>,
    /// Last handled URL, used to drop duplicate deliveries.
    last_url: Option<(String, Instant)>,
}

impl DeepLinkInbox {
    const fn new() -> Self {
        Self {
            frontend_ready: false,
            pending: None,
            last_url: None,
        }
    }

    fn is_duplicate(&mut self, url: &str, now: Instant) -> bool {
        if let Some((last, at)) = &self.last_url {
            if last == url && now.saturating_duration_since(*at) < DUPLICATE_WINDOW {
                return true;
            }
        }
        self.last_url = Some((url.to_string(), now));
        false
    }

    /// Returns the item when it should be emitted right away; otherwise buffers it.
    fn deliver(&mut self, item: PendingDeepLink) -> Option<PendingDeepLink> {
        if self.frontend_ready {
            Some(item)
        } else {
            self.pending = Some(item);
            None
        }
    }

    fn mark_not_ready(&mut self) {
        self.frontend_ready = false;
    }

    fn frontend_ready(&mut self) -> Option<PendingDeepLink> {
        self.frontend_ready = true;
        self.pending.take()
    }
}

static DEEPLINK_INBOX: Mutex<DeepLinkInbox> = Mutex::new(DeepLinkInbox::new());

fn with_inbox<T>(f: impl FnOnce(&mut DeepLinkInbox) -> T) -> T {
    let mut guard = DEEPLINK_INBOX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut guard)
}

/// Returns true when `url` was already handled within the last few seconds.
pub(crate) fn deeplink_is_duplicate(url: &str) -> bool {
    with_inbox(|inbox| inbox.is_duplicate(url, Instant::now()))
}

/// Returns the item if the frontend is ready and it should be emitted now;
/// otherwise stores it until the frontend calls `deeplink_frontend_ready`.
pub(crate) fn deeplink_deliver(item: PendingDeepLink) -> Option<PendingDeepLink> {
    with_inbox(|inbox| inbox.deliver(item))
}

/// Call before the main webview is (re)created, e.g. when leaving lightweight mode.
pub(crate) fn mark_deeplink_frontend_not_ready() {
    with_inbox(DeepLinkInbox::mark_not_ready);
}

/// Called by the frontend once its `deeplink-import` / `deeplink-error` listeners
/// are registered. Marks the webview ready and returns the link that arrived
/// before it was listening (cold start, lightweight mode), at most once.
#[tauri::command]
pub fn deeplink_frontend_ready() -> Option<PendingDeepLink> {
    with_inbox(DeepLinkInbox::frontend_ready)
}

/// Parse a deep link URL and return the parsed request for frontend confirmation
#[tauri::command]
pub fn parse_deeplink(url: String) -> Result<DeepLinkImportRequest, String> {
    log::info!("Parsing deep link URL: {}", crate::redact_url_for_log(&url));
    parse_deeplink_url(&url).map_err(|e| e.to_string())
}

/// Merge configuration from Base64/URL into a deep link request
/// This is used by the frontend to show the complete configuration in the confirmation dialog
#[tauri::command]
pub fn merge_deeplink_config(
    request: DeepLinkImportRequest,
) -> Result<DeepLinkImportRequest, String> {
    log::info!("Merging config for deep link request: {:?}", request.name);
    crate::deeplink::parse_and_merge_config(&request).map_err(|e| e.to_string())
}

/// Import a provider from a deep link request (legacy, kept for compatibility)
#[tauri::command]
pub fn import_from_deeplink(
    state: State<AppState>,
    request: DeepLinkImportRequest,
) -> Result<String, String> {
    log::info!(
        "Importing provider from deep link: {:?} for app {:?}",
        request.name,
        request.app
    );

    let provider_id = import_provider_from_deeplink(&state, request).map_err(|e| e.to_string())?;

    log::info!("Successfully imported provider with ID: {provider_id}");

    Ok(provider_id)
}

/// Import resource from a deep link request (unified handler)
#[tauri::command]
pub async fn import_from_deeplink_unified(
    state: State<'_, AppState>,
    request: DeepLinkImportRequest,
) -> Result<serde_json::Value, String> {
    log::info!("Importing {} resource from deep link", request.resource);

    match request.resource.as_str() {
        "provider" => {
            let provider_id =
                import_provider_from_deeplink(&state, request).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "type": "provider",
                "id": provider_id
            }))
        }
        "prompt" => {
            let prompt_id =
                import_prompt_from_deeplink(&state, request).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "type": "prompt",
                "id": prompt_id
            }))
        }
        "mcp" => {
            let result = import_mcp_from_deeplink(&state, request).map_err(|e| e.to_string())?;
            // Add type field to the result
            Ok(serde_json::json!({
                "type": "mcp",
                "importedCount": result.imported_count,
                "importedIds": result.imported_ids,
                "failed": result.failed
            }))
        }
        "skill" => {
            let skill_key =
                import_skill_from_deeplink(&state, request).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "type": "skill",
                "key": skill_key
            }))
        }
        _ => Err(format!("Unsupported resource type: {}", request.resource)),
    }
}

#[cfg(test)]
mod inbox_tests {
    use super::*;

    fn import(name: &str) -> PendingDeepLink {
        PendingDeepLink::Import {
            request: Box::new(DeepLinkImportRequest {
                name: Some(name.to_string()),
                ..Default::default()
            }),
        }
    }

    fn name_of(item: Option<PendingDeepLink>) -> Option<String> {
        match item? {
            PendingDeepLink::Import { request } => request.name,
            PendingDeepLink::Error { .. } => None,
        }
    }

    #[test]
    fn buffers_until_ready_and_returns_pending_exactly_once() {
        let mut inbox = DeepLinkInbox::new();
        assert!(inbox.deliver(import("first")).is_none());
        // Latest link wins while the frontend is not ready.
        assert!(inbox.deliver(import("second")).is_none());

        assert_eq!(name_of(inbox.frontend_ready()).as_deref(), Some("second"));
        assert!(inbox.frontend_ready().is_none());

        // Once ready, links are emitted immediately and not buffered.
        assert_eq!(
            name_of(inbox.deliver(import("live"))).as_deref(),
            Some("live")
        );
        assert!(inbox.frontend_ready().is_none());
    }

    #[test]
    fn webview_recreation_buffers_again() {
        let mut inbox = DeepLinkInbox::new();
        assert!(inbox.frontend_ready().is_none());

        inbox.mark_not_ready();
        assert!(inbox
            .deliver(PendingDeepLink::Error {
                url: "ccswitch://v1/import?[keys:apiKey]".into(),
                error: "bad".into(),
            })
            .is_none());
        assert!(matches!(
            inbox.frontend_ready(),
            Some(PendingDeepLink::Error { .. })
        ));
    }

    #[test]
    fn duplicate_delivery_of_same_url_is_dropped() {
        let mut inbox = DeepLinkInbox::new();
        let t0 = Instant::now();
        assert!(!inbox.is_duplicate("ccswitch://a", t0));
        assert!(inbox.is_duplicate("ccswitch://a", t0 + Duration::from_millis(100)));
        assert!(!inbox.is_duplicate("ccswitch://b", t0 + Duration::from_millis(200)));
        assert!(!inbox.is_duplicate("ccswitch://b", t0 + DUPLICATE_WINDOW * 2));
    }

    #[test]
    fn pending_payload_serializes_with_kind_tag() {
        let value = serde_json::to_value(import("x")).unwrap();
        assert_eq!(value["kind"], "import");
        assert_eq!(value["request"]["name"], "x");
    }
}

#[cfg(test)]
mod redaction_tests {
    use crate::redact_url_for_log;

    #[test]
    fn redact_strips_secret_query_values() {
        let url = "ccswitch://v1/import?resource=provider&app=claude&name=Test\
                   &apiKey=SECRET123&usageApiKey=USAGESECRET&usageAccessToken=TOKENSECRET";
        let redacted = redact_url_for_log(url);

        for secret in ["SECRET123", "USAGESECRET", "TOKENSECRET", "Test"] {
            assert!(
                !redacted.contains(secret),
                "redacted URL leaked {secret}: {redacted}"
            );
        }
        assert!(redacted.starts_with("ccswitch://v1/import?[keys:"));
        assert!(redacted.contains("apiKey"));
    }

    #[test]
    fn redact_handles_unparseable_urls_without_leaking_query() {
        let redacted = redact_url_for_log("not a url?apiKey=SECRET123#frag");
        assert!(!redacted.contains("SECRET123"));
        assert_eq!(redacted, "not a url?[redacted]");
    }
}
