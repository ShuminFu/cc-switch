//! 使用量写入任务（usage ingestion writer）
//!
//! 代理请求路径不再直接执行 SQLite：每条使用量记录被封装成一个 [`WriteJob`]，
//! 通过**有界**通道发送给一个专用写线程。写线程：
//!
//! - 阻塞等待第一个任务，然后把通道里已经排队的任务一并取出（最多
//!   [`MAX_BATCH`] 个），在**一个事务**里执行（group commit）；每个任务包在
//!   自己的 SAVEPOINT 里，单个任务失败只回滚它自己，不影响同批其它记录；
//! - 任务内部使用 `prepare_cached`，语句在同一连接上只编译一次；
//! - 提交后统一触发一次 `usage-log-recorded` 前端通知；
//! - 通道满时丢弃新任务并计数（`dropped`），绝不阻塞请求路径；
//! - [`UsageWriter::flush`] 以同步方式等待此前入队的所有任务落库（测试与退出时使用）；
//! - `Drop` 时关闭通道、等待写线程把剩余任务写完再退出。
//!
//! 写线程与其它调用方共享同一个主连接（`Arc<Mutex<Connection>>`），因此：
//! 单写者不变量、`update_hook`（WebDAV/S3 自动同步）以及备份/恢复对主连接的原地
//! 替换都保持不变；变化只是锁在写线程上获取，而不是在 Tokio worker 上。

use crate::error::AppError;
use rusqlite::Connection;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

/// 通道容量：超过即丢弃（并计数），保证请求路径永不阻塞。
pub(crate) const CHANNEL_CAPACITY: usize = 4096;
/// 每个事务最多合并的任务数，限制单次持锁时间。
pub(crate) const MAX_BATCH: usize = 256;
/// 退出时等待剩余任务落库的上限。
pub(crate) const SHUTDOWN_FLUSH_TIMEOUT: Duration = Duration::from_secs(3);

type JobFn = Box<dyn FnOnce(&Connection) -> Result<(), AppError> + Send + 'static>;

/// 一个写任务：在写线程上、批事务内执行的闭包。
pub(crate) struct WriteJob {
    /// 日志标签（失败时输出）
    label: &'static str,
    /// 提交后是否需要通知前端使用统计有更新
    notify_usage: bool,
    run: JobFn,
}

impl WriteJob {
    pub(crate) fn new(
        label: &'static str,
        run: impl FnOnce(&Connection) -> Result<(), AppError> + Send + 'static,
    ) -> Self {
        Self {
            label,
            notify_usage: false,
            run: Box::new(run),
        }
    }

    /// 标记该任务写入了 `proxy_request_logs`，提交后触发前端刷新事件。
    pub(crate) fn notify_usage(mut self) -> Self {
        self.notify_usage = true;
        self
    }
}

enum Msg {
    Job(WriteJob),
    Flush(SyncSender<()>),
}

/// 写线程统计（单调递增计数器）。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct UsageWriterStats {
    /// 成功入队的任务数
    pub enqueued: u64,
    /// 成功写入（随事务提交）的任务数
    pub written: u64,
    /// 执行失败（其 SAVEPOINT 被回滚，或整批提交失败）的任务数
    pub failed: u64,
    /// 因通道已满而被丢弃的任务数
    pub dropped: u64,
    /// 提交的事务（批）数
    pub batches: u64,
}

#[derive(Default)]
struct Counters {
    enqueued: AtomicU64,
    written: AtomicU64,
    failed: AtomicU64,
    dropped: AtomicU64,
    batches: AtomicU64,
}

impl Counters {
    fn snapshot(&self) -> UsageWriterStats {
        UsageWriterStats {
            enqueued: self.enqueued.load(Ordering::Relaxed),
            written: self.written.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
            dropped: self.dropped.load(Ordering::Relaxed),
            batches: self.batches.load(Ordering::Relaxed),
        }
    }
}

struct Running {
    tx: SyncSender<Msg>,
    handle: JoinHandle<()>,
}

/// 写线程句柄。线程在第一次入队时惰性启动。
pub(crate) struct UsageWriter {
    conn: Arc<Mutex<Connection>>,
    capacity: usize,
    counters: Arc<Counters>,
    running: OnceLock<Mutex<Option<Running>>>,
}

impl UsageWriter {
    pub(crate) fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self::with_capacity(conn, CHANNEL_CAPACITY)
    }

    pub(crate) fn with_capacity(conn: Arc<Mutex<Connection>>, capacity: usize) -> Self {
        Self {
            conn,
            capacity: capacity.max(1),
            counters: Arc::new(Counters::default()),
            running: OnceLock::new(),
        }
    }

    pub(crate) fn stats(&self) -> UsageWriterStats {
        self.counters.snapshot()
    }

    fn sender(&self) -> Result<SyncSender<Msg>, AppError> {
        let slot = self.running.get_or_init(|| Mutex::new(None));
        let mut guard = slot
            .lock()
            .map_err(|e| AppError::Database(format!("usage writer lock poisoned: {e}")))?;
        if let Some(running) = guard.as_ref() {
            return Ok(running.tx.clone());
        }
        let (tx, rx) = mpsc::sync_channel::<Msg>(self.capacity);
        let conn = Arc::clone(&self.conn);
        let counters = Arc::clone(&self.counters);
        let handle = std::thread::Builder::new()
            .name("cc-switch-usage-writer".to_string())
            .spawn(move || writer_loop(conn, rx, counters))
            .map_err(|e| AppError::Database(format!("启动使用量写入线程失败: {e}")))?;
        *guard = Some(Running {
            tx: tx.clone(),
            handle,
        });
        Ok(tx)
    }

    /// 非阻塞入队。通道已满时丢弃并返回错误（调用方只需记录 warn）。
    pub(crate) fn enqueue(&self, job: WriteJob) -> Result<(), AppError> {
        let tx = self.sender()?;
        match tx.try_send(Msg::Job(job)) {
            Ok(()) => {
                self.counters.enqueued.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
            Err(TrySendError::Full(_)) => {
                let dropped = self.counters.dropped.fetch_add(1, Ordering::Relaxed) + 1;
                Err(AppError::Database(format!(
                    "使用量写入队列已满（容量 {}），已丢弃该记录（累计丢弃 {dropped} 条）",
                    self.capacity
                )))
            }
            Err(TrySendError::Disconnected(_)) => {
                Err(AppError::Database("使用量写入线程已退出".to_string()))
            }
        }
    }

    /// 同步等待此前所有已入队任务提交（或超时）。写线程未启动时立即返回。
    pub(crate) fn flush(&self, timeout: Duration) -> Result<(), AppError> {
        let tx = {
            let Some(slot) = self.running.get() else {
                return Ok(());
            };
            let guard = slot
                .lock()
                .map_err(|e| AppError::Database(format!("usage writer lock poisoned: {e}")))?;
            match guard.as_ref() {
                Some(running) => running.tx.clone(),
                None => return Ok(()),
            }
        };
        let (ack_tx, ack_rx) = mpsc::sync_channel::<()>(1);
        // Flush 消息可以阻塞等待通道空位：只有测试和退出路径会调用它。
        tx.send(Msg::Flush(ack_tx))
            .map_err(|_| AppError::Database("使用量写入线程已退出".to_string()))?;
        match ack_rx.recv_timeout(timeout) {
            Ok(()) => Ok(()),
            Err(RecvTimeoutError::Timeout) => Err(AppError::Database(format!(
                "等待使用量写入完成超时（{} ms）",
                timeout.as_millis()
            ))),
            Err(RecvTimeoutError::Disconnected) => {
                Err(AppError::Database("使用量写入线程已退出".to_string()))
            }
        }
    }
}

impl Drop for UsageWriter {
    fn drop(&mut self) {
        let Some(slot) = self.running.get_mut() else {
            return;
        };
        let running = match slot.get_mut() {
            Ok(r) => r.take(),
            Err(e) => e.into_inner().take(),
        };
        if let Some(Running { tx, handle }) = running {
            // 关闭通道：写线程把剩余任务写完后自行退出
            drop(tx);
            if handle.join().is_err() {
                log::warn!("[usage-writer] 写线程异常退出");
            }
        }
    }
}

fn writer_loop(conn: Arc<Mutex<Connection>>, rx: Receiver<Msg>, counters: Arc<Counters>) {
    let mut batch: Vec<WriteJob> = Vec::with_capacity(MAX_BATCH);
    let mut acks: Vec<SyncSender<()>> = Vec::new();

    // recv() 只有在所有发送端都已关闭且通道清空后才返回 Err，保证退出前写完剩余任务
    while let Ok(first) = rx.recv() {
        push_msg(first, &mut batch, &mut acks);
        // group commit：顺带取出所有已排队任务；遇到 Flush 先结束本批
        while batch.len() < MAX_BATCH && acks.is_empty() {
            match rx.try_recv() {
                Ok(msg) => push_msg(msg, &mut batch, &mut acks),
                Err(_) => break,
            }
        }

        if !batch.is_empty() {
            write_batch(&conn, &mut batch, &counters);
        }
        for ack in acks.drain(..) {
            let _ = ack.send(());
        }
    }
}

fn push_msg(msg: Msg, batch: &mut Vec<WriteJob>, acks: &mut Vec<SyncSender<()>>) {
    match msg {
        Msg::Job(job) => batch.push(job),
        Msg::Flush(ack) => acks.push(ack),
    }
}

fn write_batch(conn: &Mutex<Connection>, batch: &mut Vec<WriteJob>, counters: &Counters) {
    let total = batch.len() as u64;
    let mut guard = match conn.lock() {
        Ok(g) => g,
        Err(e) => {
            log::error!("[usage-writer] 数据库锁已毒化，丢弃 {total} 条记录: {e}");
            counters.failed.fetch_add(total, Ordering::Relaxed);
            batch.clear();
            return;
        }
    };

    let mut tx = match guard.transaction() {
        Ok(tx) => tx,
        Err(e) => {
            log::error!("[usage-writer] 开启事务失败，丢弃 {total} 条记录: {e}");
            counters.failed.fetch_add(total, Ordering::Relaxed);
            batch.clear();
            return;
        }
    };

    let mut ok = 0u64;
    let mut failed = 0u64;
    let mut notify = false;
    for job in batch.drain(..) {
        let WriteJob {
            label,
            notify_usage,
            run,
        } = job;
        let result = match tx.savepoint() {
            Ok(sp) => match run(&sp) {
                Ok(()) => sp
                    .commit()
                    .map_err(|e| AppError::Database(format!("释放 SAVEPOINT 失败: {e}"))),
                // Savepoint 在 drop 时回滚
                Err(e) => Err(e),
            },
            Err(e) => Err(AppError::Database(format!("创建 SAVEPOINT 失败: {e}"))),
        };
        match result {
            Ok(()) => {
                ok += 1;
                notify |= notify_usage;
            }
            Err(e) => {
                failed += 1;
                log::warn!("[usage-writer] 写入任务 {label} 失败: {e}");
            }
        }
    }

    match tx.commit() {
        Ok(()) => {
            counters.written.fetch_add(ok, Ordering::Relaxed);
            counters.failed.fetch_add(failed, Ordering::Relaxed);
            counters.batches.fetch_add(1, Ordering::Relaxed);
            drop(guard);
            if notify {
                crate::usage_events::notify_log_recorded();
            }
        }
        Err(e) => {
            log::error!("[usage-writer] 提交批事务失败，{total} 条记录未写入: {e}");
            counters.failed.fetch_add(total, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> Arc<Mutex<Connection>> {
        let conn = Connection::open_in_memory().expect("open memory db");
        conn.execute_batch("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT NOT NULL);")
            .expect("create table");
        Arc::new(Mutex::new(conn))
    }

    fn insert_job(id: i64) -> WriteJob {
        WriteJob::new("test-insert", move |conn| {
            conn.prepare_cached("INSERT INTO t (id, v) VALUES (?1, 'x')")
                .and_then(|mut stmt| stmt.execute([id]))
                .map(|_| ())
                .map_err(|e| AppError::Database(e.to_string()))
        })
    }

    fn count(conn: &Mutex<Connection>) -> i64 {
        conn.lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM t", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn flush_without_started_writer_is_noop() {
        let writer = UsageWriter::new(test_conn());
        writer.flush(Duration::from_secs(1)).expect("flush");
        assert_eq!(writer.stats(), UsageWriterStats::default());
    }

    #[test]
    fn flush_makes_enqueued_rows_visible() {
        let conn = test_conn();
        let writer = UsageWriter::new(Arc::clone(&conn));
        for id in 0..10 {
            writer.enqueue(insert_job(id)).expect("enqueue");
        }
        writer.flush(Duration::from_secs(5)).expect("flush");
        assert_eq!(count(&conn), 10);
        let stats = writer.stats();
        assert_eq!(stats.enqueued, 10);
        assert_eq!(stats.written, 10);
        assert_eq!(stats.failed, 0);
    }

    /// 入队一个会阻塞写线程的任务，返回后写线程确定正在执行它；
    /// 向返回的 Sender 发送 () 放行。
    fn block_writer(writer: &UsageWriter) -> SyncSender<()> {
        let (started_tx, started_rx) = mpsc::sync_channel::<()>(1);
        let (release_tx, release_rx) = mpsc::sync_channel::<()>(1);
        writer
            .enqueue(WriteJob::new("blocker", move |_conn| {
                let _ = started_tx.send(());
                let _ = release_rx.recv();
                Ok(())
            }))
            .expect("enqueue blocker");
        started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("writer started blocker");
        release_tx
    }

    #[test]
    fn queued_jobs_are_coalesced_into_one_transaction() {
        let conn = test_conn();
        let writer = UsageWriter::new(Arc::clone(&conn));
        let release = block_writer(&writer);
        for id in 0..200 {
            writer.enqueue(insert_job(id)).expect("enqueue");
        }
        release.send(()).unwrap();
        writer.flush(Duration::from_secs(5)).expect("flush");

        assert_eq!(count(&conn), 200);
        let stats = writer.stats();
        // 阻塞任务一批 + 排队期间到达的 200 条合并为一批
        assert_eq!(stats.batches, 2, "expected group commit: {stats:?}");
        assert_eq!(stats.written, 201);
    }

    #[test]
    fn failing_job_rolls_back_only_itself() {
        let conn = test_conn();
        let writer = UsageWriter::new(Arc::clone(&conn));
        writer.enqueue(insert_job(1)).unwrap();
        // 主键冲突 -> 失败
        writer.enqueue(insert_job(1)).unwrap();
        writer
            .enqueue(WriteJob::new("partial", |conn| {
                conn.execute("INSERT INTO t (id, v) VALUES (2, 'y')", [])
                    .map_err(|e| AppError::Database(e.to_string()))?;
                Err(AppError::Database("boom".to_string()))
            }))
            .unwrap();
        writer.enqueue(insert_job(3)).unwrap();
        writer.flush(Duration::from_secs(5)).unwrap();

        let ids: Vec<i64> = {
            let c = conn.lock().unwrap();
            let mut stmt = c.prepare("SELECT id FROM t ORDER BY id").unwrap();
            let rows = stmt.query_map([], |r| r.get(0)).unwrap();
            rows.map(|r| r.unwrap()).collect()
        };
        assert_eq!(ids, vec![1, 3], "partial job must be rolled back");
        let stats = writer.stats();
        assert_eq!(stats.written, 2);
        assert_eq!(stats.failed, 2);
    }

    #[test]
    fn full_channel_drops_instead_of_blocking() {
        let conn = test_conn();
        let writer = UsageWriter::with_capacity(Arc::clone(&conn), 2);
        let release = block_writer(&writer);
        let mut dropped = 0;
        for id in 0..20 {
            if writer.enqueue(insert_job(id)).is_err() {
                dropped += 1;
            }
        }
        release.send(()).unwrap();
        writer.flush(Duration::from_secs(5)).unwrap();
        assert_eq!(dropped, 18, "bounded channel must reject when full");
        let stats = writer.stats();
        assert_eq!(stats.dropped, 18);
        assert_eq!(count(&conn), 2);
    }

    #[test]
    fn drop_drains_pending_jobs() {
        let conn = test_conn();
        {
            let writer = UsageWriter::new(Arc::clone(&conn));
            for id in 0..50 {
                writer.enqueue(insert_job(id)).unwrap();
            }
        }
        assert_eq!(count(&conn), 50);
    }
}
