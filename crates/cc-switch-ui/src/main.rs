//! cc-switch Dioxus frontend. Phase 0: proves the toolchain by rendering the
//! backend's settings inside the Tauri window.

// Phase 0 only exercises `get_settings`; the rest of the bridge is wired in
// Phase 2. Keep clippy quiet until then.
#[allow(dead_code)]
mod api;
#[allow(dead_code, unused_imports)]
mod components;
#[allow(dead_code)]
mod i18n;
#[allow(dead_code)]
mod ipc;

use cc_switch_contract::{AppId, AppSettings, IpcError};
use dioxus::prelude::*;

const BASE_CSS: Asset = asset!("/assets/base.css");
const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");
const COMPONENTS_CSS: Asset = asset!("/assets/dx-components-theme.css");

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    let settings = use_resource(|| async move {
        if !ipc::is_tauri() {
            return Err(IpcError::message(
                "Not running inside Tauri: window.__TAURI__ is missing",
            ));
        }
        api::settings::get_settings().await
    });

    rsx! {
        document::Stylesheet { href: BASE_CSS }
        document::Stylesheet { href: TAILWIND_CSS }
        document::Stylesheet { href: COMPONENTS_CSS }
        div { class: "min-h-screen bg-background text-foreground",
            header {
                class: "glass-header flex h-16 items-center justify-between border-b border-border px-6",
                "data-tauri-drag-region": true,
                h1 { class: "text-lg font-semibold", "CC Switch" }
                span { class: "rounded-md bg-primary/10 px-2 py-1 text-xs text-primary",
                    "Dioxus preview"
                }
            }
            main { class: "mx-auto max-w-3xl space-y-4 p-6",
                match &*settings.read_unchecked() {
                    None => rsx! { p { class: "text-muted-foreground", "Loading settings…" } },
                    Some(Err(err)) => rsx! {
                        div { class: "rounded-lg border border-destructive/40 bg-destructive/10 p-4 text-sm text-destructive",
                            "Failed to load settings: {err}"
                        }
                    },
                    Some(Ok(s)) => rsx! { SettingsSummary { settings: s.clone() } },
                }
            }
        }
    }
}

#[component]
fn SettingsSummary(settings: AppSettings) -> Element {
    let visible = settings.visible_apps.clone().unwrap_or_default();
    let apps: Vec<AppId> = AppId::ALL
        .iter()
        .copied()
        .filter(|a| visible.is_visible(*a))
        .collect();
    let json = serde_json::to_string_pretty(&settings).unwrap_or_default();
    rsx! {
        section { class: "glass-card rounded-xl border border-border p-4",
            h2 { class: "mb-2 text-sm font-medium text-muted-foreground", "Backend settings loaded" }
            ul { class: "grid grid-cols-2 gap-2 text-sm",
                li { "Language: " b { {settings.language.clone().unwrap_or_else(|| "system".into())} } }
                li { "Tray: " b { "{settings.show_in_tray}" } }
                li { "Local proxy: " b { "{settings.enable_local_proxy}" } }
                li { "Failover toggle: " b { "{settings.enable_failover_toggle}" } }
            }
            p { class: "mt-3 text-sm",
                "Visible apps: "
                for app in apps {
                    span { class: "mr-1 rounded bg-muted px-2 py-0.5 text-xs", "{app}" }
                }
            }
        }
        details { class: "rounded-xl border border-border p-4 text-xs",
            summary { class: "cursor-pointer text-muted-foreground", "Raw JSON" }
            pre { class: "mt-2 overflow-x-auto font-mono", "{json}" }
        }
    }
}
