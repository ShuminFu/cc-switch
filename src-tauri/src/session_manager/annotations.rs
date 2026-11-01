//! User annotations on sessions: pin, archive and tags (SESS-18, first increment).
//!
//! The session index (`index.rs`) is derived data that may be deleted and
//! rebuilt at any time, so annotations live in their own small JSON file,
//! `~/.cc-switch/session-annotations.json`, keyed by the stable session key
//! (`provider:session:source`). Writes are atomic; a missing or unreadable file
//! reads as "no annotations" and is never overwritten when it fails to parse.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

const FILE_NAME: &str = "session-annotations.json";
/// Upper bounds that keep the file small and the UI readable.
pub const MAX_TAGS: usize = 20;
pub const MAX_TAG_CHARS: usize = 40;

/// Serialises read-modify-write of the annotations file.
static WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionAnnotation {
    pub pinned: bool,
    pub archived: bool,
    pub tags: Vec<String>,
}

impl SessionAnnotation {
    fn is_empty(&self) -> bool {
        !self.pinned && !self.archived && self.tags.is_empty()
    }

    /// Trim, drop empty and duplicate (case-insensitive) tags, cap count/length.
    fn normalized(mut self) -> Self {
        let mut seen = std::collections::HashSet::new();
        self.tags = self
            .tags
            .into_iter()
            .map(|tag| tag.trim().chars().take(MAX_TAG_CHARS).collect::<String>())
            .filter(|tag| !tag.is_empty() && seen.insert(tag.to_lowercase()))
            .take(MAX_TAGS)
            .collect();
        self
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct AnnotationsFile {
    #[serde(default = "version_one")]
    version: u32,
    #[serde(default)]
    sessions: BTreeMap<String, SessionAnnotation>,
}

fn version_one() -> u32 {
    1
}

fn default_path() -> PathBuf {
    crate::config::get_app_config_dir().join(FILE_NAME)
}

fn read_file(path: &Path) -> Result<AnnotationsFile, String> {
    match std::fs::read(path) {
        Ok(data) => serde_json::from_slice(&data)
            .map_err(|e| format!("Failed to parse {}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(AnnotationsFile::default()),
        Err(e) => Err(format!("Failed to read {}: {e}", path.display())),
    }
}

/// Every annotation, keyed by session key. A broken file reads as empty.
pub fn load_from(path: &Path) -> BTreeMap<String, SessionAnnotation> {
    match read_file(path) {
        Ok(file) => file.sessions,
        Err(e) => {
            log::warn!("[SessionAnnotations] {e}");
            BTreeMap::new()
        }
    }
}

/// Replace one session's annotation (an empty one removes the entry) and
/// return the stored, normalized value.
pub fn set_in(
    path: &Path,
    session_key: &str,
    annotation: SessionAnnotation,
) -> Result<SessionAnnotation, String> {
    if session_key.trim().is_empty() {
        return Err("Session key is required".to_string());
    }
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Refuse to rewrite a file that failed to parse (it may be user-edited).
    let mut file = read_file(path)?;
    let annotation = annotation.normalized();
    if annotation.is_empty() {
        file.sessions.remove(session_key);
    } else {
        file.sessions
            .insert(session_key.to_string(), annotation.clone());
    }
    file.version = 1;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let data = serde_json::to_vec_pretty(&file).map_err(|e| e.to_string())?;
    crate::config::atomic_write(path, &data).map_err(|e| e.to_string())?;
    Ok(annotation)
}

/// Annotations from `~/.cc-switch/session-annotations.json`.
pub fn load() -> BTreeMap<String, SessionAnnotation> {
    load_from(&default_path())
}

/// Store one annotation in `~/.cc-switch/session-annotations.json`.
pub fn set(session_key: &str, annotation: SessionAnnotation) -> Result<SessionAnnotation, String> {
    set_in(&default_path(), session_key, annotation)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn annotations_round_trip_and_survive_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        assert!(load_from(&path).is_empty());

        let stored = set_in(
            &path,
            "claude:abc:/p/abc.jsonl",
            SessionAnnotation {
                pinned: true,
                archived: false,
                tags: tags(&[" bug ", "Bug", "", "release"]),
            },
        )
        .unwrap();
        assert_eq!(stored.tags, tags(&["bug", "release"]));
        set_in(
            &path,
            "codex:def:/p/def.jsonl",
            SessionAnnotation {
                archived: true,
                ..Default::default()
            },
        )
        .unwrap();

        // "Restart": a fresh read sees both.
        let all = load_from(&path);
        assert_eq!(all.len(), 2);
        assert!(all["claude:abc:/p/abc.jsonl"].pinned);
        assert!(all["codex:def:/p/def.jsonl"].archived);

        // Clearing every flag removes the entry.
        set_in(
            &path,
            "codex:def:/p/def.jsonl",
            SessionAnnotation::default(),
        )
        .unwrap();
        assert_eq!(load_from(&path).len(), 1);
    }

    #[test]
    fn tags_are_capped() {
        let many: Vec<String> = (0..50).map(|i| format!("tag-{i}")).collect();
        let long = "x".repeat(200);
        let normalized = SessionAnnotation {
            tags: [vec![long], many].concat(),
            ..Default::default()
        }
        .normalized();
        assert_eq!(normalized.tags.len(), MAX_TAGS);
        assert_eq!(normalized.tags[0].chars().count(), MAX_TAG_CHARS);
    }

    #[test]
    fn unparsable_file_is_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        std::fs::write(&path, "{ not json").unwrap();
        assert!(load_from(&path).is_empty());
        let err = set_in(
            &path,
            "k",
            SessionAnnotation {
                pinned: true,
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(err.contains("parse"), "{err}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
    }

    #[test]
    fn empty_key_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        assert!(set_in(
            &dir.path().join(FILE_NAME),
            " ",
            SessionAnnotation::default()
        )
        .is_err());
    }
}
