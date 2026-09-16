//! View bodies. Each ported area replaces its placeholder here.

use dioxus::prelude::*;

use crate::nav::View;

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
