use crate::deeplink::{
    import_mcp_from_deeplink, import_prompt_from_deeplink, import_provider_from_deeplink,
    import_skill_from_deeplink, DeepLinkImportRequest,
};
use crate::store::AppState;
use serde::{Deserialize, Serialize};
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
///
/// Import links are registered as pending imports first, so the request that
/// reaches the webview carries the nonce the import commands require.
pub(crate) fn deeplink_deliver(item: PendingDeepLink) -> Option<PendingDeepLink> {
    let item = match item {
        PendingDeepLink::Import { request } => PendingDeepLink::Import {
            request: Box::new(register_pending(*request)),
        },
        other => other,
    };
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

// ============================================================
// Pending imports (SESS-10): the backend keeps the reviewed request
// ============================================================

/// How long an OS-delivered link stays importable.
const PENDING_TTL: Duration = Duration::from_secs(30 * 60);
/// Oldest pending links are dropped beyond this many.
const MAX_PENDING: usize = 16;

struct PendingImport {
    nonce: String,
    request: DeepLinkImportRequest,
    created: Instant,
}

static PENDING_IMPORTS: Mutex<Vec<PendingImport>> = Mutex::new(Vec::new());

fn with_pending<T>(f: impl FnOnce(&mut Vec<PendingImport>) -> T) -> T {
    let mut guard = PENDING_IMPORTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let now = Instant::now();
    guard.retain(|p| now.saturating_duration_since(p.created) < PENDING_TTL);
    f(&mut guard)
}

/// Store an OS-delivered request and return it with its nonce.
pub(crate) fn register_pending(mut request: DeepLinkImportRequest) -> DeepLinkImportRequest {
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    request.nonce = Some(nonce.clone());
    let stored = request.clone();
    with_pending(|pending| {
        pending.push(PendingImport {
            nonce,
            request: stored,
            created: Instant::now(),
        });
        let overflow = pending.len().saturating_sub(MAX_PENDING);
        pending.drain(..overflow);
    });
    request
}

/// Remove and return the pending request for `nonce` (single use).
fn take_pending(nonce: Option<&str>) -> Result<DeepLinkImportRequest, String> {
    let nonce = nonce.filter(|n| !n.is_empty()).ok_or_else(|| {
        "Deep-link imports must come from a link opened by the system; this request has no pending-import nonce".to_string()
    })?;
    with_pending(|pending| {
        let index = pending
            .iter()
            .position(|p| p.nonce == nonce)
            .ok_or_else(|| "This deep-link import has expired or was already used".to_string())?;
        Ok(pending.remove(index).request)
    })
}

/// Apply the user's explicit choices to the stored request. Only these
/// booleans are taken from the webview; every other field comes from the link
/// as the backend parsed it.
fn apply_choices(
    mut stored: DeepLinkImportRequest,
    enabled: Option<bool>,
    usage_enabled: Option<bool>,
) -> DeepLinkImportRequest {
    // Adding a skill repository only makes it browsable, so it stays enabled
    // unless the dialog says otherwise; every other resource is activated
    // only on an explicit opt-in (SESS-32).
    stored.enabled = if stored.resource == "skill" {
        Some(enabled != Some(false))
    } else {
        Some(enabled == Some(true))
    };
    stored.usage_enabled = Some(usage_enabled == Some(true) && stored.usage_script.is_some());
    stored.nonce = None;
    stored.bundle = None;
    stored
}

fn import_one(
    state: &AppState,
    request: DeepLinkImportRequest,
) -> Result<serde_json::Value, String> {
    match request.resource.as_str() {
        "provider" => {
            let provider_id =
                import_provider_from_deeplink(state, request).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "type": "provider", "id": provider_id }))
        }
        "prompt" => {
            let prompt_id =
                import_prompt_from_deeplink(state, request).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "type": "prompt", "id": prompt_id }))
        }
        "mcp" => {
            let result = import_mcp_from_deeplink(state, request).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "type": "mcp",
                "importedCount": result.imported_count,
                "importedIds": result.imported_ids,
                "failed": result.failed
            }))
        }
        "skill" => {
            let skill_key =
                import_skill_from_deeplink(state, request).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "type": "skill", "key": skill_key }))
        }
        _ => Err(format!("Unsupported resource type: {}", request.resource)),
    }
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

/// Import a single-resource deep link the user confirmed.
///
/// `request` must carry the nonce of a pending link (SESS-10); the imported
/// data is the backend's copy of that link, with only `enabled` /
/// `usageEnabled` taken from the dialog. A request fabricated in the webview
/// is rejected.
#[tauri::command]
pub async fn import_from_deeplink_unified(
    state: State<'_, AppState>,
    request: DeepLinkImportRequest,
) -> Result<serde_json::Value, String> {
    let stored = take_pending(request.nonce.as_deref())?;
    if stored.bundle.as_ref().is_some_and(|b| b.items.len() > 1) {
        return Err("This link contains several items; import it as a bundle".to_string());
    }
    log::info!("Importing {} resource from deep link", stored.resource);
    let final_request = apply_choices(stored, request.enabled, request.usage_enabled);
    import_one(&state, final_request)
}

/// The user's decision for one item of a bundle.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleItemChoice {
    pub index: usize,
    #[serde(default)]
    pub accept: bool,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub usage_enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleItemResult {
    pub index: usize,
    pub resource: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Import the accepted items of a pending (v2) link. Items not listed or not
/// accepted are skipped. `trustPublisher` remembers the signing key (TOFU).
#[tauri::command]
pub async fn import_deeplink_bundle(
    state: State<'_, AppState>,
    nonce: String,
    choices: Vec<BundleItemChoice>,
    trustPublisher: Option<bool>,
) -> Result<Vec<BundleItemResult>, String> {
    let stored = take_pending(Some(&nonce))?;
    import_bundle(&state, stored, &choices, trustPublisher == Some(true))
}

fn import_bundle(
    state: &AppState,
    stored: DeepLinkImportRequest,
    choices: &[BundleItemChoice],
    trust_publisher: bool,
) -> Result<Vec<BundleItemResult>, String> {
    let (items, signature) = match stored.bundle.clone() {
        Some(bundle) => (bundle.items, Some(bundle.signature)),
        None => (vec![stored], None),
    };

    if trust_publisher {
        if let Some(sig) = signature.as_ref().filter(|s| {
            s.status == crate::deeplink::SignatureStatus::Verified
                && s.trust == Some(crate::deeplink::trust::PublisherTrust::New)
        }) {
            if let (Some(name), Some(key)) = (&sig.publisher, &sig.public_key) {
                crate::deeplink::trust::trust_publisher(name, key)?;
            }
        }
    }

    let mut results = Vec::new();
    for (index, item) in items.into_iter().enumerate() {
        let Some(choice) = choices.iter().find(|c| c.index == index && c.accept) else {
            continue;
        };
        let resource = item.resource.clone();
        let request = apply_choices(item, choice.enabled, choice.usage_enabled);
        match import_one(state, request) {
            Ok(result) => results.push(BundleItemResult {
                index,
                resource,
                ok: true,
                result: Some(result),
                error: None,
            }),
            Err(error) => results.push(BundleItemResult {
                index,
                resource,
                ok: false,
                result: None,
                error: Some(error),
            }),
        }
    }
    Ok(results)
}

/// Drop a pending link the user dismissed.
#[tauri::command]
#[specta::specta]
pub fn discard_pending_deeplink(nonce: String) -> bool {
    take_pending(Some(&nonce)).is_ok()
}

/// Build an unsigned `ccswitch://v2/import` link for a configured provider,
/// MCP server or prompt ("share as link"). Secrets are only included when
/// `includeSecrets` is true.
#[tauri::command]
#[specta::specta]
pub fn build_deeplink_share_link(
    state: State<AppState>,
    resource: String,
    app: String,
    id: String,
    includeSecrets: Option<bool>,
) -> Result<String, String> {
    let item = share_item(&state, &resource, &app, &id, includeSecrets == Some(true))?;
    Ok(crate::deeplink::build_v2_link(vec![item]))
}

fn share_item(
    state: &AppState,
    resource: &str,
    app: &str,
    id: &str,
    include_secrets: bool,
) -> Result<serde_json::Value, String> {
    use serde_json::{json, Value};
    match resource {
        "provider" => {
            let providers = state.db.get_all_providers(app).map_err(|e| e.to_string())?;
            let provider = providers
                .get(id)
                .ok_or_else(|| format!("Provider {id} not found"))?;
            let mut item = json!({
                "resource": "provider",
                "app": app,
                "name": provider.name,
            });
            if let Some(url) = provider.website_url.as_deref().filter(|u| !u.is_empty()) {
                item["homepage"] = json!(url);
            }
            let (endpoint, api_key, model) = provider_share_fields(app, &provider.settings_config);
            if let Some(endpoint) = endpoint {
                item["endpoint"] = json!(endpoint);
            }
            if let Some(model) = model {
                item["model"] = json!(model);
            }
            if include_secrets {
                if let Some(key) = api_key {
                    item["apiKey"] = json!(key);
                }
            }
            Ok(item)
        }
        "mcp" => {
            let servers = state.db.get_all_mcp_servers().map_err(|e| e.to_string())?;
            let server = servers
                .get(id)
                .ok_or_else(|| format!("MCP server {id} not found"))?;
            let mut spec = server.server.clone();
            if !include_secrets {
                if let Some(env) = spec.get_mut("env").and_then(Value::as_object_mut) {
                    for value in env.values_mut() {
                        *value = json!("");
                    }
                }
                if let Some(headers) = spec.get_mut("headers").and_then(Value::as_object_mut) {
                    for value in headers.values_mut() {
                        *value = json!("");
                    }
                }
            }
            let apps: Vec<String> = server
                .apps
                .enabled_apps()
                .iter()
                .map(|a| a.as_str().to_string())
                .collect();
            let apps = if apps.is_empty() {
                app.to_string()
            } else {
                apps.join(",")
            };
            Ok(json!({
                "resource": "mcp",
                "apps": apps,
                "config": { "mcpServers": { id: spec } },
            }))
        }
        "prompt" => {
            let prompts = state.db.get_prompts(app).map_err(|e| e.to_string())?;
            let prompt = prompts
                .get(id)
                .ok_or_else(|| format!("Prompt {id} not found"))?;
            use base64::Engine;
            Ok(json!({
                "resource": "prompt",
                "app": app,
                "name": prompt.name,
                "description": prompt.description,
                "content": base64::engine::general_purpose::STANDARD.encode(&prompt.content),
            }))
        }
        other => Err(format!("Sharing {other} is not supported")),
    }
}

/// Endpoint, API key and model from a stored provider config.
fn provider_share_fields(
    app: &str,
    settings: &serde_json::Value,
) -> (Option<String>, Option<String>, Option<String>) {
    use serde_json::Value;
    let s = |v: Option<&Value>| v.and_then(Value::as_str).map(str::to_string);
    match app {
        "claude" => {
            let env = settings.get("env");
            let get = |k: &str| s(env.and_then(|e| e.get(k)));
            (
                get("ANTHROPIC_BASE_URL"),
                get("ANTHROPIC_AUTH_TOKEN").or_else(|| get("ANTHROPIC_API_KEY")),
                get("ANTHROPIC_MODEL"),
            )
        }
        "codex" => {
            let config = settings
                .get("config")
                .and_then(Value::as_str)
                .and_then(|text| toml::from_str::<toml::Value>(text).ok());
            let provider_key = config
                .as_ref()
                .and_then(|c| c.get("model_provider"))
                .and_then(toml::Value::as_str)
                .map(str::to_string);
            let base_url = config.as_ref().and_then(|c| {
                provider_key
                    .as_deref()
                    .and_then(|k| c.get("model_providers")?.get(k)?.get("base_url"))
                    .or_else(|| c.get("base_url"))
                    .and_then(toml::Value::as_str)
                    .map(str::to_string)
            });
            let model = config
                .as_ref()
                .and_then(|c| c.get("model"))
                .and_then(toml::Value::as_str)
                .map(str::to_string);
            (
                base_url,
                s(settings.get("auth").and_then(|a| a.get("OPENAI_API_KEY"))),
                model,
            )
        }
        "gemini" => {
            let env = settings.get("env");
            let get = |k: &str| s(env.and_then(|e| e.get(k)));
            (
                get("GOOGLE_GEMINI_BASE_URL"),
                get("GEMINI_API_KEY"),
                get("GEMINI_MODEL"),
            )
        }
        _ => (
            s(settings
                .get("baseUrl")
                .or_else(|| settings.get("options").and_then(|o| o.get("baseURL")))),
            s(settings
                .get("apiKey")
                .or_else(|| settings.get("options").and_then(|o| o.get("apiKey")))),
            None,
        ),
    }
}

#[cfg(test)]
mod pending_tests {
    use super::*;

    fn request(resource: &str) -> DeepLinkImportRequest {
        DeepLinkImportRequest {
            version: "v1".into(),
            resource: resource.into(),
            name: Some("x".into()),
            ..Default::default()
        }
    }

    #[test]
    fn fabricated_request_without_nonce_is_rejected() {
        let err = take_pending(None).unwrap_err();
        assert!(err.contains("nonce"));
        let err = take_pending(Some("made-up")).unwrap_err();
        assert!(err.contains("expired"));
    }

    #[test]
    fn nonce_is_single_use_and_returns_the_stored_request() {
        let delivered = register_pending(request("prompt"));
        let nonce = delivered.nonce.clone().unwrap();
        let stored = take_pending(Some(&nonce)).unwrap();
        assert_eq!(stored.resource, "prompt");
        assert!(take_pending(Some(&nonce)).is_err(), "single use");
    }

    #[test]
    fn only_choice_flags_come_from_the_webview() {
        let mut stored = request("provider");
        stored.enabled = Some(true); // link asked for it
        stored.usage_script = Some("c2NyaXB0".into());
        let applied = apply_choices(stored.clone(), None, Some(true));
        assert_eq!(
            applied.enabled,
            Some(false),
            "link flag alone never enables"
        );
        assert_eq!(applied.usage_enabled, Some(true));
        let no_script = apply_choices(request("provider"), Some(true), Some(true));
        assert_eq!(no_script.usage_enabled, Some(false));
        assert_eq!(no_script.enabled, Some(true));
    }

    #[test]
    fn delivered_imports_carry_a_nonce() {
        let item = PendingDeepLink::Import {
            request: Box::new(request("skill")),
        };
        let PendingDeepLink::Import { request } = deeplink_deliver(item)
            .or_else(deeplink_frontend_ready)
            .expect("delivered or buffered")
        else {
            panic!("expected import");
        };
        assert!(request.nonce.is_some());
        let _ = take_pending(request.nonce.as_deref());
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
