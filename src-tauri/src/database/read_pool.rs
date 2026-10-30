//! 只读连接池
//!
//! WAL 模式下读者与写者互不阻塞，但前提是它们使用**不同的连接**。主连接
//! （`Database::conn`）承担所有写入以及大多数短查询；仪表盘聚合这类耗时的只读
//! 查询改走这里的只读连接（`SQLITE_OPEN_READ_ONLY`），不再与代理日志写入、
//! 维护任务争用同一把互斥锁。
//!
//! 只在文件库且 `journal_mode=WAL` 时启用；内存库或回滚日志模式下
//! [`Database::read_conn`](super::Database::read_conn) 回落到主连接，行为与之前一致。

use super::DB_BUSY_TIMEOUT;
use crate::error::AppError;
use rusqlite::{Connection, OpenFlags};
use std::ops::Deref;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

/// 只读连接数。仪表盘一次刷新会并发发起若干查询，2 个足够且开销很小。
pub(crate) const READ_POOL_SIZE: usize = 2;

pub(crate) struct ReadPool {
    conns: Vec<Mutex<Connection>>,
    next: AtomicUsize,
}

impl ReadPool {
    /// 为 `path` 打开 `size` 个只读连接。数据库不是 WAL 模式时返回 `Ok(None)`：
    /// 回滚日志模式下读事务会阻塞写者提交，此时不值得引入额外连接。
    pub(crate) fn open(path: &Path, size: usize) -> Result<Option<Self>, AppError> {
        let mut conns = Vec::with_capacity(size);
        for _ in 0..size.max(1) {
            let conn = Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )
            .map_err(|e| AppError::Database(format!("打开只读数据库连接失败: {e}")))?;
            conn.busy_timeout(DB_BUSY_TIMEOUT)
                .map_err(|e| AppError::Database(format!("配置只读连接失败: {e}")))?;
            let mode: String = conn
                .query_row("PRAGMA journal_mode;", [], |row| row.get(0))
                .map_err(|e| AppError::Database(format!("读取 journal_mode 失败: {e}")))?;
            if !mode.eq_ignore_ascii_case("wal") {
                return Ok(None);
            }
            conn.execute_batch("PRAGMA temp_store = MEMORY;")
                .map_err(|e| AppError::Database(format!("配置只读连接失败: {e}")))?;
            conns.push(Mutex::new(conn));
        }
        Ok(Some(Self {
            conns,
            next: AtomicUsize::new(0),
        }))
    }

    /// 取一个只读连接：优先取空闲的，全忙时按轮转阻塞等待其中一个。
    pub(crate) fn get(&self) -> Result<MutexGuard<'_, Connection>, AppError> {
        for conn in &self.conns {
            if let Ok(guard) = conn.try_lock() {
                return Ok(guard);
            }
        }
        let idx = self.next.fetch_add(1, Ordering::Relaxed) % self.conns.len();
        self.conns[idx]
            .lock()
            .map_err(|e| AppError::Database(format!("Mutex lock failed: {e}")))
    }
}

/// [`Database::read_conn`](super::Database::read_conn) 返回的连接守卫：
/// 只读池连接或（无池时）主连接。解引用为 `&Connection`。
pub(crate) struct ReadConn<'a> {
    guard: MutexGuard<'a, Connection>,
    pooled: bool,
}

impl<'a> ReadConn<'a> {
    pub(crate) fn pooled(guard: MutexGuard<'a, Connection>) -> Self {
        Self {
            guard,
            pooled: true,
        }
    }

    pub(crate) fn main(guard: MutexGuard<'a, Connection>) -> Self {
        Self {
            guard,
            pooled: false,
        }
    }

    /// 是否来自只读池（`false` 表示回落到主连接）。
    #[allow(dead_code)]
    pub(crate) fn is_pooled(&self) -> bool {
        self.pooled
    }
}

impl Deref for ReadConn<'_> {
    type Target = Connection;

    fn deref(&self) -> &Connection {
        &self.guard
    }
}
