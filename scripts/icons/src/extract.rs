//! Copies icons from `@lobehub/icons-static-svg` into the bundle and registers
//! them in `index.ts` / `metadata.ts`.
//!
//! Port of the former `scripts/extract-icons.js`. Existing entries in the two
//! TypeScript files are never touched; only missing ones are appended.

use std::collections::HashSet;

use crate::{io_context, read_text, svg, ts, write_text, Config, Result};

/// A group of icons in the default extraction list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Group {
    pub name: &'static str,
    pub icons: &'static [&'static str],
}

/// Icons extracted when no names are given on the command line.
pub const DEFAULT_GROUPS: &[Group] = &[
    Group {
        name: "AI providers",
        icons: &[
            "openai",
            "anthropic",
            "claude",
            "google",
            "gemini",
            "deepseek",
            "kimi",
            "moonshot",
            "stepfun",
            "zhipu",
            "minimax",
            "baidu",
            "alibaba",
            "tencent",
            "meta",
            "microsoft",
            "cohere",
            "perplexity",
            "mistral",
            "huggingface",
        ],
    },
    Group {
        name: "Cloud platforms",
        icons: &["aws", "azure", "huawei", "cloudflare"],
    },
    Group {
        name: "Developer tools",
        icons: &["github", "gitlab", "docker", "kubernetes", "vscode"],
    },
    Group {
        name: "Others",
        icons: &["settings", "folder", "file", "link"],
    },
];

/// Flattened default icon list.
pub fn default_names() -> Vec<String> {
    DEFAULT_GROUPS
        .iter()
        .flat_map(|g| g.icons.iter().map(|s| s.to_string()))
        .collect()
}

/// Built-in metadata for well-known icons: (name, display, category, keywords, colour).
#[rustfmt::skip]
const KNOWN_METADATA: &[(&str, &str, &str, &[&str], &str)] = &[
    ("openai", "OpenAI", "ai-provider", &["gpt", "chatgpt"], "#00A67E"),
    ("anthropic", "Anthropic", "ai-provider", &["claude"], "#D4915D"),
    ("claude", "Claude", "ai-provider", &["anthropic"], "#D4915D"),
    ("google", "Google", "ai-provider", &["gemini", "bard"], "#4285F4"),
    ("gemini", "Gemini", "ai-provider", &["google"], "#4285F4"),
    ("deepseek", "DeepSeek", "ai-provider", &["deep", "seek"], "#1E88E5"),
    ("moonshot", "Moonshot", "ai-provider", &["kimi", "moonshot"], "#6366F1"),
    ("kimi", "Kimi", "ai-provider", &["moonshot"], "#6366F1"),
    (
        "stepfun",
        "StepFun",
        "ai-provider",
        &["stepfun", "step", "jieyue", "阶跃星辰"],
        "#005AFF",
    ),
    ("zhipu", "Zhipu AI", "ai-provider", &["chatglm", "glm"], "#0F62FE"),
    ("minimax", "MiniMax", "ai-provider", &["minimax"], "#FF6B6B"),
    ("baidu", "Baidu", "ai-provider", &["ernie", "wenxin"], "#2932E1"),
    ("alibaba", "Alibaba", "ai-provider", &["qwen", "tongyi"], "#FF6A00"),
    ("tencent", "Tencent", "ai-provider", &["hunyuan"], "#00A4FF"),
    ("meta", "Meta", "ai-provider", &["facebook", "llama"], "#0081FB"),
    ("microsoft", "Microsoft", "ai-provider", &["copilot", "azure"], "#00A4EF"),
    ("cohere", "Cohere", "ai-provider", &["cohere"], "#39594D"),
    ("perplexity", "Perplexity", "ai-provider", &["perplexity"], "#20808D"),
    ("mistral", "Mistral", "ai-provider", &["mistral"], "#FF7000"),
    ("huggingface", "Hugging Face", "ai-provider", &["huggingface", "hf"], "#FFD21E"),
    ("aws", "AWS", "cloud", &["amazon", "cloud"], "#FF9900"),
    ("azure", "Azure", "cloud", &["microsoft", "cloud"], "#0078D4"),
    ("huawei", "Huawei", "cloud", &["huawei", "cloud"], "#FF0000"),
    ("cloudflare", "Cloudflare", "cloud", &["cloudflare", "cdn"], "#F38020"),
    ("github", "GitHub", "tool", &["git", "version control"], "#181717"),
    ("gitlab", "GitLab", "tool", &["git", "version control"], "#FC6D26"),
    ("docker", "Docker", "tool", &["container"], "#2496ED"),
    ("kubernetes", "Kubernetes", "tool", &["k8s", "container"], "#326CE5"),
    ("vscode", "VS Code", "tool", &["editor", "ide"], "#007ACC"),
    ("settings", "Settings", "other", &["config", "preferences"], "#6B7280"),
    ("folder", "Folder", "other", &["directory"], "#6B7280"),
    ("file", "File", "other", &["document"], "#6B7280"),
    ("link", "Link", "other", &["url", "hyperlink"], "#6B7280"),
];

/// Metadata for `name`: the built-in entry when known, otherwise a stub.
pub fn metadata_for(name: &str) -> ts::MetadataEntry {
    if let Some((_, display, category, keywords, color)) =
        KNOWN_METADATA.iter().find(|(n, ..)| *n == name)
    {
        return ts::MetadataEntry {
            name: name.to_string(),
            display_name: display.to_string(),
            category: category.to_string(),
            keywords: keywords.iter().map(|k| k.to_string()).collect(),
            default_color: Some(color.to_string()),
        };
    }
    ts::MetadataEntry {
        name: name.to_string(),
        display_name: display_name_for(name),
        category: "ai-provider".to_string(),
        keywords: vec![name.to_string()],
        default_color: None,
    }
}

/// Capitalises a bare icon name for use as a display name.
pub fn display_name_for(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Validates an icon name from the command line.
pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        return Err(format!(
            "invalid icon name `{name}`: use lower-case letters, digits, `-` or `_`"
        )
        .into());
    }
    Ok(())
}

/// What happened to each requested icon.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Copied from the upstream package.
    pub extracted: Vec<String>,
    /// Missing upstream but already present locally.
    pub kept_local: Vec<String>,
    /// Missing both upstream and locally.
    pub not_found: Vec<String>,
    /// Newly appended to `index.ts` (inline).
    pub indexed_inline: Vec<String>,
    /// Newly appended to `index.ts` (URL import).
    pub indexed_url: Vec<String>,
    /// Newly appended to `metadata.ts`.
    pub metadata_added: Vec<String>,
}

impl Outcome {
    /// Names that are available after extraction.
    pub fn available(&self) -> Vec<String> {
        let mut all = self.extracted.clone();
        all.extend(self.kept_local.iter().cloned());
        all
    }
}

/// Extracts `names` and registers whatever is missing from the index.
pub fn run(cfg: &Config, names: &[String], write_readme: bool) -> Result<Outcome> {
    for name in names {
        validate_name(name)?;
    }
    io_context(
        std::fs::create_dir_all(&cfg.icons_dir),
        "cannot create",
        &cfg.icons_dir,
    )?;

    let mut outcome = Outcome::default();
    let mut seen = HashSet::new();
    for name in names {
        if !seen.insert(name.clone()) {
            continue;
        }
        let source = cfg.source_dir.join(format!("{name}.svg"));
        let target = cfg.icons_dir.join(format!("{name}.svg"));
        if source.is_file() {
            io_context(std::fs::copy(&source, &target), "cannot copy", &source)?;
            outcome.extracted.push(name.clone());
        } else if target.is_file() {
            outcome.kept_local.push(name.clone());
        } else {
            outcome.not_found.push(name.clone());
        }
    }

    register(cfg, &outcome.available(), &mut outcome)?;

    if write_readme {
        write_text(&cfg.icons_dir.join("README.md"), &readme(&outcome))?;
    }
    Ok(outcome)
}

/// Appends index and metadata entries for `names` that are not registered yet.
fn register(cfg: &Config, names: &[String], outcome: &mut Outcome) -> Result<()> {
    let index_path = cfg.index_path();
    let metadata_path = cfg.metadata_path();
    let index_src = read_text(&index_path)?;
    let metadata_src = read_text(&metadata_path)?;
    let index = ts::parse_index(&index_src);
    let metadata = ts::parse_metadata(&metadata_src);
    let existing_meta: HashSet<&str> = metadata.keys.iter().map(String::as_str).collect();

    let mut inline_lines = Vec::new();
    let mut import_lines = Vec::new();
    let mut url_lines = Vec::new();
    let mut metadata_lines = Vec::new();

    for name in names {
        let key = name.replace('-', "_");
        if !index.has_key(&key) {
            let file = format!("{name}.svg");
            let content = read_text(&cfg.icons_dir.join(&file))?;
            if svg::prefers_url(&content) {
                import_lines.push(format!("import _{key} from \"./{file}?url\";"));
                url_lines.push(format!("  {key}: _{key},"));
                outcome.indexed_url.push(key.clone());
            } else {
                let entry = metadata_for(name);
                let normalized = svg::normalize(&content, &entry.display_name);
                inline_lines.push(ts::inline_entry(&key, &normalized));
                outcome.indexed_inline.push(key.clone());
            }
        }
        if !existing_meta.contains(key.as_str()) {
            let mut entry = metadata_for(name);
            entry.name = key.clone();
            metadata_lines.push(entry.render());
            outcome.metadata_added.push(key);
        }
    }

    if !inline_lines.is_empty() || !url_lines.is_empty() {
        let mut updated = ts::insert_before_block_end(&index_src, ts::ICONS_HEADER, &inline_lines)?;
        if !url_lines.is_empty() {
            updated = ts::insert_before_block_end(&updated, ts::ICON_URLS_HEADER, &url_lines)?;
            updated = insert_imports(&updated, &import_lines)?;
        }
        write_text(&index_path, &updated)?;
    }
    if !metadata_lines.is_empty() {
        let updated =
            ts::insert_before_block_end(&metadata_src, ts::METADATA_HEADER, &metadata_lines)?;
        write_text(&metadata_path, &updated)?;
    }
    Ok(())
}

/// Adds import lines after the last existing import, or before the `icons`
/// block when the file has none.
fn insert_imports(src: &str, imports: &[String]) -> Result<String> {
    let trailing_newline = src.ends_with('\n');
    let lines: Vec<&str> = src.lines().collect();
    let last_import = lines.iter().rposition(|l| l.starts_with("import "));
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + imports.len() + 1);
    match last_import {
        Some(pos) => {
            for (i, line) in lines.iter().enumerate() {
                out.push(line.to_string());
                if i == pos {
                    out.extend(imports.iter().cloned());
                }
            }
        }
        None => {
            let pos = lines
                .iter()
                .position(|l| *l == ts::ICONS_HEADER)
                .ok_or_else(|| format!("index.ts has no `{}` block", ts::ICONS_HEADER))?;
            for (i, line) in lines.iter().enumerate() {
                if i == pos {
                    out.extend(imports.iter().cloned());
                    out.push(String::new());
                }
                out.push(line.to_string());
            }
        }
    }
    let mut joined = out.join("\n");
    if trailing_newline {
        joined.push('\n');
    }
    Ok(joined)
}

/// Renders the summary README written next to the icons.
pub fn readme(outcome: &Outcome) -> String {
    let available = outcome.available();
    let mut out = String::new();
    out.push_str("# Extracted Icons\n\n");
    out.push_str("This directory contains provider icons bundled with CC Switch. Most come from\n");
    out.push_str(
        "`@lobehub/icons-static-svg`; the rest are custom icons contributed for providers\n",
    );
    out.push_str("that have no upstream icon.\n\n");
    out.push_str("## Last extraction\n\n");
    out.push_str(&format!(
        "- Extracted from upstream: {}\n",
        outcome.extracted.len()
    ));
    out.push_str(&format!("- Kept local: {}\n", outcome.kept_local.len()));
    out.push_str(&format!("- Not found: {}\n\n", outcome.not_found.len()));
    if !available.is_empty() {
        out.push_str("## Available\n\n");
        for name in &available {
            out.push_str(&format!("- {name}\n"));
        }
        out.push('\n');
    }
    if !outcome.not_found.is_empty() {
        out.push_str("## Not found\n\n");
        for name in &outcome.not_found {
            out.push_str(&format!("- {name}\n"));
        }
        out.push('\n');
    }
    out.push_str("## Usage\n\n");
    out.push_str("```typescript\n");
    out.push_str("import { getIcon, hasIcon, iconList } from \"@/icons/extracted\";\n\n");
    out.push_str("const svg = getIcon(\"openai\");\n");
    out.push_str("if (hasIcon(\"openai\")) {\n  // ...\n}\n");
    out.push_str("console.log(iconList);\n");
    out.push_str("```\n\n");
    out.push_str("---\n");
    out.push_str("Generated by `cargo run --manifest-path scripts/icons/Cargo.toml -- extract`\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_metadata_is_used() {
        let entry = metadata_for("openai");
        assert_eq!(entry.display_name, "OpenAI");
        assert_eq!(entry.default_color.as_deref(), Some("#00A67E"));
    }

    #[test]
    fn unknown_metadata_is_stubbed() {
        let entry = metadata_for("newprovider");
        assert_eq!(entry.display_name, "Newprovider");
        assert_eq!(entry.keywords, vec!["newprovider"]);
        assert_eq!(entry.default_color, None);
    }

    #[test]
    fn validates_names() {
        assert!(validate_name("open-ai_2").is_ok());
        assert!(validate_name("OpenAI").is_err());
        assert!(validate_name("../etc").is_err());
        assert!(validate_name("").is_err());
    }

    #[test]
    fn default_list_matches_groups() {
        assert_eq!(default_names().len(), 33);
        assert_eq!(default_names()[0], "openai");
    }

    #[test]
    fn inserts_imports_after_existing_ones() {
        let src = "// header\nimport _a from \"./a.png\";\n\nexport const icons: Record<string, string> = {\n};\n";
        let out = insert_imports(src, &["import _b from \"./b.svg?url\";".into()]).unwrap();
        assert_eq!(
            out,
            "// header\nimport _a from \"./a.png\";\nimport _b from \"./b.svg?url\";\n\nexport const icons: Record<string, string> = {\n};\n"
        );
    }

    #[test]
    fn inserts_imports_before_icons_block_when_none_exist() {
        let src = "// header\n\nexport const icons: Record<string, string> = {\n};\n";
        let out = insert_imports(src, &["import _b from \"./b.png\";".into()]).unwrap();
        assert_eq!(
            out,
            "// header\n\nimport _b from \"./b.png\";\n\nexport const icons: Record<string, string> = {\n};\n"
        );
    }

    #[test]
    fn readme_lists_outcome() {
        let outcome = Outcome {
            extracted: vec!["openai".into()],
            not_found: vec!["nope".into()],
            ..Outcome::default()
        };
        let text = readme(&outcome);
        assert!(text.contains("- Extracted from upstream: 1"));
        assert!(text.contains("## Not found\n\n- nope"));
    }
}
