//! Light / dark / system theme, mirroring `src/components/theme-provider.tsx`:
//! persisted under `cc-switch-theme`, applied as a class on `<html>`, and
//! mirrored to the native window through `set_window_theme`.

use dioxus::prelude::*;
use wasm_bindgen::prelude::*;

use crate::{api, storage};

pub const STORAGE_KEY: &str = "cc-switch-theme";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    Light,
    Dark,
    #[default]
    System,
}

impl Theme {
    pub const ALL: [Theme; 3] = [Theme::Light, Theme::Dark, Theme::System];

    pub fn as_str(self) -> &'static str {
        match self {
            Theme::Light => "light",
            Theme::Dark => "dark",
            Theme::System => "system",
        }
    }

    pub fn from_str(s: &str) -> Option<Theme> {
        Theme::ALL.into_iter().find(|t| t.as_str() == s)
    }

    pub fn initial() -> Theme {
        storage::get(STORAGE_KEY)
            .and_then(|s| Theme::from_str(&s))
            .unwrap_or_default()
    }
}

fn system_prefers_dark() -> bool {
    web_sys::window()
        .and_then(|w| w.match_media("(prefers-color-scheme: dark)").ok().flatten())
        .map(|mq| mq.matches())
        .unwrap_or(false)
}

/// Sets the `light`/`dark` class on the document element.
pub fn apply(theme: Theme) {
    let Some(root) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
    else {
        return;
    };
    let dark = match theme {
        Theme::Light => false,
        Theme::Dark => true,
        Theme::System => system_prefers_dark(),
    };
    let list = root.class_list();
    let _ = list.remove_2("light", "dark");
    let _ = list.add_1(if dark { "dark" } else { "light" });
}

/// Provides `Signal<Theme>` to the tree and keeps storage, the DOM class and
/// the native window in sync.
pub fn use_theme_provider() -> Signal<Theme> {
    let theme = use_context_provider(|| Signal::new(Theme::initial()));

    use_effect(move || {
        let current = theme();
        storage::set(STORAGE_KEY, current.as_str());
        apply(current);
        spawn(async move {
            // Errors are ignored: outside Tauri there is no native window.
            let _ = api::app::set_window_theme(current.as_str()).await;
        });
    });

    // Follow OS changes while in system mode.
    use_hook(move || {
        if let Some(mq) = web_sys::window()
            .and_then(|w| w.match_media("(prefers-color-scheme: dark)").ok().flatten())
        {
            let closure = Closure::<dyn FnMut()>::new(move || {
                if theme() == Theme::System {
                    apply(Theme::System);
                }
            });
            let _ = mq.add_event_listener_with_callback("change", closure.as_ref().unchecked_ref());
            closure.forget();
        }
    });

    theme
}

pub fn use_theme() -> Signal<Theme> {
    use_context::<Signal<Theme>>()
}
