//! Preview of a live-config write (CFG-22, first increment).
//!
//! An adapter that supports previews renders the exact bytes it would write
//! with [`super::AppAdapter::plan_live`]; its `write_live` writes those same
//! bytes, so what the preview shows is what the switch applies. The command
//! layer turns the planned writes into [`LiveWritePlan`] with the current
//! content and a line diff for the UI.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::app_config::AppType;

/// One file an adapter would write, with its full new content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedWrite {
    pub path: PathBuf,
    pub content: String,
}

/// One file of a [`LiveWritePlan`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PlannedFileChange {
    pub path: String,
    /// Current content; `None` when the file does not exist yet.
    pub before: Option<String>,
    pub after: String,
    pub changed: bool,
    /// Line diff (`-`/`+`/` ` prefixed lines, unchanged runs collapsed).
    pub unified_diff: String,
}

/// What a provider switch would write to an app's live files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LiveWritePlan {
    pub app: AppType,
    pub provider_id: String,
    /// `false` when the app's adapter cannot preview yet; `files` is empty.
    pub supported: bool,
    pub files: Vec<PlannedFileChange>,
}

/// Larger inputs skip the line diff (quadratic) and show a summary instead.
const MAX_DIFF_LINES: usize = 4000;
/// Unchanged lines kept around each change.
const DIFF_CONTEXT: usize = 3;

fn read_current(path: &Path) -> Option<String> {
    std::fs::read(path)
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// Compare planned writes against the files on disk.
pub fn describe_changes(writes: &[PlannedWrite]) -> Vec<PlannedFileChange> {
    writes
        .iter()
        .map(|write| {
            let before = read_current(&write.path);
            let changed = before.as_deref() != Some(write.content.as_str());
            let unified_diff = if changed {
                line_diff(before.as_deref().unwrap_or(""), &write.content)
            } else {
                String::new()
            };
            PlannedFileChange {
                path: write.path.to_string_lossy().into_owned(),
                before,
                after: write.content.clone(),
                changed,
                unified_diff,
            }
        })
        .collect()
}

/// A minimal line diff (longest common subsequence) with collapsed context.
pub fn line_diff(before: &str, after: &str) -> String {
    let old: Vec<&str> = before.lines().collect();
    let new: Vec<&str> = after.lines().collect();
    if old.len() + new.len() > MAX_DIFF_LINES {
        return format!(
            "@@ {} lines -> {} lines (too large to diff) @@\n",
            old.len(),
            new.len()
        );
    }

    // lcs[i][j] = LCS length of old[i..] and new[j..]
    let mut lcs = vec![vec![0usize; new.len() + 1]; old.len() + 1];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            lcs[i][j] = if old[i] == new[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }

    let mut ops: Vec<(char, &str)> = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < old.len() || j < new.len() {
        if i < old.len() && j < new.len() && old[i] == new[j] {
            ops.push((' ', old[i]));
            i += 1;
            j += 1;
        } else if i < old.len() && (j == new.len() || lcs[i + 1][j] >= lcs[i][j + 1]) {
            ops.push(('-', old[i]));
            i += 1;
        } else {
            ops.push(('+', new[j]));
            j += 1;
        }
    }

    let keep: Vec<bool> = (0..ops.len())
        .map(|idx| {
            let lo = idx.saturating_sub(DIFF_CONTEXT);
            let hi = (idx + DIFF_CONTEXT + 1).min(ops.len());
            ops[lo..hi].iter().any(|(tag, _)| *tag != ' ')
        })
        .collect();

    let mut out = String::new();
    let mut skipped = false;
    for (idx, (tag, line)) in ops.iter().enumerate() {
        if keep[idx] {
            if skipped {
                out.push_str("@@\n");
                skipped = false;
            }
            out.push(*tag);
            out.push_str(line);
            out.push('\n');
        } else {
            skipped = true;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_diff_marks_changes_and_collapses_context() {
        let before = "a\nb\nc\nd\ne\nf\ng\nh\ni\n";
        let after = "a\nb\nc\nd\nE\nf\ng\nh\ni\n";
        let diff = line_diff(before, after);
        assert_eq!(diff, "@@\n b\n c\n d\n-e\n+E\n f\n g\n h\n");
    }

    #[test]
    fn line_diff_new_file_is_all_additions() {
        assert_eq!(line_diff("", "x\ny"), "+x\n+y\n");
    }

    #[test]
    fn describe_changes_reads_current_content() {
        let dir = tempfile::tempdir().unwrap();
        let existing = dir.path().join("a.json");
        std::fs::write(&existing, "{}\n").unwrap();
        let changes = describe_changes(&[
            PlannedWrite {
                path: existing.clone(),
                content: "{}\n".into(),
            },
            PlannedWrite {
                path: dir.path().join("new.json"),
                content: "{\"k\":1}\n".into(),
            },
        ]);
        assert!(!changes[0].changed);
        assert!(changes[0].unified_diff.is_empty());
        assert!(changes[1].changed);
        assert_eq!(changes[1].before, None);
    }
}
