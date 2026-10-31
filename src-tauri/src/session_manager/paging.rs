//! Paged transcript delivery (SESS-14, first increment).
//!
//! `get_session_messages` ships a whole transcript over IPC in one payload. The
//! paged variant here bounds every response: a page holds at most `limit`
//! messages, each message's content is capped at `max_content_bytes` (the UI
//! can ask for one message in full), and the page stops early once the total
//! content would exceed [`MAX_PAGE_BYTES`]. The table of contents is served by
//! [`list_turns`], which returns short previews only.
//!
//! The parsed transcript of the last opened file-backed session is cached by
//! `(provider, path, mtime, size)`, so paging through it does not re-parse the
//! file for every page.

use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use serde::Serialize;

use super::SessionMessage;

/// Default number of messages per page.
pub const DEFAULT_PAGE_LIMIT: usize = 200;
/// Default per-message content cap (64 KiB).
pub const DEFAULT_MAX_CONTENT_BYTES: usize = 64 * 1024;
/// Hard upper bound on the content bytes in one page (8 MiB).
pub const MAX_PAGE_BYTES: usize = 8 * 1024 * 1024;
/// Length of a TOC preview, in characters.
pub const TURN_PREVIEW_CHARS: usize = 160;

/// One page of a transcript.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessagePage {
    pub messages: Vec<SessionMessage>,
    /// Index of `messages[0]` in the whole transcript.
    pub offset: usize,
    /// Number of messages in the whole transcript.
    pub total: usize,
    /// Whether messages after this page exist.
    pub has_more: bool,
    /// Transcript indices whose content was cut at `max_content_bytes`.
    pub truncated: Vec<usize>,
}

/// A lightweight table-of-contents entry.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionTurn {
    pub index: usize,
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<i64>,
    pub preview: String,
    /// Full content length in bytes.
    pub content_bytes: usize,
}

/// Cut `s` to at most `max` bytes on a char boundary.
fn truncate_on_char_boundary(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Slice `all` into a bounded page.
pub fn page(
    all: &[SessionMessage],
    offset: usize,
    limit: usize,
    max_content_bytes: usize,
) -> SessionMessagePage {
    let total = all.len();
    let offset = offset.min(total);
    let limit = limit.max(1);
    let mut messages = Vec::new();
    let mut truncated = Vec::new();
    let mut bytes = 0usize;
    let mut next = offset;
    for (i, message) in all.iter().enumerate().skip(offset).take(limit) {
        let cut = truncate_on_char_boundary(&message.content, max_content_bytes);
        // Always deliver at least one message so paging makes progress.
        if !messages.is_empty() && bytes + cut.len() > MAX_PAGE_BYTES {
            break;
        }
        bytes += cut.len();
        if cut.len() < message.content.len() {
            truncated.push(i);
        }
        messages.push(SessionMessage {
            role: message.role.clone(),
            content: cut.to_string(),
            ts: message.ts,
        });
        next = i + 1;
    }
    SessionMessagePage {
        messages,
        offset,
        total,
        has_more: next < total,
        truncated,
    }
}

/// Short previews of every message for the TOC.
pub fn turns(all: &[SessionMessage]) -> Vec<SessionTurn> {
    all.iter()
        .enumerate()
        .map(|(index, m)| SessionTurn {
            index,
            role: m.role.clone(),
            ts: m.ts,
            preview: m
                .content
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(TURN_PREVIEW_CHARS)
                .collect(),
            content_bytes: m.content.len(),
        })
        .collect()
}

type CacheKey = (String, String, SystemTime, u64);

static LAST_TRANSCRIPT: Mutex<Option<(CacheKey, Arc<Vec<SessionMessage>>)>> = Mutex::new(None);

fn cache_key(provider_id: &str, source_path: &str) -> Option<CacheKey> {
    let meta = std::fs::metadata(source_path).ok()?;
    if !meta.is_file() {
        return None;
    }
    Some((
        provider_id.to_string(),
        source_path.to_string(),
        meta.modified().ok()?,
        meta.len(),
    ))
}

/// Load (or reuse) the parsed transcript with `loader`.
pub fn load_cached<F>(
    provider_id: &str,
    source_path: &str,
    loader: F,
) -> Result<Arc<Vec<SessionMessage>>, String>
where
    F: FnOnce() -> Result<Vec<SessionMessage>, String>,
{
    let key = cache_key(provider_id, source_path);
    if let Some(key) = &key {
        if let Ok(guard) = LAST_TRANSCRIPT.lock() {
            if let Some((cached_key, messages)) = guard.as_ref() {
                if cached_key == key {
                    return Ok(Arc::clone(messages));
                }
            }
        }
    }
    let messages = Arc::new(loader()?);
    if let Some(key) = key {
        if let Ok(mut guard) = LAST_TRANSCRIPT.lock() {
            *guard = Some((key, Arc::clone(&messages)));
        }
    }
    Ok(messages)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: &str, content: &str) -> SessionMessage {
        SessionMessage {
            role: role.to_string(),
            content: content.to_string(),
            ts: Some(1),
        }
    }

    #[test]
    fn pages_are_bounded_and_report_more() {
        let all: Vec<_> = (0..5).map(|i| msg("user", &format!("m{i}"))).collect();
        let first = page(&all, 0, 2, 1024);
        assert_eq!(first.messages.len(), 2);
        assert_eq!((first.offset, first.total, first.has_more), (0, 5, true));
        let last = page(&all, 4, 2, 1024);
        assert_eq!(last.messages[0].content, "m4");
        assert!(!last.has_more);
        let past_end = page(&all, 99, 2, 1024);
        assert!(past_end.messages.is_empty());
        assert_eq!(past_end.offset, 5);
    }

    #[test]
    fn long_content_is_cut_on_a_char_boundary_and_flagged() {
        let all = vec![msg("assistant", "ééééé"), msg("user", "short")];
        let p = page(&all, 0, 10, 3);
        assert_eq!(p.messages[0].content, "é");
        assert_eq!(p.truncated, vec![0, 1]);
        assert_eq!(p.messages[1].content, "sho");
    }

    #[test]
    fn total_page_bytes_are_capped() {
        let big = "x".repeat(MAX_PAGE_BYTES / 2 + 1);
        let all = vec![msg("a", &big), msg("b", &big), msg("c", &big)];
        let p = page(&all, 0, 10, usize::MAX);
        assert_eq!(p.messages.len(), 1);
        assert!(p.has_more);
    }

    #[test]
    fn turns_are_short_previews() {
        let long = format!("hello\n  world {}", "y".repeat(500));
        let t = turns(&[msg("user", &long)]);
        assert_eq!(t[0].preview.chars().count(), TURN_PREVIEW_CHARS);
        assert!(t[0].preview.starts_with("hello world "));
        assert_eq!(t[0].content_bytes, long.len());
    }

    #[test]
    fn cache_reuses_parse_until_the_file_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        std::fs::write(&path, "a").unwrap();
        let p = path.to_string_lossy().to_string();
        let calls = std::cell::Cell::new(0);
        let load = || {
            calls.set(calls.get() + 1);
            Ok(vec![msg("user", "x")])
        };
        load_cached("test-cache", &p, load).unwrap();
        load_cached("test-cache", &p, load).unwrap();
        assert_eq!(calls.get(), 1);
        std::fs::write(&path, "abc").unwrap();
        load_cached("test-cache", &p, load).unwrap();
        assert_eq!(calls.get(), 2);
    }
}
