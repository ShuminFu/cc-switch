//! Command-line interface for the icon maintenance tool.

use std::path::PathBuf;
use std::process::ExitCode;

use cc_switch_icons::{check, extract, filter, Config, Error, Result};

const USAGE: &str = "\
cc-switch-icons — maintain the provider icons in src/icons/extracted

USAGE:
    cc-switch-icons <COMMAND> [OPTIONS] [ARGS]

COMMANDS:
    extract [NAME...]   Copy icons from @lobehub/icons-static-svg into the bundle
                        and register any that index.ts / metadata.ts lack.
                        Without NAMEs the built-in default list is used.
    filter              Show which SVG files are neither in the keep list nor
                        referenced by index.ts, and which colour variants would
                        replace their monochrome twin. Nothing changes without
                        --apply.
    check               Verify index.ts, metadata.ts and the files agree.
                        Exits non-zero on errors.
    help                Print this message.

OPTIONS:
    --root <DIR>        Repository root (default: two levels above this crate)
    --source <DIR>      Upstream icon directory
                        (default: <root>/node_modules/@lobehub/icons-static-svg/icons)
    --apply             filter: perform the deletions and renames
    --keep <a,b,...>    filter: additional base names to keep
    --readme            extract: also write src/icons/extracted/README.md
    --strict            check: treat warnings as errors
    --quiet             Only print problems and the final summary
";

#[derive(Debug, Default)]
struct Args {
    command: Option<String>,
    positional: Vec<String>,
    root: Option<PathBuf>,
    source: Option<PathBuf>,
    apply: bool,
    keep: Vec<String>,
    readme: bool,
    strict: bool,
    quiet: bool,
}

fn parse_args(raw: impl Iterator<Item = String>) -> Result<Args> {
    let mut args = Args::default();
    let mut iter = raw.peekable();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--root" => args.root = Some(PathBuf::from(value(&mut iter, "--root")?)),
            "--source" => args.source = Some(PathBuf::from(value(&mut iter, "--source")?)),
            "--keep" => args.keep.extend(
                value(&mut iter, "--keep")?
                    .split(',')
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| s.trim().to_string()),
            ),
            "--apply" => args.apply = true,
            "--readme" => args.readme = true,
            "--strict" => args.strict = true,
            "--quiet" | "-q" => args.quiet = true,
            "--help" | "-h" => args.command = Some("help".to_string()),
            // pnpm forwards a bare `--` separator; it carries no meaning here.
            "--" => {}
            other if other.starts_with('-') => {
                return Err(Error(format!("unknown option `{other}`\n\n{USAGE}")));
            }
            other => {
                if args.command.is_none() {
                    args.command = Some(other.to_string());
                } else {
                    args.positional.push(other.to_string());
                }
            }
        }
    }
    Ok(args)
}

fn value(iter: &mut impl Iterator<Item = String>, flag: &str) -> Result<String> {
    iter.next()
        .ok_or_else(|| Error(format!("`{flag}` needs a value")))
}

fn config(args: &Args) -> Config {
    let root = args.root.clone().unwrap_or_else(Config::default_root);
    let mut cfg = Config::new(root);
    if let Some(source) = &args.source {
        cfg.source_dir = source.clone();
    }
    cfg
}

fn run(args: Args) -> Result<bool> {
    let cfg = config(&args);
    match args.command.as_deref() {
        None | Some("help") => {
            print!("{USAGE}");
            Ok(true)
        }
        Some("extract") => run_extract(&cfg, &args),
        Some("filter") => run_filter(&cfg, &args),
        Some("check") => run_check(&cfg, &args),
        Some(other) => Err(Error(format!("unknown command `{other}`\n\n{USAGE}"))),
    }
}

fn run_extract(cfg: &Config, args: &Args) -> Result<bool> {
    let names = if args.positional.is_empty() {
        extract::default_names()
    } else {
        args.positional.clone()
    };
    if !args.quiet {
        println!("🎨 CC-Switch Icon Extractor\n");
        println!("   source: {}", cfg.source_dir.display());
        println!("   target: {}\n", cfg.icons_dir.display());
    }
    if !cfg.source_dir.is_dir() {
        eprintln!(
            "⚠ upstream directory {} is missing (run `pnpm install`); only local icons can be kept",
            cfg.source_dir.display()
        );
    }
    let outcome = extract::run(cfg, &names, args.readme)?;
    if !args.quiet {
        for name in &outcome.extracted {
            println!("  ✓ {name}.svg");
        }
        for name in &outcome.kept_local {
            println!("  ✓ {name}.svg (kept local custom icon)");
        }
        for name in &outcome.indexed_inline {
            println!("  + index.ts: inline `{name}`");
        }
        for name in &outcome.indexed_url {
            println!("  + index.ts: url `{name}`");
        }
        for name in &outcome.metadata_added {
            println!("  + metadata.ts: `{name}`");
        }
    }
    for name in &outcome.not_found {
        println!("  ✗ {name}.svg (not found)");
    }
    println!(
        "\n✅ Extraction complete: {} extracted, {} kept local, {} not found, {} registered",
        outcome.extracted.len(),
        outcome.kept_local.len(),
        outcome.not_found.len(),
        outcome.indexed_inline.len() + outcome.indexed_url.len()
    );
    Ok(true)
}

fn run_filter(cfg: &Config, args: &Args) -> Result<bool> {
    let plan = filter::build_plan(cfg, &args.keep)?;
    if !args.quiet {
        println!("🧹 CC-Switch Icon Filter\n");
        println!("   directory: {}", cfg.icons_dir.display());
        println!("   kept: {}\n", plan.kept.len());
    }
    for (from, to) in &plan.rename {
        println!("  ↻ {from} -> {to}");
    }
    for file in &plan.delete {
        println!("  ✗ {file}");
    }
    if plan.is_noop() {
        println!("Nothing to do: every SVG is kept and no colour variant needs renaming.");
        return Ok(true);
    }
    if args.apply {
        filter::apply(&cfg.icons_dir, &plan)?;
        println!(
            "\n✅ Applied: {} deleted, {} renamed",
            plan.delete.len(),
            plan.rename.len()
        );
        let report = check::run(cfg)?;
        print_report(&report, args.quiet);
        Ok(report.is_ok())
    } else {
        println!(
            "\nDry run: {} would be deleted, {} renamed. Re-run with --apply to proceed.",
            plan.delete.len(),
            plan.rename.len()
        );
        Ok(true)
    }
}

fn run_check(cfg: &Config, args: &Args) -> Result<bool> {
    let report = check::run(cfg)?;
    print_report(&report, args.quiet);
    Ok(report.is_ok() && (!args.strict || report.warnings.is_empty()))
}

fn print_report(report: &check::Report, quiet: bool) {
    if !quiet {
        println!(
            "🔎 {} inline icons, {} url icons, {} metadata entries",
            report.inline_count, report.url_count, report.metadata_count
        );
    }
    for warning in &report.warnings {
        println!("  ⚠ {warning}");
    }
    for error in &report.errors {
        println!("  ✗ {error}");
    }
    if report.is_ok() {
        println!(
            "✅ index.ts and metadata.ts are consistent ({} warnings)",
            report.warnings.len()
        );
    } else {
        println!(
            "❌ {} errors, {} warnings",
            report.errors.len(),
            report.warnings.len()
        );
    }
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(2);
        }
    };
    match run(args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(list: &[&str]) -> Args {
        parse_args(list.iter().map(|s| s.to_string())).unwrap()
    }

    #[test]
    fn parses_command_and_options() {
        let args = parse(&[
            "extract", "openai", "claude", "--root", "/r", "--source", "/s", "--readme", "-q",
        ]);
        assert_eq!(args.command.as_deref(), Some("extract"));
        assert_eq!(args.positional, vec!["openai", "claude"]);
        assert_eq!(args.root, Some(PathBuf::from("/r")));
        assert_eq!(args.source, Some(PathBuf::from("/s")));
        assert!(args.readme && args.quiet);
    }

    #[test]
    fn parses_keep_lists() {
        let args = parse(&["filter", "--keep", "a, b,,c", "--keep", "d", "--apply"]);
        assert_eq!(args.keep, vec!["a", "b", "c", "d"]);
        assert!(args.apply);
    }

    #[test]
    fn rejects_unknown_option_and_missing_value() {
        assert!(parse_args(["--bogus".to_string()].into_iter()).is_err());
        assert!(parse_args(["--root".to_string()].into_iter()).is_err());
    }

    #[test]
    fn ignores_bare_separator() {
        let args = parse(&["extract", "--", "openai"]);
        assert_eq!(args.positional, vec!["openai"]);
    }

    #[test]
    fn help_flag_selects_help() {
        assert_eq!(parse(&["check", "--help"]).command.as_deref(), Some("help"));
    }
}
