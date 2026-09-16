//! Add / edit provider form. Port of `src/components/providers/forms/*`
//! (`ProviderForm.tsx` dispatcher, preset selector, vendor field sets) and
//! the `AddProviderDialog` / `EditProviderDialog` full-screen panels.
//!
//! Placeholder until the form port lands; the list opens it through
//! [`ProviderFormPanel`].

use cc_switch_contract::{AppId, Provider};
use dioxus::prelude::*;

use crate::t;

/// What the panel edits.
#[derive(Debug, Clone, PartialEq)]
pub enum FormMode {
    Add,
    Edit(Provider),
    /// Pre-filled copy of an existing provider (duplicate action).
    Duplicate(Provider),
}

/// Full-screen add/edit panel. `on_saved` fires after a successful
/// `add_provider` / `update_provider`; the caller invalidates its queries.
#[component]
pub fn ProviderFormPanel(
    app: AppId,
    mode: FormMode,
    on_close: EventHandler<()>,
    on_saved: EventHandler<()>,
) -> Element {
    let title = match &mode {
        FormMode::Add | FormMode::Duplicate(_) => t!("provider.addNewProvider"),
        FormMode::Edit(_) => t!("provider.editProvider"),
    };
    rsx! {
        div { class: "fixed inset-0 z-50 flex flex-col bg-background",
            header { class: "flex h-14 items-center justify-between border-b border-border px-6",
                h2 { class: "text-base font-semibold", "{title}" }
                button {
                    r#type: "button",
                    class: "rounded-lg border border-border px-3 py-1.5 text-sm hover:bg-muted",
                    onclick: move |_| on_close.call(()),
                    {t!("common.cancel")}
                }
            }
            div { class: "flex-1 overflow-y-auto p-6 text-sm text-muted-foreground",
                "The provider form for {app} is being ported."
            }
        }
    }
}
