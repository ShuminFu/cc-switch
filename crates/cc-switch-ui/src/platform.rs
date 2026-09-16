//! Lightweight platform detection from the user agent, mirroring
//! `src/lib/platform.ts`.

use std::sync::OnceLock;

fn user_agent() -> &'static str {
    static UA: OnceLock<String> = OnceLock::new();
    UA.get_or_init(|| {
        web_sys::window()
            .and_then(|w| w.navigator().user_agent().ok())
            .unwrap_or_default()
    })
}

fn platform() -> &'static str {
    static PLATFORM: OnceLock<String> = OnceLock::new();
    PLATFORM.get_or_init(|| {
        web_sys::window()
            .and_then(|w| w.navigator().platform().ok())
            .unwrap_or_default()
            .to_lowercase()
    })
}

pub fn is_mac() -> bool {
    user_agent().to_lowercase().contains("mac") || platform().contains("mac")
}

pub fn is_windows() -> bool {
    let ua = user_agent().to_lowercase();
    ua.contains("windows") || ua.contains("win32") || ua.contains("win64")
}

pub fn is_linux() -> bool {
    let ua = user_agent().to_lowercase();
    (ua.contains("linux") || ua.contains("x11"))
        && !ua.contains("android")
        && !is_mac()
        && !is_windows()
}

/// Drag regions are disabled on Linux (Tauri #13440), like the React app.
pub fn drag_region_enabled() -> bool {
    !is_linux()
}

/// Height of the transparent drag strip above the header.
pub fn default_drag_bar_height() -> u32 {
    if is_windows() || is_linux() {
        0
    } else {
        28
    }
}
