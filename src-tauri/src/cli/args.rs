//! Argument definitions for `ccs`.

use std::path::PathBuf;
use std::str::FromStr;

use clap::{ArgAction, Args, Parser, Subcommand};

use crate::app_config::AppType;

#[derive(Debug, Parser)]
#[command(
    name = "ccs",
    version,
    about = "CC Switch command-line companion",
    long_about = "CC Switch command-line companion.\n\nWorks on the same database and live config files as the desktop app. Write commands (switch, import, proxy start) refuse to run while the desktop app is open."
)]
pub struct Cli {
    /// Print machine-readable JSON instead of text
    #[arg(long, global = true)]
    pub json: bool,

    /// Use this CC Switch data directory instead of the configured one
    #[arg(long, global = true, value_name = "DIR")]
    pub config_dir: Option<PathBuf>,

    /// Log more (-v info, -vv debug) to stderr
    #[arg(short, long, global = true, action = ArgAction::Count)]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List providers (all apps, or one app)
    #[command(visible_alias = "ls")]
    List {
        /// App id: claude, claude-desktop, codex, gemini, grokbuild, opencode, openclaw, hermes
        #[arg(value_parser = parse_app)]
        app: Option<AppType>,
    },
    /// Show the current provider (all apps, or one app)
    Current {
        #[arg(value_parser = parse_app)]
        app: Option<AppType>,
    },
    /// Switch an app to a provider (by id or exact name), exactly like the GUI
    Switch {
        #[arg(value_parser = parse_app)]
        app: AppType,
        /// Provider id or name
        provider: String,
    },
    /// Local proxy: status, start (foreground, headless) or stop
    Proxy {
        #[command(subcommand)]
        action: ProxyAction,
    },
    /// Usage statistics
    Usage {
        #[command(subcommand)]
        action: UsageAction,
    },
    /// Projects (profiles): list them, or apply one to an app
    Profile {
        #[command(subcommand)]
        action: ProfileAction,
    },
    /// Check the data directory, database and live config files (read-only)
    Doctor,
    /// Export the database to a SQL backup file
    Export { file: PathBuf },
    /// Import a SQL backup file (same as Settings → Import in the GUI)
    Import { file: PathBuf },
    /// Show the managed (team) policy that applies to this machine
    Policy,
}

#[derive(Debug, Subcommand)]
pub enum ProfileAction {
    /// List projects and which one is current per app
    #[command(visible_alias = "ls")]
    List,
    /// Apply a project's saved configuration to one app, exactly like the GUI
    Apply {
        /// App id whose configuration group is applied (e.g. claude, codex)
        #[arg(value_parser = parse_app)]
        app: AppType,
        /// Project id or exact name
        profile: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum ProxyAction {
    /// Show whether the proxy is running and which apps are taken over
    Status,
    /// Run the proxy in the foreground without the GUI (Ctrl-C to stop)
    Start(ProxyStartArgs),
    /// Stop a headless proxy started with `ccs proxy start` / `cc-switch --headless`
    Stop {
        /// Seconds to wait for the proxy to shut down
        #[arg(long, default_value_t = 15)]
        timeout: u64,
    },
}

#[derive(Debug, Clone, Default, Args)]
pub struct ProxyStartArgs {
    /// Only run the listener; do not restore live-config takeover for apps
    /// that had it enabled
    #[arg(long)]
    pub no_takeover: bool,
}

#[derive(Debug, Subcommand)]
pub enum UsageAction {
    /// Totals for the last N days
    Summary {
        #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u32).range(1..=3650))]
        days: u32,
        /// Restrict to one app
        #[arg(long, value_parser = parse_app)]
        app: Option<AppType>,
    },
}

fn parse_app(raw: &str) -> Result<AppType, String> {
    AppType::from_str(raw).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("ccs").chain(args.iter().copied()))
    }

    #[test]
    fn clap_definition_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn list_and_alias_with_optional_app() {
        let cli = parse(&["list"]).unwrap();
        assert!(matches!(cli.command, Command::List { app: None }));
        let cli = parse(&["ls", "codex", "--json"]).unwrap();
        assert!(cli.json);
        assert!(matches!(
            cli.command,
            Command::List {
                app: Some(AppType::Codex)
            }
        ));
        // App aliases go through AppType::from_str.
        let cli = parse(&["current", "Claude-Desktop"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Current {
                app: Some(AppType::ClaudeDesktop)
            }
        ));
    }

    #[test]
    fn switch_requires_app_and_provider() {
        let cli = parse(&["--json", "switch", "claude", "packycode"]).unwrap();
        assert!(cli.json);
        match cli.command {
            Command::Switch { app, provider } => {
                assert_eq!(app, AppType::Claude);
                assert_eq!(provider, "packycode");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(parse(&["switch", "claude"]).is_err());
        let err = parse(&["switch", "vim", "x"]).unwrap_err();
        assert!(err.to_string().contains("vim"));
    }

    #[test]
    fn proxy_and_usage_subcommands() {
        assert!(matches!(
            parse(&["proxy", "status"]).unwrap().command,
            Command::Proxy {
                action: ProxyAction::Status
            }
        ));
        match parse(&["proxy", "start", "--no-takeover"]).unwrap().command {
            Command::Proxy {
                action: ProxyAction::Start(args),
            } => assert!(args.no_takeover),
            other => panic!("unexpected {other:?}"),
        }
        match parse(&["proxy", "stop"]).unwrap().command {
            Command::Proxy {
                action: ProxyAction::Stop { timeout },
            } => assert_eq!(timeout, 15),
            other => panic!("unexpected {other:?}"),
        }
        match parse(&["usage", "summary", "--days", "7", "--app", "gemini"])
            .unwrap()
            .command
        {
            Command::Usage {
                action: UsageAction::Summary { days, app },
            } => {
                assert_eq!(days, 7);
                assert_eq!(app, Some(AppType::Gemini));
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(parse(&["usage", "summary", "--days", "0"]).is_err());
        assert!(parse(&["proxy"]).is_err());
    }

    #[test]
    fn export_import_and_global_flags() {
        let cli = parse(&["export", "out.sql", "--config-dir", "/tmp/x", "-vv"]).unwrap();
        assert_eq!(cli.config_dir, Some(PathBuf::from("/tmp/x")));
        assert_eq!(cli.verbose, 2);
        assert!(matches!(cli.command, Command::Export { file } if file == Path::new("out.sql")));
        assert!(matches!(
            parse(&["import", "in.sql"]).unwrap().command,
            Command::Import { .. }
        ));
        assert!(parse(&["export"]).is_err());
        assert!(parse(&[]).is_err());
    }

    #[test]
    fn profile_and_doctor_subcommands() {
        assert!(matches!(
            parse(&["profile", "ls"]).unwrap().command,
            Command::Profile {
                action: ProfileAction::List
            }
        ));
        match parse(&["profile", "apply", "codex", "Work"])
            .unwrap()
            .command
        {
            Command::Profile {
                action: ProfileAction::Apply { app, profile },
            } => {
                assert_eq!(app, AppType::Codex);
                assert_eq!(profile, "Work");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(parse(&["profile", "apply", "codex"]).is_err());
        assert!(matches!(
            parse(&["doctor", "--json"]).unwrap().command,
            Command::Doctor
        ));
    }

    use std::path::Path;
}
