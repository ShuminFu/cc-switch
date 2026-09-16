use std::collections::HashMap;

use cc_switch_contract::{AppId, IpcError, Provider};
use dioxus::prelude::*;
use dioxus_icons::lucide::{Plus, Search};
use dioxus_primitives::toast::{use_toast, ToastOptions};

use super::card::ProviderCard;
use crate::api;
use crate::components::alert_dialog::{
    AlertDialog, AlertDialogAction, AlertDialogActions, AlertDialogCancel, AlertDialogDescription,
    AlertDialogTitle,
};
use crate::query::{keys, use_query, use_query_client};
use crate::state::use_app_state;
use crate::t;

/// Providers of `app` sorted like the React list: by `sortIndex`, then by
/// creation time, then by name.
pub fn sorted_providers(map: &HashMap<String, Provider>) -> Vec<Provider> {
    let mut list: Vec<Provider> = map.values().cloned().collect();
    list.sort_by(|a, b| {
        a.sort_index
            .cmp(&b.sort_index)
            .then_with(|| a.created_at.cmp(&b.created_at))
            .then_with(|| a.name.cmp(&b.name))
    });
    list
}

/// Case-insensitive match on name, notes and website URL.
pub fn matches_query(provider: &Provider, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    provider.name.to_lowercase().contains(&q)
        || provider
            .notes
            .as_deref()
            .map(|n| n.to_lowercase().contains(&q))
            .unwrap_or(false)
        || provider
            .website_url
            .as_deref()
            .map(|u| u.to_lowercase().contains(&q))
            .unwrap_or(false)
}

#[component]
pub fn ProvidersView() -> Element {
    let state = use_app_state();
    let app = state.active_app;
    rsx! { ProviderList { app } }
}

#[component]
fn ProviderList(app: ReadSignal<AppId>) -> Element {
    let queries = use_query_client();
    let toast = use_toast();
    let providers = use_query(
        move || keys::providers(app()),
        move || api::providers::get_all(app()),
    );
    let current = use_query(
        move || keys::current_provider(app()),
        move || api::providers::get_current(app()),
    );
    let mut search = use_signal(String::new);
    let mut pending_delete: Signal<Option<Provider>> = use_signal(|| None);
    let mut busy = use_signal(|| false);

    // Reset transient state when the user switches tools.
    use_effect(move || {
        let _ = app();
        search.set(String::new());
        pending_delete.set(None);
    });

    let refresh = move || {
        let app = app();
        queries.invalidate(&keys::providers(app));
        queries.invalidate(&keys::current_provider(app));
    };

    let on_switch = move |provider: Provider| {
        if busy() {
            return;
        }
        busy.set(true);
        spawn(async move {
            match api::providers::switch(&provider.id, app()).await {
                Ok(_) => {
                    toast.success(t!("notifications.switchSuccess"), ToastOptions::default());
                    refresh();
                }
                Err(err) => toast.error(
                    t!("common.error"),
                    ToastOptions::new().description(err.to_string()),
                ),
            }
            busy.set(false);
        });
    };

    let confirm_delete = move |_| {
        let Some(provider) = pending_delete() else {
            return;
        };
        spawn(async move {
            match api::providers::delete(&provider.id, app()).await {
                Ok(_) => {
                    toast.success(t!("notifications.providerDeleted"), ToastOptions::default());
                    refresh();
                }
                Err(err) => toast.error(
                    t!("common.error"),
                    ToastOptions::new().description(err.to_string()),
                ),
            }
            pending_delete.set(None);
        });
    };

    let current_id = current
        .read()
        .as_ref()
        .and_then(|r| r.as_ref().ok().cloned())
        .unwrap_or_default();

    rsx! {
        div { class: "space-y-4",
            div { class: "flex items-center justify-between gap-3",
                label { class: "relative flex-1 max-w-md",
                    Search { class: "pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground", size: "16px" }
                    input {
                        r#type: "search",
                        class: "h-9 w-full rounded-lg border border-border bg-background/60 pl-9 pr-3 text-sm outline-none focus:ring-2 focus:ring-ring",
                        placeholder: t!("provider.searchPlaceholder"),
                        "aria-label": t!("provider.searchAriaLabel"),
                        value: "{search}",
                        oninput: move |e| search.set(e.value()),
                    }
                }
                button {
                    r#type: "button",
                    class: "inline-flex h-9 items-center gap-2 rounded-lg bg-primary px-3 text-sm font-medium text-primary-foreground hover:bg-primary/90",
                    onclick: move |_| toast.info(t!("provider.addProvider"), ToastOptions::new().description("The add-provider form is ported in Phase 3.")),
                    Plus { size: "16px" }
                    {t!("provider.addProvider")}
                }
            }
            match &*providers.read() {
                None => rsx! { p { class: "text-sm text-muted-foreground", {t!("common.loading")} } },
                Some(Err(err)) => rsx! { ErrorBox { error: err.clone() } },
                Some(Ok(map)) => {
                    let list: Vec<Provider> = sorted_providers(map)
                        .into_iter()
                        .filter(|p| matches_query(p, &search()))
                        .collect();
                    if map.is_empty() {
                        rsx! {
                            section { class: "glass-card rounded-xl border border-dashed border-border p-8 text-center",
                                h2 { class: "text-base font-semibold", {t!("provider.noProviders")} }
                                p { class: "mt-2 text-sm text-muted-foreground", {t!("provider.noProvidersDescription")} }
                            }
                        }
                    } else if list.is_empty() {
                        rsx! { p { class: "text-sm text-muted-foreground", {t!("provider.noSearchResults")} } }
                    } else {
                        rsx! {
                            ul { class: "space-y-2",
                                for provider in list {
                                    ProviderCard {
                                        key: "{provider.id}",
                                        provider: provider.clone(),
                                        app: app(),
                                        is_current: provider.id == current_id,
                                        busy: busy(),
                                        on_switch: on_switch,
                                        on_delete: move |p| pending_delete.set(Some(p)),
                                    }
                                }
                            }
                        }
                    }
                }
            }
            AlertDialog {
                open: pending_delete().is_some(),
                on_open_change: move |open: bool| { if !open { pending_delete.set(None); } },
                AlertDialogTitle { {t!("provider.deleteProvider")} }
                AlertDialogDescription {
                    {pending_delete().map(|p| p.name).unwrap_or_default()}
                }
                AlertDialogActions {
                    AlertDialogCancel { {t!("common.cancel")} }
                    AlertDialogAction { on_click: confirm_delete, {t!("common.delete")} }
                }
            }
        }
    }
}

#[component]
fn ErrorBox(error: IpcError) -> Element {
    rsx! {
        div { class: "rounded-lg border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive",
            "{error}"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(id: &str, sort: Option<usize>, created: Option<i64>) -> Provider {
        Provider {
            id: id.into(),
            name: id.to_uppercase(),
            sort_index: sort,
            created_at: created,
            notes: Some(format!("note {id}")),
            website_url: Some(format!("https://{id}.example")),
            ..Provider::default()
        }
    }

    #[test]
    fn sorts_by_sort_index_then_created_then_name() {
        let map: HashMap<String, Provider> = [
            ("b".to_string(), provider("b", Some(1), None)),
            ("a".to_string(), provider("a", Some(0), None)),
            ("d".to_string(), provider("d", None, Some(2))),
            ("c".to_string(), provider("c", None, Some(1))),
        ]
        .into_iter()
        .collect();
        let ids: Vec<String> = sorted_providers(&map).into_iter().map(|p| p.id).collect();
        assert_eq!(ids, vec!["c", "d", "a", "b"]);
    }

    #[test]
    fn search_matches_name_notes_and_url() {
        let p = provider("kimi", None, None);
        assert!(matches_query(&p, ""));
        assert!(matches_query(&p, "KIMI"));
        assert!(matches_query(&p, "note kimi"));
        assert!(matches_query(&p, "kimi.example"));
        assert!(!matches_query(&p, "openai"));
    }
}
