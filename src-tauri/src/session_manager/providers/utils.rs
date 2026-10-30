use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use chrono::DateTime;
use serde_json::Value;

/// Maximum number of characters for session titles (shared across providers).
pub const TITLE_MAX_CHARS: usize = 80;

/// Initial size of the backwards tail window.
const TAIL_WINDOW_INITIAL: u64 = 16_384;
/// Upper bound for the backwards tail window. A final line larger than this is
/// not returned; callers fall back to other metadata (e.g. file mtime).
const TAIL_WINDOW_MAX: u64 = 8 * 1024 * 1024;

/// Read the first `head_n` lines and last `tail_n` lines from a file.
/// For small files (< 16 KB), reads all lines once to avoid unnecessary seeking.
/// For larger files the tail is read backwards in growing windows (16 KB, 64 KB,
/// ... up to 8 MB) until `tail_n` complete lines are available, so a very long
/// final line (e.g. a large tool result) does not leave the tail empty.
pub fn read_head_tail_lines(
    path: &Path,
    head_n: usize,
    tail_n: usize,
) -> io::Result<(Vec<String>, Vec<String>)> {
    let file = File::open(path)?;
    let file_len = file.metadata()?.len();

    // For small files, read all lines once and split
    if file_len < TAIL_WINDOW_INITIAL {
        let reader = BufReader::new(file);
        let all: Vec<String> = reader.lines().map_while(Result::ok).collect();
        let head = all.iter().take(head_n).cloned().collect();
        let skip = all.len().saturating_sub(tail_n);
        let tail = all.into_iter().skip(skip).collect();
        return Ok((head, tail));
    }

    // Read head lines from the beginning
    let reader = BufReader::new(file);
    let head: Vec<String> = reader.lines().take(head_n).map_while(Result::ok).collect();

    let tail =
        read_tail_lines_with_limits(path, file_len, tail_n, TAIL_WINDOW_INITIAL, TAIL_WINDOW_MAX)?;
    Ok((head, tail))
}

fn read_tail_lines_with_limits(
    path: &Path,
    file_len: u64,
    tail_n: usize,
    initial_window: u64,
    max_window: u64,
) -> io::Result<Vec<String>> {
    if tail_n == 0 {
        return Ok(Vec::new());
    }

    let mut file = File::open(path)?;
    let mut window = initial_window.max(1);
    loop {
        let window_len = window.min(file_len);
        let seek_pos = file_len - window_len;
        file.seek(SeekFrom::Start(seek_pos))?;
        let mut buf = Vec::with_capacity(window_len as usize);
        (&mut file).take(window_len).read_to_end(&mut buf)?;

        let text = String::from_utf8_lossy(&buf);
        let mut lines: Vec<&str> = text.lines().collect();
        // The first line is partial unless the window starts at the file
        // beginning or exactly after a newline.
        if seek_pos > 0 && !lines.is_empty() {
            let mut prev = [0u8; 1];
            file.seek(SeekFrom::Start(seek_pos - 1))?;
            file.read_exact(&mut prev)?;
            if prev[0] != b'\n' {
                lines.remove(0);
            }
        }

        let reached_limit = seek_pos == 0 || window >= max_window;
        if lines.len() >= tail_n || reached_limit {
            let skip = lines.len().saturating_sub(tail_n);
            return Ok(lines.into_iter().skip(skip).map(str::to_string).collect());
        }
        window = window.saturating_mul(4).min(max_window);
    }
}

/// File modification time in milliseconds since the Unix epoch.
pub fn file_mtime_ms(path: &Path) -> Option<i64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let dur = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    i64::try_from(dur.as_millis()).ok()
}

/// How long SQLite waits for a lock held by the owning tool before failing.
pub const TOOL_DB_BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// Open another tool's SQLite database (OpenCode, Hermes, ...). These tools keep
/// their database open and write-locked while running, so a busy timeout is set
/// to avoid spurious "database is locked" failures. `writable` opens read-write
/// without creating the file; otherwise the database is opened read-only.
pub fn open_tool_db(path: &Path, writable: bool) -> rusqlite::Result<rusqlite::Connection> {
    use rusqlite::OpenFlags;
    let mode = if writable {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    } else {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    };
    let conn = rusqlite::Connection::open_with_flags(path, mode | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    conn.busy_timeout(TOOL_DB_BUSY_TIMEOUT)?;
    Ok(conn)
}

/// Scale an integer epoch timestamp to milliseconds. Values are classified by
/// magnitude: seconds (< 1e11), milliseconds (< 1e14), microseconds (< 1e17)
/// and nanoseconds above that.
fn epoch_to_ms(n: i64) -> i64 {
    let abs = n.unsigned_abs();
    if abs < 100_000_000_000 {
        n.saturating_mul(1000)
    } else if abs < 100_000_000_000_000 {
        n
    } else if abs < 100_000_000_000_000_000 {
        n / 1000
    } else {
        n / 1_000_000
    }
}

/// Parse a timestamp into epoch milliseconds. Accepts integers / floats in
/// seconds, milliseconds, microseconds or nanoseconds, RFC 3339 strings, naive
/// ISO-8601 strings without an offset (assumed UTC), SQLite `datetime()` output
/// (`YYYY-MM-DD HH:MM:SS[.fff]`) and numeric strings.
pub fn parse_timestamp_to_ms(value: &Value) -> Option<i64> {
    if let Some(n) = value.as_i64() {
        return Some(epoch_to_ms(n));
    }
    if let Some(n) = value.as_f64() {
        if !n.is_finite() {
            return None;
        }
        // Fractional seconds keep their millisecond precision.
        if n.abs() < 100_000_000_000.0 {
            return Some((n * 1000.0) as i64);
        }
        return Some(epoch_to_ms(n as i64));
    }
    let raw = value.as_str()?.trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        return Some(dt.timestamp_millis());
    }
    if let Ok(n) = raw.parse::<i64>() {
        return Some(epoch_to_ms(n));
    }
    if let Ok(n) = raw.parse::<f64>() {
        return parse_timestamp_to_ms(&Value::from(n));
    }
    const NAIVE_FORMATS: [&str; 4] = [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M",
    ];
    for format in NAIVE_FORMATS {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(raw, format) {
            return Some(naive.and_utc().timestamp_millis());
        }
    }
    // "2026-03-06 10:17:58+08:00" style (space separator with offset).
    if let Ok(dt) = DateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%.f%:z") {
        return Some(dt.timestamp_millis());
    }
    None
}

pub fn extract_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.to_string(),
        Value::Array(items) => items
            .iter()
            .filter_map(extract_text_from_item)
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(map) => map
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}

fn extract_text_from_item(item: &Value) -> Option<String> {
    let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");

    // tool_use: show tool name
    if item_type == "tool_use" {
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        return Some(format!("[Tool: {name}]"));
    }

    // tool_result: extract nested content
    if item_type == "tool_result" {
        if let Some(content) = item.get("content") {
            let text = extract_text(content);
            if !text.is_empty() {
                return Some(text);
            }
        }
        return None;
    }

    if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
        return Some(text.to_string());
    }

    if let Some(text) = item.get("input_text").and_then(|v| v.as_str()) {
        return Some(text.to_string());
    }

    if let Some(text) = item.get("output_text").and_then(|v| v.as_str()) {
        return Some(text.to_string());
    }

    if let Some(content) = item.get("content") {
        let text = extract_text(content);
        if !text.is_empty() {
            return Some(text);
        }
    }

    None
}

pub fn truncate_summary(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }

    let mut result = trimmed.chars().take(max_chars).collect::<String>();
    result.push_str("...");
    result
}

pub fn path_basename(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalized = trimmed.trim_end_matches(['/', '\\']);
    let last = normalized
        .split(['/', '\\'])
        .next_back()
        .filter(|segment| !segment.is_empty())?;
    Some(last.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    fn write_lines(lines: &[String]) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().expect("tempfile");
        for line in lines {
            writeln!(file, "{line}").expect("write");
        }
        file.flush().expect("flush");
        file
    }

    fn big_line(tag: &str, size: usize) -> String {
        format!("{{\"tag\":\"{tag}\",\"pad\":\"{}\"}}", "x".repeat(size))
    }

    fn locked_db_fixture() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("tool.db");
        let conn = rusqlite::Connection::open(&path).expect("open");
        conn.execute_batch("CREATE TABLE t(x INTEGER); INSERT INTO t VALUES (1), (2);")
            .expect("schema");
        (dir, path)
    }

    /// Hold an exclusive lock from another connection for `hold`, then release.
    fn hold_exclusive_lock(path: &Path, hold: std::time::Duration) -> std::thread::JoinHandle<()> {
        let locker = rusqlite::Connection::open(path).expect("open locker");
        locker.execute_batch("BEGIN EXCLUSIVE;").expect("lock");
        std::thread::spawn(move || {
            std::thread::sleep(hold);
            locker.execute_batch("COMMIT;").expect("unlock");
        })
    }

    #[test]
    fn open_tool_db_read_waits_for_writer_lock() {
        let (_dir, path) = locked_db_fixture();
        let handle = hold_exclusive_lock(&path, std::time::Duration::from_millis(300));

        let conn = open_tool_db(&path, false).expect("open read-only");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM t", [], |row| row.get(0))
            .expect("read succeeds once the lock is released");
        assert_eq!(count, 2);
        assert!(conn.execute("DELETE FROM t", []).is_err(), "read-only");
        handle.join().unwrap();
    }

    #[test]
    fn open_tool_db_write_waits_for_writer_lock() {
        let (_dir, path) = locked_db_fixture();
        let handle = hold_exclusive_lock(&path, std::time::Duration::from_millis(300));

        let conn = open_tool_db(&path, true).expect("open read-write");
        let deleted = conn
            .execute("DELETE FROM t WHERE x = 1", [])
            .expect("delete succeeds once the lock is released");
        assert_eq!(deleted, 1);
        handle.join().unwrap();
    }

    #[test]
    fn open_tool_db_does_not_create_missing_database() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("missing.db");
        assert!(open_tool_db(&path, true).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn tail_returns_final_line_larger_than_initial_window() {
        let last = big_line("last", 40 * 1024);
        let file = write_lines(&["{\"first\":1}".to_string(), last.clone()]);
        let (head, tail) = read_head_tail_lines(file.path(), 1, 30).expect("read");
        assert_eq!(head, vec!["{\"first\":1}".to_string()]);
        assert_eq!(tail.last(), Some(&last));
    }

    #[test]
    fn tail_returns_short_lines_after_a_large_line() {
        let lines = vec![
            big_line("first", 20 * 1024),
            big_line("big", 30 * 1024),
            "{\"n\":1}".to_string(),
            "{\"n\":2}".to_string(),
            "{\"n\":3}".to_string(),
        ];
        let file = write_lines(&lines);
        let (_, tail) = read_head_tail_lines(file.path(), 1, 3).expect("read");
        assert_eq!(tail, lines[2..].to_vec());

        let (_, tail) = read_head_tail_lines(file.path(), 1, 30).expect("read");
        assert_eq!(tail, lines, "growing window reaches file start");
    }

    #[test]
    fn tail_window_starting_on_line_boundary_keeps_first_line() {
        // Window boundary falls exactly after a newline: the first line in the
        // window is complete and must not be dropped.
        let lines = vec!["aaaa".to_string(), "bbbb".to_string(), "cccc".to_string()];
        let file = write_lines(&lines);
        let len = std::fs::metadata(file.path()).unwrap().len();
        let tail = read_tail_lines_with_limits(file.path(), len, 2, 10, 10).expect("read");
        assert_eq!(tail, vec!["bbbb".to_string(), "cccc".to_string()]);
    }

    #[test]
    fn tail_respects_max_window() {
        let lines = vec!["short".to_string(), "y".repeat(100)];
        let file = write_lines(&lines);
        let len = std::fs::metadata(file.path()).unwrap().len();
        let tail = read_tail_lines_with_limits(file.path(), len, 1, 8, 32).expect("read");
        assert!(tail.is_empty(), "line above the cap is not returned");
    }

    #[test]
    fn parse_timestamp_to_ms_supports_integers_and_rfc3339() {
        assert_eq!(
            parse_timestamp_to_ms(&json!(1_771_061_953_033_i64)),
            Some(1_771_061_953_033)
        );
        assert_eq!(
            parse_timestamp_to_ms(&json!(1_771_061_953_i64)),
            Some(1_771_061_953_000)
        );
        assert_eq!(
            parse_timestamp_to_ms(&json!("1970-01-01T00:00:01Z")),
            Some(1_000)
        );
    }

    #[test]
    fn parse_timestamp_accepts_naive_sqlite_and_scaled_integers() {
        let expected = 1_772_792_278_000_i64; // 2026-03-06T10:17:58Z
        assert_eq!(
            parse_timestamp_to_ms(&json!("2026-03-06T10:17:58")),
            Some(expected),
            "naive ISO is treated as UTC"
        );
        assert_eq!(
            parse_timestamp_to_ms(&json!("2026-03-06 10:17:58")),
            Some(expected),
            "SQLite datetime('now') format"
        );
        assert_eq!(
            parse_timestamp_to_ms(&json!("2026-03-06 10:17:58.250")),
            Some(expected + 250)
        );
        assert_eq!(
            parse_timestamp_to_ms(&json!(expected * 1000)),
            Some(expected),
            "microseconds"
        );
        assert_eq!(
            parse_timestamp_to_ms(&json!(expected * 1_000_000)),
            Some(expected),
            "nanoseconds"
        );
        assert_eq!(parse_timestamp_to_ms(&json!("1772792278")), Some(expected));
        assert_eq!(
            parse_timestamp_to_ms(&json!(1_772_792_278.5_f64)),
            Some(expected + 500)
        );
        assert_eq!(parse_timestamp_to_ms(&json!("not a date")), None);
    }
}
