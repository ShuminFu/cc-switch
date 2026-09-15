//! Parsing and rendering of `src/icons/extracted/index.ts`.
//!
//! The index has two maps: `icons` holds inline SVG markup as template
//! literals and `iconUrls` holds Vite asset imports for raster images (and
//! for SVGs that are too large to inline).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::Path;

use crate::svg;
use crate::util;

/// First lines of every generated index.
pub const HEADER: &str = "// Auto-generated icon index\n// Do not edit manually\n";

const ICONS_OPEN: &str = "export const icons: Record<string, string> = {";
const URLS_OPEN: &str = "export const iconUrls: Record<string, string> = {";

/// File extensions that are considered icon images.
pub const IMAGE_EXTENSIONS: &[&str] = &["svg", "png", "jpg", "jpeg", "webp", "gif", "ico", "avif"];

/// Suffixes stripped from a file stem when deriving its icon key. Longer
/// suffixes come first so `-icon-dark` wins over `-dark`.
pub const STRIPPED_SUFFIXES: &[&str] = &[
    "-icon-dark",
    "-icon-light",
    "-logo-light",
    "-logo-dark",
    "-color",
    "-icon",
    "_icon",
    "-logo",
    "_logo",
    "-dark",
    "-light",
];

/// A Vite asset import at the top of the index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    pub ident: String,
    pub file: String,
    pub url_query: bool,
}

/// An entry of the `icons` map. `body` is the template literal content as
/// written in the file (still escaped).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineEntry {
    pub key: String,
    pub body: String,
}

/// An entry of the `iconUrls` map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrlEntry {
    pub key: String,
    pub ident: String,
}

/// Parsed representation of `index.ts`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct IndexFile {
    pub imports: Vec<Import>,
    pub inline: Vec<InlineEntry>,
    pub urls: Vec<UrlEntry>,
}

impl IndexFile {
    /// All keys, inline first, in file order.
    pub fn keys(&self) -> Vec<&str> {
        self.inline
            .iter()
            .map(|e| e.key.as_str())
            .chain(self.urls.iter().map(|e| e.key.as_str()))
            .collect()
    }

    /// File names referenced by the index: imported assets plus the
    /// `<key>.svg` companion of every inline entry.
    pub fn referenced_files(&self) -> BTreeSet<String> {
        let mut files: BTreeSet<String> = self.imports.iter().map(|i| i.file.clone()).collect();
        for entry in &self.inline {
            files.insert(format!("{}.svg", entry.key));
        }
        files
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    None,
    Icons,
    Urls,
}

/// Parses the content of an `index.ts` produced by this tool or maintained
/// by hand in the same layout.
pub fn parse_index(src: &str) -> Result<IndexFile, String> {
    let mut index = IndexFile::default();
    let mut section = Section::None;
    let mut lines = src.lines().enumerate().peekable();

    while let Some((line_no, line)) = lines.next() {
        let human_line = line_no + 1;
        match section {
            Section::None => {
                if line == ICONS_OPEN {
                    section = Section::Icons;
                } else if line == URLS_OPEN {
                    section = Section::Urls;
                } else if line.starts_with("import ") && line.contains(" from \"./") {
                    index.imports.push(parse_import(line, human_line)?);
                }
            }
            Section::Icons => {
                if line == "};" {
                    section = Section::None;
                    continue;
                }
                if is_ignorable(line) {
                    continue;
                }
                let trimmed = line.trim_start();
                let (key, after_key) = split_key(trimmed, human_line)?;
                let Some(body_start) = after_key.strip_prefix('`') else {
                    return Err(format!(
                        "line {human_line}: expected a template literal for icon `{key}`"
                    ));
                };
                let mut body = String::new();
                let mut current = body_start.to_string();
                loop {
                    if let Some(stripped) = strip_literal_end(&current) {
                        body.push_str(stripped);
                        break;
                    }
                    body.push_str(&current);
                    body.push('\n');
                    match lines.next() {
                        Some((_, next)) => current = next.to_string(),
                        None => {
                            return Err(format!(
                                "line {human_line}: unterminated template literal for icon `{key}`"
                            ))
                        }
                    }
                }
                index.inline.push(InlineEntry { key, body });
            }
            Section::Urls => {
                if line == "};" {
                    section = Section::None;
                    continue;
                }
                if is_ignorable(line) {
                    continue;
                }
                let trimmed = line.trim_start();
                let (key, after_key) = split_key(trimmed, human_line)?;
                let ident = after_key.trim_end_matches(',').trim();
                if ident.is_empty() || !ident.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                {
                    return Err(format!(
                        "line {human_line}: expected an import identifier for icon `{key}`"
                    ));
                }
                index.urls.push(UrlEntry {
                    key,
                    ident: ident.to_string(),
                });
            }
        }
    }

    if section != Section::None {
        return Err("unterminated map: missing closing `};`".to_string());
    }
    Ok(index)
}

fn is_ignorable(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.is_empty() || trimmed.starts_with("//")
}

fn strip_literal_end(line: &str) -> Option<&str> {
    let body = line.strip_suffix("`,").or_else(|| line.strip_suffix('`'))?;
    // A backslash right before the closing backtick means it was escaped.
    let trailing_backslashes = body.chars().rev().take_while(|c| *c == '\\').count();
    if trailing_backslashes % 2 == 1 {
        return None;
    }
    Some(body)
}

fn split_key(line: &str, human_line: usize) -> Result<(String, &str), String> {
    let (raw_key, rest) = if let Some(after_quote) = line.strip_prefix('"') {
        let end = after_quote
            .find('"')
            .ok_or_else(|| format!("line {human_line}: unterminated quoted key"))?;
        (&after_quote[..end], &after_quote[end + 1..])
    } else {
        let end = line
            .find(':')
            .ok_or_else(|| format!("line {human_line}: expected `key:`"))?;
        (&line[..end], &line[end..])
    };
    let rest = rest
        .strip_prefix(':')
        .ok_or_else(|| format!("line {human_line}: expected `:` after key"))?
        .trim_start();
    if raw_key.is_empty() {
        return Err(format!("line {human_line}: empty key"));
    }
    Ok((raw_key.to_string(), rest))
}

fn parse_import(line: &str, human_line: usize) -> Result<Import, String> {
    let rest = line
        .strip_prefix("import ")
        .ok_or_else(|| format!("line {human_line}: malformed import"))?;
    let (ident, rest) = rest
        .split_once(" from \"./")
        .ok_or_else(|| format!("line {human_line}: malformed import"))?;
    let spec = rest
        .strip_suffix("\";")
        .ok_or_else(|| format!("line {human_line}: import must end with `\";`"))?;
    let (file, url_query) = match spec.strip_suffix("?url") {
        Some(file) => (file, true),
        None => (spec, false),
    };
    if ident.is_empty() || file.is_empty() {
        return Err(format!("line {human_line}: malformed import"));
    }
    Ok(Import {
        ident: ident.trim().to_string(),
        file: file.to_string(),
        url_query,
    })
}

/// How an icon file is exposed by the index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// SVG markup embedded as a template literal.
    InlineSvg,
    /// SVG imported as an asset URL (`?url`).
    UrlSvg,
    /// Raster image imported as an asset URL.
    Raster,
}

/// One file selected for the generated index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub key: String,
    pub file: String,
    pub kind: Kind,
    pub size: u64,
}

/// Options for [`plan_index`] and [`render_index`].
#[derive(Debug, Clone)]
pub struct GenOptions {
    /// SVG files larger than this are imported as URLs instead of inlined.
    pub inline_max_bytes: u64,
    /// Apply [`svg::apply_root_attributes`] to inlined markup.
    pub normalize: bool,
    /// Explicit `file -> key` overrides.
    pub aliases: BTreeMap<String, String>,
    /// File names to leave out.
    pub ignore: BTreeSet<String>,
}

impl Default for GenOptions {
    fn default() -> Self {
        Self {
            inline_max_bytes: 32 * 1024,
            normalize: true,
            aliases: BTreeMap::new(),
            ignore: BTreeSet::new(),
        }
    }
}

/// Result of scanning a directory for icon files.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct IndexPlan {
    /// Selected files, sorted by key.
    pub entries: Vec<Candidate>,
    /// Files that were not selected, with the reason.
    pub skipped: Vec<(String, String)>,
}

/// Derives the icon key from a file name: lowercase stem with well-known
/// variant suffixes removed.
pub fn derive_key(file_name: &str) -> String {
    let (stem, _) = util::split_extension(file_name);
    let mut key = stem.to_ascii_lowercase();
    loop {
        let before = key.len();
        for suffix in STRIPPED_SUFFIXES {
            if let Some(stripped) = key.strip_suffix(suffix) {
                if !stripped.is_empty() {
                    key = stripped.to_string();
                    break;
                }
            }
        }
        if key.len() == before {
            break;
        }
    }
    key
}

/// `true` when `key` can be written without quotes in an object literal.
pub fn is_identifier(key: &str) -> bool {
    let mut chars = key.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '$' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// Identifier used for the asset import of `key`.
pub fn import_ident(key: &str) -> String {
    let mut ident = String::from("_");
    ident.extend(
        key.chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }),
    );
    ident
}

fn priority(file: &str, kind: Kind) -> u8 {
    match kind {
        Kind::InlineSvg | Kind::UrlSvg if derive_stem(file).ends_with("-color") => 3,
        Kind::InlineSvg | Kind::UrlSvg => 2,
        Kind::Raster => 1,
    }
}

fn derive_stem(file: &str) -> String {
    util::split_extension(file).0.to_ascii_lowercase()
}

/// Scans `dir` and decides which file backs each icon key.
pub fn plan_index(dir: &Path, opts: &GenOptions) -> io::Result<IndexPlan> {
    let mut by_key: BTreeMap<String, Candidate> = BTreeMap::new();
    let mut skipped = Vec::new();

    for file in util::list_files(dir)? {
        if util::is_generated_or_hidden(&file) {
            continue;
        }
        if opts.ignore.contains(&file) {
            skipped.push((file, "ignored".to_string()));
            continue;
        }
        let (_, ext) = util::split_extension(&file);
        if !IMAGE_EXTENSIONS.contains(&ext.as_str()) {
            skipped.push((file, format!("unsupported extension `.{ext}`")));
            continue;
        }
        let size = fs::metadata(dir.join(&file))?.len();
        let kind = if ext == "svg" {
            if size > opts.inline_max_bytes {
                Kind::UrlSvg
            } else {
                Kind::InlineSvg
            }
        } else {
            Kind::Raster
        };
        let key = opts
            .aliases
            .get(&file)
            .cloned()
            .unwrap_or_else(|| derive_key(&file));
        if key.is_empty() {
            skipped.push((file, "empty key".to_string()));
            continue;
        }
        let candidate = Candidate {
            key: key.clone(),
            file: file.clone(),
            kind,
            size,
        };
        match by_key.get(&key) {
            Some(existing) => {
                let existing_priority = priority(&existing.file, existing.kind);
                let new_priority = priority(&file, kind);
                let replace = new_priority > existing_priority
                    || (new_priority == existing_priority && file < existing.file);
                if replace {
                    let old = by_key
                        .insert(key.clone(), candidate)
                        .expect("existing entry");
                    skipped.push((old.file, format!("duplicate key `{key}`, kept `{file}`")));
                } else {
                    skipped.push((
                        file,
                        format!("duplicate key `{key}`, kept `{}`", existing.file),
                    ));
                }
            }
            None => {
                by_key.insert(key, candidate);
            }
        }
    }

    Ok(IndexPlan {
        entries: by_key.into_values().collect(),
        skipped,
    })
}

fn render_key(key: &str) -> String {
    if is_identifier(key) {
        key.to_string()
    } else {
        format!("\"{key}\"")
    }
}

/// Renders the TypeScript source for `plan`.
pub fn render_index(dir: &Path, plan: &IndexPlan, opts: &GenOptions) -> io::Result<String> {
    let mut out = String::from(HEADER);

    let url_entries: Vec<&Candidate> = plan
        .entries
        .iter()
        .filter(|c| c.kind != Kind::InlineSvg)
        .collect();
    let inline_entries: Vec<&Candidate> = plan
        .entries
        .iter()
        .filter(|c| c.kind == Kind::InlineSvg)
        .collect();

    if !url_entries.is_empty() {
        out.push('\n');
        for entry in &url_entries {
            let query = if entry.kind == Kind::UrlSvg {
                "?url"
            } else {
                ""
            };
            out.push_str(&format!(
                "import {} from \"./{}{}\";\n",
                import_ident(&entry.key),
                entry.file,
                query
            ));
        }
    }

    out.push('\n');
    if inline_entries.is_empty() {
        out.push_str("export const icons: Record<string, string> = {};\n");
    } else {
        out.push_str(ICONS_OPEN);
        out.push('\n');
        for entry in &inline_entries {
            let raw = fs::read_to_string(dir.join(&entry.file))
                .map_err(|err| io::Error::new(err.kind(), format!("{}: {err}", entry.file)))?;
            let markup = if opts.normalize {
                svg::normalize(&raw)
            } else {
                svg::strip_and_collapse(&raw)
            };
            out.push_str(&format!(
                "  {}: `{}`,\n",
                render_key(&entry.key),
                svg::escape_template_literal(&markup)
            ));
        }
        out.push_str("};\n");
    }

    out.push('\n');
    if url_entries.is_empty() {
        out.push_str("export const iconUrls: Record<string, string> = {};\n");
    } else {
        out.push_str(URLS_OPEN);
        out.push('\n');
        for entry in &url_entries {
            out.push_str(&format!(
                "  {}: {},\n",
                render_key(&entry.key),
                import_ident(&entry.key)
            ));
        }
        out.push_str("};\n");
    }

    out.push_str(FOOTER);
    Ok(out)
}

/// Helper functions appended to every generated index. They mirror the
/// hand-maintained file so consumers keep working unchanged.
pub const FOOTER: &str = r#"
export const iconList = [
  ...Object.keys(icons),
  ...Object.keys(iconUrls),
].sort();

export function getIcon(name: string): string {
  return icons[name.toLowerCase()] || "";
}

export function getIconUrl(name: string): string {
  return iconUrls[name.toLowerCase()] || "";
}

export function hasIcon(name: string): boolean {
  const key = name.toLowerCase();
  return key in icons || key in iconUrls;
}

export function isUrlIcon(name: string): boolean {
  return name.toLowerCase() in iconUrls;
}

export { getIconMetadata } from "./metadata";
"#;

/// Convenience wrapper: plan and render in one step.
pub fn generate_index(dir: &Path, opts: &GenOptions) -> io::Result<(String, IndexPlan)> {
    let plan = plan_index(dir, opts)?;
    let source = render_index(dir, &plan, opts)?;
    Ok((source, plan))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"// Auto-generated icon index
// Do not edit manually

import _logo from "./logo.png";
import _big from "./big.svg?url";

export const icons: Record<string, string> = {
  openai: `<svg viewBox="0 0 1 1"><path d="M0 0"/></svg>`,
  "0x": `<svg>\`tick\` \${x}</svg>`,
};

export const iconUrls: Record<string, string> = {
  logo: _logo,
  big: _big,
};

export const iconList = [
  ...Object.keys(icons),
  ...Object.keys(iconUrls),
].sort();
"#;

    #[test]
    fn parses_imports_inline_and_url_entries() {
        let index = parse_index(SAMPLE).expect("parse");
        assert_eq!(
            index.imports,
            vec![
                Import {
                    ident: "_logo".into(),
                    file: "logo.png".into(),
                    url_query: false
                },
                Import {
                    ident: "_big".into(),
                    file: "big.svg".into(),
                    url_query: true
                },
            ]
        );
        assert_eq!(index.inline.len(), 2);
        assert_eq!(index.inline[0].key, "openai");
        assert_eq!(
            index.inline[0].body,
            "<svg viewBox=\"0 0 1 1\"><path d=\"M0 0\"/></svg>"
        );
        assert_eq!(index.inline[1].key, "0x");
        assert_eq!(index.inline[1].body, "<svg>\\`tick\\` \\${x}</svg>");
        assert_eq!(
            index.urls,
            vec![
                UrlEntry {
                    key: "logo".into(),
                    ident: "_logo".into()
                },
                UrlEntry {
                    key: "big".into(),
                    ident: "_big".into()
                },
            ]
        );
        assert_eq!(index.keys(), vec!["openai", "0x", "logo", "big"]);
        let referenced: Vec<String> = index.referenced_files().into_iter().collect();
        assert_eq!(
            referenced,
            vec!["0x.svg", "big.svg", "logo.png", "openai.svg"]
        );
    }

    #[test]
    fn parses_multiline_template_literals() {
        let src =
            "export const icons: Record<string, string> = {\n  multi: `<svg>\n<g/>\n</svg>`,\n};\n";
        let index = parse_index(src).expect("parse");
        assert_eq!(index.inline[0].body, "<svg>\n<g/>\n</svg>");
    }

    #[test]
    fn parses_empty_maps() {
        let src = "export const icons: Record<string, string> = {};\n\nexport const iconUrls: Record<string, string> = {};\n";
        let index = parse_index(src).expect("parse");
        assert!(index.inline.is_empty());
        assert!(index.urls.is_empty());
    }

    #[test]
    fn rejects_malformed_entries() {
        let src =
            "export const icons: Record<string, string> = {\n  broken: \"not a template\",\n};\n";
        let err = parse_index(src).unwrap_err();
        assert!(err.contains("line 2"), "{err}");

        let src = "export const iconUrls: Record<string, string> = {\n  broken: `x`,\n};\n";
        let err = parse_index(src).unwrap_err();
        assert!(err.contains("import identifier"), "{err}");

        let src = "export const icons: Record<string, string> = {\n  a: `<svg/>`,\n";
        let err = parse_index(src).unwrap_err();
        assert!(err.contains("unterminated map"), "{err}");
    }

    #[test]
    fn derives_keys_from_file_names() {
        assert_eq!(derive_key("OpenAI.svg"), "openai");
        assert_eq!(derive_key("longcat-color.svg"), "longcat");
        assert_eq!(derive_key("TeamoRouter-icon-dark.png"), "teamorouter");
        assert_eq!(derive_key("apinebula_icon.png"), "apinebula");
        assert_eq!(derive_key("opencode-logo-light.svg"), "opencode");
        assert_eq!(derive_key("fenno-icon.webp"), "fenno");
        assert_eq!(derive_key("zetaapi-icon.png"), "zetaapi");
        assert_eq!(derive_key("-color.svg"), "-color");
        assert_eq!(derive_key("foo-bar.svg"), "foo-bar");
    }

    #[test]
    fn identifier_detection_and_import_idents() {
        assert!(is_identifier("openai"));
        assert!(is_identifier("_x1"));
        assert!(!is_identifier("0x"));
        assert!(!is_identifier("foo-bar"));
        assert_eq!(import_ident("foo-bar"), "_foo_bar");
        assert_eq!(import_ident("0x"), "_0x");
        assert_eq!(render_key("foo-bar"), "\"foo-bar\"");
        assert_eq!(render_key("ok"), "ok");
    }
}
