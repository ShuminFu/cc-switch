//! Subscribing to backend events from components.

use std::cell::RefCell;
use std::rc::Rc;

use dioxus::prelude::*;
use serde::de::DeserializeOwned;

use crate::ipc;

/// Listens to a Tauri event for the lifetime of the component. The handler
/// runs on every delivery with the deserialized payload.
pub fn use_tauri_event<T, F>(event: &'static str, handler: F)
where
    T: DeserializeOwned + 'static,
    F: FnMut(T) + 'static,
{
    let slot: Rc<RefCell<Option<ipc::Unlisten>>> = use_hook(|| {
        let slot = Rc::new(RefCell::new(None));
        let slot_for_task = slot.clone();
        spawn(async move {
            if let Ok(unlisten) = ipc::listen::<T, F>(event, handler).await {
                *slot_for_task.borrow_mut() = Some(unlisten);
            }
        });
        slot
    });
    use_drop(move || {
        if let Some(unlisten) = slot.borrow_mut().take() {
            unlisten.unlisten();
        }
    });
}
