//! Consistency checks between `index.ts`, `metadata.ts` and the files in
//! the icon directory.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::Path;

use crate::index::{self, IndexFile};
use crate::svg;
use crate::util;

/// Severity of a [`Finding`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// The application would misbehave (missing asset, unreachable key...).
    Error,
    /// Worth a look but harmless at runtime.
    Warning,
}

/// One problem found by [`analyze`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub level: Level,
    pub message: String,
}

impl Finding {
    fn error(message: impl Into<String>) -> Self {
        Self {
            level: Level::Error,
            message: message.into(),
        }
    }

    fn warning(message: impl Into<String>) -> Self {
        Self {
            level: Level::Warning,
            message: message.into(),
        }
    }
}

/// Outcome of [`run`].
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Report {
    pub findings: Vec<Finding>,
    pub index_keys: usize,
    pub metadata_keys: usize,
    pub files: usize,
    /// Index keys that have no metadata entry, in index order.
    pub missing_metadata: Vec<String>,
}

impl Report {
    pub fn errors(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.level == Level::Error)
            .count()
    }

    pub fn warnings(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.level == Level::Warning)
            .count()
    }

    /// `true` when there are no errors, and no warnings either in strict mode.
    pub fn passed(&self, strict: bool) -> bool {
        self.errors() == 0 && (!strict || self.warnings() == 0)
    }
}

/// Extracts the top-level keys of the `iconMetadata` object literal.
pub fn parse_metadata_keys(src: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut inside = false;
    for line in src.lines() {
        if !inside {
            if line.starts_with("export const iconMetadata") && line.ends_with("= {") {
                inside = true;
            }
            continue;
        }
        if line == "};" {
            break;
        }
        let Some(rest) = line.strip_prefix("  ") else {
            continue;
        };
        if rest.starts_with(' ') || rest.starts_with("//") {
            continue;
        }
        let Some(key_part) = rest.strip_suffix(": {") else {
            continue;
        };
        let key = key_part.trim().trim_matches('"').trim_matches('\'');
        if !key.is_empty() {
            keys.push(key.to_string());
        }
    }
    keys
}

/// Index keys with no metadata entry, in index order.
pub fn missing_metadata_keys(index: &IndexFile, metadata_keys: &[String]) -> Vec<String> {
    let metadata: BTreeSet<&str> = metadata_keys.iter().map(String::as_str).collect();
    index
        .keys()
        .into_iter()
        .filter(|key| !metadata.contains(key))
        .map(str::to_string)
        .collect()
}

/// Runs every check against already loaded inputs.
pub fn analyze(
    index: &IndexFile,
    metadata_keys: &[String],
    files: &[String],
    external_refs: &BTreeSet<String>,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let file_set: BTreeSet<&str> = files.iter().map(String::as_str).collect();

    // Imports must point at existing files and be used exactly once.
    let mut import_idents: HashMap<&str, usize> = HashMap::new();
    for import in &index.imports {
        if !file_set.contains(import.file.as_str()) {
            findings.push(Finding::error(format!(
                "import `{}` refers to `{}`, which does not exist",
                import.ident, import.file
            )));
        }
        *import_idents.entry(import.ident.as_str()).or_default() += 1;
    }
    for (ident, count) in &import_idents {
        if *count > 1 {
            findings.push(Finding::error(format!(
                "import identifier `{ident}` is declared {count} times"
            )));
        }
    }
    let used_idents: BTreeSet<&str> = index.urls.iter().map(|u| u.ident.as_str()).collect();
    for import in &index.imports {
        if !used_idents.contains(import.ident.as_str()) {
            findings.push(Finding::error(format!(
                "import `{}` is never used in iconUrls",
                import.ident
            )));
        }
    }
    for url in &index.urls {
        if !import_idents.contains_key(url.ident.as_str()) {
            findings.push(Finding::error(format!(
                "iconUrls entry `{}` references undefined import `{}`",
                url.key, url.ident
            )));
        }
    }

    // Keys must be unique and lowercase, because lookups lowercase the name.
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for key in index.keys() {
        *seen.entry(key).or_default() += 1;
        if key != key.to_lowercase() {
            findings.push(Finding::error(format!(
                "index key `{key}` is not lowercase, so getIcon()/hasIcon() can never match it"
            )));
        }
    }
    let mut duplicates: Vec<&str> = seen
        .iter()
        .filter(|(_, count)| **count > 1)
        .map(|(key, _)| *key)
        .collect();
    duplicates.sort_unstable();
    for key in duplicates {
        findings.push(Finding::error(format!(
            "index key `{key}` is defined more than once"
        )));
    }

    // Inline markup must be a complete <svg> element.
    for entry in &index.inline {
        let markup = svg::unescape_template_literal(&entry.body);
        let trimmed = markup.trim();
        if !trimmed.starts_with("<svg") || !trimmed.ends_with('>') {
            findings.push(Finding::error(format!(
                "inline icon `{}` does not look like an <svg> element",
                entry.key
            )));
        }
    }

    // Metadata keys: lowercase and unique.
    let mut seen_metadata: HashMap<&str, usize> = HashMap::new();
    for key in metadata_keys {
        *seen_metadata.entry(key.as_str()).or_default() += 1;
        if *key != key.to_lowercase() {
            findings.push(Finding::error(format!(
                "metadata key `{key}` is not lowercase"
            )));
        }
    }
    let mut duplicate_metadata: Vec<&str> = seen_metadata
        .iter()
        .filter(|(_, count)| **count > 1)
        .map(|(key, _)| *key)
        .collect();
    duplicate_metadata.sort_unstable();
    for key in duplicate_metadata {
        findings.push(Finding::error(format!(
            "metadata key `{key}` is defined more than once"
        )));
    }

    // Cross references between the index and the metadata.
    for key in missing_metadata_keys(index, metadata_keys) {
        findings.push(Finding::warning(format!(
            "index key `{key}` has no entry in metadata.ts"
        )));
    }
    let index_keys: BTreeSet<&str> = index.keys().into_iter().collect();
    for key in metadata_keys {
        if !index_keys.contains(key.as_str()) {
            findings.push(Finding::warning(format!(
                "metadata key `{key}` has no icon in index.ts"
            )));
        }
    }

    // Files nobody references.
    let referenced = index.referenced_files();
    for file in files {
        if util::is_generated_or_hidden(file) {
            continue;
        }
        let (_, ext) = util::split_extension(file);
        if !index::IMAGE_EXTENSIONS.contains(&ext.as_str()) {
            findings.push(Finding::warning(format!(
                "`{file}` is not an icon image and does not belong in this directory"
            )));
            continue;
        }
        let by_key = index_keys.contains(index::derive_key(file).as_str());
        if !referenced.contains(file) && !external_refs.contains(file) && !by_key {
            findings.push(Finding::warning(format!(
                "`{file}` is not referenced by index.ts or any source file"
            )));
        }
    }

    findings
}

/// Loads `index.ts`, `metadata.ts` and the directory listing from `dir`,
/// scans `root/src` for direct asset imports and runs [`analyze`].
pub fn run(root: &Path, dir: &Path) -> Result<Report, String> {
    let index_path = dir.join("index.ts");
    let metadata_path = dir.join("metadata.ts");
    let index_src =
        fs::read_to_string(&index_path).map_err(|e| format!("{}: {e}", index_path.display()))?;
    let metadata_src = fs::read_to_string(&metadata_path)
        .map_err(|e| format!("{}: {e}", metadata_path.display()))?;
    let index =
        index::parse_index(&index_src).map_err(|e| format!("{}: {e}", index_path.display()))?;
    let metadata_keys = parse_metadata_keys(&metadata_src);
    let files = util::list_files(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let external_refs = util::scan_external_references(&root.join("src"), dir)
        .map_err(|e| format!("scanning {}: {e}", root.join("src").display()))?;

    let findings = analyze(&index, &metadata_keys, &files, &external_refs);
    Ok(Report {
        missing_metadata: missing_metadata_keys(&index, &metadata_keys),
        findings,
        index_keys: index.keys().len(),
        metadata_keys: metadata_keys.len(),
        files: files.len(),
    })
}

/// Renders `metadata.ts` entry skeletons for `keys`, ready to paste.
pub fn scaffold_metadata(keys: &[String]) -> String {
    let mut out = String::new();
    for key in keys {
        let mut display: Vec<char> = key.chars().collect();
        if let Some(first) = display.first_mut() {
            *first = first.to_ascii_uppercase();
        }
        let display: String = display.into_iter().collect();
        out.push_str(&format!(
            "  {}: {{\n    name: \"{key}\",\n    displayName: \"{display}\",\n    category: \"ai-provider\",\n    keywords: [\"{key}\"],\n    defaultColor: \"#6B7280\",\n  }},\n",
            index_key(key)
        ));
    }
    out
}

fn index_key(key: &str) -> String {
    if index::is_identifier(key) {
        key.to_string()
    } else {
        format!("\"{key}\"")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{Import, InlineEntry, UrlEntry};

    const METADATA: &str = r##"// Icon metadata
import { IconMetadata } from "@/types/icon";

export const iconMetadata: Record<string, IconMetadata> = {
  openai: {
    name: "openai",
    displayName: "OpenAI",
    category: "ai-provider",
    keywords: ["gpt", "chatgpt"],
    defaultColor: "#00A67E",
  },
  "foo-bar": {
    name: "foo-bar",
    keywords: [],
  },
};

export function getIconMetadata(name: string): IconMetadata | undefined {
  return iconMetadata[name.toLowerCase()];
}
"##;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn sample_index() -> IndexFile {
        IndexFile {
            imports: vec![Import {
                ident: "_logo".into(),
                file: "logo.png".into(),
                url_query: false,
            }],
            inline: vec![InlineEntry {
                key: "openai".into(),
                body: "<svg viewBox=\"0 0 1 1\"/>".into(),
            }],
            urls: vec![UrlEntry {
                key: "logo".into(),
                ident: "_logo".into(),
            }],
        }
    }

    #[test]
    fn parses_metadata_keys_including_quoted_ones() {
        assert_eq!(parse_metadata_keys(METADATA), vec!["openai", "foo-bar"]);
        assert!(parse_metadata_keys(
            "export const iconMetadata: Record<string, IconMetadata> = {};"
        )
        .is_empty());
    }

    #[test]
    fn consistent_inputs_produce_no_findings() {
        let findings = analyze(
            &sample_index(),
            &strings(&["openai", "logo"]),
            &strings(&["index.ts", "metadata.ts", "logo.png", "openai.svg"]),
            &BTreeSet::new(),
        );
        assert!(findings.is_empty(), "{findings:?}");
    }

    #[test]
    fn detects_missing_files_and_bad_keys() {
        let mut index = sample_index();
        index.inline.push(InlineEntry {
            key: "OpenAI".into(),
            body: "<svg/>".into(),
        });
        index.inline.push(InlineEntry {
            key: "logo".into(),
            body: "not svg".into(),
        });
        index.imports.push(Import {
            ident: "_unused".into(),
            file: "unused.png".into(),
            url_query: false,
        });
        index.urls.push(UrlEntry {
            key: "ghost".into(),
            ident: "_ghost".into(),
        });
        let findings = analyze(
            &index,
            &strings(&["openai", "logo", "Ghost", "ghost", "ghost"]),
            &strings(&["index.ts", "metadata.ts", "openai.svg"]),
            &BTreeSet::new(),
        );
        let messages: Vec<&str> = findings
            .iter()
            .filter(|f| f.level == Level::Error)
            .map(|f| f.message.as_str())
            .collect();
        assert!(
            messages
                .iter()
                .any(|m| m.contains("`logo.png`, which does not exist")),
            "{messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("`unused.png`, which does not exist")),
            "{messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("`_unused` is never used")),
            "{messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("undefined import `_ghost`")),
            "{messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("`OpenAI` is not lowercase")),
            "{messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("`logo` is defined more than once")),
            "{messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("inline icon `logo` does not look like")),
            "{messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("metadata key `Ghost` is not lowercase")),
            "{messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("metadata key `ghost` is defined more than once")),
            "{messages:?}"
        );
    }

    #[test]
    fn reports_metadata_gaps_and_orphans_as_warnings() {
        let findings = analyze(
            &sample_index(),
            &strings(&["openai", "vanished"]),
            &strings(&[
                "index.ts",
                "metadata.ts",
                "logo.png",
                "openai.svg",
                "openai-color.svg",
                "claw.svg",
                "notes.txt",
                "orphan.svg",
            ]),
            &["claw.svg".to_string()].into_iter().collect(),
        );
        assert!(
            findings.iter().all(|f| f.level == Level::Warning),
            "{findings:?}"
        );
        let messages: Vec<&str> = findings.iter().map(|f| f.message.as_str()).collect();
        assert_eq!(
            messages,
            vec![
                "index key `logo` has no entry in metadata.ts",
                "metadata key `vanished` has no icon in index.ts",
                "`notes.txt` is not an icon image and does not belong in this directory",
                "`orphan.svg` is not referenced by index.ts or any source file",
            ]
        );
    }

    #[test]
    fn scaffolds_metadata_entries() {
        let snippet = scaffold_metadata(&strings(&["newapi", "foo-bar"]));
        assert!(
            snippet.contains("  newapi: {\n    name: \"newapi\",\n    displayName: \"Newapi\",")
        );
        assert!(snippet.contains("  \"foo-bar\": {"));
    }

    #[test]
    fn report_pass_logic() {
        let report = Report {
            findings: vec![Finding::warning("w")],
            ..Report::default()
        };
        assert!(report.passed(false));
        assert!(!report.passed(true));
        let report = Report {
            findings: vec![Finding::error("e")],
            ..Report::default()
        };
        assert!(!report.passed(false));
    }
}
