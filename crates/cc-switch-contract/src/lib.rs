//! Contract between the cc-switch Tauri backend (`src-tauri`) and its Rust
//! frontend (`crates/cc-switch-ui`).
//!
//! The crate has no Tauri dependency so it compiles for `wasm32`. Types here
//! describe the JSON that crosses `invoke`, matching the backend's serde
//! attributes; the backend's own structs stay where they are and a test in
//! `src-tauri` checks the two agree. Command and event names are constants so
//! the frontend never spells a command as a bare string.

pub mod app;
pub mod commands;
pub mod config;
pub mod error;
pub mod events;
pub mod mcp;
pub mod profiles;
pub mod prompts;
pub mod provider;
pub mod proxy;
pub mod sessions;
pub mod settings;
pub mod skills;
pub mod universal;
pub mod usage;

pub use app::AppId;
pub use error::IpcError;
pub use provider::{AuthBinding, AuthBindingSource, CustomEndpoint, Provider, ProviderMeta};
pub use settings::{AppSettings, VisibleApps};

#[cfg(test)]
mod tests {
    #[test]
    fn command_list_is_sorted_and_unique() {
        let all = super::commands::ALL;
        let mut sorted = all.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted, all);
        assert!(all.contains(&super::commands::settings::GET_SETTINGS));
    }
}
