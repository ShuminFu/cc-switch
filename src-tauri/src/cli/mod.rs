//! `ccs` command-line companion (PRODUCT-11) and the headless proxy mode of the
//! desktop binary (PRODUCT-10).
//!
//! The CLI links the same library as the GUI and drives the same services, so
//! a `ccs switch` writes live files, backups and takeover state exactly like a
//! click in the app. See `docs/cli.md`.

pub mod args;
pub mod config_dir;
pub mod headless;
pub mod lock;
pub mod ops;

use std::ffi::OsString;
use std::io::Write;
use std::time::Duration;

use clap::Parser;
use serde_json::{json, Value};

use args::{Cli, Command, ProxyAction, ProxyStartArgs, UsageAction};
use lock::{LockHeld, LockOwner, WriterLock};
use ops::Context;

/// Exit codes (documented in docs/cli.md).
pub const EXIT_OK: i32 = 0;
pub const EXIT_FAILED: i32 = 1;
pub const EXIT_USAGE: i32 = 2;
pub const EXIT_LOCKED: i32 = 3;

/// Result of a command: human text and the `--json` payload.
#[derive(Debug, Clone)]
pub struct Output {
    pub text: String,
    pub json: Value,
}

#[derive(Debug, Clone)]
pub enum CliError {
    /// Another process holds the writer lock.
    Locked {
        held: LockHeld,
        hint: String,
    },
    Failed(String),
}

impl CliError {
    pub fn exit_code(&self) -> i32 {
        match self {
            CliError::Locked { .. } => EXIT_LOCKED,
            CliError::Failed(_) => EXIT_FAILED,
        }
    }

    pub fn message(&self) -> String {
        match self {
            CliError::Locked { held, hint } => format!("{} {hint}", held.message()),
            CliError::Failed(msg) => msg.clone(),
        }
    }

    fn json(&self) -> Value {
        match self {
            CliError::Locked { held, .. } => json!({
                "error": self.message(),
                "code": "locked",
                "lockedBy": held.holder,
            }),
            CliError::Failed(_) => json!({ "error": self.message(), "code": "failed" }),
        }
    }
}

// ─── logging ────────────────────────────────────────────────

struct StderrLogger;

impl log::Log for StderrLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!(
                "[{} {}] {}",
                record.level().as_str().to_lowercase(),
                record.target(),
                record.args()
            );
        }
    }

    fn flush(&self) {}
}

static LOGGER: StderrLogger = StderrLogger;

fn init_logging(verbose: u8, long_running: bool) {
    let level = match (verbose, long_running) {
        (0, false) => log::LevelFilter::Error,
        (0, true) | (1, _) => log::LevelFilter::Info,
        (2, _) => log::LevelFilter::Debug,
        _ => log::LevelFilter::Trace,
    };
    // Ignore "already set" (tests call `run` repeatedly in one process).
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(level);
}

fn install_crypto_provider() {
    // Same process-wide TLS provider the GUI installs in setup (ring only).
    let _ = rustls::crypto::ring::default_provider().install_default();
}

// ─── entry points ───────────────────────────────────────────

/// `ccs` entry point. Returns the process exit code.
pub fn main() -> i32 {
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    run(std::env::args_os(), &mut stdout.lock(), &mut stderr.lock())
}

/// Parse `args` (including argv\[0]) and run the command, writing to `out` /
/// `err`. Used by the binary and by the integration tests.
pub fn run<I, T>(args: I, out: &mut dyn Write, err: &mut dyn Write) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(e) => {
            let code = if e.use_stderr() { EXIT_USAGE } else { EXIT_OK };
            let rendered = e.render().to_string();
            if e.use_stderr() {
                let _ = write!(err, "{rendered}");
            } else {
                let _ = write!(out, "{rendered}");
            }
            return code;
        }
    };
    let long_running = matches!(
        cli.command,
        Command::Proxy {
            action: ProxyAction::Start(_)
        }
    );
    init_logging(cli.verbose, long_running);
    install_crypto_provider();

    let (config_dir, _source) = config_dir::apply(cli.config_dir.as_deref());
    let ctx = Context { config_dir };
    let result = dispatch(&ctx, &cli, out);
    finish(result, cli.json, out, err)
}

fn dispatch(ctx: &Context, cli: &Cli, out: &mut dyn Write) -> Result<Output, CliError> {
    match &cli.command {
        Command::List { app } => ops::list(ctx, app.clone()),
        Command::Current { app } => ops::current(ctx, app.clone()),
        Command::Switch { app, provider } => ops::switch(ctx, app.clone(), provider),
        Command::Proxy { action } => match action {
            ProxyAction::Status => ops::proxy_status(ctx),
            ProxyAction::Start(args) => headless::run_foreground(ctx, args, cli.json, out),
            ProxyAction::Stop { timeout } => ops::proxy_stop(ctx, Duration::from_secs(*timeout)),
        },
        Command::Usage {
            action: UsageAction::Summary { days, app },
        } => ops::usage_summary(ctx, *days, app.clone()),
        Command::Export { file } => ops::export(ctx, file),
        Command::Import { file } => ops::import(ctx, file),
    }
}

fn finish(
    result: Result<Output, CliError>,
    json_mode: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    match result {
        Ok(output) => {
            if json_mode {
                let _ = writeln!(
                    out,
                    "{}",
                    serde_json::to_string_pretty(&output.json).unwrap_or_default()
                );
            } else {
                let _ = write!(out, "{}", output.text);
            }
            let _ = out.flush();
            EXIT_OK
        }
        Err(e) => {
            if json_mode {
                let _ = writeln!(
                    out,
                    "{}",
                    serde_json::to_string_pretty(&e.json()).unwrap_or_default()
                );
            }
            let _ = writeln!(err, "ccs: {}", e.message());
            e.exit_code()
        }
    }
}

// ─── desktop binary hooks (main.rs) ─────────────────────────

/// Arguments of `cc-switch --headless`.
#[derive(Debug, Parser)]
#[command(
    name = "cc-switch",
    about = "Run the CC Switch proxy without a window or tray"
)]
pub struct HeadlessCli {
    /// Run headless (required)
    #[arg(long)]
    pub headless: bool,
    #[command(flatten)]
    pub start: ProxyStartArgs,
    /// Use this CC Switch data directory instead of the configured one
    #[arg(long, value_name = "DIR")]
    pub config_dir: Option<std::path::PathBuf>,
    /// Print JSON events instead of text
    #[arg(long)]
    pub json: bool,
    /// Log more (-v info, -vv debug)
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,
}

/// True when the desktop binary was started with `--headless`.
pub fn is_headless_invocation<I, T>(args: I) -> bool
where
    I: IntoIterator<Item = T>,
    T: AsRef<std::ffi::OsStr>,
{
    args.into_iter().skip(1).any(|a| a.as_ref() == "--headless")
}

/// `cc-switch --headless`: start the proxy without creating a window or tray.
pub fn run_headless<I, T>(args: I) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let (mut out, mut err) = (stdout.lock(), stderr.lock());
    let cli = match HeadlessCli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(e) => {
            let _ = write!(err, "{}", e.render());
            return if e.use_stderr() { EXIT_USAGE } else { EXIT_OK };
        }
    };
    init_logging(cli.verbose, true);
    install_crypto_provider();
    let (config_dir, _) = config_dir::apply(cli.config_dir.as_deref());
    let ctx = Context { config_dir };
    let result = headless::run_foreground(&ctx, &cli.start, cli.json, &mut out);
    finish(result, cli.json, &mut out, &mut err)
}

/// Called by the desktop app at launch: take the writer lock for the GUI's
/// lifetime so `ccs` write commands refuse to run while the app is open.
///
/// Never blocks or fails the launch. If the lock is busy (a second instance
/// that the single-instance plugin is about to hand off, an app restart where
/// the old process is still exiting, or a headless proxy), it keeps retrying
/// in the background for a short while.
pub fn acquire_gui_writer_lock() {
    // Resolve the directory the GUI will use: its store override (if any) or
    // the default. Env/flag overrides are CLI-only and deliberately ignored.
    let dir = config_dir::gui_store_path()
        .as_deref()
        .and_then(config_dir::read_store_override)
        .unwrap_or_else(crate::config::get_app_config_dir);

    match WriterLock::try_acquire(&dir, LockOwner::Gui) {
        Ok(lock) => lock.hold_until_exit(),
        Err(lock::AcquireError::Held(held)) => {
            if held.owner() == Some(LockOwner::HeadlessProxy) {
                eprintln!(
                    "cc-switch: {} Stop it with `ccs proxy stop` before using the desktop app's proxy.",
                    held.message()
                );
            }
            std::thread::Builder::new()
                .name("gui-writer-lock".into())
                .spawn(move || {
                    for _ in 0..40 {
                        std::thread::sleep(Duration::from_millis(500));
                        if let Ok(lock) = WriterLock::try_acquire(&dir, LockOwner::Gui) {
                            lock.hold_until_exit();
                            return;
                        }
                    }
                })
                .ok();
        }
        Err(lock::AcquireError::Io(e)) => eprintln!("cc-switch: writer lock unavailable: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_flag_detection_skips_argv0() {
        assert!(is_headless_invocation(["cc-switch", "--headless"]));
        assert!(is_headless_invocation([
            "cc-switch",
            "--config-dir",
            "/x",
            "--headless"
        ]));
        assert!(!is_headless_invocation(["--headless"]));
        assert!(!is_headless_invocation([
            "cc-switch",
            "ccswitch://v1/import"
        ]));
    }

    #[test]
    fn headless_cli_parses_options() {
        let cli = HeadlessCli::try_parse_from([
            "cc-switch",
            "--headless",
            "--no-takeover",
            "--json",
            "-v",
        ])
        .unwrap();
        assert!(cli.headless && cli.start.no_takeover && cli.json);
        assert_eq!(cli.verbose, 1);
        assert!(HeadlessCli::try_parse_from(["cc-switch", "--headless", "--bogus"]).is_err());
    }

    #[test]
    fn usage_errors_exit_with_code_2() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(["ccs", "frobnicate"], &mut out, &mut err);
        assert_eq!(code, EXIT_USAGE);
        assert!(String::from_utf8_lossy(&err).contains("frobnicate"));
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(run(["ccs", "--help"], &mut out, &mut err), EXIT_OK);
        assert!(String::from_utf8_lossy(&out).contains("switch"));
    }
}
