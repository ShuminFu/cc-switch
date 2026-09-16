//! Application-wide reactive state, provided once at the root.

use cc_switch_contract::{AppId, AppSettings, IpcError, VisibleApps};
use dioxus::prelude::*;

use crate::i18n::Locale;
use crate::nav::{self, View};
use crate::{api, storage};

#[derive(Clone, Copy)]
pub struct AppState {
    pub settings: Signal<Option<AppSettings>>,
    pub settings_error: Signal<Option<IpcError>>,
    pub active_app: Signal<AppId>,
    pub view: Signal<View>,
    pub locale: Signal<Locale>,
}

impl AppState {
    pub fn visible_apps(&self) -> VisibleApps {
        self.settings
            .read()
            .as_ref()
            .and_then(|s| s.visible_apps.clone())
            .unwrap_or_default()
    }

    pub fn set_view(&self, view: View) {
        if *self.view.read() == view {
            return;
        }
        storage::set(nav::VIEW_STORAGE_KEY, view.as_str());
        let mut signal = self.view;
        signal.set(view);
    }

    pub fn set_active_app(&self, app: AppId) {
        if *self.active_app.read() == app {
            return;
        }
        storage::set(nav::APP_STORAGE_KEY, app.as_str());
        let mut signal = self.active_app;
        signal.set(app);
    }

    pub fn set_locale(&self, locale: Locale) {
        storage::set("language", locale.code());
        let mut signal = self.locale;
        signal.set(locale);
    }

    /// Fetches settings from the backend and applies the language it holds.
    pub async fn reload_settings(&self) {
        let mut settings = self.settings;
        let mut error = self.settings_error;
        match api::settings::get().await {
            Ok(loaded) => {
                if let Some(locale) = loaded.language.as_deref().and_then(Locale::from_code) {
                    if *self.locale.read() != locale {
                        let mut signal = self.locale;
                        signal.set(locale);
                    }
                }
                settings.set(Some(loaded));
                error.set(None);
            }
            Err(err) => error.set(Some(err)),
        }
    }
}

fn initial_locale() -> Locale {
    if let Some(locale) = storage::get("language").and_then(|s| Locale::from_code(&s)) {
        return locale;
    }
    web_sys::window()
        .and_then(|w| w.navigator().language())
        .and_then(|tag| Locale::from_navigator(&tag))
        .unwrap_or(Locale::DEFAULT)
}

/// Creates the state, provides it as context and kicks off the initial load.
pub fn use_app_state_provider() -> AppState {
    let state = use_context_provider(|| AppState {
        settings: Signal::new(None),
        settings_error: Signal::new(None),
        active_app: Signal::new(nav::initial_app()),
        view: Signal::new(nav::initial_view()),
        locale: Signal::new(initial_locale()),
    });
    use_hook(move || {
        spawn(async move {
            state.reload_settings().await;
        });
    });
    state
}

pub fn use_app_state() -> AppState {
    use_context::<AppState>()
}
