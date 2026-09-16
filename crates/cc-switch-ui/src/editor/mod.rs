//! CodeMirror 6 editor embedded through a small JS bundle
//! (`assets/editor.js`, built from `editor-src/editor.js`). Replaces the
//! React `JsonEditor` / `MarkdownEditor`.

use dioxus::prelude::*;
use js_sys::{Function, Object, Reflect};
use wasm_bindgen::prelude::*;

use crate::t;
use crate::theme::{use_theme, Theme};

const EDITOR_JS: Asset = asset!("/assets/editor.js");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorLanguage {
    #[default]
    Json,
    Javascript,
    Markdown,
    /// TOML / .env / plain text: no syntax mode, no linter.
    Plain,
}

impl EditorLanguage {
    fn as_str(self) -> &'static str {
        match self {
            EditorLanguage::Json => "json",
            EditorLanguage::Javascript => "javascript",
            EditorLanguage::Markdown => "markdown",
            EditorLanguage::Plain => "plain",
        }
    }
}

struct Handle {
    object: Object,
    _on_change: Closure<dyn FnMut(String)>,
}

impl Handle {
    fn call(&self, method: &str, arg: Option<JsValue>) -> Option<JsValue> {
        let f: Function = Reflect::get(&self.object, &JsValue::from_str(method))
            .ok()?
            .into();
        match arg {
            Some(a) => f.call1(&self.object, &a).ok(),
            None => f.call0(&self.object).ok(),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn mount(
    element: &web_sys::Element,
    value: &str,
    language: EditorLanguage,
    dark: bool,
    placeholder: &str,
    rows: u32,
    height: Option<&str>,
    read_only: bool,
    on_change: Closure<dyn FnMut(String)>,
) -> Option<Handle> {
    let window = web_sys::window()?;
    let cc = Reflect::get(&window, &JsValue::from_str("CCEditor")).ok()?;
    if cc.is_undefined() {
        return None;
    }
    let mount_fn: Function = Reflect::get(&cc, &JsValue::from_str("mount")).ok()?.into();
    let opts = Object::new();
    let set = |k: &str, v: JsValue| {
        let _ = Reflect::set(&opts, &JsValue::from_str(k), &v);
    };
    set("value", JsValue::from_str(value));
    set("language", JsValue::from_str(language.as_str()));
    set("dark", JsValue::from_bool(dark));
    set("placeholder", JsValue::from_str(placeholder));
    set("rows", JsValue::from_f64(rows as f64));
    if let Some(h) = height {
        set("height", JsValue::from_str(h));
    }
    set("readOnly", JsValue::from_bool(read_only));
    set(
        "mustBeObjectMessage",
        JsValue::from_str(&t!("jsonEditor.mustBeObject")),
    );
    set(
        "invalidJsonMessage",
        JsValue::from_str(&t!("jsonEditor.invalidJson")),
    );
    let object: Object = mount_fn
        .call3(&cc, element, &opts, on_change.as_ref().unchecked_ref())
        .ok()?
        .into();
    Some(Handle {
        object,
        _on_change: on_change,
    })
}

/// Code editor bound to `value`. Falls back to a plain `<textarea>` when the
/// editor bundle is unavailable (e.g. blocked script), so forms stay usable.
#[component]
pub fn CodeEditor(
    value: ReadSignal<String>,
    on_change: EventHandler<String>,
    #[props(default)] language: EditorLanguage,
    #[props(default)] placeholder: String,
    #[props(default = 12)] rows: u32,
    #[props(default)] height: Option<String>,
    #[props(default)] read_only: bool,
    #[props(default)] class: String,
    #[props(default)] id: Option<String>,
) -> Element {
    let theme = use_theme();
    let mut handle: Signal<Option<Handle>> = use_signal(|| None);
    let mut fallback = use_signal(|| false);
    // The last value the editor reported, to avoid echoing it back.
    let mut last_reported = use_signal(String::new);

    // Push external value changes into the editor.
    use_effect(move || {
        let v = value();
        if v != *last_reported.peek() {
            if let Some(h) = handle.read().as_ref() {
                h.call("setValue", Some(JsValue::from_str(&v)));
            }
            last_reported.set(v);
        }
    });
    use_effect(move || {
        let dark = theme() == Theme::Dark
            || (theme() == Theme::System && crate::theme::system_prefers_dark());
        if let Some(h) = handle.read().as_ref() {
            h.call("setDark", Some(JsValue::from_bool(dark)));
        }
    });
    use_effect(move || {
        if let Some(h) = handle.read().as_ref() {
            h.call("setReadOnly", Some(JsValue::from_bool(read_only)));
        }
    });
    use_drop(move || {
        if let Some(h) = handle.write().take() {
            h.call("destroy", None);
        }
    });

    let placeholder_for_mount = placeholder.clone();
    let height_for_mount = height.clone();
    let on_mounted = move |evt: MountedEvent| {
        let Some(element) = evt.data().downcast::<web_sys::Element>().cloned() else {
            fallback.set(true);
            return;
        };
        let closure = Closure::wrap(Box::new(move |v: String| {
            last_reported.set(v.clone());
            on_change.call(v);
        }) as Box<dyn FnMut(String)>);
        let dark = theme() == Theme::Dark
            || (theme() == Theme::System && crate::theme::system_prefers_dark());
        match mount(
            &element,
            &value.peek(),
            language,
            dark,
            &placeholder_for_mount,
            rows,
            height_for_mount.as_deref(),
            read_only,
            closure,
        ) {
            Some(h) => handle.set(Some(h)),
            None => fallback.set(true),
        }
    };

    rsx! {
        document::Script { src: EDITOR_JS }
        if fallback() {
            textarea {
                id: id.clone().unwrap_or_default(),
                class: "w-full rounded-lg border border-border bg-transparent p-3 font-mono text-sm {class}",
                rows: "{rows}",
                placeholder: "{placeholder}",
                readonly: read_only,
                value: "{value}",
                oninput: move |e| on_change.call(e.value()),
            }
        } else {
            div {
                id: id.clone().unwrap_or_default(),
                class: "cc-editor text-sm {class}",
                onmounted: on_mounted,
            }
        }
    }
}
