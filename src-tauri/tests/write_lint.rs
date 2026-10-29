//! Guard for the "config files are written atomically" claim (CCS-082).
//!
//! `clippy.toml` bans `std::fs::write`, but clippy cannot tell a config writer
//! built on `OpenOptions` apart from a legitimate one. This test fails when
//! `OpenOptions::new()` / `File::options()` shows up in `src/` outside the
//! files that are known to need it; new writers should use
//! `config::atomic_write` instead, or be added to the allowlist with a reason.

use std::fs;
use std::path::{Path, PathBuf};

/// Files allowed to open files for writing directly.
const OPEN_OPTIONS_ALLOWLIST: &[&str] = &[
    // atomic_write / atomic_write_secret (create_new temp file with 0600)
    "src/config.rs",
    // settings.json save; moves to atomic_write in CCS-084
    "src/settings.rs",
    // append-only crash log
    "src/panic_hook.rs",
];

const PATTERNS: &[&str] = &["OpenOptions::new(", "File::options("];

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read src dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

fn find_violations(root: &Path, allowlist: &[&str]) -> Vec<String> {
    let mut files = Vec::new();
    collect_rs_files(&root.join("src"), &mut files);
    files.sort();

    let mut violations = Vec::new();
    for file in files {
        let rel = file
            .strip_prefix(root)
            .expect("file under root")
            .to_string_lossy()
            .replace('\\', "/");
        if allowlist.contains(&rel.as_str()) {
            continue;
        }
        let content = fs::read_to_string(&file).expect("read source file");
        for (idx, line) in content.lines().enumerate() {
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            if PATTERNS.iter().any(|p| code.contains(p)) {
                violations.push(format!("{rel}:{}: {}", idx + 1, line.trim()));
            }
        }
    }
    violations
}

#[test]
fn open_options_only_used_by_allowlisted_writers() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let violations = find_violations(root, OPEN_OPTIONS_ALLOWLIST);
    assert!(
        violations.is_empty(),
        "OpenOptions used outside the allowlist; write config files through \
         config::atomic_write or extend OPEN_OPTIONS_ALLOWLIST in tests/write_lint.rs:\n{}",
        violations.join("\n")
    );
}

#[test]
fn allowlist_entries_exist_and_are_needed() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for entry in OPEN_OPTIONS_ALLOWLIST {
        let content = fs::read_to_string(root.join(entry))
            .unwrap_or_else(|e| panic!("allowlisted file {entry} missing: {e}"));
        assert!(
            PATTERNS.iter().any(|p| content.contains(p)),
            "{entry} no longer uses OpenOptions; drop it from the allowlist"
        );
    }
}

#[test]
fn scanner_flags_open_options_outside_allowlist() {
    // Dropping the allowlist must surface the known writers, proving the
    // scanner actually detects the pattern.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let violations = find_violations(root, &[]);
    assert!(
        violations.iter().any(|v| v.starts_with("src/config.rs:")),
        "scanner did not detect OpenOptions in src/config.rs: {violations:?}"
    );
}
