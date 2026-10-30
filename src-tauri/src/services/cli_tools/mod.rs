//! CLI tool lifecycle and terminal launching (moved out of `commands/misc.rs`, CMD-10).
//!
//! - [`lifecycle`]: local/remote version probing, install-source detection,
//!   anchored install/update command planning and silent execution.
//! - [`terminal`]: cross-platform terminal launching (provider terminals and
//!   trusted command scripts such as `hermes dashboard`).
//!
//! Everything here is synchronous or does blocking process/file work; Tauri
//! commands call it from `spawn_blocking`.

#[allow(unused_imports)]
use crate::app_config::AppType;
#[allow(unused_imports)]
use once_cell::sync::Lazy;
#[allow(unused_imports)]
use regex::Regex;
#[allow(unused_imports)]
use std::collections::HashMap;
#[allow(unused_imports)]
use std::path::{Path, PathBuf};
#[allow(unused_imports)]
use std::str::FromStr;

#[cfg(target_os = "windows")]
#[allow(unused_imports)]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
pub(crate) const CREATE_NO_WINDOW: u32 = 0x08000000;

mod lifecycle;
mod terminal;

#[allow(unused_imports)]
pub(crate) use lifecycle::*;
#[allow(unused_imports)]
pub(crate) use terminal::*;

#[cfg(test)]
mod tests;
