//! Headless proxy (PRODUCT-10): run the local proxy in the foreground without
//! a window or tray.
//!
//! Used by `ccs proxy start` and `cc-switch --headless`. The lifecycle mirrors
//! the GUI exactly:
//!
//! 1. startup: repair takeover residue from a crash (`recover_from_crash`),
//!    then re-enable takeover for every app whose `proxy_config.enabled` flag is
//!    set (same as `restore_proxy_state_on_startup` in lib.rs), and make sure the
//!    listener is running;
//! 2. shutdown (Ctrl-C, SIGTERM, SIGHUP or `ccs proxy stop`): restore live
//!    configs while keeping the per-app takeover flags
//!    (`stop_with_restore_keep_state`, same as `cleanup_before_exit`), and flush
//!    queued usage logs.
//!
//! The process holds the writer lock for its whole lifetime, so the GUI and
//! `ccs switch` cannot rewrite live files underneath it.

use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;

use super::args::ProxyStartArgs;
use super::lock::LockOwner;
use super::ops::{acquire_writer, open_db, Context};
use super::{CliError, Output};
use crate::database::Database;
use crate::store::AppState;

// Takeover is restored for `app_registry::takeover_app_types()`, like the GUI startup.

/// Why the foreground loop ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    Signal,
    StopFile,
}

pub fn run_foreground(
    ctx: &Context,
    args: &ProxyStartArgs,
    json_mode: bool,
    out: &mut dyn Write,
) -> Result<Output, CliError> {
    let _lock = acquire_writer(ctx, LockOwner::HeadlessProxy)?;
    // A stop request left over from an earlier run must not stop this one.
    let _ = std::fs::remove_file(ctx.stop_file());

    let db = open_db(ctx)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("ccs-proxy")
        .build()
        .map_err(|e| CliError::Failed(format!("Failed to start async runtime: {e}")))?;

    let state = AppState::new(db.clone());
    let started = runtime.block_on(start(&state, args.no_takeover))?;

    let event = json!({
        "event": "started",
        "address": started.address,
        "port": started.port,
        "takeover": started.takeover,
        "pid": std::process::id(),
        "configDir": ctx.config_dir.display().to_string(),
    });
    if json_mode {
        let _ = writeln!(out, "{event}");
    } else {
        let _ = writeln!(
            out,
            "CC Switch proxy listening on {}:{} (pid {}).",
            started.address,
            started.port,
            std::process::id()
        );
        if started.takeover.is_empty() {
            let _ = writeln!(out, "No app is taken over; point clients at the proxy manually or enable takeover in Settings → Proxy.");
        } else {
            let _ = writeln!(out, "Taken over: {}", started.takeover.join(", "));
        }
        let _ = writeln!(out, "Press Ctrl-C (or run `ccs proxy stop`) to stop.");
    }
    let _ = out.flush();

    // Daily maintenance (backups, log pruning) like the GUI's background task.
    {
        let db = db.clone();
        runtime.spawn(async move {
            loop {
                let db = db.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    if let Err(e) = db.periodic_backup_if_needed() {
                        log::warn!("periodic maintenance failed: {e}");
                    }
                })
                .await;
                tokio::time::sleep(Duration::from_secs(24 * 60 * 60)).await;
            }
        });
    }

    let stop_file = ctx.stop_file();
    let reason = runtime.block_on(wait_for_shutdown(stop_file.clone()));
    let restore_result = runtime.block_on(shutdown(&state));
    flush_usage(&db);
    let _ = std::fs::remove_file(&stop_file);
    drop(runtime);

    let text = match &restore_result {
        Ok(()) => "Proxy stopped; live configs restored.\n".to_string(),
        Err(e) => format!("Proxy stopped, but restoring live configs failed: {e}\n"),
    };
    let json = json!({
        "event": "stopped",
        "reason": match reason { StopReason::Signal => "signal", StopReason::StopFile => "stop-request" },
        "restoreError": restore_result.as_ref().err(),
    });
    match restore_result {
        Ok(()) => Ok(Output { text, json }),
        Err(e) => Err(CliError::Failed(format!(
            "Proxy stopped, but restoring live configs failed: {e}. They will be repaired on the next start."
        ))),
    }
}

struct Started {
    address: String,
    port: u16,
    takeover: Vec<String>,
}

async fn start(state: &AppState, no_takeover: bool) -> Result<Started, CliError> {
    let proxy = &state.proxy_service;

    let has_backups = state.db.has_any_live_backup().await.unwrap_or_else(|e| {
        log::error!("checking live backups failed: {e}");
        false
    });
    if has_backups || proxy.detect_takeover_in_live_configs() {
        log::warn!("takeover residue found; restoring live configs first");
        if let Err(e) = proxy.recover_from_crash().await {
            log::error!("restoring live configs failed: {e}");
        }
    }

    let mut takeover = Vec::new();
    if !no_takeover {
        for app_type in crate::app_registry::takeover_app_types() {
            let app = app_type.as_str();
            let enabled = state
                .db
                .get_proxy_config_for_app(app)
                .await
                .is_ok_and(|c| c.enabled);
            if !enabled {
                continue;
            }
            match proxy.set_takeover_for_app(app, true).await {
                Ok(()) => takeover.push(app.to_string()),
                Err(e) => {
                    log::error!("restoring takeover for {app} failed: {e}");
                    let _ = proxy.set_takeover_for_app(app, false).await;
                }
            }
        }
    }

    let info = proxy
        .start()
        .await
        .map_err(|e| CliError::Failed(format!("Failed to start the proxy: {e}")))?;
    Ok(Started {
        address: info.address,
        port: info.port,
        takeover,
    })
}

async fn wait_for_shutdown(stop_file: std::path::PathBuf) -> StopReason {
    let poll_stop_file = async {
        loop {
            if stop_file.exists() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
    };
    tokio::select! {
        _ = shutdown_signal() => StopReason::Signal,
        _ = poll_stop_file => StopReason::StopFile,
    }
}

#[cfg(unix)]
async fn shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let term = signal(SignalKind::terminate());
    let hup = signal(SignalKind::hangup());
    match (term, hup) {
        (Ok(mut term), Ok(mut hup)) => {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
                _ = hup.recv() => {}
            }
        }
        _ => {
            let _ = tokio::signal::ctrl_c().await;
        }
    }
}

#[cfg(not(unix))]
async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

/// Same decision as `cleanup_before_exit` in lib.rs.
async fn shutdown(state: &AppState) -> Result<(), String> {
    let proxy = &state.proxy_service;
    let has_backups = state.db.has_any_live_backup().await.unwrap_or(false);
    if has_backups || proxy.detect_takeover_in_live_configs() {
        return proxy.stop_with_restore_keep_state().await;
    }
    if proxy.is_running().await {
        proxy.stop().await?;
    }
    Ok(())
}

fn flush_usage(db: &Arc<Database>) {
    if let Err(e) = db.flush_usage_writes() {
        log::warn!("flushing usage logs on exit failed: {e}");
    }
}
