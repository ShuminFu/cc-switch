//! Keep-list based clean-up of the SVG files in the icon directory.
//!
//! This mirrors the former `filter-icons.js`: every `<name>.svg` /
//! `<name>-color.svg` pair is grouped by `<name>`; groups that are not on the
//! keep list are deleted, and for kept groups the colour variant is renamed
//! over the monochrome one. Unlike the script, files that are referenced by
//! `index.ts` or imported directly from the frontend are never deleted or
//! renamed, and nothing is changed unless the plan is explicitly applied.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::Path;

use crate::index;
use crate::util;

/// Icons that are always kept. Carried over from `filter-icons.js`.
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

/// What [`plan`] decided for a directory.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FilterPlan {
    /// Files kept because their group is on the keep list.
    pub keep: Vec<String>,
    /// Files kept only because something references them.
    pub protected: Vec<String>,
    /// Files to delete.
    pub delete: Vec<String>,
    /// `(from, to)` renames of colour variants over the base name.
    pub rename: Vec<(String, String)>,
}

impl FilterPlan {
    /// `true` when applying the plan would not touch the file system.
    pub fn is_noop(&self) -> bool {
        self.delete.is_empty() && self.rename.is_empty()
    }
}

/// Splits `file` (an `.svg`) into its group name and whether it is the
/// colour variant.
fn group_of(file: &str) -> Option<(String, bool)> {
    let stem = file.strip_suffix(".svg")?;
    match stem.strip_suffix("-color") {
        Some(base) if !base.is_empty() => Some((base.to_string(), true)),
        _ => Some((stem.to_string(), false)),
    }
}

/// Builds the plan for `svg_files`. `keep_names` are group names to keep;
/// `protected` are exact file names that must not be deleted or renamed.
pub fn plan(
    svg_files: &[String],
    keep_names: &BTreeSet<String>,
    protected: &BTreeSet<String>,
) -> FilterPlan {
    let mut groups: BTreeMap<String, (Option<String>, Option<String>)> = BTreeMap::new();
    for file in svg_files {
        let Some((base, is_color)) = group_of(file) else {
            continue;
        };
        let entry = groups.entry(base).or_default();
        if is_color {
            entry.0 = Some(file.clone());
        } else {
            entry.1 = Some(file.clone());
        }
    }

    let mut result = FilterPlan::default();
    for (base, (color, mono)) in groups {
        let is_protected = |f: &String| protected.contains(f);
        if keep_names.contains(&base) {
            match (color, mono) {
                (Some(c), Some(m)) => {
                    if is_protected(&c) || is_protected(&m) {
                        for f in [c, m] {
                            if is_protected(&f) {
                                result.protected.push(f);
                            } else {
                                result.keep.push(f);
                            }
                        }
                    } else {
                        result.rename.push((c, m.clone()));
                        result.keep.push(m);
                    }
                }
                (Some(c), None) => {
                    if is_protected(&c) {
                        result.protected.push(c);
                    } else {
                        let target = format!("{base}.svg");
                        result.rename.push((c, target.clone()));
                        result.keep.push(target);
                    }
                }
                (None, Some(m)) => {
                    if is_protected(&m) {
                        result.protected.push(m);
                    } else {
                        result.keep.push(m);
                    }
                }
                (None, None) => {}
            }
        } else {
            for f in [color, mono].into_iter().flatten() {
                if is_protected(&f) {
                    result.protected.push(f);
                } else {
                    result.delete.push(f);
                }
            }
        }
    }
    result
}

/// Collects the file names in `dir` that must never be removed: assets
/// imported by `index.ts`, files whose derived key has an entry in
/// `index.ts`, and files imported directly from the frontend sources.
pub fn protected_files(root: &Path, dir: &Path) -> Result<BTreeSet<String>, String> {
    let mut protected = BTreeSet::new();
    let index_path = dir.join("index.ts");
    if index_path.is_file() {
        let src = fs::read_to_string(&index_path)
            .map_err(|e| format!("{}: {e}", index_path.display()))?;
        let parsed =
            index::parse_index(&src).map_err(|e| format!("{}: {e}", index_path.display()))?;
        for file in parsed.referenced_files() {
            if dir.join(&file).is_file() {
                protected.insert(file);
            }
        }
        // A file whose derived key is in the index (e.g. `longcat-color.svg`
        // for the inline `longcat` entry) backs that entry and stays too.
        let keys: BTreeSet<&str> = parsed.keys().into_iter().collect();
        for file in util::list_files(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            if !util::is_generated_or_hidden(&file)
                && keys.contains(index::derive_key(&file).as_str())
            {
                protected.insert(file);
            }
        }
    }
    let external = util::scan_external_references(&root.join("src"), dir)
        .map_err(|e| format!("scanning {}: {e}", root.join("src").display()))?;
    for file in external {
        if dir.join(&file).is_file() {
            protected.insert(file);
        }
    }
    Ok(protected)
}

/// Executes `plan` inside `dir`: deletions first, then renames.
pub fn apply(dir: &Path, plan: &FilterPlan) -> io::Result<()> {
    for file in &plan.delete {
        fs::remove_file(dir.join(file))?;
    }
    for (from, to) in &plan.rename {
        fs::rename(dir.join(from), dir.join(to))?;
    }
    Ok(())
}

/// Default keep set: [`KEEP_LIST`] plus `extra` names.
pub fn keep_set(extra: &[String]) -> BTreeSet<String> {
    KEEP_LIST
        .iter()
        .map(|s| s.to_string())
        .chain(extra.iter().cloned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn files(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn deletes_groups_not_on_keep_list() {
        let plan = plan(
            &files(&["foo.svg", "foo-color.svg", "bar.svg"]),
            &set(&["bar"]),
            &set(&[]),
        );
        assert_eq!(plan.delete, vec!["foo-color.svg", "foo.svg"]);
        assert_eq!(plan.keep, vec!["bar.svg"]);
        assert!(plan.rename.is_empty());
    }

    #[test]
    fn prefers_colour_variant_for_kept_groups() {
        let plan = plan(
            &files(&["kimi.svg", "kimi-color.svg", "yi-color.svg"]),
            &set(&["kimi", "yi"]),
            &set(&[]),
        );
        assert_eq!(
            plan.rename,
            vec![
                ("kimi-color.svg".to_string(), "kimi.svg".to_string()),
                ("yi-color.svg".to_string(), "yi.svg".to_string()),
            ]
        );
        assert_eq!(plan.keep, vec!["kimi.svg", "yi.svg"]);
        assert!(plan.delete.is_empty());
    }

    #[test]
    fn protected_files_are_never_deleted_or_renamed() {
        let plan = plan(
            &files(&["claw.svg", "claude.svg", "claude-color.svg", "junk.svg"]),
            &set(&["claude"]),
            &set(&["claw.svg", "claude.svg"]),
        );
        assert_eq!(plan.delete, vec!["junk.svg"]);
        assert!(plan.rename.is_empty());
        assert_eq!(plan.protected, vec!["claude.svg", "claw.svg"]);
        assert_eq!(plan.keep, vec!["claude-color.svg"]);
        assert!(!plan.is_noop());
    }

    #[test]
    fn ignores_non_svg_files_and_bare_color_suffix() {
        let plan = plan(&files(&["logo.png", "-color.svg"]), &set(&[]), &set(&[]));
        assert_eq!(plan.delete, vec!["-color.svg"]);
    }

    #[test]
    fn keep_set_merges_extras() {
        let keep = keep_set(&["custom".to_string()]);
        assert!(keep.contains("openai"));
        assert!(keep.contains("custom"));
    }
}
