//! Settings page. Port of `src/components/settings/*` (see
//! docs/dev/specs/settings-area.md). Placeholder until the port lands.

use dioxus::prelude::*;

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
