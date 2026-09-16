//! Bridge to the Tauri runtime injected into the webview
//! (`window.__TAURI__`, requires `app.withGlobalTauri = true`).
//!
//! Arguments and results travel as JSON text so every `serde` type works
//! without caring about `Map` vs. object representations.

use cc_switch_contract::IpcError;
use js_sys::{Function, Promise, Reflect};
use serde::de::DeserializeOwned;
use serde::Serialize;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

fn tauri_namespace(path: &[&str]) -> Result<JsValue, IpcError> {
    let window = web_sys::window().ok_or_else(|| IpcError::message("no window"))?;
    let mut current: JsValue = window.into();
    let mut walked = String::from("window");
    for key in path {
        current = Reflect::get(&current, &JsValue::from_str(key))
            .map_err(|_| IpcError::message(format!("{walked}.{key} is not accessible")))?;
        walked.push('.');
        walked.push_str(key);
        if current.is_undefined() || current.is_null() {
            return Err(IpcError::message(format!(
                "{walked} is undefined; is withGlobalTauri enabled?"
            )));
        }
    }
    Ok(current)
}

fn js_error(value: JsValue) -> IpcError {
    if let Some(text) = value.as_string() {
        return IpcError::from_raw(&text);
    }
    if let Ok(text) = js_sys::JSON::stringify(&value) {
        if let Some(text) = text.as_string() {
            return IpcError::from_raw(&text);
        }
    }
    IpcError::message(format!("{value:?}"))
}

fn to_js<T: Serialize>(value: &T) -> Result<JsValue, IpcError> {
    let text = serde_json::to_string(value).map_err(|e| IpcError::message(e.to_string()))?;
    js_sys::JSON::parse(&text).map_err(js_error)
}

fn from_js<T: DeserializeOwned>(value: JsValue) -> Result<T, IpcError> {
    if value.is_undefined() || value.is_null() {
        return serde_json::from_str("null").map_err(|e| IpcError::message(e.to_string()));
    }
    let text = js_sys::JSON::stringify(&value)
        .map_err(js_error)?
        .as_string()
        .ok_or_else(|| IpcError::message("result is not JSON serializable"))?;
    serde_json::from_str(&text).map_err(|e| IpcError::message(format!("invalid response: {e}")))
}

/// Calls a Tauri command with a JSON-serializable argument object.
pub async fn invoke<T: DeserializeOwned, A: Serialize>(cmd: &str, args: &A) -> Result<T, IpcError> {
    let invoke_fn: Function = tauri_namespace(&["__TAURI__", "core", "invoke"])?.into();
    let core = tauri_namespace(&["__TAURI__", "core"])?;
    let promise: Promise = invoke_fn
        .call2(&core, &JsValue::from_str(cmd), &to_js(args)?)
        .map_err(js_error)?
        .into();
    let result = JsFuture::from(promise).await.map_err(js_error)?;
    from_js(result)
}

/// Calls a Tauri command that takes no arguments.
pub async fn invoke_no_args<T: DeserializeOwned>(cmd: &str) -> Result<T, IpcError> {
    invoke(cmd, &serde_json::json!({})).await
}

/// Handle returned by [`listen`]; dropping it does not unsubscribe, call
/// [`Unlisten::unlisten`] explicitly when the component unmounts.
pub struct Unlisten {
    function: Function,
    _closure: Closure<dyn FnMut(JsValue)>,
}

impl Unlisten {
    pub fn unlisten(self) {
        let _ = self.function.call0(&JsValue::NULL);
    }
}

/// Subscribes to a Tauri event; the payload is deserialized into `T`.
pub async fn listen<T, F>(event: &str, mut handler: F) -> Result<Unlisten, IpcError>
where
    T: DeserializeOwned + 'static,
    F: FnMut(T) + 'static,
{
    let listen_fn: Function = tauri_namespace(&["__TAURI__", "event", "listen"])?.into();
    let event_ns = tauri_namespace(&["__TAURI__", "event"])?;
    let closure = Closure::wrap(Box::new(move |raw: JsValue| {
        let payload = Reflect::get(&raw, &JsValue::from_str("payload")).unwrap_or(JsValue::NULL);
        if let Ok(value) = from_js::<T>(payload) {
            handler(value);
        }
    }) as Box<dyn FnMut(JsValue)>);
    let promise: Promise = listen_fn
        .call2(
            &event_ns,
            &JsValue::from_str(event),
            closure.as_ref().unchecked_ref(),
        )
        .map_err(js_error)?
        .into();
    let unlisten = JsFuture::from(promise).await.map_err(js_error)?;
    Ok(Unlisten {
        function: unlisten.into(),
        _closure: closure,
    })
}

/// `true` when running inside a Tauri webview (as opposed to a plain browser
/// during `dx serve` without the shell).
pub fn is_tauri() -> bool {
    tauri_namespace(&["__TAURI__", "core", "invoke"]).is_ok()
}
