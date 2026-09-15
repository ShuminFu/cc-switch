//! End-to-end tests that drive the `icon-tools` binary against temporary
//! directories laid out like the repository.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "icon-tools-test-{}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst),
            nanos
        ));
        fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Output {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run(cwd: &Path, args: &[&str]) -> Output {
    let output = Command::new(env!("CARGO_BIN_EXE_icon-tools"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run icon-tools");
    Output {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    fs::write(path, content).expect("write file");
}

fn svg(label: &str) -> String {
    format!("<svg viewBox=\"0 0 24 24\" xmlns=\"http://www.w3.org/2000/svg\"><title>{label}</title><path d=\"M0 0h24v24H0z\"/></svg>\n")
}

/// Creates a fake repository: `package.json`, the icon directory and one
/// component that imports `claw.svg` directly.
fn make_repo(tmp: &TempDir) -> (PathBuf, PathBuf) {
    let root = tmp.path().join("repo");
    write(&root.join("package.json"), "{ \"name\": \"fixture\" }\n");
    let icons = root.join("src").join("icons").join("extracted");
    fs::create_dir_all(&icons).expect("icons dir");
    write(
        &root.join("src").join("components").join("Brand.tsx"),
        "import ClawSvg from \"@/icons/extracted/claw.svg?url\";\nexport const claw = ClawSvg;\n",
    );
    (root, icons)
}

fn metadata(keys: &[&str]) -> String {
    let mut out = String::from(
        "// Icon metadata\nimport { IconMetadata } from \"@/types/icon\";\n\nexport const iconMetadata: Record<string, IconMetadata> = {\n",
    );
    for key in keys {
        out.push_str(&format!(
            "  {key}: {{\n    name: \"{key}\",\n    displayName: \"{key}\",\n    category: \"ai-provider\",\n    keywords: [\"{key}\"],\n  }},\n"
        ));
    }
    out.push_str("};\n");
    out
}

const CONSISTENT_INDEX: &str = "// Auto-generated icon index
// Do not edit manually

import _big from \"./big.svg?url\";
import _logo from \"./logo.png\";

export const icons: Record<string, string> = {
  custom: `<svg viewBox=\"0 0 1 1\"><path d=\"M0 0\"/></svg>`,
};

export const iconUrls: Record<string, string> = {
  big: _big,
  logo: _logo,
};

export const iconList = [
  ...Object.keys(icons),
  ...Object.keys(iconUrls),
].sort();
";

/// Populates the icon directory with a consistent index and matching files.
fn seed_consistent(icons: &Path) {
    write(&icons.join("index.ts"), CONSISTENT_INDEX);
    write(
        &icons.join("metadata.ts"),
        &metadata(&["custom", "big", "logo"]),
    );
    write(&icons.join("custom.svg"), &svg("custom"));
    write(&icons.join("big.svg"), &svg("big"));
    write(&icons.join("logo.png"), "png-bytes");
    write(&icons.join("claw.svg"), &svg("claw"));
}

#[test]
fn shows_usage_without_a_command() {
    let tmp = TempDir::new();
    let out = run(tmp.path(), &[]);
    assert_eq!(out.code, 2);
    assert!(out.stdout.contains("USAGE:"), "{}", out.stdout);

    let out = run(tmp.path(), &["--help"]);
    assert_eq!(out.code, 0);
    assert!(out.stdout.contains("COMMANDS:"));

    let out = run(tmp.path(), &["check", "--bogus"]);
    assert_eq!(out.code, 2);
    assert!(
        out.stderr.contains("unknown option --bogus"),
        "{}",
        out.stderr
    );
}

#[test]
fn extract_copies_keeps_local_and_reports_missing() {
    let tmp = TempDir::new();
    let source = tmp.path().join("pkg");
    let out_dir = tmp.path().join("out");
    write(&source.join("openai.svg"), &svg("openai"));
    write(&source.join("anthropic.svg"), &svg("anthropic"));
    write(&out_dir.join("custom.svg"), &svg("custom"));
    write(&out_dir.join("anthropic.svg"), &svg("anthropic-local"));

    let args = [
        "extract",
        "--source",
        source.to_str().unwrap(),
        "--out",
        out_dir.to_str().unwrap(),
        "--icons",
        "openai,anthropic",
        "--icon",
        "custom",
        "--icon",
        "missing",
    ];
    let out = run(tmp.path(), &args);
    assert_eq!(
        out.code, 0,
        "stdout: {}\nstderr: {}",
        out.stdout, out.stderr
    );
    assert!(out.stdout.contains("✓ openai.svg\n"), "{}", out.stdout);
    assert!(
        out.stdout
            .contains("✓ anthropic.svg (already present, use --overwrite to refresh)"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("✓ custom.svg (kept local custom icon)"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("✗ missing.svg (not found)"),
        "{}",
        out.stdout
    );
    assert!(out.stdout.contains("Copied:     1"), "{}", out.stdout);
    assert!(out.stdout.contains("Available:  3"), "{}", out.stdout);
    assert!(out.stdout.contains("Not found:  1"), "{}", out.stdout);
    assert_eq!(
        fs::read_to_string(out_dir.join("openai.svg")).unwrap(),
        svg("openai")
    );
    // Existing local files are left alone unless --overwrite is given.
    assert_eq!(
        fs::read_to_string(out_dir.join("anthropic.svg")).unwrap(),
        svg("anthropic-local")
    );
    assert_eq!(
        fs::read_to_string(out_dir.join("custom.svg")).unwrap(),
        svg("custom")
    );
    assert!(!out_dir.join("missing.svg").exists());
    // The curated files are never generated by extract.
    assert!(!out_dir.join("index.ts").exists());
    assert!(!out_dir.join("metadata.ts").exists());

    let mut overwrite_args = args.to_vec();
    overwrite_args.push("--overwrite");
    let out = run(tmp.path(), &overwrite_args);
    assert_eq!(
        out.code, 0,
        "stdout: {}\nstderr: {}",
        out.stdout, out.stderr
    );
    assert!(out.stdout.contains("Copied:     2"), "{}", out.stdout);
    assert_eq!(
        fs::read_to_string(out_dir.join("anthropic.svg")).unwrap(),
        svg("anthropic")
    );
}

#[test]
fn extract_fails_clearly_when_the_package_is_missing() {
    let tmp = TempDir::new();
    let (root, _) = make_repo(&tmp);
    let out = run(&root, &["extract"]);
    assert_eq!(out.code, 2);
    assert!(
        out.stderr.contains("run `pnpm install` first"),
        "{}",
        out.stderr
    );
}

#[test]
fn extract_runs_the_check_when_curated_files_exist() {
    let tmp = TempDir::new();
    let (root, icons) = make_repo(&tmp);
    seed_consistent(&icons);
    let source = tmp.path().join("pkg");
    write(&source.join("openai.svg"), &svg("openai"));

    let out = run(
        &root,
        &[
            "extract",
            "--source",
            source.to_str().unwrap(),
            "--icons",
            "openai",
        ],
    );
    assert_eq!(
        out.code, 0,
        "stdout: {}\nstderr: {}",
        out.stdout, out.stderr
    );
    assert!(icons.join("openai.svg").is_file());
    // The freshly copied file is not referenced yet, which the check reports.
    assert!(
        out.stdout
            .contains("`openai.svg` is not referenced by index.ts"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("0 errors, 1 warnings"),
        "{}",
        out.stdout
    );
}

#[test]
fn filter_dry_run_changes_nothing_and_apply_executes_the_plan() {
    let tmp = TempDir::new();
    let (root, icons) = make_repo(&tmp);
    seed_consistent(&icons);
    write(&icons.join("claude.svg"), &svg("claude-mono"));
    write(&icons.join("claude-color.svg"), &svg("claude-color"));
    write(&icons.join("junk.svg"), &svg("junk"));
    write(&icons.join("yi-color.svg"), &svg("yi"));

    let dry = run(&root, &["filter"]);
    assert_eq!(
        dry.code, 0,
        "stdout: {}\nstderr: {}",
        dry.stdout, dry.stderr
    );
    assert!(
        dry.stdout.contains("rename claude-color.svg -> claude.svg"),
        "{}",
        dry.stdout
    );
    assert!(
        dry.stdout.contains("rename yi-color.svg -> yi.svg"),
        "{}",
        dry.stdout
    );
    assert!(dry.stdout.contains("delete junk.svg"), "{}", dry.stdout);
    assert!(
        dry.stdout.contains("keep   claw.svg (referenced"),
        "{}",
        dry.stdout
    );
    assert!(
        dry.stdout.contains("keep   custom.svg (referenced"),
        "{}",
        dry.stdout
    );
    assert!(
        dry.stdout.contains("keep   big.svg (referenced"),
        "{}",
        dry.stdout
    );
    assert!(
        dry.stdout.contains("Dry run: no files were changed"),
        "{}",
        dry.stdout
    );
    for file in [
        "claude.svg",
        "claude-color.svg",
        "junk.svg",
        "yi-color.svg",
        "claw.svg",
    ] {
        assert!(
            icons.join(file).is_file(),
            "{file} should still exist after a dry run"
        );
    }

    let applied = run(&root, &["filter", "--apply"]);
    assert_eq!(
        applied.code, 0,
        "stdout: {}\nstderr: {}",
        applied.stdout, applied.stderr
    );
    assert!(
        applied.stdout.contains("Changes applied."),
        "{}",
        applied.stdout
    );
    assert!(!icons.join("junk.svg").exists());
    assert!(!icons.join("claude-color.svg").exists());
    assert!(!icons.join("yi-color.svg").exists());
    assert_eq!(
        fs::read_to_string(icons.join("claude.svg")).unwrap(),
        svg("claude-color")
    );
    assert_eq!(fs::read_to_string(icons.join("yi.svg")).unwrap(), svg("yi"));
    for file in [
        "claw.svg",
        "custom.svg",
        "big.svg",
        "logo.png",
        "index.ts",
        "metadata.ts",
    ] {
        assert!(icons.join(file).is_file(), "{file} must survive filtering");
    }
    // The post-apply check still passes.
    assert!(applied.stdout.contains("0 errors"), "{}", applied.stdout);

    let again = run(&root, &["filter", "--apply"]);
    assert_eq!(again.code, 0);
    assert!(
        again.stdout.contains("Nothing to change."),
        "{}",
        again.stdout
    );
}

#[test]
fn filter_honours_extra_keep_names() {
    let tmp = TempDir::new();
    let (root, icons) = make_repo(&tmp);
    seed_consistent(&icons);
    write(&icons.join("junk.svg"), &svg("junk"));
    write(&icons.join("other.svg"), &svg("other"));

    let out = run(&root, &["filter", "--keep", "junk", "--apply"]);
    assert_eq!(
        out.code, 0,
        "stdout: {}\nstderr: {}",
        out.stdout, out.stderr
    );
    assert!(icons.join("junk.svg").is_file());
    assert!(!icons.join("other.svg").exists());
}

#[test]
fn check_passes_on_a_consistent_directory() {
    let tmp = TempDir::new();
    let (root, icons) = make_repo(&tmp);
    seed_consistent(&icons);

    let out = run(&root, &["check"]);
    assert_eq!(
        out.code, 0,
        "stdout: {}\nstderr: {}",
        out.stdout, out.stderr
    );
    assert!(
        out.stdout
            .contains("3 index keys, 3 metadata keys, 6 files"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("0 errors, 0 warnings"),
        "{}",
        out.stdout
    );

    // The same check from another working directory using --root.
    let out = run(
        tmp.path(),
        &["--root", root.to_str().unwrap(), "check", "--strict"],
    );
    assert_eq!(
        out.code, 0,
        "stdout: {}\nstderr: {}",
        out.stdout, out.stderr
    );
}

#[test]
fn check_fails_on_broken_index() {
    let tmp = TempDir::new();
    let (root, icons) = make_repo(&tmp);
    seed_consistent(&icons);
    write(
        &icons.join("index.ts"),
        "import _logo from \"./logo.png\";
import _gone from \"./gone.png\";

export const icons: Record<string, string> = {
  custom: `<svg viewBox=\"0 0 1 1\"/>`,
  Custom: `<svg/>`,
  logo: `<svg/>`,
};

export const iconUrls: Record<string, string> = {
  logo: _logo,
  gone: _gone,
};
",
    );

    let out = run(&root, &["check"]);
    assert_eq!(
        out.code, 1,
        "stdout: {}\nstderr: {}",
        out.stdout, out.stderr
    );
    assert!(
        out.stdout.contains("`gone.png`, which does not exist"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("`Custom` is not lowercase"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("`logo` is defined more than once"),
        "{}",
        out.stdout
    );
    assert!(out.stdout.contains("3 errors"), "{}", out.stdout);
}

#[test]
fn check_strict_and_scaffold() {
    let tmp = TempDir::new();
    let (root, icons) = make_repo(&tmp);
    seed_consistent(&icons);
    write(&icons.join("metadata.ts"), &metadata(&["custom", "logo"]));

    let lenient = run(&root, &["check"]);
    assert_eq!(
        lenient.code, 0,
        "stdout: {}\nstderr: {}",
        lenient.stdout, lenient.stderr
    );
    assert!(
        lenient
            .stdout
            .contains("index key `big` has no entry in metadata.ts"),
        "{}",
        lenient.stdout
    );

    let strict = run(&root, &["check", "--strict"]);
    assert_eq!(strict.code, 1);

    let scaffold = run(&root, &["check", "--scaffold-metadata"]);
    assert_eq!(scaffold.code, 0);
    assert!(
        scaffold.stdout.contains("metadata.ts entries to add:"),
        "{}",
        scaffold.stdout
    );
    assert!(
        scaffold
            .stdout
            .contains("  big: {\n    name: \"big\",\n    displayName: \"Big\","),
        "{}",
        scaffold.stdout
    );
}

#[test]
fn check_reports_unparsable_index_as_io_error() {
    let tmp = TempDir::new();
    let (root, icons) = make_repo(&tmp);
    seed_consistent(&icons);
    write(
        &icons.join("index.ts"),
        "export const icons: Record<string, string> = {\n  a: `<svg/>`,\n",
    );
    let out = run(&root, &["check"]);
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("unterminated map"), "{}", out.stderr);
}

#[test]
fn index_generates_a_complete_typescript_module() {
    let tmp = TempDir::new();
    let (root, icons) = make_repo(&tmp);
    write(
        &icons.join("Foo-color.svg"),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- exported -->\n<svg width=\"24\" height=\"24\" viewBox=\"0 0 24 24\" xmlns=\"http://www.w3.org/2000/svg\">\n  <title>Foo `${x}`</title>\n  <path d=\"M0 0\n    L1 1\"/>\n</svg>\n",
    );
    write(&icons.join("foo.svg"), &svg("foo-mono"));
    write(&icons.join("logo_icon.png"), "png");
    write(
        &icons.join("huge.svg"),
        &format!("<svg>{}</svg>", "x".repeat(2000)),
    );
    write(&icons.join("claw.svg"), &svg("claw"));
    write(&icons.join("ignored.svg"), &svg("ignored"));
    write(&icons.join("notes.txt"), "not an icon");
    write(&icons.join("metadata.ts"), &metadata(&[]));

    let target = tmp.path().join("generated.ts");
    let out = run(
        &root,
        &[
            "index",
            "--out",
            target.to_str().unwrap(),
            "--inline-max-bytes",
            "1000",
            "--alias",
            "openclaw=claw.svg",
            "--ignore",
            "ignored.svg",
        ],
    );
    assert_eq!(
        out.code, 0,
        "stdout: {}\nstderr: {}",
        out.stdout, out.stderr
    );
    assert!(
        out.stderr
            .contains("skipped foo.svg: duplicate key `foo`, kept `Foo-color.svg`"),
        "{}",
        out.stderr
    );
    assert!(
        out.stderr.contains("skipped ignored.svg: ignored"),
        "{}",
        out.stderr
    );
    assert!(
        out.stderr
            .contains("skipped notes.txt: unsupported extension `.txt`"),
        "{}",
        out.stderr
    );

    let generated = fs::read_to_string(&target).unwrap();
    let expected_inline = "  foo: `<svg width=\"1em\" height=\"1em\" viewBox=\"0 0 24 24\" xmlns=\"http://www.w3.org/2000/svg\" style=\"flex:none;line-height:1\"><title>Foo \\`\\${x}\\`</title><path d=\"M0 0 L1 1\"/></svg>`,\n";
    let expected = format!(
        "// Auto-generated icon index
// Do not edit manually

import _huge from \"./huge.svg?url\";
import _logo from \"./logo_icon.png\";

export const icons: Record<string, string> = {{
{expected_inline}  openclaw: `<svg viewBox=\"0 0 24 24\" xmlns=\"http://www.w3.org/2000/svg\" width=\"1em\" height=\"1em\" style=\"flex:none;line-height:1\"><title>claw</title><path d=\"M0 0h24v24H0z\"/></svg>`,
}};

export const iconUrls: Record<string, string> = {{
  huge: _huge,
  logo: _logo,
}};

export const iconList = [
  ...Object.keys(icons),
  ...Object.keys(iconUrls),
].sort();

export function getIcon(name: string): string {{
  return icons[name.toLowerCase()] || \"\";
}}

export function getIconUrl(name: string): string {{
  return iconUrls[name.toLowerCase()] || \"\";
}}

export function hasIcon(name: string): boolean {{
  const key = name.toLowerCase();
  return key in icons || key in iconUrls;
}}

export function isUrlIcon(name: string): boolean {{
  return name.toLowerCase() in iconUrls;
}}

export {{ getIconMetadata }} from \"./metadata\";
"
    );
    assert_eq!(generated, expected);
}

#[test]
fn index_write_then_check_round_trips() {
    let tmp = TempDir::new();
    let (root, icons) = make_repo(&tmp);
    write(&icons.join("openai.svg"), &svg("openai"));
    write(&icons.join("logo.png"), "png");
    write(&icons.join("claw.svg"), &svg("claw"));
    write(
        &icons.join("metadata.ts"),
        &metadata(&["openai", "logo", "claw"]),
    );

    let written = run(&root, &["index", "--write"]);
    assert_eq!(
        written.code, 0,
        "stdout: {}\nstderr: {}",
        written.stdout, written.stderr
    );
    assert!(
        written.stdout.contains("(3 icons: 2 inline, 1 url)"),
        "{}",
        written.stdout
    );
    assert!(icons.join("index.ts").is_file());

    let up_to_date = run(&root, &["index", "--check"]);
    assert_eq!(
        up_to_date.code, 0,
        "stdout: {}\nstderr: {}",
        up_to_date.stdout, up_to_date.stderr
    );
    assert!(
        up_to_date.stdout.contains("is up to date (3 icons)"),
        "{}",
        up_to_date.stdout
    );

    // The generated index satisfies the consistency check, strictly.
    let checked = run(&root, &["check", "--strict"]);
    assert_eq!(
        checked.code, 0,
        "stdout: {}\nstderr: {}",
        checked.stdout, checked.stderr
    );

    // Adding a file makes --check report drift.
    write(&icons.join("newcomer.svg"), &svg("newcomer"));
    let drift = run(&root, &["index", "--check"]);
    assert_eq!(
        drift.code, 1,
        "stdout: {}\nstderr: {}",
        drift.stdout, drift.stderr
    );
    assert!(
        drift.stdout.contains("differs from the generated index"),
        "{}",
        drift.stdout
    );

    let conflicting = run(&root, &["index", "--write", "--check"]);
    assert_eq!(conflicting.code, 2);
    assert!(
        conflicting.stderr.contains("mutually exclusive"),
        "{}",
        conflicting.stderr
    );
}

#[test]
fn index_stdout_and_empty_directory() {
    let tmp = TempDir::new();
    let (root, icons) = make_repo(&tmp);
    let _ = icons;
    let out = run(&root, &["index"]);
    assert_eq!(
        out.code, 0,
        "stdout: {}\nstderr: {}",
        out.stdout, out.stderr
    );
    assert!(
        out.stdout
            .contains("export const icons: Record<string, string> = {};\n"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("export const iconUrls: Record<string, string> = {};\n"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout
            .ends_with("export { getIconMetadata } from \"./metadata\";\n"),
        "{}",
        out.stdout
    );
}
