use dioxus::prelude::*;
use dioxus_icons::lucide::{
    ArrowLeft, BookOpen, Boxes, FolderOpen, History, MessageSquare, Settings, Sparkles,
};

use super::AppSwitcher;
use crate::nav::View;
use crate::state::use_app_state;
use crate::{platform, t};

#[component]
pub fn Header(top: u32) -> Element {
    let state = use_app_state();
    let view = (state.view)();
    let active_app = (state.active_app)();
    let visible = state.visible_apps();
    let drag = platform::drag_region_enabled();

    rsx! {
        header {
            class: "glass-header fixed inset-x-0 z-40 flex items-center justify-between border-b border-border px-6",
            style: "top: {top}px; height: {super::HEADER_HEIGHT}px",
            "data-tauri-drag-region": drag,
            div { class: "flex items-center gap-3", style: "-webkit-app-region: no-drag",
                if let Some(title_key) = view.title_key() {
                    button {
                        r#type: "button",
                        class: "inline-flex h-9 w-9 items-center justify-center rounded-lg border border-border bg-background/60 text-foreground hover:bg-muted",
                        title: t!("common.back"),
                        onclick: move |_| state.set_view(view.back_target()),
                        ArrowLeft { size: "16px" }
                    }
                    h1 { class: "text-lg font-semibold", {t!(title_key)} }
                } else {
                    AppSwitcher {
                        active_app,
                        visible_apps: visible.clone(),
                        on_switch: move |app| state.set_active_app(app),
                    }
                }
                if view != View::Providers && view.shows_app_switcher() {
                    AppSwitcher {
                        active_app,
                        visible_apps: visible,
                        compact: true,
                        on_switch: move |app| state.set_active_app(app),
                    }
                }
            }
            div { class: "flex items-center gap-1", style: "-webkit-app-region: no-drag",
                HeaderButton { icon_view: View::Prompts, label: t!("prompts.title"), current: view, BookOpen { size: "16px" } }
                HeaderButton { icon_view: View::Skills, label: t!("skills.title"), current: view, Sparkles { size: "16px" } }
                HeaderButton { icon_view: View::Mcp, label: t!("mcp.unifiedPanel.title"), current: view, Boxes { size: "16px" } }
                HeaderButton { icon_view: View::Sessions, label: t!("sessionManager.title"), current: view, History { size: "16px" } }
                HeaderButton { icon_view: View::Workspace, label: t!("workspace.title"), current: view, FolderOpen { size: "16px" } }
                HeaderButton { icon_view: View::Universal, label: t!("universalProvider.title"), current: view, MessageSquare { size: "16px" } }
                HeaderButton { icon_view: View::Settings, label: t!("settings.title"), current: view, Settings { size: "16px" } }
            }
        }
    }
}

#[component]
fn HeaderButton(icon_view: View, label: String, current: View, children: Element) -> Element {
    let state = use_app_state();
    let active = current == icon_view;
    let class = if active {
        "bg-muted text-foreground"
    } else {
        "text-muted-foreground hover:bg-muted hover:text-foreground"
    };
    rsx! {
        button {
            r#type: "button",
            class: "inline-flex h-9 w-9 items-center justify-center rounded-lg transition-colors {class}",
            title: "{label}",
            "aria-label": "{label}",
            onclick: move |_| state.set_view(icon_view),
            {children}
        }
    }
}
