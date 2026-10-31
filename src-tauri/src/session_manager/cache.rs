//! Per-file parse cache keyed by `(provider, path)` and `(mtime, size)`
//! (SESS-09, SESS-21).
//!
//! Providers wrap their per-file metadata parser in [`ScanCache::get_or_parse`].
//! An unchanged file (same modification time and size as the cached entry) is
//! never opened again; only `stat` is performed. The cache is owned by the
//! session index, which persists it to SQLite so a cold start also skips
//! parsing.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::SessionMeta;

/// Cheap change detector for a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fingerprint {
    pub mtime_ms: i64,
    pub size: u64,
}

impl Fingerprint {
    pub fn of(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        let mtime_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|d| i64::try_from(d.as_millis()).ok())
            .unwrap_or(0);
        Some(Self {
            mtime_ms,
            size: meta.len(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    pub provider_id: String,
    pub path: PathBuf,
}

/// Cached outcome of parsing one file. `meta` is `None` when the file is not
/// a session (e.g. a sub-agent transcript), so it is not re-parsed either.
#[derive(Debug, Clone)]
pub struct CachedParse {
    pub fingerprint: Fingerprint,
    pub meta: Option<SessionMeta>,
}

#[derive(Default)]
struct State {
    entries: HashMap<CacheKey, CachedParse>,
    seen: HashSet<CacheKey>,
    dirty: HashSet<CacheKey>,
}

/// Shared across the provider threads of one scan.
pub struct ScanCache {
    enabled: bool,
    state: Mutex<State>,
    parses: AtomicUsize,
}

impl ScanCache {
    /// A cache that never stores anything: every call parses.
    #[cfg(test)]
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            state: Mutex::new(State::default()),
            parses: AtomicUsize::new(0),
        }
    }

    pub fn with_entries(entries: HashMap<CacheKey, CachedParse>) -> Self {
        Self {
            enabled: true,
            state: Mutex::new(State {
                entries,
                ..State::default()
            }),
            parses: AtomicUsize::new(0),
        }
    }

    /// Return the cached metadata when the file's fingerprint is unchanged,
    /// otherwise call `parse` and remember the result.
    pub fn get_or_parse<F>(&self, provider_id: &str, path: &Path, parse: F) -> Option<SessionMeta>
    where
        F: FnOnce() -> Option<SessionMeta>,
    {
        if !self.enabled {
            self.parses.fetch_add(1, Ordering::Relaxed);
            return parse();
        }
        let Some(fingerprint) = Fingerprint::of(path) else {
            self.parses.fetch_add(1, Ordering::Relaxed);
            return parse();
        };
        let key = CacheKey {
            provider_id: provider_id.to_string(),
            path: path.to_path_buf(),
        };
        {
            let mut state = self.lock();
            state.seen.insert(key.clone());
            if let Some(hit) = state.entries.get(&key) {
                if hit.fingerprint == fingerprint {
                    return hit.meta.clone();
                }
            }
        }
        self.parses.fetch_add(1, Ordering::Relaxed);
        let meta = parse();
        let mut state = self.lock();
        state.entries.insert(
            key.clone(),
            CachedParse {
                fingerprint,
                meta: meta.clone(),
            },
        );
        state.dirty.insert(key);
        meta
    }

    /// Number of parser invocations since the last [`Self::begin_scan`].
    pub fn parse_count(&self) -> usize {
        self.parses.load(Ordering::Relaxed)
    }

    pub fn begin_scan(&self) {
        let mut state = self.lock();
        state.seen.clear();
        self.parses.store(0, Ordering::Relaxed);
    }

    /// Drop entries of `provider_ids` not touched by the scan that just ran
    /// (deleted or moved files). Returns the removed keys.
    pub fn prune_unseen(&self, provider_ids: &[&str]) -> Vec<CacheKey> {
        let mut state = self.lock();
        let State {
            entries,
            seen,
            dirty,
        } = &mut *state;
        let removed: Vec<CacheKey> = entries
            .keys()
            .filter(|key| provider_ids.contains(&key.provider_id.as_str()) && !seen.contains(*key))
            .cloned()
            .collect();
        for key in &removed {
            entries.remove(key);
            dirty.remove(key);
        }
        removed
    }

    /// Entries changed since the last call.
    pub fn take_dirty(&self) -> Vec<(CacheKey, CachedParse)> {
        let mut state = self.lock();
        let dirty: Vec<CacheKey> = state.dirty.drain().collect();
        dirty
            .into_iter()
            .filter_map(|key| state.entries.get(&key).cloned().map(|v| (key, v)))
            .collect()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.lock().entries.len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(id: &str) -> SessionMeta {
        SessionMeta {
            provider_id: "claude".into(),
            session_id: id.into(),
            ..SessionMeta::default()
        }
    }

    #[test]
    fn unchanged_file_is_not_reparsed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.jsonl");
        std::fs::write(&path, "x").unwrap();
        let cache = ScanCache::with_entries(HashMap::new());

        let first = cache.get_or_parse("claude", &path, || Some(meta("a")));
        assert_eq!(first.unwrap().session_id, "a");
        assert_eq!(cache.parse_count(), 1);

        cache.begin_scan();
        let second = cache.get_or_parse("claude", &path, || panic!("must not parse"));
        assert_eq!(second.unwrap().session_id, "a");
        assert_eq!(cache.parse_count(), 0);
    }

    #[test]
    fn size_change_triggers_reparse_and_unseen_entries_are_pruned() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.jsonl");
        let b = dir.path().join("b.jsonl");
        std::fs::write(&a, "x").unwrap();
        std::fs::write(&b, "x").unwrap();
        let cache = ScanCache::with_entries(HashMap::new());
        cache.get_or_parse("claude", &a, || Some(meta("a")));
        cache.get_or_parse("claude", &b, || Some(meta("b")));

        cache.begin_scan();
        std::fs::write(&a, "xyz").unwrap();
        let updated = cache.get_or_parse("claude", &a, || Some(meta("a2")));
        assert_eq!(updated.unwrap().session_id, "a2");
        assert_eq!(cache.parse_count(), 1);

        let removed = cache.prune_unseen(&["claude"]);
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].path, b);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn disabled_cache_always_parses() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.jsonl");
        std::fs::write(&path, "x").unwrap();
        let cache = ScanCache::disabled();
        cache.get_or_parse("claude", &path, || Some(meta("a")));
        cache.get_or_parse("claude", &path, || Some(meta("a")));
        assert_eq!(cache.parse_count(), 2);
        assert_eq!(cache.len(), 0);
    }
}
