//! Command-level timing (CMD-30).
//!
//! Commands wrap their body with [`timed`] (or the blocking-pool helpers
//! `timed_blocking` / `timed_blocking_with_state`, which take the command name
//! as their first argument). Each call gets a process-unique request id and is
//! logged as `cmd=<name> id=<n> dur_ms=<ms> ok=<bool>` at `debug` level, or at
//! `warn` when it takes longer than [`SLOW_COMMAND_MS`]. Durations are kept in
//! a small per-command ring buffer so `get_command_stats` can report p50/p95.
//!
//! Only the command name, id, duration and outcome are logged: never the
//! arguments or the error text (they may contain secrets).

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

/// Calls slower than this are logged at `warn`.
pub const SLOW_COMMAND_MS: u128 = 500;
/// Samples kept per command for percentiles.
const SAMPLES_PER_COMMAND: usize = 256;

static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Default)]
struct Samples {
    count: u64,
    errors: u64,
    max_ms: u64,
    recent: Vec<u64>,
    next: usize,
}

static STATS: Mutex<Option<HashMap<&'static str, Samples>>> = Mutex::new(None);

/// Whether a command's return value counts as success.
pub trait CommandOutcome {
    fn is_success(&self) -> bool;
}

impl<T, E> CommandOutcome for Result<T, E> {
    fn is_success(&self) -> bool {
        self.is_ok()
    }
}

/// Aggregated timing for one command.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommandStat {
    pub name: String,
    pub count: u64,
    pub errors: u64,
    pub p50_ms: u64,
    pub p95_ms: u64,
    pub max_ms: u64,
}

fn record(name: &'static str, duration: Duration, ok: bool) {
    let ms = duration.as_millis().min(u128::from(u64::MAX)) as u64;
    let Ok(mut guard) = STATS.lock() else {
        return;
    };
    let entry = guard
        .get_or_insert_with(HashMap::new)
        .entry(name)
        .or_default();
    entry.count += 1;
    if !ok {
        entry.errors += 1;
    }
    entry.max_ms = entry.max_ms.max(ms);
    if entry.recent.len() < SAMPLES_PER_COMMAND {
        entry.recent.push(ms);
    } else {
        entry.recent[entry.next] = ms;
    }
    entry.next = (entry.next + 1) % SAMPLES_PER_COMMAND;
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = ((p / 100.0) * (sorted.len() as f64)).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

/// Per-command statistics, slowest p95 first.
pub fn snapshot() -> Vec<CommandStat> {
    let Ok(guard) = STATS.lock() else {
        return Vec::new();
    };
    let mut stats: Vec<CommandStat> = guard
        .iter()
        .flat_map(|map| map.iter())
        .map(|(name, samples)| {
            let mut sorted = samples.recent.clone();
            sorted.sort_unstable();
            CommandStat {
                name: (*name).to_string(),
                count: samples.count,
                errors: samples.errors,
                p50_ms: percentile(&sorted, 50.0),
                p95_ms: percentile(&sorted, 95.0),
                max_ms: samples.max_ms,
            }
        })
        .collect();
    stats.sort_by(|a, b| b.p95_ms.cmp(&a.p95_ms).then(a.name.cmp(&b.name)));
    stats
}

/// Log and record one finished call.
fn finish(name: &'static str, id: u64, started: Instant, ok: bool) {
    let elapsed = started.elapsed();
    let ms = elapsed.as_millis();
    if ms > SLOW_COMMAND_MS {
        log::warn!("cmd={name} id={id} dur_ms={ms} ok={ok} slow=true");
    } else {
        log::debug!("cmd={name} id={id} dur_ms={ms} ok={ok}");
    }
    record(name, elapsed, ok);
}

/// Run a command body, recording its duration and outcome under `name`.
pub async fn timed<R, F>(name: &'static str, fut: F) -> R
where
    F: Future<Output = R>,
    R: CommandOutcome,
{
    let id = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    let started = Instant::now();
    let result = fut.await;
    finish(name, id, started, result.is_success());
    result
}

/// Synchronous counterpart of [`timed`].
pub fn timed_sync<R, F>(name: &'static str, f: F) -> R
where
    F: FnOnce() -> R,
    R: CommandOutcome,
{
    let id = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    let started = Instant::now();
    let result = f();
    finish(name, id, started, result.is_success());
    result
}

/// Timing of the commands called so far in this process (CMD-30), slowest
/// p95 first. Debug aid for the "backend took 3 s" class of reports.
#[tauri::command]
pub fn get_command_stats() -> Vec<CommandStat> {
    snapshot()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn timed_records_count_errors_and_percentiles() {
        let name = "test_cmd_timed";
        for _ in 0..3 {
            let r: Result<u8, String> = timed(name, async { Ok(1) }).await;
            assert_eq!(r, Ok(1));
        }
        let r: Result<u8, String> = timed_sync(name, || Err("x".to_string()));
        assert!(r.is_err());
        let stat = snapshot().into_iter().find(|s| s.name == name).unwrap();
        assert_eq!((stat.count, stat.errors), (4, 1));
        assert!(stat.p95_ms >= stat.p50_ms);
    }

    #[test]
    fn percentile_uses_nearest_rank() {
        let v: Vec<u64> = (1..=100).collect();
        assert_eq!(percentile(&v, 50.0), 50);
        assert_eq!(percentile(&v, 95.0), 95);
        assert_eq!(percentile(&[], 95.0), 0);
        assert_eq!(percentile(&[7], 95.0), 7);
    }

    #[test]
    fn ring_buffer_keeps_the_latest_samples() {
        let name = "test_cmd_ring";
        for _ in 0..SAMPLES_PER_COMMAND + 10 {
            record(name, Duration::from_millis(1), true);
        }
        let guard = STATS.lock().unwrap();
        let samples = &guard.as_ref().unwrap()[name];
        assert_eq!(samples.recent.len(), SAMPLES_PER_COMMAND);
        assert_eq!(samples.count as usize, SAMPLES_PER_COMMAND + 10);
    }
}
