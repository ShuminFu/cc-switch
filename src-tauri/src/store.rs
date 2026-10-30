use crate::database::Database;
use crate::services::{ProxyService, UsageCache};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// 全局应用状态
pub struct AppState {
    pub db: Arc<Database>,
    pub proxy_service: ProxyService,
    pub usage_cache: Arc<UsageCache>,
    /// 轻量模式（主窗口已销毁、仅保留托盘）
    lightweight_mode: AtomicBool,
}

impl AppState {
    /// 创建新的应用状态
    pub fn new(db: Arc<Database>) -> Self {
        let proxy_service = ProxyService::new(db.clone());

        Self {
            db,
            proxy_service,
            usage_cache: Arc::new(UsageCache::new()),
            lightweight_mode: AtomicBool::new(false),
        }
    }

    pub fn is_lightweight_mode(&self) -> bool {
        self.lightweight_mode.load(Ordering::Acquire)
    }

    pub fn set_lightweight_mode(&self, enabled: bool) {
        self.lightweight_mode.store(enabled, Ordering::Release);
    }
}
