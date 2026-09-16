use cc_switch_contract::{AppId, VisibleApps};
use dioxus::prelude::*;
use dioxus_icons::lucide::{Monitor, Terminal};

use crate::nav::{app_display_name, app_icon_name};
use crate::widgets::ProviderIcon;

/// Row of tool buttons, ported from `src/components/AppSwitcher.tsx`.
#[component]
pub fn AppSwitcher(
    active_app: AppId,
    visible_apps: VisibleApps,
    on_switch: EventHandler<AppId>,
    #[props(default)] compact: bool,
) -> Element {
    let apps: Vec<AppId> = AppId::ALL
        .iter()
        .copied()
        .filter(|a| visible_apps.is_visible(*a))
        .collect();
    let label_class = if compact {
        "max-w-0 opacity-0 ml-0"
    } else {
        "max-w-[120px] opacity-100 ml-2"
    };

    rsx! {
        div { class: "inline-flex bg-muted rounded-xl p-1 gap-1",
            for app in apps {
                {
                    let is_active = app == active_app;
                    let button_class = if is_active {
                        "bg-background text-foreground shadow-sm"
                    } else {
                        "text-muted-foreground hover:text-foreground hover:bg-background/50"
                    };
                    let badge_class = if is_active {
                        "bg-background border-border text-foreground"
                    } else {
                        "bg-muted border-background text-muted-foreground group-hover:bg-background group-hover:text-foreground"
                    };
                    rsx! {
                        button {
                            key: "{app}",
                            r#type: "button",
                            class: "group inline-flex items-center px-3 h-8 rounded-md text-sm font-medium transition-all duration-200 {button_class}",
                            onclick: move |_| on_switch.call(app),
                            span { class: "relative inline-flex shrink-0",
                                ProviderIcon { icon: app_icon_name(app).to_string(), name: app_display_name(app).to_string(), size: 20 }
                                if app == AppId::Claude {
                                    span { class: "absolute -bottom-0.5 -right-0.5 flex items-center justify-center rounded-[3px] border h-[11px] w-[11px] {badge_class}", "aria-hidden": "true",
                                        Terminal { class: "h-[8px] w-[8px]", size: "8px", stroke_width: "2.5" }
                                    }
                                }
                                if app == AppId::ClaudeDesktop {
                                    span { class: "absolute -bottom-0.5 -right-0.5 flex items-center justify-center rounded-[3px] border h-[11px] w-[11px] {badge_class}", "aria-hidden": "true",
                                        Monitor { class: "h-[8px] w-[8px] translate-y-[0.5px]", size: "8px", stroke_width: "2.5" }
                                    }
                                }
                            }
                            span { class: "transition-all duration-200 whitespace-nowrap overflow-hidden {label_class}",
                                {app_display_name(app)}
                            }
                        }
                    }
                }
            }
        }
    }
}
