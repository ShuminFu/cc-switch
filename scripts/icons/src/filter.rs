//! Prunes the icon directory down to a keep list, preferring colour variants.
//!
//! Port of the former `scripts/filter-icons.js`. The keep list below is the
//! original "famous icons" list; on top of it every icon that `index.ts`
//! already references is kept, so running the command on a curated tree never
//! deletes something that is in use.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;

use crate::check::key_for_file;
use crate::{io_context, list_files, read_text, ts, Config, Result};

/// Icons that are always kept, regardless of what `index.ts` references.
pub const KEEP_LIST: &[&str] = &[
    // AI providers
    "openai",
    "anthropic",
    "claude",
    "google",
    "gemini",
    "gemma",
    "palm",
    "microsoft",
    "azure",
    "copilot",
    "meta",
    "llama",
    "alibaba",
    "qwen",
    "tencent",
    "hunyuan",
    "baidu",
    "wenxin",
    "bytedance",
    "doubao",
    "deepseek",
    "moonshot",
    "kimi",
    "stepfun",
    "zhipu",
    "chatglm",
    "glm",
    "minimax",
    "mistral",
    "cohere",
    "perplexity",
    "huggingface",
    "midjourney",
    "stability",
    "xai",
    "grok",
    "yi",
    "zeroone",
    "ollama",
    "packycode",
    // Cloud / tools
    "aws",
    "googlecloud",
    "huawei",
    "cloudflare",
    "github",
    "githubcopilot",
    "vercel",
    "notion",
    "discord",
    "gitlab",
    "docker",
    "kubernetes",
    "vscode",
    "settings",
    "folder",
    "file",
    "link",
];

/// Suffix marking the colour variant of an upstream icon.
pub const COLOR_SUFFIX: &str = "-color";

/// What `filter` would do to the directory.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Files to delete (file names).
    pub delete: Vec<String>,
    /// Files to rename `(from, to)`; the target may already exist and is
    /// overwritten by the colour variant.
    pub rename: Vec<(String, String)>,
    /// Base names that survive.
    pub kept: BTreeSet<String>,
}

impl Plan {
    /// True when applying the plan would change nothing.
    pub fn is_noop(&self) -> bool {
        self.delete.is_empty() && self.rename.is_empty()
    }
}

/// Splits a file name into `(base, is_color)`; `None` for non-SVG files.
pub fn split_variant(file: &str) -> Option<(String, bool)> {
    let stem = file.strip_suffix(".svg")?;
    match stem.strip_suffix(COLOR_SUFFIX) {
        Some(base) => Some((base.to_string(), true)),
        None => Some((stem.to_string(), false)),
    }
}

/// Computes the plan for `files` given the set of base names to keep.
pub fn plan(files: &[String], keep: &HashSet<String>) -> Plan {
    #[derive(Default)]
    struct Variants {
        color: bool,
        mono: bool,
    }
    let mut map: BTreeMap<String, Variants> = BTreeMap::new();
    for file in files {
        if let Some((base, is_color)) = split_variant(file) {
            let entry = map.entry(base).or_default();
            if is_color {
                entry.color = true;
            } else {
                entry.mono = true;
            }
        }
    }

    let mut out = Plan::default();
    for (base, variants) in map {
        let color_file = format!("{base}{COLOR_SUFFIX}.svg");
        let mono_file = format!("{base}.svg");
        // A file is kept when its base name or its conventional index key
        // (see `check::key_for_file`) is in the keep set.
        if !keep.contains(&base) && !keep.contains(&key_for_file(&mono_file)) {
            if variants.color {
                out.delete.push(color_file);
            }
            if variants.mono {
                out.delete.push(mono_file);
            }
            continue;
        }
        if variants.color {
            out.rename.push((color_file, mono_file));
        }
        out.kept.insert(base);
    }
    out
}

/// Keep set for a repository: the built-in list, extra names, and every key
/// or file that `index.ts` references.
pub fn keep_set(index: &ts::IndexFile, extra: &[String]) -> HashSet<String> {
    let mut keep: HashSet<String> = KEEP_LIST.iter().map(|s| s.to_string()).collect();
    keep.extend(extra.iter().map(|s| s.trim().to_lowercase()));
    keep.extend(index.inline_keys.iter().cloned());
    keep.extend(index.url_keys.iter().cloned());
    for import in &index.imports {
        if let Some((base, _)) = split_variant(&import.file) {
            keep.insert(base);
        }
    }
    keep
}

/// Builds the plan for the configured icons directory.
pub fn build_plan(cfg: &Config, extra_keep: &[String]) -> Result<Plan> {
    let index = ts::parse_index(&read_text(&cfg.index_path())?);
    let files = list_files(&cfg.icons_dir)?;
    Ok(plan(&files, &keep_set(&index, extra_keep)))
}

/// Applies a plan to `dir`.
pub fn apply(dir: &Path, plan: &Plan) -> Result<()> {
    for file in &plan.delete {
        let path = dir.join(file);
        io_context(std::fs::remove_file(&path), "cannot delete", &path)?;
    }
    for (from, to) in &plan.rename {
        let from_path = dir.join(from);
        let to_path = dir.join(to);
        io_context(
            std::fs::rename(&from_path, &to_path),
            "cannot rename",
            &from_path,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    fn keep(names: &[&str]) -> HashSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn deletes_unkept_and_prefers_color() {
        let plan = plan(
            &files(&[
                "openai.svg",
                "openai-color.svg",
                "junk.svg",
                "junk-color.svg",
                "mono.svg",
                "photo.png",
            ]),
            &keep(&["openai", "mono"]),
        );
        assert_eq!(plan.delete, vec!["junk-color.svg", "junk.svg"]);
        assert_eq!(
            plan.rename,
            vec![("openai-color.svg".to_string(), "openai.svg".to_string())]
        );
        assert_eq!(
            plan.kept.iter().cloned().collect::<Vec<_>>(),
            vec!["mono", "openai"]
        );
        assert!(!plan.is_noop());
    }

    #[test]
    fn keeps_files_whose_conventional_key_is_kept() {
        let plan = plan(
            &files(&["opencode-logo-light.svg", "Other-icon.svg"]),
            &keep(&["opencode"]),
        );
        assert_eq!(plan.delete, vec!["Other-icon.svg"]);
        assert!(plan.kept.contains("opencode-logo-light"));
    }

    #[test]
    fn keep_set_includes_index_references() {
        let index = ts::parse_index(
            "import _x from \"./custom-color.svg?url\";\n\
             export const icons: Record<string, string> = {\n  aigocode: `<svg/>`,\n};\n\
             export const iconUrls: Record<string, string> = {\n  x: _x,\n};\n",
        );
        let keep = keep_set(&index, &["Extra ".into()]);
        assert!(keep.contains("openai"));
        assert!(keep.contains("aigocode"));
        assert!(keep.contains("x"));
        assert!(keep.contains("custom"));
        assert!(keep.contains("extra"));
    }

    #[test]
    fn applies_plan_to_directory() {
        let dir = std::env::temp_dir().join(format!(
            "cc-switch-icons-filter-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["a.svg", "a-color.svg", "b.svg"] {
            std::fs::write(dir.join(name), name).unwrap();
        }
        let plan = plan(&files(&["a.svg", "a-color.svg", "b.svg"]), &keep(&["a"]));
        apply(&dir, &plan).unwrap();
        let mut left = list_files(&dir).unwrap();
        left.sort();
        assert_eq!(left, vec!["a.svg"]);
        assert_eq!(
            std::fs::read_to_string(dir.join("a.svg")).unwrap(),
            "a-color.svg"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
