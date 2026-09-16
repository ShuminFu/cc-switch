//! View bodies. Each ported area replaces its placeholder here.

use dioxus::prelude::*;

use crate::nav::View;
use crate::state::use_app_state;
use crate::t;

#[component]
pub fn SettingsView() -> Element {
    let state = use_app_state();
    let json = state
        .settings
        .read()
        .as_ref()
        .and_then(|s| serde_json::to_string_pretty(s).ok())
        .unwrap_or_default();
    rsx! {
        section { class: "glass-card rounded-xl border border-border p-6",
            h2 { class: "text-base font-semibold", {t!("settings.title")} }
            details { class: "mt-3 text-xs",
                summary { class: "cursor-pointer text-muted-foreground", "Raw JSON" }
                pre { class: "mt-2 overflow-x-auto font-mono", "{json}" }
            }
        }
    }
}

#[component]
pub fn PlaceholderView(view: View) -> Element {
    let title = view
        .title_key()
        .map(crate::i18n::t)
        .unwrap_or_else(|| view.as_str().to_string());
    rsx! {
        section { class: "glass-card rounded-xl border border-dashed border-border p-6 text-sm text-muted-foreground",
            h2 { class: "text-base font-semibold text-foreground", "{title}" }
            p { class: "mt-2", "This view has not been ported to Dioxus yet." }
        }
    }
}
