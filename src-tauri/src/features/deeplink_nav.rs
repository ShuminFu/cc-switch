//! Navigation and action deep links (PRODUCT-20).
//!
//! Next to the `/import` links handled by `crate::deeplink`, CC Switch accepts:
//!
//! * `ccswitch://v1/open?view=usage|proxy|providers|mcp|skills|prompts|sessions|universal|settings[&tab=…][&app=…]`
//! * `ccswitch://v1/switch?app=claude&provider=<id or name>`
//! * `ccswitch://v1/profile/apply?name=<project name>[&scope=claude]`
//!
//! `open` only navigates. `switch` and `profile/apply` mutate configuration, so
//! the frontend asks for confirmation before doing anything; a managed policy
//! that disables deep-link imports also disables them.
//!
//! The parsed action is kept in a one-slot mailbox and the frontend is poked
//! with a `deeplink-action` event; it takes the action with
//! `take_deeplink_action`, both on that event and once on start-up, so links
//! that arrive before the webview listens (cold start) are not lost.

use std::str::FromStr;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{Emitter, Manager};
use url::Url;

use crate::app_config::AppType;
use crate::error::AppError;

pub const DEEPLINK_ACTION_EVENT: &str = "deeplink-action";

const VIEWS: &[&str] = &[
    "providers",
    "settings",
    "usage",
    "proxy",
    "mcp",
    "skills",
    "prompts",
    "sessions",
    "universal",
];
const SETTINGS_TABS: &[&str] = &["general", "proxy", "auth", "advanced", "usage", "about"];
const MAX_VALUE_LEN: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DeepLinkAction {
    #[serde(rename_all = "camelCase")]
    Open {
        view: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        tab: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        app: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Switch { app: String, provider: String },
    #[serde(rename_all = "camelCase")]
    ApplyProfile {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        scope: Option<String>,
    },
}

fn invalid(msg: impl Into<String>) -> AppError {
    AppError::InvalidInput(msg.into())
}

fn param(url: &Url, key: &str) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn bounded(value: String, key: &str) -> Result<String, AppError> {
    if value.chars().count() > MAX_VALUE_LEN || value.chars().any(char::is_control) {
        return Err(invalid(format!("Invalid '{key}' parameter")));
    }
    Ok(value)
}

fn app_param(url: &Url) -> Result<Option<String>, AppError> {
    param(url, "app")
        .map(|a| AppType::from_str(&a).map(|t| t.as_str().to_string()))
        .transpose()
}

/// Parse an action deep link. Returns `None` for links this module does not
/// handle (e.g. `/import`), so the import parser keeps owning them.
pub fn parse_action(url_str: &str) -> Option<Result<DeepLinkAction, AppError>> {
    let url = Url::parse(url_str).ok()?;
    if url.scheme() != "ccswitch" {
        return None;
    }
    let version = url.host_str()?;
    if version != "v1" && version != "v2" {
        return None;
    }
    let path = url.path().trim_end_matches('/');
    let result = match path {
        "/open" => parse_open(&url),
        "/switch" => parse_switch(&url),
        "/profile/apply" => parse_profile(&url),
        _ => return None,
    };
    Some(result)
}

fn parse_open(url: &Url) -> Result<DeepLinkAction, AppError> {
    let view = param(url, "view").unwrap_or_else(|| "providers".to_string());
    if !VIEWS.contains(&view.as_str()) {
        return Err(invalid(format!(
            "Unknown view '{view}'. Allowed: {}",
            VIEWS.join(", ")
        )));
    }
    let tab = param(url, "tab");
    if let Some(tab) = tab.as_deref() {
        if view != "settings" || !SETTINGS_TABS.contains(&tab) {
            return Err(invalid(format!("Unknown settings tab '{tab}'")));
        }
    }
    Ok(DeepLinkAction::Open {
        view,
        tab,
        app: app_param(url)?,
    })
}

fn ensure_mutation_allowed() -> Result<(), AppError> {
    crate::features::policy::enforce_deeplink_allowed()
}

fn parse_switch(url: &Url) -> Result<DeepLinkAction, AppError> {
    ensure_mutation_allowed()?;
    let app = app_param(url)?.ok_or_else(|| invalid("Missing 'app' parameter"))?;
    let provider = param(url, "provider").ok_or_else(|| invalid("Missing 'provider' parameter"))?;
    Ok(DeepLinkAction::Switch {
        app,
        provider: bounded(provider, "provider")?,
    })
}

fn parse_profile(url: &Url) -> Result<DeepLinkAction, AppError> {
    ensure_mutation_allowed()?;
    let name = param(url, "name").ok_or_else(|| invalid("Missing 'name' parameter"))?;
    Ok(DeepLinkAction::ApplyProfile {
        name: bounded(name, "name")?,
        scope: param(url, "scope")
            .map(|a| AppType::from_str(&a).map(|t| t.as_str().to_string()))
            .transpose()?,
    })
}

static PENDING: Mutex<Option<DeepLinkAction>> = Mutex::new(None);

/// Keep `action` for the frontend (newest wins) and notify it.
pub fn stash<R: tauri::Runtime>(app: &tauri::AppHandle<R>, action: DeepLinkAction) {
    *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(action);
    if let Err(e) = app.emit(DEEPLINK_ACTION_EVENT, ()) {
        log::warn!("Failed to emit {DEEPLINK_ACTION_EVENT}: {e}");
    }
}

/// Bring the main window forward after an action link.
pub fn focus_main_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn take() -> Option<DeepLinkAction> {
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// Take the pending action deep link, if any (delivered at most once).
#[tauri::command]
pub fn take_deeplink_action() -> Option<DeepLinkAction> {
    take()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(url: &str) -> DeepLinkAction {
        parse_action(url).expect("handled").expect("valid")
    }

    #[test]
    fn import_links_are_left_to_the_import_parser() {
        assert!(parse_action("ccswitch://v1/import?resource=provider").is_none());
        assert!(parse_action("https://v1/open?view=usage").is_none());
        assert!(parse_action("ccswitch://v9/open?view=usage").is_none());
    }

    #[test]
    fn open_links_map_to_views_and_tabs() {
        assert_eq!(
            ok("ccswitch://v1/open?view=usage"),
            DeepLinkAction::Open {
                view: "usage".into(),
                tab: None,
                app: None
            }
        );
        assert_eq!(
            ok("ccswitch://v1/open?view=settings&tab=advanced&app=Codex"),
            DeepLinkAction::Open {
                view: "settings".into(),
                tab: Some("advanced".into()),
                app: Some("codex".into())
            }
        );
        assert!(parse_action("ccswitch://v1/open?view=evil")
            .unwrap()
            .is_err());
        assert!(parse_action("ccswitch://v1/open?view=mcp&tab=about")
            .unwrap()
            .is_err());
        assert!(parse_action("ccswitch://v1/open?view=usage&app=nope")
            .unwrap()
            .is_err());
    }

    #[test]
    fn switch_and_profile_links_require_their_parameters() {
        assert_eq!(
            ok("ccswitch://v1/switch?app=claude&provider=My%20Relay"),
            DeepLinkAction::Switch {
                app: "claude".into(),
                provider: "My Relay".into()
            }
        );
        assert!(parse_action("ccswitch://v1/switch?app=claude")
            .unwrap()
            .is_err());
        assert!(parse_action("ccswitch://v1/switch?provider=x")
            .unwrap()
            .is_err());
        let long = "x".repeat(MAX_VALUE_LEN + 1);
        assert!(
            parse_action(&format!("ccswitch://v1/switch?app=claude&provider={long}"))
                .unwrap()
                .is_err()
        );

        assert_eq!(
            ok("ccswitch://v1/profile/apply?name=Work&scope=codex"),
            DeepLinkAction::ApplyProfile {
                name: "Work".into(),
                scope: Some("codex".into())
            }
        );
        assert!(parse_action("ccswitch://v1/profile/apply")
            .unwrap()
            .is_err());
    }

    #[test]
    fn serializes_with_a_kind_tag() {
        let v = serde_json::to_value(DeepLinkAction::Switch {
            app: "claude".into(),
            provider: "p".into(),
        })
        .unwrap();
        assert_eq!(
            v,
            serde_json::json!({"kind":"switch","app":"claude","provider":"p"})
        );
    }

    #[test]
    fn mailbox_delivers_once() {
        *PENDING.lock().unwrap() = Some(DeepLinkAction::Open {
            view: "usage".into(),
            tab: None,
            app: None,
        });
        assert!(take().is_some());
        assert!(take().is_none());
    }
}
