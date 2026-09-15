//! Line-oriented reading and editing of `index.ts` and `metadata.ts`.
//!
//! Both files follow a fixed, Prettier-formatted layout, which lets the tool
//! work on them without a TypeScript parser: object entries always start on
//! their own line with two spaces of indentation and every top-level object
//! literal closes with a bare `};` line.

use crate::{Error, Result};

/// Header line of the inline SVG map in `index.ts`.
pub const ICONS_HEADER: &str = "export const icons: Record<string, string> = {";
/// Header line of the URL icon map in `index.ts`.
pub const ICON_URLS_HEADER: &str = "export const iconUrls: Record<string, string> = {";
/// Header line of the metadata map in `metadata.ts`.
pub const METADATA_HEADER: &str = "export const iconMetadata: Record<string, IconMetadata> = {";

/// An `import _ident from "./file";` line in `index.ts`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    /// Local identifier (including the leading underscore).
    pub ident: String,
    /// File name relative to the icons directory, without the `?url` suffix.
    pub file: String,
    /// Whether the import carried Vite's `?url` suffix.
    pub url: bool,
}

/// Everything the tool needs to know about `index.ts`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct IndexFile {
    /// Keys of the `icons` map (inline SVG), in file order.
    pub inline_keys: Vec<String>,
    /// Keys of the `iconUrls` map, in file order.
    pub url_keys: Vec<String>,
    /// Imports feeding the `iconUrls` map, in file order.
    pub imports: Vec<Import>,
}

impl IndexFile {
    /// Returns true when `key` is present in either map.
    pub fn has_key(&self, key: &str) -> bool {
        self.inline_keys.iter().any(|k| k == key) || self.url_keys.iter().any(|k| k == key)
    }
}

/// Metadata keys found in `metadata.ts`, in file order.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MetadataFile {
    pub keys: Vec<String>,
}

/// Parses the maps and imports of `index.ts`.
pub fn parse_index(src: &str) -> IndexFile {
    let mut out = IndexFile::default();
    let mut block: Option<&'static str> = None;

    for line in src.lines() {
        if let Some(import) = parse_import(line) {
            out.imports.push(import);
            continue;
        }
        if line == ICONS_HEADER {
            block = Some("icons");
            continue;
        }
        if line == ICON_URLS_HEADER {
            block = Some("urls");
            continue;
        }
        if line.trim() == "};" {
            block = None;
            continue;
        }
        match block {
            Some("icons") => {
                if let Some(key) = entry_key(line, '`') {
                    out.inline_keys.push(key);
                }
            }
            Some("urls") => {
                if let Some(key) = entry_key(line, '_') {
                    out.url_keys.push(key);
                }
            }
            _ => {}
        }
    }
    out
}

/// Parses the keys of the metadata map in `metadata.ts`.
pub fn parse_metadata(src: &str) -> MetadataFile {
    let mut out = MetadataFile::default();
    let mut inside = false;
    let mut depth = 0usize;

    for line in src.lines() {
        if line == METADATA_HEADER {
            inside = true;
            depth = 0;
            continue;
        }
        if !inside {
            continue;
        }
        if line.trim() == "};" && depth == 0 {
            inside = false;
            continue;
        }
        if depth == 0 {
            if let Some(key) = entry_key(line, '{') {
                out.keys.push(key);
            }
        }
        // Track nesting so keys inside an entry (e.g. `name:`) are skipped.
        for ch in line.chars() {
            match ch {
                '{' => depth += 1,
                '}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    out
}

/// Extracts the key of an object entry line such as `` ` `` `  openai: `<svg`
/// when the value starts with `value_start`. Quoted keys are unquoted.
fn entry_key(line: &str, value_start: char) -> Option<String> {
    let rest = line.strip_prefix("  ")?;
    if rest.starts_with(' ') {
        return None;
    }
    let (key, value) = rest.split_once(':')?;
    if !value.trim_start().starts_with(value_start) {
        return None;
    }
    let key = key.trim().trim_matches(|c| c == '\'' || c == '"');
    if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    Some(key.to_string())
}

/// Parses an `import _x from "./file.png";` line.
fn parse_import(line: &str) -> Option<Import> {
    let rest = line.strip_prefix("import ")?;
    let (ident, rest) = rest.split_once(" from ")?;
    let ident = ident.trim();
    if ident.is_empty() || ident.contains(' ') || ident.contains('{') {
        return None;
    }
    let rest = rest.trim().trim_end_matches(';').trim();
    let spec = rest
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| rest.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))?;
    let spec = spec.strip_prefix("./")?;
    let (file, url) = match spec.strip_suffix("?url") {
        Some(file) => (file, true),
        None => (spec, false),
    };
    Some(Import {
        ident: ident.to_string(),
        file: file.to_string(),
        url,
    })
}

/// Escapes a string for use inside a JavaScript template literal.
pub fn escape_template(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('`', "\\`")
        .replace('$', "\\$")
}

/// Escapes a string for use inside a double-quoted JavaScript string.
pub fn escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

/// Renders an inline SVG entry line for the `icons` map.
pub fn inline_entry(key: &str, svg: &str) -> String {
    format!("  {key}: `{}`,", escape_template(svg))
}

/// Inserts `lines` just before the `};` that closes the block introduced by
/// `header`. Returns the updated source.
pub fn insert_before_block_end(src: &str, header: &str, lines: &[String]) -> Result<String> {
    if lines.is_empty() {
        return Ok(src.to_string());
    }
    let trailing_newline = src.ends_with('\n');
    let mut out: Vec<String> = Vec::new();
    let mut state = 0u8; // 0 = before header, 1 = inside block, 2 = done
    for line in src.lines() {
        if state == 0 && line == header {
            state = 1;
        } else if state == 1 && line.trim() == "};" {
            out.extend(lines.iter().cloned());
            state = 2;
        }
        out.push(line.to_string());
    }
    if state != 2 {
        return Err(Error(format!(
            "could not find the block `{header}` followed by a closing `}};` line"
        )));
    }
    let mut joined = out.join("\n");
    if trailing_newline {
        joined.push('\n');
    }
    Ok(joined)
}

/// One entry of the `iconMetadata` map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataEntry {
    pub name: String,
    pub display_name: String,
    pub category: String,
    pub keywords: Vec<String>,
    pub default_color: Option<String>,
}

impl MetadataEntry {
    /// Renders the entry the way Prettier formats it (80 column print width).
    pub fn render(&self) -> String {
        let mut lines = vec![
            format!("  {}: {{", self.name),
            format!("    name: \"{}\",", escape_string(&self.name)),
            format!(
                "    displayName: \"{}\",",
                escape_string(&self.display_name)
            ),
            format!("    category: \"{}\",", escape_string(&self.category)),
        ];
        let quoted: Vec<String> = self
            .keywords
            .iter()
            .map(|k| format!("\"{}\"", escape_string(k)))
            .collect();
        let one_line = format!("    keywords: [{}],", quoted.join(", "));
        if one_line.chars().count() <= 80 {
            lines.push(one_line);
        } else {
            lines.push("    keywords: [".to_string());
            lines.extend(quoted.iter().map(|q| format!("      {q},")));
            lines.push("    ],".to_string());
        }
        if let Some(color) = &self.default_color {
            lines.push(format!("    defaultColor: \"{}\",", escape_string(color)));
        }
        lines.push("  },".to_string());
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX: &str = r#"// Auto-generated icon index
import _foo from "./foo.png";
import _bar from "./bar.svg?url";

export const icons: Record<string, string> = {
  openai: `<svg>a</svg>`,
  'claude': `<svg>b</svg>`,
};

export const iconUrls: Record<string, string> = {
  foo: _foo,
  bar: _bar,
};

export const iconList = [
  ...Object.keys(icons),
  ...Object.keys(iconUrls),
].sort();
"#;

    #[test]
    fn parses_index_keys_and_imports() {
        let parsed = parse_index(INDEX);
        assert_eq!(parsed.inline_keys, vec!["openai", "claude"]);
        assert_eq!(parsed.url_keys, vec!["foo", "bar"]);
        assert_eq!(
            parsed.imports,
            vec![
                Import {
                    ident: "_foo".into(),
                    file: "foo.png".into(),
                    url: false
                },
                Import {
                    ident: "_bar".into(),
                    file: "bar.svg".into(),
                    url: true
                },
            ]
        );
        assert!(parsed.has_key("openai"));
        assert!(parsed.has_key("bar"));
        assert!(!parsed.has_key("nope"));
    }

    #[test]
    fn ignores_named_imports() {
        assert_eq!(parse_import("import { x } from \"./y\";"), None);
        assert_eq!(parse_import("import _x from \"react\";"), None);
    }

    #[test]
    fn inserts_inline_entry_before_block_end() {
        let updated = insert_before_block_end(
            INDEX,
            ICONS_HEADER,
            &[inline_entry("gemini", "<svg>$`</svg>")],
        )
        .unwrap();
        let parsed = parse_index(&updated);
        assert_eq!(parsed.inline_keys, vec!["openai", "claude", "gemini"]);
        assert!(updated.contains("  gemini: `<svg>\\$\\`</svg>`,\n};"));
        assert!(updated.ends_with('\n'));
    }

    #[test]
    fn insert_fails_without_block() {
        let err = insert_before_block_end("nothing", ICONS_HEADER, &["x".into()]).unwrap_err();
        assert!(err.0.contains("could not find"));
    }

    #[test]
    fn insert_with_no_lines_is_identity() {
        assert_eq!(
            insert_before_block_end(INDEX, ICONS_HEADER, &[]).unwrap(),
            INDEX
        );
    }

    const METADATA: &str = r##"import { IconMetadata } from "@/types/icon";

export const iconMetadata: Record<string, IconMetadata> = {
  openai: {
    name: "openai",
    displayName: "OpenAI",
    category: "ai-provider",
    keywords: ["gpt", "chatgpt"],
    defaultColor: "#00A67E",
  },
  claude: {
    name: "claude",
    displayName: "Claude",
    category: "ai-provider",
    keywords: [
      "anthropic",
      "claude",
    ],
  },
};

export function getIconMetadata(name: string): IconMetadata | undefined {
  return iconMetadata[name.toLowerCase()];
}
"##;

    #[test]
    fn parses_metadata_keys_only_at_top_level() {
        let parsed = parse_metadata(METADATA);
        assert_eq!(parsed.keys, vec!["openai", "claude"]);
    }

    #[test]
    fn renders_metadata_entry_like_prettier() {
        let entry = MetadataEntry {
            name: "gemini".into(),
            display_name: "Gemini".into(),
            category: "ai-provider".into(),
            keywords: vec!["google".into()],
            default_color: Some("#4285F4".into()),
        };
        assert_eq!(
            entry.render(),
            "  gemini: {\n    name: \"gemini\",\n    displayName: \"Gemini\",\n    category: \"ai-provider\",\n    keywords: [\"google\"],\n    defaultColor: \"#4285F4\",\n  },"
        );

        let long = MetadataEntry {
            name: "x".into(),
            display_name: "X".into(),
            category: "ai-provider".into(),
            keywords: (0..12).map(|i| format!("keyword{i}")).collect(),
            default_color: None,
        };
        let rendered = long.render();
        assert!(rendered.contains("    keywords: [\n      \"keyword0\",\n"));
        assert!(rendered.ends_with("    ],\n  },"));
        assert!(!rendered.contains("defaultColor"));

        let updated =
            insert_before_block_end(METADATA, METADATA_HEADER, &[entry.render()]).unwrap();
        assert_eq!(
            parse_metadata(&updated).keys,
            vec!["openai", "claude", "gemini"]
        );
    }

    #[test]
    fn escapes_strings() {
        assert_eq!(escape_string("a\"b\\c"), "a\\\"b\\\\c");
        assert_eq!(escape_template("a`b$c\\d"), "a\\`b\\$c\\\\d");
    }
}
