//! Tauri 命令。参数与命令名一律 snake_case（Tauri 将参数映射为前端的 camelCase 键）；
//! 用 forbid 而非 deny，子模块无法再用 `allow(non_snake_case)` 绕过（CCS-118）。
#![forbid(non_snake_case)]

mod apps;
mod auth;
mod balance;
mod cli_tools;
mod codex_oauth;
mod coding_plan;
mod config;
mod copilot;
mod deeplink;
mod drift;
mod env;
mod failover;
mod global_proxy;
mod hermes;
mod import_export;
mod mcp;
mod model_fetch;
mod omo;
mod openclaw;
mod plugin;
mod profile;
mod prompt;
mod provider;
mod proxy;
mod session_manager;
mod settings;
pub mod skill;
mod stream_check;
mod subscription;
mod sync_support;
mod system;
mod terminal;

mod s3_sync;
mod usage;
mod webdav_sync;
mod workspace;

pub use apps::*;
pub use auth::*;
pub use balance::*;
pub use cli_tools::*;
pub use codex_oauth::*;
pub use coding_plan::*;
pub use config::*;
pub use copilot::*;
pub use deeplink::*;
pub use drift::*;
pub use env::*;
pub use failover::*;
pub use global_proxy::*;
pub use hermes::*;
pub use import_export::*;
pub use mcp::*;
pub use model_fetch::*;
pub use omo::*;
pub use openclaw::*;
pub use plugin::*;
pub use profile::*;
pub use prompt::*;
pub use provider::*;
pub use proxy::*;
pub use session_manager::*;
pub use settings::*;
pub use skill::*;
pub use stream_check::*;
pub use subscription::*;
pub use system::*;
pub use terminal::*;

pub use s3_sync::*;
pub use usage::*;
pub use webdav_sync::*;
pub use workspace::*;
