//! Command line entry point for the icon tooling. See `README.md` in this
//! directory for usage.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use icon_tools::check::{self, Level, Report};
use icon_tools::index::{self, GenOptions};
use icon_tools::{extract, filter, util};

/// `println!` that ignores a closed stdout (e.g. when piped into `head`).
macro_rules! say {
    ($($arg:tt)*) => {{
        use std::io::Write;
        let _ = writeln!(std::io::stdout(), $($arg)*);
    }};
}

/// `print!` that ignores a closed stdout.
macro_rules! emit {
    ($($arg:tt)*) => {{
        use std::io::Write;
        let _ = write!(std::io::stdout(), $($arg)*);
    }};
}

const USAGE: &str = "\
icon-tools — maintain the provider icons in src/icons/extracted

USAGE:
    icon-tools <COMMAND> [OPTIONS]

COMMANDS:
    extract   Copy icons from node_modules/@lobehub/icons-static-svg
                --source <DIR>      Icon package directory
                --out <DIR>         Destination directory
                --icons <a,b,...>   Icons to extract (default: built-in list)
                --icon <NAME>       Add one icon (repeatable)
                --overwrite         Replace files that already exist locally
    filter    Remove SVGs that are not on the keep list, prefer colour variants
                --dir <DIR>         Icon directory
                --keep <a,b,...>    Extra names to keep (repeatable)
                --apply             Perform the changes (default: dry run)
    check     Validate index.ts, metadata.ts and the files on disk
                --dir <DIR>         Icon directory
                --strict            Treat warnings as errors
                --scaffold-metadata Print metadata.ts entries for icons lacking one
    index     Render an index.ts from the files in a directory
    emit-rust Emit the curated icon set as a Rust module for crates/cc-switch-ui
                --dir <DIR>         Icon directory
                --out <FILE>        Rust file to write (default: crates/cc-switch-ui/src/icons/generated.rs)
                --assets <DIR>      Where URL icon files are copied (default: crates/cc-switch-ui/assets/icons)
                --asset-prefix <P>  Published prefix of --assets (default: /assets/icons)
                --dir <DIR>         Icon directory
                --out <FILE>        Write to FILE (default: stdout)
                --write             Overwrite <DIR>/index.ts
                --check             Compare with <DIR>/index.ts and exit 1 on drift
                --inline-max-bytes <N>  Inline SVGs up to N bytes (default 32768)
                --raw               Do not normalise root <svg> attributes
                --alias <KEY=FILE>  Force KEY for FILE (repeatable)
                --ignore <FILE>     Skip FILE (repeatable)

GLOBAL OPTIONS:
    --root <DIR>   Repository root (default: auto-detected)
    -h, --help     Show this help

EXIT CODES:
    0  success            1  validation failed            2  usage or I/O error
";

const VALUE_FLAGS: &[&str] = &[
    "root",
    "assets",
    "asset-prefix",
    "source",
    "out",
    "dir",
    "icons",
    "icon",
    "keep",
    "alias",
    "ignore",
    "inline-max-bytes",
];

const BOOL_FLAGS: &[&str] = &[
    "apply",
    "overwrite",
    "dry-run",
    "strict",
    "scaffold-metadata",
    "write",
    "check",
    "raw",
    "help",
];

#[derive(Debug, Default)]
struct Args {
    command: Option<String>,
    values: BTreeMap<String, Vec<String>>,
    flags: BTreeSet<String>,
}

impl Args {
    fn value(&self, name: &str) -> Option<&str> {
        self.values
            .get(name)
            .and_then(|v| v.last())
            .map(String::as_str)
    }

    fn values(&self, name: &str) -> Vec<String> {
        self.values.get(name).cloned().unwrap_or_default()
    }

    fn has(&self, name: &str) -> bool {
        self.flags.contains(name)
    }

    /// Comma separated values collected from every occurrence of `name`.
    fn list(&self, name: &str) -> Vec<String> {
        self.values(name)
            .iter()
            .flat_map(|v| v.split(','))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    }
}

fn parse_args(raw: &[String]) -> Result<Args, String> {
    let mut args = Args::default();
    let mut iter = raw.iter();
    while let Some(token) = iter.next() {
        if token == "-h" || token == "--help" {
            args.flags.insert("help".to_string());
            continue;
        }
        if let Some(flag) = token.strip_prefix("--") {
            let (name, inline_value) = match flag.split_once('=') {
                Some((name, value)) => (name, Some(value.to_string())),
                None => (flag, None),
            };
            if VALUE_FLAGS.contains(&name) {
                let value = match inline_value {
                    Some(value) => value,
                    None => iter
                        .next()
                        .cloned()
                        .ok_or_else(|| format!("--{name} requires a value"))?,
                };
                args.values.entry(name.to_string()).or_default().push(value);
            } else if BOOL_FLAGS.contains(&name) {
                if inline_value.is_some() {
                    return Err(format!("--{name} does not take a value"));
                }
                args.flags.insert(name.to_string());
            } else {
                return Err(format!("unknown option --{name}"));
            }
        } else if args.command.is_none() {
            args.command = Some(token.clone());
        } else {
            return Err(format!("unexpected argument `{token}`"));
        }
    }
    Ok(args)
}

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&raw) {
        Ok(args) => args,
        Err(err) => {
            eprintln!("error: {err}\n");
            eprint!("{USAGE}");
            return ExitCode::from(2);
        }
    };

    if args.has("help") || args.command.is_none() {
        emit!("{USAGE}");
        return if args.command.is_none() && !args.has("help") {
            ExitCode::from(2)
        } else {
            ExitCode::SUCCESS
        };
    }

    let root = args
        .value("root")
        .map(PathBuf::from)
        .unwrap_or_else(util::default_repo_root);

    let result = match args.command.as_deref().unwrap_or_default() {
        "extract" => cmd_extract(&root, &args),
        "filter" => cmd_filter(&root, &args),
        "check" => cmd_check(&root, &args),
        "index" => cmd_index(&root, &args),
        "emit-rust" => cmd_emit_rust(&root, &args),
        other => Err(CliError::usage(format!("unknown command `{other}`"))),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(CliError::Validation) => ExitCode::from(1),
        Err(CliError::Usage(message)) => {
            eprintln!("error: {message}\n");
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
        Err(CliError::Io(message)) => {
            eprintln!("error: {message}");
            ExitCode::from(2)
        }
    }
}

enum CliError {
    /// Reported already; exit 1.
    Validation,
    Usage(String),
    Io(String),
}

impl CliError {
    fn usage(message: impl Into<String>) -> Self {
        CliError::Usage(message.into())
    }

    fn io(message: impl Into<String>) -> Self {
        CliError::Io(message.into())
    }
}

fn icon_dir(root: &Path, args: &Args) -> PathBuf {
    args.value("dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join(util::EXTRACTED_REL))
}

fn cmd_extract(root: &Path, args: &Args) -> Result<(), CliError> {
    let source = args
        .value("source")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join(util::LOBEHUB_REL));
    let out = args
        .value("out")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join(util::EXTRACTED_REL));
    let mut names = args.list("icons");
    names.extend(args.list("icon"));
    if names.is_empty() {
        names = extract::default_icon_names();
    }

    say!("CC-Switch icon extractor");
    say!("  source: {}", source.display());
    say!("  output: {}\n", out.display());

    let outcome = extract::extract(&source, &out, &names, args.has("overwrite"))
        .map_err(|e| CliError::io(e.to_string()))?;
    for name in &names {
        if outcome.copied.contains(name) {
            say!("  ✓ {name}.svg");
        } else if outcome.already_present.contains(name) {
            say!("  ✓ {name}.svg (already present, use --overwrite to refresh)");
        } else if outcome.kept_local.contains(name) {
            say!("  ✓ {name}.svg (kept local custom icon)");
        } else {
            say!("  ✗ {name}.svg (not found)");
        }
    }
    say!();
    say!("Extraction complete");
    say!("  Copied:     {}", outcome.copied.len());
    say!("  Present:    {}", outcome.already_present.len());
    say!("  Kept local: {}", outcome.kept_local.len());
    say!("  Available:  {}", outcome.available());
    say!("  Not found:  {}", outcome.not_found.len());
    say!();
    say!("index.ts and metadata.ts are curated by hand and were not modified.");
    say!("Add entries for new icons, then run `icon-tools check`.");

    if out.join("index.ts").is_file() && out.join("metadata.ts").is_file() {
        say!();
        let report = check::run(root, &out).map_err(CliError::io)?;
        print_report(&report);
    }
    Ok(())
}

fn cmd_filter(root: &Path, args: &Args) -> Result<(), CliError> {
    let dir = icon_dir(root, args);
    if !dir.is_dir() {
        return Err(CliError::io(format!(
            "{} is not a directory",
            dir.display()
        )));
    }
    let apply = args.has("apply");
    let keep = filter::keep_set(&args.list("keep"));
    let protected = filter::protected_files(root, &dir).map_err(CliError::io)?;
    let files =
        util::list_files(&dir).map_err(|e| CliError::io(format!("{}: {e}", dir.display())))?;
    let svg_files: Vec<String> = files.into_iter().filter(|f| f.ends_with(".svg")).collect();

    say!(
        "Scanning {} SVG files in {}",
        svg_files.len(),
        dir.display()
    );
    let plan = filter::plan(&svg_files, &keep, &protected);

    for (from, to) in &plan.rename {
        say!("  rename {from} -> {to}");
    }
    for file in &plan.delete {
        say!("  delete {file}");
    }
    for file in &plan.protected {
        say!("  keep   {file} (referenced by index.ts or a source file)");
    }
    say!();
    say!("Plan summary");
    say!("  Kept:      {}", plan.keep.len() + plan.protected.len());
    say!("  Deleted:   {}", plan.delete.len());
    say!("  Renamed:   {}", plan.rename.len());

    if plan.is_noop() {
        say!("\nNothing to change.");
        return Ok(());
    }

    if !apply {
        say!("\nDry run: no files were changed. Re-run with --apply to perform the changes.");
        return Ok(());
    }

    filter::apply(&dir, &plan).map_err(|e| CliError::io(format!("applying plan: {e}")))?;
    say!("\nChanges applied.");
    if dir.join("index.ts").is_file() && dir.join("metadata.ts").is_file() {
        say!();
        let report = check::run(root, &dir).map_err(CliError::io)?;
        print_report(&report);
    }
    Ok(())
}

fn cmd_check(root: &Path, args: &Args) -> Result<(), CliError> {
    let dir = icon_dir(root, args);
    let strict = args.has("strict");
    say!("Checking {}", dir.display());
    let report = check::run(root, &dir).map_err(CliError::io)?;
    print_report(&report);

    if args.has("scaffold-metadata") && !report.missing_metadata.is_empty() {
        say!("\nmetadata.ts entries to add:\n");
        emit!("{}", check::scaffold_metadata(&report.missing_metadata));
    }

    if report.passed(strict) {
        Ok(())
    } else {
        Err(CliError::Validation)
    }
}

fn cmd_index(root: &Path, args: &Args) -> Result<(), CliError> {
    let dir = icon_dir(root, args);
    if !dir.is_dir() {
        return Err(CliError::io(format!(
            "{} is not a directory",
            dir.display()
        )));
    }
    let modes = [
        args.value("out").is_some(),
        args.has("write"),
        args.has("check"),
    ]
    .iter()
    .filter(|m| **m)
    .count();
    if modes > 1 {
        return Err(CliError::usage(
            "--out, --write and --check are mutually exclusive",
        ));
    }

    let mut opts = GenOptions {
        normalize: !args.has("raw"),
        ..GenOptions::default()
    };
    if let Some(value) = args.value("inline-max-bytes") {
        opts.inline_max_bytes = value.parse().map_err(|_| {
            CliError::usage(format!(
                "--inline-max-bytes expects a number, got `{value}`"
            ))
        })?;
    }
    for alias in args.values("alias") {
        let (key, file) = alias
            .split_once('=')
            .ok_or_else(|| CliError::usage(format!("--alias expects KEY=FILE, got `{alias}`")))?;
        if key.is_empty() || file.is_empty() {
            return Err(CliError::usage(format!(
                "--alias expects KEY=FILE, got `{alias}`"
            )));
        }
        opts.aliases.insert(file.to_string(), key.to_string());
    }
    opts.ignore.extend(args.list("ignore"));

    let (source, plan) =
        index::generate_index(&dir, &opts).map_err(|e| CliError::io(e.to_string()))?;

    for (file, reason) in &plan.skipped {
        eprintln!("skipped {file}: {reason}");
    }

    if args.has("check") {
        let existing_path = dir.join("index.ts");
        let existing = fs::read_to_string(&existing_path)
            .map_err(|e| CliError::io(format!("{}: {e}", existing_path.display())))?;
        if existing == source {
            say!(
                "{} is up to date ({} icons)",
                existing_path.display(),
                plan.entries.len()
            );
            return Ok(());
        }
        let line = first_difference(&existing, &source);
        say!(
            "{} differs from the generated index (first difference at line {line})",
            existing_path.display()
        );
        return Err(CliError::Validation);
    }

    let target = if args.has("write") {
        Some(dir.join("index.ts"))
    } else {
        args.value("out").map(PathBuf::from)
    };
    match target {
        Some(path) => {
            fs::write(&path, &source)
                .map_err(|e| CliError::io(format!("{}: {e}", path.display())))?;
            let inline = plan
                .entries
                .iter()
                .filter(|c| c.kind == index::Kind::InlineSvg)
                .count();
            say!(
                "Wrote {} ({} icons: {} inline, {} url)",
                path.display(),
                plan.entries.len(),
                inline,
                plan.entries.len() - inline
            );
        }
        None => emit!("{source}"),
    }
    Ok(())
}

fn cmd_emit_rust(root: &Path, args: &Args) -> Result<(), CliError> {
    let dir = icon_dir(root, args);
    let out = args
        .value("out")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("crates/cc-switch-ui/src/icons/generated.rs"));
    let assets = args
        .value("assets")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("crates/cc-switch-ui/assets/icons"));
    let prefix = args.value("asset-prefix").unwrap_or("/assets/icons");

    let index_src = fs::read_to_string(dir.join("index.ts"))
        .map_err(|e| CliError::io(format!("{}: {e}", dir.join("index.ts").display())))?;
    let parsed = index::parse_index(&index_src).map_err(CliError::io)?;
    let metadata_src = fs::read_to_string(dir.join("metadata.ts"))
        .map_err(|e| CliError::io(format!("{}: {e}", dir.join("metadata.ts").display())))?;
    let metadata = icon_tools::rust_out::parse_metadata(&metadata_src);

    let source = icon_tools::rust_out::render_rust(&parsed, &metadata, prefix);
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent).map_err(|e| CliError::io(e.to_string()))?;
    }
    fs::write(&out, source).map_err(|e| CliError::io(format!("{}: {e}", out.display())))?;
    let copied = icon_tools::rust_out::copy_url_assets(&dir, &parsed, &assets)
        .map_err(|e| CliError::io(format!("copying icon assets: {e}")))?;
    say!(
        "Wrote {} ({} inline icons, {} url icons, {} metadata entries); copied {copied} files to {}",
        out.display(),
        parsed.inline.len(),
        parsed.urls.len(),
        metadata.len(),
        assets.display()
    );
    Ok(())
}

fn first_difference(a: &str, b: &str) -> usize {
    let mut line = 1;
    let mut a_lines = a.lines();
    let mut b_lines = b.lines();
    loop {
        match (a_lines.next(), b_lines.next()) {
            (Some(x), Some(y)) if x == y => line += 1,
            _ => return line,
        }
    }
}

fn print_report(report: &Report) {
    say!(
        "  {} index keys, {} metadata keys, {} files",
        report.index_keys,
        report.metadata_keys,
        report.files
    );
    for finding in &report.findings {
        match finding.level {
            Level::Error => say!("  ✗ error: {}", finding.message),
            Level::Warning => say!("  ! warning: {}", finding.message),
        }
    }
    say!(
        "  {} errors, {} warnings",
        report.errors(),
        report.warnings()
    );
}
