//! Implementation of the `ccs` subcommands.
//!
//! Every command goes through the same services the GUI commands use
//! (`ProviderService`, `ProxyService`, `Database`), so live files, backups and
//! takeover rules behave identically.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use indexmap::IndexMap;
use serde_json::{json, Value};

use super::lock::{self, AcquireError, LockHeld, LockOwner, WriterLock};
use super::{CliError, Output};
use crate::app_config::AppType;
use crate::database::{Database, SCHEMA_VERSION};
use crate::provider::Provider;
use crate::services::ProviderService;
use crate::store::AppState;

/// File a running headless proxy polls for; `ccs proxy stop` creates it.
pub const PROXY_STOP_FILE_NAME: &str = "proxy-daemon.stop";

/// Resolved runtime context for one CLI invocation.
#[derive(Debug, Clone)]
pub struct Context {
    pub config_dir: PathBuf,
}

impl Context {
    pub fn db_path(&self) -> PathBuf {
        self.config_dir.join("cc-switch.db")
    }

    pub fn stop_file(&self) -> PathBuf {
        self.config_dir.join(PROXY_STOP_FILE_NAME)
    }
}

// ─── database / lock helpers ────────────────────────────────

/// Open the shared database with the same checks the GUI does before writing:
/// a database from a newer app version is never touched, and a not yet
/// migrated legacy `config.json` is left for the GUI's interactive migration.
pub fn open_db(ctx: &Context) -> Result<Arc<Database>, CliError> {
    let db_path = ctx.db_path();
    if !db_path.exists() && ctx.config_dir.join("config.json").exists() {
        return Err(CliError::Failed(format!(
            "{} holds a legacy config.json that has not been migrated yet. Open the CC Switch desktop app once to migrate it, then retry.",
            ctx.config_dir.display()
        )));
    }
    match Database::stored_user_version_exceeds_supported(&db_path) {
        Ok(Some(version)) => {
            return Err(CliError::Failed(format!(
                "The database at {} is schema v{version}, newer than this ccs supports (v{SCHEMA_VERSION}). Update CC Switch.",
                db_path.display()
            )))
        }
        Ok(None) => {}
        Err(e) => log::warn!("database version precheck failed: {e}"),
    }
    Database::init()
        .map(Arc::new)
        .map_err(|e| CliError::Failed(format!("Failed to open {}: {e}", db_path.display())))
}

fn lock_hint(held: &LockHeld) -> &'static str {
    match held.owner() {
        Some(LockOwner::Gui) => {
            "Make the change in the desktop app (window or tray menu), or quit it and run the command again."
        }
        Some(LockOwner::HeadlessProxy) => {
            "Stop the headless proxy first (`ccs proxy stop`), or make the change while it is stopped."
        }
        Some(LockOwner::Cli) => "Wait for the other ccs command to finish and try again.",
        None => "Close the other CC Switch process and try again.",
    }
}

/// Take the writer lock or explain who has it.
pub fn acquire_writer(ctx: &Context, owner: LockOwner) -> Result<WriterLock, CliError> {
    match WriterLock::try_acquire(&ctx.config_dir, owner) {
        Ok(lock) => Ok(lock),
        Err(AcquireError::Held(held)) => Err(CliError::Locked {
            hint: lock_hint(&held).to_string(),
            held,
        }),
        Err(AcquireError::Io(e)) => Err(CliError::Failed(format!(
            "Could not create the CC Switch lock file: {e}"
        ))),
    }
}

fn app_err(e: impl std::fmt::Display) -> CliError {
    CliError::Failed(e.to_string())
}

fn current_thread_runtime() -> Result<tokio::runtime::Runtime, CliError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| CliError::Failed(format!("Failed to start async runtime: {e}")))
}

// ─── list / current ─────────────────────────────────────────

fn apps_or_all(app: Option<AppType>) -> Vec<AppType> {
    match app {
        Some(app) => vec![app],
        None => AppType::all().collect(),
    }
}

fn provider_json(provider: &Provider, current: bool) -> Value {
    json!({
        "id": provider.id,
        "name": provider.name,
        "category": provider.category,
        "current": current,
    })
}

pub fn list(ctx: &Context, app: Option<AppType>) -> Result<Output, CliError> {
    let state = AppState::new(open_db(ctx)?);
    let single = app.is_some();
    let mut apps_json = Vec::new();
    let mut text = String::new();
    for app in apps_or_all(app) {
        let providers: IndexMap<String, Provider> =
            ProviderService::list(&state, app.clone()).map_err(app_err)?;
        let current = ProviderService::current(&state, app.clone()).map_err(app_err)?;
        apps_json.push(json!({
            "app": app.as_str(),
            "current": (!current.is_empty()).then_some(current.clone()),
            "providers": providers
                .values()
                .map(|p| provider_json(p, p.id == current))
                .collect::<Vec<_>>(),
        }));
        if providers.is_empty() && !single {
            continue;
        }
        text.push_str(app.as_str());
        text.push('\n');
        if providers.is_empty() {
            text.push_str("  (no providers)\n");
        }
        let width = providers.keys().map(|k| k.len()).max().unwrap_or(0);
        for p in providers.values() {
            let marker = if p.id == current { '*' } else { ' ' };
            text.push_str(&format!("  {marker} {:<width$}  {}\n", p.id, p.name));
        }
    }
    if text.is_empty() {
        text.push_str("No providers configured.\n");
    }
    let json = if single {
        apps_json.into_iter().next().unwrap_or(Value::Null)
    } else {
        json!({ "apps": apps_json })
    };
    Ok(Output { text, json })
}

pub fn current(ctx: &Context, app: Option<AppType>) -> Result<Output, CliError> {
    let state = AppState::new(open_db(ctx)?);
    let single = app.is_some();
    let mut entries = Vec::new();
    let mut text = String::new();
    for app in apps_or_all(app) {
        if app.is_additive_mode() && !single {
            continue;
        }
        let id = ProviderService::current(&state, app.clone()).map_err(app_err)?;
        let provider = if id.is_empty() {
            None
        } else {
            state
                .db
                .get_provider_by_id(&id, app.as_str())
                .map_err(app_err)?
        };
        if single {
            match &provider {
                Some(p) => text.push_str(&format!("{} ({})\n", p.id, p.name)),
                None if app.is_additive_mode() => text.push_str(&format!(
                    "{} has no single current provider (all enabled providers are written to its config).\n",
                    app.as_str()
                )),
                None => text.push_str("(none)\n"),
            }
        } else {
            let label = provider
                .as_ref()
                .map(|p| format!("{} ({})", p.id, p.name))
                .unwrap_or_else(|| "(none)".to_string());
            text.push_str(&format!("{:<15} {label}\n", app.as_str()));
        }
        entries.push(json!({
            "app": app.as_str(),
            "provider": provider.as_ref().map(|p| json!({ "id": p.id, "name": p.name })),
        }));
    }
    let json = if single {
        entries.into_iter().next().unwrap_or(Value::Null)
    } else {
        json!({ "apps": entries })
    };
    Ok(Output { text, json })
}

// ─── switch ─────────────────────────────────────────────────

/// Resolve a provider argument: exact id first, then a unique
/// case-insensitive name match.
pub fn resolve_provider_id(
    providers: &IndexMap<String, Provider>,
    query: &str,
) -> Result<String, String> {
    if providers.contains_key(query) {
        return Ok(query.to_string());
    }
    let wanted = query.trim().to_lowercase();
    let matches: Vec<&Provider> = providers
        .values()
        .filter(|p| p.name.trim().to_lowercase() == wanted)
        .collect();
    match matches.as_slice() {
        [one] => Ok(one.id.clone()),
        [] => {
            let available = providers
                .values()
                .map(|p| format!("{} ({})", p.id, p.name))
                .collect::<Vec<_>>()
                .join(", ");
            Err(if available.is_empty() {
                format!("No provider '{query}': this app has no providers.")
            } else {
                format!("No provider '{query}'. Available: {available}")
            })
        }
        many => Err(format!(
            "'{query}' matches several providers by name ({}); use the id instead.",
            many.iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

pub fn switch(ctx: &Context, app: AppType, provider: &str) -> Result<Output, CliError> {
    let _lock = acquire_writer(ctx, LockOwner::Cli)?;
    if app.is_additive_mode() {
        return Err(CliError::Failed(format!(
            "{} has no single current provider; enable or disable providers in the desktop app instead.",
            app.as_str()
        )));
    }
    let state = AppState::new(open_db(ctx)?);
    let providers = ProviderService::list(&state, app.clone()).map_err(app_err)?;
    let id = resolve_provider_id(&providers, provider).map_err(CliError::Failed)?;
    let name = providers
        .get(&id)
        .map(|p| p.name.clone())
        .unwrap_or_default();
    let result = ProviderService::switch(&state, app.clone(), &id).map_err(app_err)?;
    let mut text = format!("Switched {} to {id} ({name}).\n", app.as_str());
    for w in &result.warnings {
        text.push_str(&format!("warning: {w}\n"));
    }
    Ok(Output {
        text,
        json: json!({
            "app": app.as_str(),
            "provider": { "id": id, "name": name },
            "warnings": result.warnings,
        }),
    })
}

// ─── usage ──────────────────────────────────────────────────

pub fn usage_summary(ctx: &Context, days: u32, app: Option<AppType>) -> Result<Output, CliError> {
    let db = open_db(ctx)?;
    let now = chrono::Utc::now().timestamp();
    let start = now - i64::from(days) * 86_400;
    let app_str = app.as_ref().map(|a| a.as_str().to_string());
    let summary = db
        .get_usage_summary(Some(start), Some(now), app_str.as_deref(), None, None)
        .map_err(app_err)?;
    let mut json = serde_json::to_value(&summary).map_err(app_err)?;
    if let Some(obj) = json.as_object_mut() {
        obj.insert("days".into(), json!(days));
        obj.insert("app".into(), json!(app_str));
        obj.insert("startDate".into(), json!(start));
        obj.insert("endDate".into(), json!(now));
    }
    let scope = app_str.as_deref().unwrap_or("all apps");
    let text = format!(
        "Usage, last {days} day(s), {scope}:\n  requests      {}\n  cost (USD)    {}\n  input tokens  {}\n  output tokens {}\n  cache read    {}\n  cache write   {}\n  success rate  {:.1}%\n",
        summary.total_requests,
        summary.total_cost,
        summary.total_input_tokens,
        summary.total_output_tokens,
        summary.total_cache_read_tokens,
        summary.total_cache_creation_tokens,
        summary.success_rate,
    );
    Ok(Output { text, json })
}

// ─── export / import ────────────────────────────────────────

pub fn export(ctx: &Context, file: &Path) -> Result<Output, CliError> {
    let db = open_db(ctx)?;
    db.export_sql(file).map_err(app_err)?;
    Ok(Output {
        text: format!("Exported to {}\n", file.display()),
        json: json!({ "success": true, "filePath": file.display().to_string() }),
    })
}

pub fn import(ctx: &Context, file: &Path) -> Result<Output, CliError> {
    let _lock = acquire_writer(ctx, LockOwner::Cli)?;
    if !file.is_file() {
        return Err(CliError::Failed(format!(
            "{} is not a file",
            file.display()
        )));
    }
    let db = open_db(ctx)?;
    let backup_id = db.import_sql(file).map_err(app_err)?;
    // Same post-import sync as the GUI: write the imported current providers to
    // the live files and reload settings.
    let state = AppState::new(db);
    let warning = ProviderService::sync_current_to_live(&state)
        .and_then(|_| crate::settings::reload_settings())
        .err()
        .map(|e| format!("Imported, but syncing live config files failed: {e}"));
    let mut text = format!(
        "Imported {} (previous database backed up as {backup_id}).\n",
        file.display()
    );
    if let Some(w) = &warning {
        text.push_str(&format!("warning: {w}\n"));
    }
    Ok(Output {
        text,
        json: json!({ "success": true, "backupId": backup_id, "warning": warning }),
    })
}

// ─── proxy status / stop ────────────────────────────────────

fn probe_host(address: &str) -> &str {
    match address {
        "0.0.0.0" | "" => "127.0.0.1",
        "::" | "[::]" => "[::1]",
        other => other,
    }
}

fn probe_status(address: &str, port: u16) -> Option<Value> {
    if port == 0 {
        return None;
    }
    let host = probe_host(address);
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    let url = format!("http://{host}:{port}/status");
    let rt = current_thread_runtime().ok()?;
    rt.block_on(async {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
            .ok()?;
        let resp = client.get(&url).send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        resp.json::<Value>().await.ok()
    })
}

fn owner_label(owner: Option<LockOwner>) -> Option<&'static str> {
    owner.map(|o| match o {
        LockOwner::Gui => "gui",
        LockOwner::HeadlessProxy => "headless",
        LockOwner::Cli => "ccs",
    })
}

pub fn proxy_status(ctx: &Context) -> Result<Output, CliError> {
    let db = open_db(ctx)?;
    let state = AppState::new(db.clone());
    let rt = current_thread_runtime()?;
    let (config, takeover) = rt.block_on(async {
        let config = db.get_proxy_config().await.map_err(app_err)?;
        let takeover = state
            .proxy_service
            .get_takeover_status()
            .await
            .map_err(app_err)?;
        Ok::<_, CliError>((config, takeover))
    })?;
    drop(rt);
    let holder = lock::current_holder(&ctx.config_dir);
    let managed_by = owner_label(holder.as_ref().and_then(|h| h.owner()));
    let live = probe_status(&config.listen_address, config.listen_port);
    let running = live
        .as_ref()
        .and_then(|v| v.get("running"))
        .and_then(Value::as_bool)
        .unwrap_or(live.is_some());
    let taken_over: Vec<&str> = [
        ("claude", takeover.claude),
        ("codex", takeover.codex),
        ("gemini", takeover.gemini),
        ("grokbuild", takeover.grokbuild),
        ("opencode", takeover.opencode),
        ("openclaw", takeover.openclaw),
    ]
    .into_iter()
    .filter_map(|(name, on)| on.then_some(name))
    .collect();

    let mut text = if running {
        format!(
            "Proxy: running on {}:{}{}\n",
            config.listen_address,
            config.listen_port,
            match managed_by {
                Some("gui") => " (managed by the desktop app)",
                Some("headless") => " (headless)",
                _ => "",
            }
        )
    } else {
        format!(
            "Proxy: not running (configured for {}:{})\n",
            config.listen_address, config.listen_port
        )
    };
    text.push_str(&format!(
        "Takeover: {}\n",
        if taken_over.is_empty() {
            "none".to_string()
        } else {
            taken_over.join(", ")
        }
    ));
    if let Some(stats) = &live {
        if let Some(total) = stats.get("total_requests").and_then(Value::as_u64) {
            text.push_str(&format!("Requests since start: {total}\n"));
        }
    }
    Ok(Output {
        text,
        json: json!({
            "running": running,
            "managedBy": managed_by,
            "pid": holder.as_ref().and_then(|h| h.holder.as_ref()).map(|h| h.pid),
            "address": config.listen_address,
            "port": config.listen_port,
            "takeover": takeover,
            "status": live,
        }),
    })
}

pub fn proxy_stop(ctx: &Context, timeout: Duration) -> Result<Output, CliError> {
    let Some(held) = lock::current_holder(&ctx.config_dir) else {
        return Ok(Output {
            text: "No headless proxy is running.\n".into(),
            json: json!({ "stopped": false, "reason": "not-running" }),
        });
    };
    match held.owner() {
        Some(LockOwner::Gui) => {
            return Err(CliError::Failed(
                "The proxy is run by the CC Switch desktop app; stop it there (Settings → Proxy) or quit the app.".into(),
            ))
        }
        Some(LockOwner::Cli) => {
            return Err(CliError::Locked {
                hint: lock_hint(&held).to_string(),
                held,
            })
        }
        Some(LockOwner::HeadlessProxy) | None => {}
    }
    let stop_file = ctx.stop_file();
    std::fs::write(&stop_file, std::process::id().to_string()).map_err(|e| {
        CliError::Failed(format!(
            "Failed to request shutdown ({}): {e}",
            stop_file.display()
        ))
    })?;
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if lock::current_holder(&ctx.config_dir).is_none() {
            let _ = std::fs::remove_file(&stop_file);
            return Ok(Output {
                text: "Headless proxy stopped; live configs restored.\n".into(),
                json: json!({ "stopped": true }),
            });
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err(CliError::Failed(format!(
        "The headless proxy did not stop within {}s (pid {}).",
        timeout.as_secs(),
        held.holder
            .map(|h| h.pid.to_string())
            .unwrap_or_else(|| "unknown".into())
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(id: &str, name: &str) -> Provider {
        Provider::with_id(id.into(), name.into(), json!({}), None)
    }

    #[test]
    fn resolves_by_id_then_unique_name() {
        let mut map = IndexMap::new();
        map.insert("a1".to_string(), provider("a1", "PackyCode"));
        map.insert("b2".to_string(), provider("b2", "Official"));
        map.insert("c3".to_string(), provider("c3", "Dup"));
        map.insert("d4".to_string(), provider("d4", "dup"));
        assert_eq!(resolve_provider_id(&map, "b2").unwrap(), "b2");
        assert_eq!(resolve_provider_id(&map, "packycode").unwrap(), "a1");
        let err = resolve_provider_id(&map, "DUP").unwrap_err();
        assert!(err.contains("c3") && err.contains("d4"));
        let err = resolve_provider_id(&map, "nope").unwrap_err();
        assert!(err.contains("Available"));
        assert!(resolve_provider_id(&IndexMap::new(), "x")
            .unwrap_err()
            .contains("no providers"));
    }

    #[test]
    fn probe_host_maps_wildcards_to_loopback() {
        assert_eq!(probe_host("0.0.0.0"), "127.0.0.1");
        assert_eq!(probe_host("::"), "[::1]");
        assert_eq!(probe_host("127.0.0.1"), "127.0.0.1");
    }
}
