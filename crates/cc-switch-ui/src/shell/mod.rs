//! Application shell: title bar, app switcher, header actions and the view
//! router. Ported from `src/App.tsx`.

mod app_switcher;
mod header;
mod views;

use dioxus::prelude::*;

use crate::components::toast::ToastProvider;
use crate::events::use_tauri_event;
use crate::nav::View;
use crate::query::use_query_client_provider;
use crate::state::use_app_state_provider;
use crate::theme::use_theme_provider;
use crate::{platform, t};
use cc_switch_contract::events;

pub use app_switcher::AppSwitcher;

pub const HEADER_HEIGHT: u32 = 64;

#[component]
pub fn App() -> Element {
    let state = use_app_state_provider();
    let _theme = use_theme_provider();
    let queries = use_query_client_provider();

    // Backend events that change cached data.
    use_tauri_event::<serde_json::Value, _>(events::PROVIDER_SWITCHED, move |_| {
        queries.invalidate_prefix("providers:");
        queries.invalidate_prefix("current-provider:");
    });
    use_tauri_event::<serde_json::Value, _>(events::PROFILE_APPLIED, move |_| {
        queries.invalidate_all();
        spawn(async move { state.reload_settings().await });
    });
    use_tauri_event::<serde_json::Value, _>(events::UNIVERSAL_PROVIDER_SYNCED, move |_| {
        queries.invalidate_prefix("providers:");
    });

    let use_window_controls = state
        .settings
        .read()
        .as_ref()
        .map(|s| s.use_app_window_controls)
        .unwrap_or(false);
    let drag_bar_height = if use_window_controls {
        32
    } else {
        platform::default_drag_bar_height()
    };
    let content_top = drag_bar_height + HEADER_HEIGHT;
    let view = (state.view)();

    rsx! {
        ToastProvider {
            div { class: "min-h-screen bg-background text-foreground",
                if drag_bar_height > 0 {
                    div {
                        class: "fixed inset-x-0 top-0 z-50",
                        style: "height: {drag_bar_height}px",
                        "data-tauri-drag-region": platform::drag_region_enabled(),
                    }
                }
                header::Header { top: drag_bar_height }
                main {
                    class: "mx-auto max-w-6xl px-6 pb-8",
                    style: "padding-top: {content_top + 16}px",
                    if let Some(err) = (state.settings_error)() {
                        div { class: "mb-4 rounded-lg border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive",
                            {t!("common.error")} ": {err}"
                        }
                    }
                    ViewRouter { view }
                }
            }
        }
    }
}

#[component]
fn ViewRouter(view: View) -> Element {
    match view {
        View::Providers => rsx! { crate::views::providers::ProvidersView {} },
        View::Settings => rsx! { views::SettingsView {} },
        other => rsx! { views::PlaceholderView { view: other } },
    }
}
