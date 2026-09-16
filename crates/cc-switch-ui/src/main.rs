//! cc-switch Dioxus frontend.

// Foundation modules are landed ahead of the views that use them; drop this
// once the port is complete.
#![allow(dead_code)]

mod api;
#[allow(dead_code, unused_imports)]
mod components;
mod events;
mod i18n;
mod icons;
mod ipc;
mod nav;
mod platform;
mod query;
mod shell;
mod state;
mod storage;
mod theme;
mod views;
mod widgets;

use dioxus::prelude::*;

const BASE_CSS: Asset = asset!("/assets/base.css");
const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");
const COMPONENTS_CSS: Asset = asset!("/assets/dx-components-theme.css");

fn main() {
    dioxus::launch(Root);
}

#[component]
fn Root() -> Element {
    rsx! {
        document::Stylesheet { href: BASE_CSS }
        document::Stylesheet { href: TAILWIND_CSS }
        document::Stylesheet { href: COMPONENTS_CSS }
        shell::App {}
    }
}
