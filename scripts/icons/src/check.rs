//! Consistency checks between the icon files, `index.ts` and `metadata.ts`.

use std::collections::{HashMap, HashSet};

use crate::{list_files, read_text, ts, Config, Result};

/// Outcome of a check run.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Report {
    /// Problems that make the index unusable or inconsistent.
    pub errors: Vec<String>,
    /// Gaps worth a look that do not break anything.
    pub warnings: Vec<String>,
    /// Number of inline icons.
    pub inline_count: usize,
    /// Number of URL icons.
    pub url_count: usize,
    /// Number of metadata entries.
    pub metadata_count: usize,
}

impl Report {
    /// True when no error was found.
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Name suffixes that upstream and custom files carry but index keys drop.
const STEM_SUFFIXES: &[&str] = &["-color", "-icon-dark", "-logo-light", "-icon", "_icon"];

/// Derives the index key a file would be registered under by convention.
pub fn key_for_file(file: &str) -> String {
    let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file);
    let mut key = stem.to_lowercase();
    for suffix in STEM_SUFFIXES {
        if let Some(base) = key.strip_suffix(suffix) {
            key = base.to_string();
            break;
        }
    }
    key
}

/// Runs the checks against the given inputs. `files` are the names of the
/// files in the icons directory.
pub fn analyze(index_src: &str, metadata_src: &str, files: &[String]) -> Report {
    let index = ts::parse_index(index_src);
    let metadata = ts::parse_metadata(metadata_src);
    let mut report = Report {
        inline_count: index.inline_keys.len(),
        url_count: index.url_keys.len(),
        metadata_count: metadata.keys.len(),
        ..Report::default()
    };
    let file_set: HashSet<&str> = files.iter().map(String::as_str).collect();

    // Duplicate keys across both maps.
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for key in index.inline_keys.iter().chain(index.url_keys.iter()) {
        *seen.entry(key.as_str()).or_default() += 1;
    }
    let mut dupes: Vec<&str> = seen
        .iter()
        .filter(|(_, n)| **n > 1)
        .map(|(k, _)| *k)
        .collect();
    dupes.sort_unstable();
    for key in dupes {
        report
            .errors
            .push(format!("index.ts: key `{key}` is defined more than once"));
    }
    for key in index.inline_keys.iter().chain(index.url_keys.iter()) {
        if *key != key.to_lowercase() {
            report.errors.push(format!(
                "index.ts: key `{key}` must be lower-case (lookups lower-case the name)"
            ));
        }
    }

    // Imports must point at existing files, and SVG imports must use ?url.
    let mut idents: HashSet<&str> = HashSet::new();
    for import in &index.imports {
        idents.insert(import.ident.as_str());
        if !file_set.contains(import.file.as_str()) {
            report.errors.push(format!(
                "index.ts: import `{}` points at missing file {}",
                import.ident, import.file
            ));
        }
        if import.file.ends_with(".svg") && !import.url {
            report.errors.push(format!(
                "index.ts: SVG import `{}` must use the `?url` suffix",
                import.ident
            ));
        }
    }
    // Every URL entry needs an import and vice versa.
    for key in &index.url_keys {
        let ident = format!("_{key}");
        if !idents.contains(ident.as_str()) {
            report.errors.push(format!(
                "index.ts: iconUrls entry `{key}` has no matching `import {ident}`"
            ));
        }
    }
    let url_idents: HashSet<String> = index.url_keys.iter().map(|k| format!("_{k}")).collect();
    for import in &index.imports {
        if !url_idents.contains(&import.ident) {
            report.warnings.push(format!(
                "index.ts: import `{}` is not used by iconUrls",
                import.ident
            ));
        }
    }

    // Metadata coverage in both directions.
    let metadata_keys: HashSet<&str> = metadata.keys.iter().map(String::as_str).collect();
    for key in index.inline_keys.iter().chain(index.url_keys.iter()) {
        if !metadata_keys.contains(key.as_str()) {
            report
                .warnings
                .push(format!("metadata.ts: no entry for icon `{key}`"));
        }
    }
    for key in &metadata.keys {
        if !index.has_key(key) {
            report.warnings.push(format!(
                "metadata.ts: entry `{key}` has no icon in index.ts"
            ));
        }
    }

    // Files that nothing references by name.
    let imported: HashSet<&str> = index.imports.iter().map(|i| i.file.as_str()).collect();
    for file in files {
        if file.ends_with(".ts") || file.ends_with(".md") {
            continue;
        }
        if imported.contains(file.as_str()) {
            continue;
        }
        let key = key_for_file(file);
        if !index.has_key(&key) {
            report.warnings.push(format!(
                "{file}: not imported and no index key `{key}` (rename the file or register it)"
            ));
        }
    }

    report
}

/// Runs the checks against the configured repository.
pub fn run(cfg: &Config) -> Result<Report> {
    let index_src = read_text(&cfg.index_path())?;
    let metadata_src = read_text(&cfg.metadata_path())?;
    let files = list_files(&cfg.icons_dir)?;
    Ok(analyze(&index_src, &metadata_src, &files))
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX: &str = "import _foo from \"./foo.png\";\nimport _bar from \"./bar.svg\";\nimport _gone from \"./gone.png\";\n\
export const icons: Record<string, string> = {\n  openai: `<svg/>`,\n  Claude: `<svg/>`,\n  foo: `<svg/>`,\n};\n\
export const iconUrls: Record<string, string> = {\n  foo: _foo,\n  bar: _bar,\n  gone: _gone,\n  baz: _baz,\n};\n";
    const METADATA: &str = "export const iconMetadata: Record<string, IconMetadata> = {\n  openai: {\n    name: \"openai\",\n  },\n  orphan: {\n    name: \"orphan\",\n  },\n};\n";

    #[test]
    fn reports_errors_and_warnings() {
        let files = vec![
            "foo.png".to_string(),
            "bar.svg".to_string(),
            "openai.svg".to_string(),
            "Some-Thing-icon.png".to_string(),
            "index.ts".to_string(),
        ];
        let report = analyze(INDEX, METADATA, &files);
        assert_eq!(report.inline_count, 3);
        assert_eq!(report.url_count, 4);
        assert_eq!(report.metadata_count, 2);
        assert!(!report.is_ok());
        let errors = report.errors.join("\n");
        assert!(errors.contains("`foo` is defined more than once"));
        assert!(errors.contains("`Claude` must be lower-case"));
        assert!(errors.contains("`_gone` points at missing file gone.png"));
        assert!(errors.contains("`_bar` must use the `?url` suffix"));
        assert!(errors.contains("`baz` has no matching `import _baz`"));
        let warnings = report.warnings.join("\n");
        assert!(warnings.contains("no entry for icon `Claude`"));
        assert!(warnings.contains("entry `orphan` has no icon"));
        assert!(
            warnings.contains("Some-Thing-icon.png: not imported and no index key `some-thing`")
        );
        assert!(!warnings.contains("openai.svg"));
    }

    #[test]
    fn clean_inputs_pass() {
        let index = "import _foo from \"./foo.png\";\n\
export const icons: Record<string, string> = {\n  openai: `<svg/>`,\n};\n\
export const iconUrls: Record<string, string> = {\n  foo: _foo,\n};\n";
        let metadata = "export const iconMetadata: Record<string, IconMetadata> = {\n  openai: {\n    name: \"openai\",\n  },\n  foo: {\n    name: \"foo\",\n  },\n};\n";
        let files = vec![
            "foo.png".into(),
            "openai-color.svg".into(),
            "metadata.ts".into(),
        ];
        let report = analyze(index, metadata, &files);
        assert!(report.is_ok(), "{:?}", report.errors);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    }

    #[test]
    fn derives_keys_from_file_names() {
        assert_eq!(key_for_file("TeamoRouter-icon-dark.png"), "teamorouter");
        assert_eq!(key_for_file("apinebula_icon.png"), "apinebula");
        assert_eq!(key_for_file("aihubmix-color.svg"), "aihubmix");
        assert_eq!(key_for_file("opencode-logo-light.svg"), "opencode");
        assert_eq!(key_for_file("ClaudeApi.png"), "claudeapi");
        assert_eq!(key_for_file("plain"), "plain");
    }
}
