//! Copying icons out of the `@lobehub/icons-static-svg` package.
//!
//! Mirrors the former `extract-icons.js`: every requested icon is copied from
//! the package when available; otherwise an existing local file is kept, and
//! anything else is reported as missing. Two deliberate differences: files
//! that already exist locally are only replaced with `overwrite`, because
//! several tracked SVGs have been customised, and `index.ts`/`metadata.ts`
//! are no longer regenerated since they are curated by hand today.

use std::fs;
use std::io;
use std::path::Path;

/// Icons extracted by default, grouped as in the original script.
pub const ICON_GROUPS: &[(&str, &[&str])] = &[
    (
        "AI providers",
        &[
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
    ),
    ("Cloud platforms", &["aws", "azure", "huawei", "cloudflare"]),
    (
        "Dev tools",
        &["github", "gitlab", "docker", "kubernetes", "vscode"],
    ),
    ("Others", &["settings", "folder", "file", "link"]),
];

/// Flattened [`ICON_GROUPS`].
pub fn default_icon_names() -> Vec<String> {
    ICON_GROUPS
        .iter()
        .flat_map(|(_, names)| names.iter().map(|n| n.to_string()))
        .collect()
}

/// Per-icon result of an extraction run.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Copied from the package.
    pub copied: Vec<String>,
    /// Available upstream but left untouched because a local file exists.
    pub already_present: Vec<String>,
    /// Not in the package; an existing local file was kept.
    pub kept_local: Vec<String>,
    /// Neither upstream nor local.
    pub not_found: Vec<String>,
}

impl Outcome {
    /// Number of icons that ended up available in the output directory.
    pub fn available(&self) -> usize {
        self.copied.len() + self.already_present.len() + self.kept_local.len()
    }
}

/// Copies `names` from `source` into `out`. Fails early when `source` is
/// not a directory (typically because `pnpm install` has not run).
pub fn extract(
    source: &Path,
    out: &Path,
    names: &[String],
    overwrite: bool,
) -> io::Result<Outcome> {
    if !source.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "icon source directory not found: {} (run `pnpm install` first)",
                source.display()
            ),
        ));
    }
    fs::create_dir_all(out)?;

    let mut outcome = Outcome::default();
    for name in names {
        let file = format!("{name}.svg");
        let source_file = source.join(&file);
        let target_file = out.join(&file);
        let target_exists = target_file.is_file();
        if source_file.is_file() {
            if target_exists && !overwrite {
                outcome.already_present.push(name.clone());
            } else {
                fs::copy(&source_file, &target_file)?;
                outcome.copied.push(name.clone());
            }
        } else if target_exists {
            outcome.kept_local.push(name.clone());
        } else {
            outcome.not_found.push(name.clone());
        }
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_list_is_flat_and_unique() {
        let names = default_icon_names();
        assert_eq!(names.len(), 33);
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len());
        assert_eq!(names[0], "openai");
        assert_eq!(names[names.len() - 1], "link");
    }

    #[test]
    fn available_counts_every_kept_icon() {
        let outcome = Outcome {
            copied: vec!["a".into()],
            already_present: vec!["b".into(), "c".into()],
            kept_local: vec!["d".into()],
            not_found: vec!["e".into()],
        };
        assert_eq!(outcome.available(), 4);
    }
}
