use crate::database::Database;
use crate::services::{ProxyService, UsageCache};
use std::sync::Arc;

/// 全局应用状态
pub struct AppState {
    pub db: Arc<Database>,
    pub proxy_service: ProxyService,
    pub usage_cache: Arc<UsageCache>,
}

impl AppState {
    /// 创建新的应用状态
    pub fn new(db: Arc<Database>) -> Self {
        let proxy_service = ProxyService::new(db.clone());

        // 托盘用量快照落盘到配置目录，重启后立即可见（24 小时内）
        let usage_cache = UsageCache::with_persistence(
            crate::config::get_app_config_dir().join("usage-cache.json"),
        );

        Self {
            db,
            proxy_service,
            usage_cache: Arc::new(usage_cache),
        }
    }
}
