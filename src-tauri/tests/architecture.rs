//! Layering guard (TEST-30): the service, proxy and database layers must not
//! depend on the Tauri command layer (`crate::commands`).
//!
//! One test walks every `.rs` file under those layers, strips comments and
//! string literals, and looks for any path into `commands`: `crate::commands`,
//! `use crate::{.., commands, ..}`, or a `super::`-chain that climbs back to
//! the crate root and into `commands`. Known pre-existing proxy violations are
//! listed in `ALLOWED` so the guard ratchets: new ones fail, and an entry that
//! is no longer needed fails too (so the list only shrinks).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Layers that must stay independent of `crate::commands`.
const GUARDED_LAYERS: &[&str] = &["services", "proxy", "database"];

/// `(file relative to src/, offending path)` pairs that predate the guard.
/// Remove an entry once the dependency is gone (the test enforces that).
const ALLOWED: &[(&str, &str)] = &[
    // OAuth state structs live in commands/ but are read by the forwarder.
    ("proxy/forwarder.rs", "crate::commands::CopilotAuthState"),
    (
        "proxy/pipeline/auth.rs",
        "crate::commands::{CodexOAuthState",
    ),
];

fn src_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap_or_else(|e| panic!("read {}: {e}", dir.display())) {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Replace comments and the contents of string/char literals with spaces so
/// that prose and test fixtures mentioning `crate::commands` do not count.
fn strip_comments_and_strings(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if c == b'/' && bytes.get(i + 1) == Some(&b'*') {
            let mut depth = 0;
            while i < bytes.len() {
                if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                    depth += 1;
                    i += 2;
                } else if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            out.push(' ');
        } else if c == b'r'
            && matches!(bytes.get(i + 1), Some(b'"') | Some(b'#'))
            && !prev_is_ident(bytes, i)
        {
            // Raw string r"..." / r#"..."#.
            let mut j = i + 1;
            let mut hashes = 0;
            while bytes.get(j) == Some(&b'#') {
                hashes += 1;
                j += 1;
            }
            if bytes.get(j) != Some(&b'"') {
                out.push(c as char);
                i += 1;
                continue;
            }
            j += 1;
            loop {
                if j >= bytes.len() {
                    break;
                }
                if bytes[j] == b'"' && (0..hashes).all(|k| bytes.get(j + 1 + k) == Some(&b'#')) {
                    j += 1 + hashes;
                    break;
                }
                j += 1;
            }
            out.push_str("\"\"");
            i = j;
        } else if c == b'\'' && is_char_literal(bytes, i) {
            // Char literal such as '"' or '\''; lifetimes fall through.
            let mut j = i + 1;
            if bytes[j] == b'\\' {
                j += 2;
            }
            while j < bytes.len() && bytes[j] != b'\'' {
                j += 1;
            }
            out.push_str("' '");
            i = j + 1;
        } else if c == b'"' {
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            i += 1;
            out.push_str("\"\"");
        } else {
            // Push the full UTF-8 character.
            let ch = src[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

fn is_char_literal(bytes: &[u8], i: usize) -> bool {
    match bytes.get(i + 1) {
        Some(b'\\') => true,
        Some(_) => {
            // 'x' (ASCII) or a multi-byte char closed within 4 bytes.
            (2..=5).any(|k| bytes.get(i + k) == Some(&b'\'')) && !bytes[i + 1].is_ascii_alphabetic()
                || bytes.get(i + 2) == Some(&b'\'')
        }
        None => false,
    }
}

fn prev_is_ident(bytes: &[u8], i: usize) -> bool {
    i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_')
}

/// Every textual path into the commands module found in `code`.
fn command_paths(code: &str, module_depth: usize) -> Vec<String> {
    let squeezed: String = code.split_whitespace().collect::<Vec<_>>().join(" ");
    let compact = squeezed
        .replace(" :: ", "::")
        .replace(":: ", "::")
        .replace(" ::", "::");
    let mut hits = Vec::new();

    // 1. `crate::commands...`
    for (idx, _) in compact.match_indices("crate::commands") {
        let rest = &compact[idx..];
        let end = rest.find([';', ' ', '(', ',', '}']).unwrap_or(rest.len());
        hits.push(rest[..end].to_string());
    }

    // 2. `use crate::{ ..., commands, ... }`
    let mut search = 0;
    while let Some(pos) = compact[search..].find("crate::{") {
        let start = search + pos;
        let block_end = compact[start..]
            .find(';')
            .map(|e| start + e)
            .unwrap_or(compact.len());
        let block = &compact[start..block_end];
        let has_commands = block
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .any(|tok| tok == "commands");
        if has_commands {
            hits.push(block.to_string());
        }
        search = start + 1;
    }

    // 3. `super::super::...::commands` that climbs to the crate root.
    let needle = format!("{}commands", "super::".repeat(module_depth));
    for (idx, _) in compact.match_indices(&needle) {
        if idx >= 7 && &compact[idx - 7..idx] == "super::" {
            continue;
        }
        hits.push(needle.clone());
    }
    hits
}

#[test]
fn guarded_layers_do_not_depend_on_commands() {
    let root = src_root();
    let mut violations = BTreeSet::new();
    let mut scanned = 0;
    for layer in GUARDED_LAYERS {
        let mut files = Vec::new();
        rust_files(&root.join(layer), &mut files);
        for file in files {
            scanned += 1;
            let rel = file
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            // Depth of the file's module below the crate root: `services/x.rs`
            // is `crate::services::x`, so `super::super` reaches the root.
            let depth = rel.matches('/').count() + usize::from(!rel.ends_with("/mod.rs"));
            let code = strip_comments_and_strings(&fs::read_to_string(&file).unwrap());
            for hit in command_paths(&code, depth) {
                violations.insert((rel.clone(), hit));
            }
        }
    }
    assert!(
        scanned > 50,
        "scanned only {scanned} files; is the src layout different?"
    );

    let mut unexpected = Vec::new();
    let mut used = BTreeSet::new();
    for (file, hit) in &violations {
        match ALLOWED
            .iter()
            .find(|(f, prefix)| f == file && hit.starts_with(prefix))
        {
            Some(entry) => {
                used.insert(*entry);
            }
            None => unexpected.push(format!("{file}: {hit}")),
        }
    }
    assert!(
        unexpected.is_empty(),
        "service/proxy/database code must not depend on the commands layer \
         (move the shared type into the lower layer instead):\n{}",
        unexpected.join("\n")
    );
    let stale: Vec<_> = ALLOWED.iter().filter(|e| !used.contains(e)).collect();
    assert!(
        stale.is_empty(),
        "these ALLOWED entries are no longer needed; delete them: {stale:?}"
    );
}

#[test]
fn guard_detects_every_import_form() {
    let samples = [
        ("use crate::commands::Foo;", 1),
        ("use crate :: commands :: Foo;", 1),
        ("use crate::{config, commands::Foo};", 1),
        ("fn f() { crate::commands::foo(); }", 1),
        ("use super::super::commands::Foo;", 1),
        ("use super::commands::Foo;", 0), // a sibling module named commands
        ("use crate::config;", 0),
    ];
    for (code, expected) in samples {
        let code = strip_comments_and_strings(code);
        assert_eq!(command_paths(&code, 2).len(), expected, "{code}");
    }
    let ignored = strip_comments_and_strings(
        "// use crate::commands::X;\n/* crate::commands */ let s = \"crate::commands\"; let r = r#\"crate::commands\"#;",
    );
    assert!(command_paths(&ignored, 2).is_empty(), "{ignored}");
}
