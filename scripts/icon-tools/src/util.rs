//! Shared helpers: repository discovery, file name handling and reference
//! scanning.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Location of the curated icon assets, relative to the repository root.
pub const EXTRACTED_REL: &str = "src/icons/extracted";

/// Location of the upstream icon package, relative to the repository root.
pub const LOBEHUB_REL: &str = "node_modules/@lobehub/icons-static-svg/icons";

/// Generated files that live next to the icon assets and are never treated
/// as icons themselves.
pub const GENERATED_FILES: &[&str] = &["index.ts", "metadata.ts", "README.md"];

/// Walks up from `start` until a directory that looks like the repository
/// root (a `package.json` next to a `src/icons` directory) is found.
pub fn find_repo_root(start: &Path) -> Option<PathBuf> {
    let mut current = Some(start);
    while let Some(dir) = current {
        if dir.join("package.json").is_file() && dir.join("src").join("icons").is_dir() {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }
    None
}

/// Resolves the repository root from the working directory, falling back to
/// the location of this crate (`scripts/icon-tools`) inside the repository.
pub fn default_repo_root() -> PathBuf {
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(root) = find_repo_root(&cwd) {
            return root;
        }
    }
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| manifest_dir.to_path_buf())
}

/// Splits `name` into `(stem, lowercase extension)`. Files without an
/// extension yield an empty extension.
pub fn split_extension(name: &str) -> (&str, String) {
    match name.rfind('.') {
        Some(pos) if pos > 0 => (&name[..pos], name[pos + 1..].to_ascii_lowercase()),
        _ => (name, String::new()),
    }
}

/// Returns `true` for a file that is one of the generated companions of the
/// icon assets (`index.ts`, `metadata.ts`, ...), or a hidden file.
pub fn is_generated_or_hidden(name: &str) -> bool {
    name.starts_with('.') || GENERATED_FILES.contains(&name)
}

/// Lists the plain files in `dir`, sorted by name.
pub fn list_files(dir: &Path) -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            if let Some(name) = entry.file_name().to_str() {
                names.push(name.to_string());
            }
        }
    }
    names.sort();
    Ok(names)
}

/// Scans every `.ts`/`.tsx` file below `src_root` (except `skip_dir`) for
/// direct references to files inside the extracted icon directory, such as
/// `import Claude from "@/icons/extracted/claude.svg?url"`.
///
/// Returns the referenced file names.
pub fn scan_external_references(src_root: &Path, skip_dir: &Path) -> io::Result<BTreeSet<String>> {
    let mut refs = BTreeSet::new();
    if !src_root.is_dir() {
        return Ok(refs);
    }
    let skip_dir = skip_dir
        .canonicalize()
        .unwrap_or_else(|_| skip_dir.to_path_buf());
    let mut stack = vec![src_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let canonical = dir.canonicalize().unwrap_or_else(|_| dir.clone());
        if canonical == skip_dir {
            continue;
        }
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() {
                let is_ts = matches!(
                    path.extension().and_then(|e| e.to_str()),
                    Some("ts") | Some("tsx")
                );
                if !is_ts {
                    continue;
                }
                if let Ok(content) = fs::read_to_string(&path) {
                    collect_references(&content, &mut refs);
                }
            }
        }
    }
    Ok(refs)
}

/// Extracts `icons/extracted/<file>` references from `content`.
pub fn collect_references(content: &str, refs: &mut BTreeSet<String>) {
    const MARKER: &str = "icons/extracted/";
    let mut rest = content;
    while let Some(pos) = rest.find(MARKER) {
        let after = &rest[pos + MARKER.len()..];
        let end = after
            .find(|c: char| c == '"' || c == '\'' || c == '`' || c == '?' || c.is_whitespace())
            .unwrap_or(after.len());
        let name = &after[..end];
        if !name.is_empty() && name.contains('.') && !name.contains('/') {
            refs.insert(name.to_string());
        }
        rest = after;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_extension_case_insensitively() {
        assert_eq!(split_extension("Foo.SVG"), ("Foo", "svg".to_string()));
        assert_eq!(split_extension("a.b.png"), ("a.b", "png".to_string()));
        assert_eq!(split_extension("noext"), ("noext", String::new()));
        assert_eq!(split_extension(".hidden"), (".hidden", String::new()));
    }

    #[test]
    fn collects_direct_file_references() {
        let mut refs = BTreeSet::new();
        collect_references(
            r#"import A from "@/icons/extracted/claude.svg?url";
               import B from '@/icons/extracted/logo.png';
               import { iconList } from "@/icons/extracted";
               import { x } from "@/icons/extracted/metadata";"#,
            &mut refs,
        );
        let expected: BTreeSet<String> = ["claude.svg", "logo.png"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(refs, expected);
    }

    #[test]
    fn generated_files_are_recognised() {
        assert!(is_generated_or_hidden("index.ts"));
        assert!(is_generated_or_hidden(".DS_Store"));
        assert!(!is_generated_or_hidden("openai.svg"));
    }
}
