use cc_switch_contract::{AppId, Provider};
use dioxus::prelude::*;
use dioxus_icons::lucide::{ExternalLink, Pencil, Trash2};

use crate::api;
use crate::t;
use crate::widgets::ProviderIcon;

#[component]
pub fn ProviderCard(
    provider: Provider,
    app: AppId,
    is_current: bool,
    busy: bool,
    on_switch: EventHandler<Provider>,
    on_delete: EventHandler<Provider>,
) -> Element {
    let is_partner = provider
        .meta
        .as_ref()
        .and_then(|m| m.is_partner)
        .unwrap_or(false);
    let hermes_managed = app == AppId::Hermes
        && cc_switch_presets::is_hermes_read_only_provider(&provider.settings_config);
    let card_class = if is_current {
        "glass-card-active border-primary/50"
    } else {
        "glass-card border-border"
    };
    let website = provider
        .website_url
        .clone()
        .filter(|u| !u.trim().is_empty());
    let switch_provider = provider.clone();
    let delete_provider = provider.clone();

    rsx! {
        li { class: "flex items-center gap-4 rounded-xl border p-4 transition-colors {card_class}",
            ProviderIcon {
                icon: provider.icon.clone(),
                name: provider.name.clone(),
                color: provider.icon_color.clone(),
                size: 36,
            }
            div { class: "min-w-0 flex-1",
                div { class: "flex flex-wrap items-center gap-2",
                    span { class: "truncate text-sm font-semibold", "{provider.name}" }
                    if is_current {
                        span { class: "rounded-md bg-primary/10 px-2 py-0.5 text-xs font-medium text-primary", {t!("provider.currentlyUsing")} }
                    }
                    if is_partner {
                        span { class: "rounded-md bg-amber-500/10 px-2 py-0.5 text-xs font-medium text-amber-600", {t!("provider.officialPartner")} }
                    }
                    if hermes_managed {
                        span { class: "rounded-md bg-muted px-2 py-0.5 text-xs text-muted-foreground", title: t!("provider.managedByHermesHint"), {t!("provider.managedByHermes")} }
                    }
                    if provider.in_failover_queue {
                        span { class: "rounded-md bg-muted px-2 py-0.5 text-xs text-muted-foreground", "Failover" }
                    }
                }
                if let Some(notes) = provider.notes.clone().filter(|n| !n.trim().is_empty()) {
                    p { class: "mt-1 truncate text-xs text-muted-foreground", "{notes}" }
                }
                if let Some(url) = website.clone() {
                    button {
                        r#type: "button",
                        class: "mt-1 inline-flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground",
                        onclick: move |_| {
                            let url = url.clone();
                            spawn(async move { let _ = api::app::open_external(&url).await; });
                        },
                        ExternalLink { size: "12px" }
                        {website.clone().unwrap_or_default()}
                    }
                }
            }
            div { class: "flex shrink-0 items-center gap-1",
                if is_current {
                    span { class: "px-3 text-xs text-muted-foreground", {t!("provider.inUse")} }
                } else {
                    button {
                        r#type: "button",
                        class: "inline-flex h-8 items-center rounded-lg border border-border px-3 text-xs font-medium hover:bg-muted disabled:opacity-50",
                        disabled: busy,
                        onclick: move |_| on_switch.call(switch_provider.clone()),
                        {t!("provider.enable")}
                    }
                }
                button {
                    r#type: "button",
                    class: "inline-flex h-8 w-8 items-center justify-center rounded-lg text-muted-foreground hover:bg-muted hover:text-foreground",
                    title: t!("common.edit"),
                    "aria-label": t!("common.edit"),
                    disabled: hermes_managed,
                    Pencil { size: "14px" }
                }
                button {
                    r#type: "button",
                    class: "inline-flex h-8 w-8 items-center justify-center rounded-lg text-muted-foreground hover:bg-destructive/10 hover:text-destructive disabled:opacity-40",
                    title: t!("common.delete"),
                    "aria-label": t!("common.delete"),
                    disabled: is_current || hermes_managed,
                    onclick: move |_| on_delete.call(delete_provider.clone()),
                    Trash2 { size: "14px" }
                }
            }
        }
    }
}
