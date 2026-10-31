//! Persistent session index (SESS-09, SESS-16, SESS-21, SESS-31).
//!
//! A small SQLite cache (`~/.cc-switch/session-index.db`, separate from the
//! main database because it is fully derived and can be rebuilt at any time)
//! stores:
//!
//! * `file_cache`: per-file parse results keyed by `(provider, path)` with the
//!   file's `(mtime, size)` fingerprint, so a refresh — including the first one
//!   after a restart — only `stat`s unchanged files;
//! * `sessions`: the last listed sessions, served instantly on cold start;
//! * `session_fts`: an FTS5 (trigram) table over title / summary / project and,
//!   lazily on the first transcript search, the redacted transcript text.
//!
//! If the database cannot be opened or is corrupt it is deleted and rebuilt;
//! if that fails too, the index runs in memory only.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::{Duration, Instant};

use regex::Regex;
use rusqlite::{params, Connection};
use serde::Serialize;

use super::cache::{CacheKey, CachedParse, Fingerprint, ScanCache};
use super::registry::{validate_source, SessionProvider};
use super::source::{session_key, SessionSource};
use super::SessionMeta;

/// Bump to rebuild the index from scratch after a format change.
const INDEX_SCHEMA_VERSION: i32 = 1;
/// Per-session cap on indexed transcript text.
const BODY_INDEX_MAX_BYTES: usize = 512 * 1024;
/// Time budget for lazily indexing transcripts during one search call.
pub const SEARCH_INDEX_BUDGET: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderScanError {
    pub provider_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub sessions: Vec<SessionMeta>,
    pub errors: Vec<ProviderScanError>,
    /// Increments whenever the listed sessions change.
    pub generation: u64,
    /// Whether this refresh changed the list compared to the previous one.
    pub changed: bool,
    /// Number of files parsed (0 when every file was unchanged).
    pub parsed_files: usize,
    /// Set by callers that passed the generation they already hold: the
    /// session list is then omitted because it did not change.
    #[serde(default)]
    pub unchanged: bool,
}

impl ScanReport {
    /// Drop the session payload when the caller already has `known`.
    pub fn elide_if_known(mut self, known: Option<u64>) -> Self {
        if known == Some(self.generation) {
            self.sessions = Vec::new();
            self.unchanged = true;
        }
        self
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub session_key: String,
    pub provider_id: String,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    pub hits: Vec<SearchHit>,
    /// False when some transcripts were not indexed yet (time budget hit);
    /// searching again continues indexing.
    pub complete: bool,
}

#[derive(Default)]
struct IndexState {
    generation: u64,
    digest: Option<u64>,
}

pub struct SessionIndex {
    conn: Mutex<Connection>,
    cache: ScanCache,
    state: Mutex<IndexState>,
    /// Single-flight: concurrent refreshes wait for the running one.
    refresh_lock: Mutex<()>,
}

impl SessionIndex {
    /// Open (or create) the index at `path`, rebuilding it when corrupt.
    pub fn open(path: &Path) -> Self {
        match Self::try_open(path) {
            Ok(index) => index,
            Err(err) => {
                log::warn!(
                    "Session index {} unusable ({err}); rebuilding",
                    path.display()
                );
                let _ = std::fs::remove_file(path);
                Self::try_open(path).unwrap_or_else(|err| {
                    log::warn!("Session index falls back to memory: {err}");
                    Self::in_memory()
                })
            }
        }
    }

    pub fn in_memory() -> Self {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        Self::from_conn(conn).expect("in-memory session index")
    }

    fn try_open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.busy_timeout(Duration::from_secs(2))
            .map_err(|e| e.to_string())?;
        let ok: String = conn
            .query_row("PRAGMA quick_check", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if ok != "ok" {
            return Err(format!("quick_check: {ok}"));
        }
        Self::from_conn(conn)
    }

    fn from_conn(conn: Connection) -> Result<Self, String> {
        let version: i32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if version != INDEX_SCHEMA_VERSION {
            conn.execute_batch(
                "DROP TABLE IF EXISTS file_cache;
                 DROP TABLE IF EXISTS sessions;
                 DROP TABLE IF EXISTS session_fts;",
            )
            .map_err(|e| e.to_string())?;
        }
        conn.execute_batch(&format!(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS file_cache (
                 provider_id TEXT NOT NULL,
                 path TEXT NOT NULL,
                 mtime_ms INTEGER NOT NULL,
                 size INTEGER NOT NULL,
                 meta TEXT,
                 PRIMARY KEY (provider_id, path)
             );
             CREATE TABLE IF NOT EXISTS sessions (
                 session_key TEXT PRIMARY KEY,
                 provider_id TEXT NOT NULL,
                 session_id TEXT NOT NULL,
                 meta TEXT NOT NULL,
                 sort_ts INTEGER NOT NULL,
                 content_hash TEXT NOT NULL,
                 body_hash TEXT
             );
             CREATE VIRTUAL TABLE IF NOT EXISTS session_fts USING fts5(
                 session_key UNINDEXED, title, summary, project_dir, body,
                 tokenize = 'trigram'
             );
             PRAGMA user_version = {INDEX_SCHEMA_VERSION};"
        ))
        .map_err(|e| e.to_string())?;

        let entries = load_file_cache(&conn).map_err(|e| e.to_string())?;
        Ok(Self {
            conn: Mutex::new(conn),
            cache: ScanCache::with_entries(entries),
            state: Mutex::new(IndexState::default()),
            refresh_lock: Mutex::new(()),
        })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Sessions from the last refresh (possibly from a previous app run).
    pub fn snapshot(&self) -> Vec<SessionMeta> {
        let conn = self.conn();
        let mut stmt = match conn.prepare("SELECT meta FROM sessions ORDER BY sort_ts DESC") {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map([], |row| row.get::<_, String>(0));
        match rows {
            Ok(rows) => rows
                .flatten()
                .filter_map(|json| serde_json::from_str(&json).ok())
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Incrementally rescan all providers and persist the result.
    pub fn refresh(&self, providers: &[&'static dyn SessionProvider]) -> ScanReport {
        let _single_flight = self.refresh_lock.lock().unwrap_or_else(|e| e.into_inner());
        self.cache.begin_scan();
        let (mut sessions, errors, ok_providers) = scan_providers(providers, &self.cache);
        finalize_sessions(&mut sessions);
        let parsed_files = self.cache.parse_count();

        let removed = self.cache.prune_unseen(&ok_providers);
        let dirty = self.cache.take_dirty();
        if let Err(err) = self.persist(&sessions, &dirty, &removed, &ok_providers) {
            log::warn!("Failed to persist session index: {err}");
        }

        let digest = digest_sessions(&sessions);
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let changed = state.digest != Some(digest);
        if changed {
            state.generation += 1;
            state.digest = Some(digest);
        }
        ScanReport {
            sessions,
            errors,
            generation: state.generation,
            changed,
            parsed_files,
            unchanged: false,
        }
    }

    fn persist(
        &self,
        sessions: &[SessionMeta],
        dirty: &[(CacheKey, CachedParse)],
        removed: &[CacheKey],
        ok_providers: &[&str],
    ) -> rusqlite::Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        {
            let mut upsert = tx.prepare_cached(
                "INSERT INTO file_cache (provider_id, path, mtime_ms, size, meta)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(provider_id, path) DO UPDATE SET
                   mtime_ms = excluded.mtime_ms, size = excluded.size, meta = excluded.meta",
            )?;
            for (key, entry) in dirty {
                let meta = entry
                    .meta
                    .as_ref()
                    .and_then(|m| serde_json::to_string(m).ok());
                upsert.execute(params![
                    key.provider_id,
                    key.path.to_string_lossy(),
                    entry.fingerprint.mtime_ms,
                    entry.fingerprint.size as i64,
                    meta
                ])?;
            }
            let mut delete =
                tx.prepare_cached("DELETE FROM file_cache WHERE provider_id = ?1 AND path = ?2")?;
            for key in removed {
                delete.execute(params![key.provider_id, key.path.to_string_lossy()])?;
            }
        }

        // Diff the session list against the stored one. Providers that failed
        // to scan keep their previous rows.
        let mut existing: HashMap<String, String> = HashMap::new();
        {
            let mut stmt =
                tx.prepare("SELECT session_key, provider_id, content_hash FROM sessions")?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;
            for (key, provider, hash) in rows.flatten() {
                if ok_providers.contains(&provider.as_str()) {
                    existing.insert(key, hash);
                }
            }
        }
        let mut seen = HashSet::new();
        {
            let mut upsert = tx.prepare_cached(
                "INSERT INTO sessions (session_key, provider_id, session_id, meta, sort_ts, content_hash, body_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)
                 ON CONFLICT(session_key) DO UPDATE SET
                   meta = excluded.meta, sort_ts = excluded.sort_ts,
                   content_hash = excluded.content_hash, body_hash = NULL",
            )?;
            let mut fts_delete =
                tx.prepare_cached("DELETE FROM session_fts WHERE session_key = ?1")?;
            let mut fts_insert = tx.prepare_cached(
                "INSERT INTO session_fts (session_key, title, summary, project_dir, body)
                 VALUES (?1, ?2, ?3, ?4, '')",
            )?;
            for meta in sessions {
                let Some(key) = meta.session_key.clone() else {
                    continue;
                };
                if !ok_providers.contains(&meta.provider_id.as_str()) {
                    continue;
                }
                seen.insert(key.clone());
                let json = serde_json::to_string(meta).unwrap_or_default();
                let hash = format!("{:016x}", hash_str(&json));
                if existing.get(&key) == Some(&hash) {
                    continue;
                }
                upsert.execute(params![
                    key,
                    meta.provider_id,
                    meta.session_id,
                    json,
                    sort_ts(meta),
                    hash
                ])?;
                fts_delete.execute([&key])?;
                fts_insert.execute(params![
                    key,
                    meta.title.as_deref().unwrap_or(""),
                    meta.summary.as_deref().unwrap_or(""),
                    meta.project_dir.as_deref().unwrap_or("")
                ])?;
            }
        }
        {
            let mut delete = tx.prepare_cached("DELETE FROM sessions WHERE session_key = ?1")?;
            let mut fts_delete =
                tx.prepare_cached("DELETE FROM session_fts WHERE session_key = ?1")?;
            for key in existing.keys().filter(|k| !seen.contains(*k)) {
                delete.execute([key])?;
                fts_delete.execute([key])?;
            }
        }
        tx.commit()
    }

    /// Drop a session from the index (after it was deleted).
    pub fn forget(&self, session_key: &str) {
        let conn = self.conn();
        let _ = conn.execute("DELETE FROM sessions WHERE session_key = ?1", [session_key]);
        let _ = conn.execute(
            "DELETE FROM session_fts WHERE session_key = ?1",
            [session_key],
        );
        self.state.lock().unwrap_or_else(|e| e.into_inner()).digest = None;
    }

    /// Full-text search over titles, summaries, project paths and (lazily
    /// indexed, redacted) transcripts across all providers.
    pub fn search(
        &self,
        providers: &[&'static dyn SessionProvider],
        query: &str,
        limit: usize,
        budget: Duration,
    ) -> SearchResponse {
        let query = query.trim();
        if query.is_empty() {
            return SearchResponse {
                hits: Vec::new(),
                complete: true,
            };
        }
        let complete = self.index_bodies(providers, budget);
        let hits = self
            .query_fts(query, limit.clamp(1, 500))
            .unwrap_or_else(|err| {
                log::warn!("Session search failed: {err}");
                Vec::new()
            });
        SearchResponse { hits, complete }
    }

    /// Index transcripts whose metadata changed since they were last indexed.
    /// Returns `true` when every session is indexed.
    fn index_bodies(&self, providers: &[&'static dyn SessionProvider], budget: Duration) -> bool {
        let started = Instant::now();
        let pending: Vec<(String, String, String, String)> = {
            let conn = self.conn();
            let mut stmt = match conn.prepare(
                "SELECT session_key, provider_id, meta, content_hash FROM sessions
                 WHERE body_hash IS NULL OR body_hash != content_hash
                 ORDER BY sort_ts DESC",
            ) {
                Ok(stmt) => stmt,
                Err(_) => return false,
            };
            let rows = stmt.query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            });
            match rows {
                Ok(rows) => rows.flatten().collect(),
                Err(_) => return false,
            }
        };

        for (key, provider_id, meta_json, content_hash) in &pending {
            if started.elapsed() > budget {
                return false;
            }
            let body = serde_json::from_str::<SessionMeta>(meta_json)
                .ok()
                .and_then(|meta| transcript_text(providers, provider_id, &meta))
                .unwrap_or_default();
            let conn = self.conn();
            let _ = conn.execute(
                "UPDATE session_fts SET body = ?2 WHERE session_key = ?1",
                params![key, body],
            );
            let _ = conn.execute(
                "UPDATE sessions SET body_hash = ?2 WHERE session_key = ?1",
                params![key, content_hash],
            );
        }
        true
    }

    fn query_fts(&self, query: &str, limit: usize) -> rusqlite::Result<Vec<SearchHit>> {
        let conn = self.conn();
        let rows: Vec<(String, String, String, String, String)> = if query.chars().count() >= 3 {
            // Trigram phrase query == case-insensitive substring match.
            let phrase = format!("\"{}\"", query.replace('"', "\"\""));
            let mut stmt = conn.prepare(
                "SELECT f.session_key, s.provider_id, s.session_id, f.title, f.body
                     FROM session_fts f JOIN sessions s ON s.session_key = f.session_key
                     WHERE session_fts MATCH ?1
                     ORDER BY s.sort_ts DESC LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![phrase, limit as i64], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })?;
            rows.flatten().collect()
        } else {
            // Shorter queries (e.g. two CJK characters) cannot use trigrams.
            let pattern = format!(
                "%{}%",
                query
                    .replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_")
            );
            let mut stmt = conn.prepare(
                "SELECT f.session_key, s.provider_id, s.session_id, f.title, f.body
                     FROM session_fts f JOIN sessions s ON s.session_key = f.session_key
                     WHERE f.title LIKE ?1 ESCAPE '\\' OR f.summary LIKE ?1 ESCAPE '\\'
                        OR f.project_dir LIKE ?1 ESCAPE '\\' OR f.body LIKE ?1 ESCAPE '\\'
                     ORDER BY s.sort_ts DESC LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![pattern, limit as i64], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })?;
            rows.flatten().collect()
        };
        Ok(rows
            .into_iter()
            .map(
                |(session_key, provider_id, session_id, title, body)| SearchHit {
                    session_key,
                    provider_id,
                    session_id,
                    snippet: make_snippet(&body, query).or_else(|| make_snippet(&title, query)),
                },
            )
            .collect())
    }

    #[cfg(test)]
    fn cached_files(&self) -> usize {
        self.cache.len()
    }
}

/// Run every provider on its own thread. A panicking or failing provider is
/// reported instead of silently producing an empty list (SESS-31).
fn scan_providers<'a>(
    providers: &[&'a dyn SessionProvider],
    cache: &ScanCache,
) -> (Vec<SessionMeta>, Vec<ProviderScanError>, Vec<&'a str>) {
    let results: Vec<(&'a str, Result<Vec<SessionMeta>, String>)> = std::thread::scope(|s| {
        let handles: Vec<_> = providers
            .iter()
            .map(|provider| {
                let provider = *provider;
                (provider.id(), s.spawn(move || provider.scan(cache)))
            })
            .collect();
        handles
            .into_iter()
            .map(|(id, handle)| {
                let result = handle.join().map_err(|panic| {
                    let message = panic
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                        .unwrap_or_else(|| "unknown panic".to_string());
                    log::error!("Session scanner for {id} panicked: {message}");
                    format!("Scanner crashed: {message}")
                });
                (id, result)
            })
            .collect()
    });

    let mut sessions = Vec::new();
    let mut errors = Vec::new();
    let mut ok = Vec::new();
    for (id, result) in results {
        match result {
            Ok(list) => {
                sessions.extend(list);
                ok.push(id);
            }
            Err(message) => errors.push(ProviderScanError {
                provider_id: id.to_string(),
                message,
            }),
        }
    }
    for provider in providers {
        for root in provider.roots() {
            if root.exists() {
                if let Err(err) = std::fs::read_dir(&root) {
                    errors.push(ProviderScanError {
                        provider_id: provider.id().to_string(),
                        message: format!("Cannot read {}: {err}", root.display()),
                    });
                }
            }
        }
    }
    (sessions, errors, ok)
}

/// Fill the typed source and key, then sort newest first.
pub(crate) fn finalize_sessions(sessions: &mut [SessionMeta]) {
    for meta in sessions.iter_mut() {
        if let Some(raw) = meta.source_path.as_deref() {
            meta.source = SessionSource::from_legacy(&meta.provider_id, raw).ok();
            meta.session_key = Some(session_key(&meta.provider_id, &meta.session_id, raw));
        }
    }
    sessions.sort_by_key(|meta| std::cmp::Reverse(sort_ts(meta)));
}

fn sort_ts(meta: &SessionMeta) -> i64 {
    meta.last_active_at.or(meta.created_at).unwrap_or(0)
}

fn hash_str(value: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn digest_sessions(sessions: &[SessionMeta]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for meta in sessions {
        serde_json::to_string(meta)
            .unwrap_or_default()
            .hash(&mut hasher);
    }
    hasher.finish()
}

fn load_file_cache(conn: &Connection) -> rusqlite::Result<HashMap<CacheKey, CachedParse>> {
    let mut stmt =
        conn.prepare("SELECT provider_id, path, mtime_ms, size, meta FROM file_cache")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, Option<String>>(4)?,
        ))
    })?;
    let mut map = HashMap::new();
    for (provider_id, path, mtime_ms, size, meta) in rows.flatten() {
        let meta = match meta {
            Some(json) => match serde_json::from_str(&json) {
                Ok(meta) => Some(meta),
                // Unreadable entry: let the next scan re-parse the file.
                Err(_) => continue,
            },
            None => None,
        };
        map.insert(
            CacheKey {
                provider_id,
                path: PathBuf::from(path),
            },
            CachedParse {
                fingerprint: Fingerprint {
                    mtime_ms,
                    size: size.max(0) as u64,
                },
                meta,
            },
        );
    }
    Ok(map)
}

/// Transcript text for the FTS index, redacted and capped.
fn transcript_text(
    providers: &[&'static dyn SessionProvider],
    provider_id: &str,
    meta: &SessionMeta,
) -> Option<String> {
    let provider = providers.iter().find(|p| p.id() == provider_id)?;
    let source = meta.source.clone().or_else(|| {
        meta.source_path
            .as_deref()
            .and_then(|raw| SessionSource::from_legacy(provider_id, raw).ok())
    })?;
    let validated = validate_source(*provider, &source).ok()?;
    let messages = provider.load_messages(&validated).ok()?;
    let mut text = String::new();
    for message in messages {
        if text.len() >= BODY_INDEX_MAX_BYTES {
            break;
        }
        text.push_str(&redact_secrets(&message.content));
        text.push('\n');
    }
    if text.len() > BODY_INDEX_MAX_BYTES {
        let mut cut = BODY_INDEX_MAX_BYTES;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
    }
    Some(text)
}

static SECRET_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)
        sk-[A-Za-z0-9_\-]{16,}
        | (?:ghp|gho|ghu|ghs|github_pat)_[A-Za-z0-9_]{20,}
        | xox[abprs]-[A-Za-z0-9\-]{10,}
        | AKIA[0-9A-Z]{16}
        | AIza[0-9A-Za-z_\-]{30,}
        | (?i:bearer)\s+[A-Za-z0-9._\-]{20,}
        ",
    )
    .expect("secret regex")
});

/// Mask API keys and tokens before they are written to the search index.
pub fn redact_secrets(text: &str) -> String {
    SECRET_RE.replace_all(text, "[REDACTED]").into_owned()
}

/// A short excerpt around the first case-insensitive match of `query`.
fn make_snippet(text: &str, query: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let needle: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    if needle.is_empty() || chars.len() < needle.len() {
        return None;
    }
    let lower: Vec<char> = chars
        .iter()
        .map(|c| c.to_lowercase().next().unwrap_or(*c))
        .collect();
    let pos = lower
        .windows(needle.len())
        .position(|w| w == needle.as_slice())?;
    const CONTEXT: usize = 60;
    let start = pos.saturating_sub(CONTEXT);
    let end = (pos + needle.len() + CONTEXT).min(chars.len());
    let mut snippet = String::new();
    if start > 0 {
        snippet.push('…');
    }
    snippet.extend(
        chars[start..end]
            .iter()
            .map(|c| if *c == '\n' { ' ' } else { *c }),
    );
    if end < chars.len() {
        snippet.push('…');
    }
    Some(snippet)
}

/// The process-wide index stored in the app config directory.
pub fn global() -> &'static SessionIndex {
    static INDEX: OnceLock<SessionIndex> = OnceLock::new();
    INDEX.get_or_init(|| {
        SessionIndex::open(&crate::config::get_app_config_dir().join("session-index.db"))
    })
}

/// Read access for tests that need the current row count.
#[cfg(test)]
fn session_rows(index: &SessionIndex) -> i64 {
    use rusqlite::OptionalExtension;
    index
        .conn()
        .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
        .optional()
        .ok()
        .flatten()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_manager::registry::ValidatedSource;
    use crate::session_manager::SessionMessage;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// A file-based provider over a temp directory: one `*.txt` file = one
    /// session whose transcript is the file content.
    struct DirProvider {
        root: PathBuf,
    }

    impl SessionProvider for DirProvider {
        fn id(&self) -> &'static str {
            "test"
        }
        fn roots(&self) -> Vec<PathBuf> {
            vec![self.root.clone()]
        }
        fn scan(&self, cache: &ScanCache) -> Vec<SessionMeta> {
            let mut out = Vec::new();
            for entry in std::fs::read_dir(&self.root).unwrap().flatten() {
                let path = entry.path();
                if let Some(meta) = cache.get_or_parse("test", &path, || {
                    let content = std::fs::read_to_string(&path).ok()?;
                    Some(SessionMeta {
                        provider_id: "test".into(),
                        session_id: path.file_stem()?.to_string_lossy().to_string(),
                        title: content.lines().next().map(str::to_string),
                        last_active_at: Some(content.len() as i64),
                        source_path: Some(path.to_string_lossy().to_string()),
                        ..SessionMeta::default()
                    })
                }) {
                    out.push(meta);
                }
            }
            out
        }
        fn load_messages(&self, source: &ValidatedSource) -> Result<Vec<SessionMessage>, String> {
            let content = std::fs::read_to_string(&source.path).map_err(|e| e.to_string())?;
            Ok(vec![SessionMessage {
                role: "user".into(),
                content,
                ts: None,
            }])
        }
        fn delete(&self, _: &ValidatedSource, _: &str) -> Result<bool, String> {
            Ok(false)
        }
    }

    fn leak(provider: DirProvider) -> &'static dyn SessionProvider {
        Box::leak(Box::new(provider))
    }

    #[test]
    fn rebuild_from_disk_and_skip_unchanged_files() {
        let root = tempfile::tempdir().unwrap();
        let db_dir = tempfile::tempdir().unwrap();
        let db = db_dir.path().join("index.db");
        std::fs::write(root.path().join("a.txt"), "alpha session\nhello world").unwrap();
        std::fs::write(root.path().join("b.txt"), "beta session\nsecond").unwrap();
        let provider = leak(DirProvider {
            root: root.path().to_path_buf(),
        });

        let index = SessionIndex::open(&db);
        let first = index.refresh(&[provider]);
        assert_eq!(first.sessions.len(), 2);
        assert_eq!(first.parsed_files, 2);
        assert!(first.changed);
        assert!(first.sessions[0].session_key.is_some());
        assert!(matches!(
            first.sessions[0].source,
            Some(SessionSource::File { .. })
        ));

        let second = index.refresh(&[provider]);
        assert_eq!(second.parsed_files, 0, "unchanged files are not parsed");
        assert!(!second.changed);
        assert_eq!(second.generation, first.generation);
        let elided = second.clone().elide_if_known(Some(first.generation));
        assert!(elided.unchanged && elided.sessions.is_empty());
        assert!(!second.elide_if_known(Some(first.generation + 7)).unchanged);
        drop(index);

        // A new process reuses the persisted cache and session list.
        let reopened = SessionIndex::open(&db);
        assert_eq!(reopened.snapshot().len(), 2);
        assert_eq!(reopened.cached_files(), 2);
        let warm = reopened.refresh(&[provider]);
        assert_eq!(warm.parsed_files, 0);
        assert_eq!(warm.sessions.len(), 2);
    }

    #[test]
    fn stale_entries_are_reparsed_and_deleted_files_pruned() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.txt"), "alpha").unwrap();
        std::fs::write(root.path().join("b.txt"), "beta").unwrap();
        let provider = leak(DirProvider {
            root: root.path().to_path_buf(),
        });
        let index = SessionIndex::in_memory();
        let first = index.refresh(&[provider]);

        std::fs::write(root.path().join("a.txt"), "alpha renamed title").unwrap();
        std::fs::remove_file(root.path().join("b.txt")).unwrap();
        let next = index.refresh(&[provider]);
        assert_eq!(next.parsed_files, 1);
        assert!(next.changed);
        assert!(next.generation > first.generation);
        assert_eq!(next.sessions.len(), 1);
        assert_eq!(
            next.sessions[0].title.as_deref(),
            Some("alpha renamed title")
        );
        assert_eq!(index.cached_files(), 1);
        assert_eq!(session_rows(&index), 1);
    }

    #[test]
    fn transcript_search_finds_body_text_and_redacts_secrets() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("a.txt"),
            "deploy notes\nthe flux capacitor needs sk-abcdefghijklmnopqrstuvwxyz",
        )
        .unwrap();
        std::fs::write(root.path().join("b.txt"), "unrelated\n部署 完成").unwrap();
        let provider = leak(DirProvider {
            root: root.path().to_path_buf(),
        });
        let index = SessionIndex::in_memory();
        index.refresh(&[provider]);

        let res = index.search(&[provider], "Flux Capacitor", 10, SEARCH_INDEX_BUDGET);
        assert!(res.complete);
        assert_eq!(res.hits.len(), 1);
        assert_eq!(res.hits[0].session_id, "a");
        assert!(res.hits[0].snippet.as_deref().unwrap().contains("flux"));

        let res = index.search(&[provider], "abcdefghijklmnop", 10, SEARCH_INDEX_BUDGET);
        assert!(res.hits.is_empty(), "secrets are not indexed");

        // Two-character CJK query uses the LIKE fallback.
        let res = index.search(&[provider], "部署", 10, SEARCH_INDEX_BUDGET);
        assert_eq!(res.hits.len(), 1);
        assert_eq!(res.hits[0].session_id, "b");
    }

    #[test]
    fn corrupt_index_file_is_rebuilt() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("index.db");
        std::fs::write(
            &db,
            b"this is not a sqlite database at all, just junk bytes",
        )
        .unwrap();
        let index = SessionIndex::open(&db);
        assert!(index.snapshot().is_empty());
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.txt"), "alpha").unwrap();
        let provider = leak(DirProvider {
            root: root.path().to_path_buf(),
        });
        assert_eq!(index.refresh(&[provider]).sessions.len(), 1);
    }

    struct PanickingProvider;
    impl SessionProvider for PanickingProvider {
        fn id(&self) -> &'static str {
            "boom"
        }
        fn roots(&self) -> Vec<PathBuf> {
            Vec::new()
        }
        fn scan(&self, _: &ScanCache) -> Vec<SessionMeta> {
            panic!("parser exploded")
        }
        fn load_messages(&self, _: &ValidatedSource) -> Result<Vec<SessionMessage>, String> {
            unreachable!()
        }
        fn delete(&self, _: &ValidatedSource, _: &str) -> Result<bool, String> {
            unreachable!()
        }
    }

    #[test]
    fn panicking_provider_is_reported_and_others_still_listed() {
        static QUIET: AtomicBool = AtomicBool::new(false);
        if !QUIET.swap(true, Ordering::SeqCst) {
            let default_hook = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                if !info.to_string().contains("parser exploded") {
                    default_hook(info);
                }
            }));
        }
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.txt"), "alpha").unwrap();
        let provider = leak(DirProvider {
            root: root.path().to_path_buf(),
        });
        let index = SessionIndex::in_memory();
        let report = index.refresh(&[provider, &PanickingProvider]);
        assert_eq!(report.sessions.len(), 1);
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].provider_id, "boom");
        assert!(report.errors[0].message.contains("parser exploded"));
    }

    #[test]
    fn redaction_masks_common_token_shapes() {
        let text = "key sk-ant-api03-abcdefghijklmnop and ghp_abcdefghijklmnopqrstuvwxyz1234 ok";
        let red = redact_secrets(text);
        assert!(!red.contains("abcdefghijklmnop"));
        assert!(red.contains("ok"));
    }
}
