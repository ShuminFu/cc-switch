//! Background usage-log writer.
//!
//! Proxy requests used to write their usage row inline: spawn a blocking task,
//! take the global SQLite mutex, look up pricing, INSERT, release. Under load
//! that meant one lock round-trip and one fsync'd transaction per request, all
//! competing with dashboard queries and provider switches.
//!
//! Instead, requests hand a [`UsageLogEntry`] to a bounded channel (never
//! blocking the caller) and a single dedicated thread drains it: it collects
//! up to [`MAX_BATCH`] entries or waits at most [`FLUSH_INTERVAL`] after the
//! first one, then takes the connection lock once, prices every entry on that
//! connection and inserts the whole batch in one transaction.
//!
//! Durability: [`UsageLogWriter::flush`] waits until everything enqueued so far
//! is committed (used on app exit and by tests), and dropping the writer (when
//! the owning `Database` is dropped) drains the channel before the thread
//! exits. If the channel is ever full the entry is dropped with a warning
//! rather than stalling the proxy.

use super::logger::{
    insert_request_log, price_proxy_usage, resolve_pricing_config_on_conn, RequestLog,
    UsageLogEntry,
};
use crate::error::AppError;
use rusqlite::Connection;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Entries that can wait in the channel before new ones are dropped. At a few
/// hundred bytes each this bounds the queue to a few MB, and it is far more
/// than the writer falls behind by in practice (it drains a batch per lock).
pub(crate) const CHANNEL_CAPACITY: usize = 8192;
/// Maximum number of rows written in one transaction.
pub(crate) const MAX_BATCH: usize = 256;
/// Longest time an entry waits for companions before its batch is written.
pub(crate) const FLUSH_INTERVAL: Duration = Duration::from_millis(250);

enum Msg {
    Entry(Box<UsageLogEntry>),
    /// Write everything received so far, then acknowledge.
    Flush(SyncSender<()>),
}

/// Outcome of [`UsageLogWriter::enqueue`].
#[derive(Debug)]
pub(crate) enum Enqueued {
    /// Accepted; it will be written in the next batch.
    Queued,
    /// The channel was full; the entry was discarded (and logged).
    Dropped,
    /// The writer thread is gone (it panicked); the caller gets the entry
    /// back to write it some other way.
    WriterGone(Box<UsageLogEntry>),
}

pub(crate) struct UsageLogWriter {
    tx: Option<SyncSender<Msg>>,
    thread: Option<JoinHandle<()>>,
    dropped: AtomicU64,
}

impl UsageLogWriter {
    /// Start the writer thread on `conn` (the database's writer connection).
    pub(crate) fn spawn(conn: Arc<Mutex<Connection>>) -> std::io::Result<Self> {
        Self::spawn_with_capacity(conn, CHANNEL_CAPACITY)
    }

    pub(crate) fn spawn_with_capacity(
        conn: Arc<Mutex<Connection>>,
        capacity: usize,
    ) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::sync_channel(capacity);
        let thread = std::thread::Builder::new()
            .name("usage-log-writer".to_string())
            .spawn(move || run(&conn, &rx))?;
        Ok(Self {
            tx: Some(tx),
            thread: Some(thread),
            dropped: AtomicU64::new(0),
        })
    }

    /// Queue an entry without blocking.
    pub(crate) fn enqueue(&self, entry: UsageLogEntry) -> Enqueued {
        let Some(tx) = &self.tx else {
            return Enqueued::WriterGone(Box::new(entry));
        };
        match tx.try_send(Msg::Entry(Box::new(entry))) {
            Ok(()) => Enqueued::Queued,
            Err(TrySendError::Full(_)) => {
                let n = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
                if n == 1 || n % 1000 == 0 {
                    log::warn!(
                        "[USG-004] 使用量日志写入队列已满（容量 {}），已丢弃 {n} 条日志",
                        CHANNEL_CAPACITY
                    );
                }
                Enqueued::Dropped
            }
            Err(TrySendError::Disconnected(Msg::Entry(entry))) => Enqueued::WriterGone(entry),
            Err(TrySendError::Disconnected(Msg::Flush(_))) => unreachable!("sent an entry"),
        }
    }

    /// Number of entries discarded because the channel was full.
    #[cfg(test)]
    pub(crate) fn dropped_count(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Block until every entry enqueued before this call is committed, or
    /// `timeout` elapses. Returns whether the flush completed.
    pub(crate) fn flush(&self, timeout: Duration) -> bool {
        let Some(tx) = &self.tx else {
            return true;
        };
        let (ack_tx, ack_rx) = mpsc::sync_channel(1);
        // `send` (not `try_send`): a flush must not be dropped when the queue
        // is full; it waits for room behind the entries it has to cover.
        if tx.send(Msg::Flush(ack_tx)).is_err() {
            return false;
        }
        ack_rx.recv_timeout(timeout).is_ok()
    }

    /// Stop accepting entries, write everything still queued and join the
    /// thread. Idempotent.
    pub(crate) fn shutdown(&mut self) {
        // Closing the channel lets the thread drain what is buffered and exit.
        self.tx.take();
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                log::error!("[USG-004] 使用量日志写线程异常退出");
            }
        }
    }
}

impl Drop for UsageLogWriter {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run(conn: &Mutex<Connection>, rx: &Receiver<Msg>) {
    let mut batch: Vec<UsageLogEntry> = Vec::with_capacity(MAX_BATCH);
    let mut acks: Vec<SyncSender<()>> = Vec::new();

    // `recv` fails only once every sender is gone and the buffer is empty,
    // so all queued entries are written before the thread exits.
    while let Ok(first) = rx.recv() {
        let mut disconnected = false;
        push(first, &mut batch, &mut acks);

        let deadline = Instant::now() + FLUSH_INTERVAL;
        while batch.len() < MAX_BATCH && acks.is_empty() {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            match rx.recv_timeout(remaining) {
                Ok(msg) => push(msg, &mut batch, &mut acks),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }

        if !batch.is_empty() {
            write_batch(conn, std::mem::take(&mut batch));
        }
        for ack in acks.drain(..) {
            let _ = ack.send(());
        }
        if disconnected {
            break;
        }
    }
}

fn push(msg: Msg, batch: &mut Vec<UsageLogEntry>, acks: &mut Vec<SyncSender<()>>) {
    match msg {
        Msg::Entry(entry) => batch.push(*entry),
        Msg::Flush(ack) => acks.push(ack),
    }
}

/// Price and insert `entries` in one transaction on the writer connection.
/// Returns how many rows were written. Also used as a synchronous fallback
/// when no writer thread is available.
pub(crate) fn write_batch(conn: &Mutex<Connection>, entries: Vec<UsageLogEntry>) -> usize {
    if entries.is_empty() {
        return 0;
    }
    // A panic elsewhere while holding the lock must not make us lose logs
    // forever; the connection itself is still usable.
    let mut guard = conn.lock().unwrap_or_else(PoisonError::into_inner);

    let written = match write_in_transaction(&mut guard, &entries) {
        Ok(n) => n,
        Err(e) => {
            // BEGIN/COMMIT failed (e.g. SQLITE_BUSY past busy_timeout): the
            // transaction rolled back as a whole, so retry row by row.
            log::warn!("[USG-001] 批量写入使用量日志失败，逐条重试: {e}");
            let mut cache = PricingConfigCache::default();
            entries
                .iter()
                .filter(|entry| write_entry(&guard, entry, &mut cache).is_ok())
                .count()
        }
    };
    drop(guard);

    if written > 0 {
        // 通知前端使用统计有更新（200ms 防抖合并）；在提交之后发出，
        // 保证前端重新查询时（只读连接）能看到这些行。
        crate::usage_events::notify_log_recorded();
    }
    written
}

fn write_in_transaction(
    conn: &mut Connection,
    entries: &[UsageLogEntry],
) -> Result<usize, AppError> {
    let tx = conn
        .transaction()
        .map_err(|e| AppError::Database(format!("开启事务失败: {e}")))?;
    let mut cache = PricingConfigCache::default();
    // A failing statement only rolls back itself, not the transaction, so a
    // single bad row doesn't cost the rest of the batch.
    let written = entries
        .iter()
        .filter(|entry| write_entry(&tx, entry, &mut cache).is_ok())
        .count();
    tx.commit()
        .map_err(|e| AppError::Database(format!("提交事务失败: {e}")))?;
    Ok(written)
}

/// Pricing config per (provider_id, app_type), resolved once per batch.
#[derive(Default)]
struct PricingConfigCache(HashMap<(String, String), (Decimal, String)>);

impl PricingConfigCache {
    fn get(&mut self, conn: &Connection, provider_id: &str, app_type: &str) -> (Decimal, String) {
        self.0
            .entry((provider_id.to_string(), app_type.to_string()))
            .or_insert_with(|| resolve_pricing_config_on_conn(conn, provider_id, app_type))
            .clone()
    }
}

fn write_entry(
    conn: &Connection,
    entry: &UsageLogEntry,
    cache: &mut PricingConfigCache,
) -> Result<(), AppError> {
    let result = build_row(conn, entry, cache).and_then(|row| insert_request_log(conn, &row));
    if let Err(e) = &result {
        log::warn!("[USG-001] 记录使用量失败: {e}");
    }
    result
}

fn build_row(
    conn: &Connection,
    entry: &UsageLogEntry,
    cache: &mut PricingConfigCache,
) -> Result<RequestLog, AppError> {
    match entry {
        UsageLogEntry::Ready(log) => Ok(log.clone()),
        UsageLogEntry::Proxy(usage) => {
            let config = cache.get(conn, &usage.provider_id, &usage.app_type);
            price_proxy_usage(conn, usage.clone(), config)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::proxy::usage::logger::ProxyUsage;
    use crate::proxy::usage::parser::TokenUsage;

    fn error_entry(id: &str) -> UsageLogEntry {
        UsageLogEntry::Ready(RequestLog::error(
            id.to_string(),
            "p1".to_string(),
            "claude".to_string(),
            "m".to_string(),
            500,
            "boom".to_string(),
            1,
            false,
            None,
            None,
        ))
    }

    fn count_logs(db: &Database) -> i64 {
        let conn = db.conn.lock().expect("lock");
        conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |r| r.get(0))
            .expect("count")
    }

    #[test]
    fn enqueued_logs_are_written_in_batches_and_flush_waits_for_commit() {
        let db = Database::memory().expect("db");
        let n = MAX_BATCH * 2 + 7;
        for i in 0..n {
            db.enqueue_usage_log(error_entry(&format!("req-{i}")));
        }
        assert!(db.flush_usage_logs(Duration::from_secs(10)));
        assert_eq!(count_logs(&db), n as i64);
    }

    #[test]
    fn batch_is_written_after_flush_interval_without_explicit_flush() {
        let db = Database::memory().expect("db");
        db.enqueue_usage_log(error_entry("lonely"));
        let deadline = Instant::now() + Duration::from_secs(5);
        while count_logs(&db) == 0 {
            assert!(Instant::now() < deadline, "entry was never written");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn proxy_usage_is_priced_in_the_writer() {
        let db = Database::memory().expect("db");
        {
            let conn = db.conn.lock().expect("lock");
            conn.execute(
                "INSERT OR REPLACE INTO model_pricing (model_id, display_name, input_cost_per_million, output_cost_per_million)
                 VALUES ('writer-model', 'Writer Model', '3.0', '0')",
                [],
            )
            .expect("seed pricing");
        }
        db.enqueue_usage_log(UsageLogEntry::Proxy(ProxyUsage {
            request_id: "priced".to_string(),
            provider_id: "nope".to_string(),
            app_type: "claude".to_string(),
            model: "writer-model".to_string(),
            request_model: "client-model".to_string(),
            outbound_model: "outbound-model".to_string(),
            usage: TokenUsage {
                input_tokens: 1_000_000,
                ..TokenUsage::default()
            },
            latency_ms: 1,
            first_token_ms: None,
            status_code: 200,
            session_id: None,
            provider_type: None,
            is_streaming: false,
        }));
        assert!(db.flush_usage_logs(Duration::from_secs(10)));

        let conn = db.conn.lock().expect("lock");
        let (total, pricing_model): (String, String) = conn
            .query_row(
                "SELECT total_cost_usd, pricing_model FROM proxy_request_logs WHERE request_id = 'priced'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("row");
        assert_eq!(pricing_model, "writer-model");
        assert_eq!(total.parse::<Decimal>().expect("decimal"), Decimal::from(3));
    }

    #[test]
    fn dropping_the_database_drains_queued_logs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("cc-switch.db");
        {
            let db = Database::open_at(&path).expect("open");
            for i in 0..100 {
                db.enqueue_usage_log(error_entry(&format!("shutdown-{i}")));
            }
            // No flush: graceful drop must still persist everything.
        }
        let db = Database::open_at(&path).expect("reopen");
        assert_eq!(count_logs(&db), 100);
    }

    #[test]
    fn full_channel_drops_with_count_instead_of_blocking() {
        let db = Database::memory().expect("db");
        let capacity = 4;
        let writer =
            UsageLogWriter::spawn_with_capacity(Arc::clone(&db.conn), capacity).expect("spawn");

        // Stall the writer on the connection lock (like a long transaction).
        let guard = db.conn.lock().expect("hold writer lock");
        // The first entry is taken off the channel by the thread, which then
        // blocks on the lock; give it a moment, then fill the channel.
        assert!(matches!(
            writer.enqueue(error_entry("first")),
            Enqueued::Queued
        ));
        std::thread::sleep(FLUSH_INTERVAL + Duration::from_millis(100));
        let mut queued = 0;
        let mut dropped = 0;
        let started = Instant::now();
        for i in 0..capacity + 3 {
            match writer.enqueue(error_entry(&format!("fill-{i}"))) {
                Enqueued::Queued => queued += 1,
                Enqueued::Dropped => dropped += 1,
                Enqueued::WriterGone(_) => panic!("writer should be alive"),
            }
        }
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "enqueue must never block"
        );
        assert_eq!(queued, capacity);
        assert_eq!(dropped, 3);
        assert_eq!(writer.dropped_count(), 3);

        drop(guard);
        assert!(writer.flush(Duration::from_secs(10)));
        assert_eq!(count_logs(&db), 1 + capacity as i64);
    }

    #[test]
    fn writes_on_the_writer_do_not_wait_for_a_long_read() {
        use std::sync::mpsc;

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("cc-switch.db");
        let db = Arc::new(Database::open_at(&path).expect("open"));

        // Hold a long read open on the read pool.
        let (started_tx, started_rx) = mpsc::channel::<()>();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let reader_db = Arc::clone(&db);
        let reader = std::thread::spawn(move || {
            let conn = reader_db.read_conn().expect("read conn");
            let mut stmt = conn
                .prepare("WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c) SELECT x FROM c")
                .expect("prepare");
            let mut rows = stmt.query([]).expect("query");
            rows.next().expect("step").expect("row");
            started_tx.send(()).expect("started");
            release_rx
                .recv_timeout(Duration::from_secs(20))
                .expect("released");
        });
        started_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("reader started");

        let started = Instant::now();
        for i in 0..500 {
            db.enqueue_usage_log(error_entry(&format!("concurrent-{i}")));
        }
        assert!(db.flush_usage_logs(Duration::from_secs(10)));
        let elapsed = started.elapsed();
        release_tx.send(()).expect("release");
        reader.join().expect("reader");

        assert_eq!(count_logs(&db), 500);
        assert!(
            elapsed < Duration::from_secs(3),
            "500 queued logs took {elapsed:?} while a read was running"
        );
    }
}
