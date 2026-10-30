//! 代理请求路径的配置快照缓存（PERF-05 / PROXY-04）
//!
//! 每个代理请求原本要读 6-8 次数据库：per-app `proxy_config`（两次）、整流器/
//! 优化器/Copilot 优化器配置、整张 providers 表（含 JSON 解析）、故障转移队列，
//! 以及使用量计价用的默认倍率/计费模式与供应商 meta。这些值只会在少数 UI 操作中
//! 变化，因此按 app 缓存为一个不可变的 [`ProxyRequestSnapshot`]，请求路径上
//! 命中缓存时只是一次 `RwLock` 读 + `Arc` 克隆，完全不触碰 SQLite。
//!
//! 失效策略：
//! - **写入即失效**：主连接注册的 `update_hook` 在任何配置类表发生
//!   INSERT/UPDATE/DELETE 时递增代数（generation）；条目代数不匹配即视为过期。
//!   高频写入的日志/健康类表（见 [`is_volatile_table`]）不会触发失效。
//! - **显式失效**：不经过行级 hook 的整库替换（备份恢复、SQL 导入）调用
//!   [`Database::invalidate_proxy_request_cache`]。
//! - **TTL 兜底**：条目最长存活 [`SNAPSHOT_TTL`]，防止遗漏的写路径导致永久陈旧。
//!
//! 重建快照时先读取代数再查询，写入若与重建并发，条目会带着旧代数入缓存并在下一次
//! 访问时重新构建，因此不会把旧数据标记为新代数。

use super::Database;
use crate::error::AppError;
use crate::provider::Provider;
use crate::proxy::types::{
    AppProxyConfig, CopilotOptimizerConfig, OptimizerConfig, RectifierConfig,
};
use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// 快照最长存活时间（兜底；正常情况下由写入 hook 立即失效）。
pub(crate) const SNAPSHOT_TTL: Duration = Duration::from_secs(30);

/// 写入时**不**使快照失效的表：高频写入且与请求路由/计价无关。
pub(crate) fn is_volatile_table(table: &str) -> bool {
    matches!(
        table,
        "proxy_request_logs"
            | "provider_health"
            | "usage_daily_rollups"
            | "session_log_sync"
            | "stream_check_logs"
    )
}

/// 单个 app 在代理请求路径上需要的全部配置。
#[derive(Debug, Clone)]
pub struct ProxyRequestSnapshot {
    /// 快照对应的 app_type（如 "claude"、"claude-desktop"）
    pub app_type: String,
    /// 应用级代理配置
    pub app_config: AppProxyConfig,
    /// 整流器配置（读取失败时为默认值）
    pub rectifier_config: RectifierConfig,
    /// 优化器配置（读取失败时为默认值）
    pub optimizer_config: OptimizerConfig,
    /// Copilot 优化器配置（读取失败时为默认值）
    pub copilot_optimizer_config: CopilotOptimizerConfig,
    /// 该 app 的全部供应商（与 `get_all_providers` 相同的顺序与内容）
    pub providers: IndexMap<String, Provider>,
    /// 故障转移队列中的供应商 ID（与 `get_failover_queue` 相同的顺序）
    pub failover_queue: Vec<String>,
    /// 数据库中 `is_current = 1` 的供应商 ID
    pub db_current_provider_id: Option<String>,
    /// 全局默认成本倍率（原始字符串；读取失败时为 `None`）
    pub default_cost_multiplier: Option<String>,
    /// 全局默认计费模式来源（原始字符串；读取失败时为 `None`）
    pub default_pricing_model_source: Option<String>,
}

struct Entry {
    generation: u64,
    built_at: Instant,
    snapshot: Arc<ProxyRequestSnapshot>,
}

pub(crate) struct RequestCache {
    generation: Arc<AtomicU64>,
    entries: RwLock<HashMap<String, Entry>>,
    builds: AtomicU64,
}

impl RequestCache {
    pub(crate) fn new() -> Self {
        Self {
            generation: Arc::new(AtomicU64::new(0)),
            entries: RwLock::new(HashMap::new()),
            builds: AtomicU64::new(0),
        }
    }

    /// 供 `update_hook` 闭包捕获的代数计数器。
    pub(crate) fn generation_handle(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.generation)
    }

    pub(crate) fn invalidate(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
    }

    fn lookup(&self, app_type: &str) -> Option<Arc<ProxyRequestSnapshot>> {
        let current = self.generation.load(Ordering::Acquire);
        let entries = self.entries.read().unwrap_or_else(|e| e.into_inner());
        let entry = entries.get(app_type)?;
        (entry.generation == current && entry.built_at.elapsed() < SNAPSHOT_TTL)
            .then(|| Arc::clone(&entry.snapshot))
    }

    fn store(&self, app_type: &str, generation: u64, snapshot: Arc<ProxyRequestSnapshot>) {
        let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
        entries.insert(
            app_type.to_string(),
            Entry {
                generation,
                built_at: Instant::now(),
                snapshot,
            },
        );
    }
}

impl Database {
    /// 获取 app 的请求配置快照：命中缓存时不访问数据库，否则同步重建。
    ///
    /// 在 async 上下文中请使用 [`proxy_request_snapshot_async`](Self::proxy_request_snapshot_async)，
    /// 它把重建放到 `spawn_blocking` 线程上。
    pub fn proxy_request_snapshot(
        &self,
        app_type: &str,
    ) -> Result<Arc<ProxyRequestSnapshot>, AppError> {
        if let Some(hit) = self.request_cache.lookup(app_type) {
            return Ok(hit);
        }
        self.rebuild_proxy_request_snapshot(app_type)
    }

    /// 异步版本：命中缓存时直接返回；未命中时在阻塞线程池上重建，
    /// 不在 Tokio worker 上持有 std 互斥锁。
    pub async fn proxy_request_snapshot_async(
        self: &Arc<Self>,
        app_type: &str,
    ) -> Result<Arc<ProxyRequestSnapshot>, AppError> {
        if let Some(hit) = self.request_cache.lookup(app_type) {
            return Ok(hit);
        }
        let db = Arc::clone(self);
        let app_type = app_type.to_string();
        tokio::task::spawn_blocking(move || db.rebuild_proxy_request_snapshot(&app_type))
            .await
            .map_err(|e| AppError::Database(format!("构建代理配置快照任务失败: {e}")))?
    }

    /// 使所有 app 的请求配置快照失效（整库替换等不触发行级 hook 的写入后调用）。
    pub fn invalidate_proxy_request_cache(&self) {
        self.request_cache.invalidate();
    }

    /// 快照重建次数（测试/诊断用）。
    #[allow(dead_code)]
    pub(crate) fn proxy_request_snapshot_builds(&self) -> u64 {
        self.request_cache.builds.load(Ordering::Relaxed)
    }

    fn rebuild_proxy_request_snapshot(
        &self,
        app_type: &str,
    ) -> Result<Arc<ProxyRequestSnapshot>, AppError> {
        // 先取代数：重建期间若有写入，条目会以旧代数入缓存，下次访问即重建。
        let generation = self.request_cache.generation.load(Ordering::Acquire);
        self.request_cache.builds.fetch_add(1, Ordering::Relaxed);

        let app_config = self.get_proxy_config_for_app_blocking(app_type)?;
        let rectifier_config = self.get_rectifier_config().unwrap_or_default();
        let optimizer_config = self.get_optimizer_config().unwrap_or_default();
        let copilot_optimizer_config = self.get_copilot_optimizer_config().unwrap_or_default();
        let providers = self.get_all_providers(app_type)?;
        let failover_queue = self
            .get_failover_queue(app_type)?
            .into_iter()
            .map(|item| item.provider_id)
            .collect();
        let db_current_provider_id = self.get_current_provider(app_type)?;
        let default_cost_multiplier = self
            .get_default_cost_multiplier_blocking(app_type)
            .map_err(|e| log::debug!("[proxy-cache] 读取默认倍率失败 (app_type={app_type}): {e}"))
            .ok();
        let default_pricing_model_source = self
            .get_pricing_model_source_blocking(app_type)
            .map_err(|e| {
                log::debug!("[proxy-cache] 读取默认计费模式失败 (app_type={app_type}): {e}")
            })
            .ok();

        let snapshot = Arc::new(ProxyRequestSnapshot {
            app_type: app_type.to_string(),
            app_config,
            rectifier_config,
            optimizer_config,
            copilot_optimizer_config,
            providers,
            failover_queue,
            db_current_provider_id,
            default_cost_multiplier,
            default_pricing_model_source,
        });
        self.request_cache
            .store(app_type, generation, Arc::clone(&snapshot));
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Provider;
    use serde_json::json;

    fn provider(id: &str) -> Provider {
        Provider::with_id(id.to_string(), id.to_string(), json!({}), None)
    }

    #[test]
    fn repeated_lookups_hit_the_cache() {
        let db = Database::memory().unwrap();
        let first = db.proxy_request_snapshot("claude").unwrap();
        let builds = db.proxy_request_snapshot_builds();
        for _ in 0..10 {
            let again = db.proxy_request_snapshot("claude").unwrap();
            assert!(Arc::ptr_eq(&first, &again));
        }
        assert_eq!(db.proxy_request_snapshot_builds(), builds);
    }

    #[test]
    fn provider_writes_invalidate_the_snapshot() {
        let db = Database::memory().unwrap();
        let before = db.proxy_request_snapshot("claude").unwrap();
        assert!(before.providers.is_empty());

        db.save_provider("claude", &provider("p1")).unwrap();
        let after = db.proxy_request_snapshot("claude").unwrap();
        assert!(after.providers.contains_key("p1"));

        db.set_current_provider("claude", "p1").unwrap();
        let current = db.proxy_request_snapshot("claude").unwrap();
        assert_eq!(current.db_current_provider_id.as_deref(), Some("p1"));
    }

    #[tokio::test]
    async fn proxy_config_writes_invalidate_the_snapshot() {
        let db = Arc::new(Database::memory().unwrap());
        let snap = db.proxy_request_snapshot_async("claude").await.unwrap();
        assert!(!snap.app_config.auto_failover_enabled);

        let mut config = snap.app_config.clone();
        config.auto_failover_enabled = true;
        db.update_proxy_config_for_app(config).await.unwrap();
        db.set_default_cost_multiplier("claude", "2.5")
            .await
            .unwrap();

        let snap = db.proxy_request_snapshot_async("claude").await.unwrap();
        assert!(snap.app_config.auto_failover_enabled);
        assert_eq!(snap.default_cost_multiplier.as_deref(), Some("2.5"));
    }

    #[test]
    fn settings_writes_invalidate_the_snapshot() {
        let db = Database::memory().unwrap();
        let snap = db.proxy_request_snapshot("claude").unwrap();
        let mut rectifier = snap.rectifier_config.clone();
        rectifier.enabled = !rectifier.enabled;
        db.set_rectifier_config(&rectifier).unwrap();
        let snap = db.proxy_request_snapshot("claude").unwrap();
        assert_eq!(snap.rectifier_config.enabled, rectifier.enabled);
    }

    #[test]
    fn volatile_tables_do_not_invalidate() {
        let db = Database::memory().unwrap();
        let first = db.proxy_request_snapshot("claude").unwrap();
        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO provider_health (provider_id, app_type, is_healthy, consecutive_failures, updated_at)
                 VALUES ('p', 'claude', 1, 0, 'now')",
                [],
            )
            .unwrap();
        }
        let again = db.proxy_request_snapshot("claude").unwrap();
        assert!(Arc::ptr_eq(&first, &again));
    }

    #[test]
    fn explicit_invalidation_forces_rebuild() {
        let db = Database::memory().unwrap();
        let first = db.proxy_request_snapshot("claude").unwrap();
        db.invalidate_proxy_request_cache();
        let again = db.proxy_request_snapshot("claude").unwrap();
        assert!(!Arc::ptr_eq(&first, &again));
    }
}
