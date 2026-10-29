//! 代理运行时配置快照
//!
//! 请求路径上需要的配置（per-app 代理配置、整流器/优化器配置、供应商列表、
//! 计费默认值、模型定价、Claude Desktop gateway token）缓存在一个 [`ArcSwap`] 快照里，稳态下单个请求不再读数据库。
//!
//! 失效规则：快照记录构建时的 [`Database::config_generation`]。任何配置类表的写入
//! （由 SQLite update hook 统计）或 [`RuntimeConfigStore::invalidate`] 都会推进代数，
//! 下一次访问时发现代数不一致即换成一个空快照，各条目随后按需重新加载。
//! 加载失败的条目不会被缓存，下次访问会重试数据库。

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use crate::proxy::types::{
    AppProxyConfig, CopilotOptimizerConfig, OptimizerConfig, RectifierConfig,
};
use crate::proxy::usage::calculator::ModelPricing;
use arc_swap::ArcSwap;
use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

/// 整流器 / 优化器配置（全局，存储在 settings 表）
#[derive(Debug, Clone, Default)]
pub struct ProcessingConfigs {
    pub rectifier: RectifierConfig,
    pub optimizer: OptimizerConfig,
    pub copilot_optimizer: CopilotOptimizerConfig,
}

/// 某个应用的供应商数据（选路与计费共用）
#[derive(Debug, Default)]
pub struct AppProviders {
    /// 该应用的全部供应商（DAO 排序）
    pub all: IndexMap<String, Provider>,
    /// 故障转移队列中的供应商 ID（按队列顺序）
    pub failover_queue: Vec<String>,
    /// 数据库中标记为 is_current 的供应商
    pub db_current: Option<String>,
}

impl AppProviders {
    /// 与 [`crate::settings::get_effective_current_provider`] 语义一致，但基于快照判断存在性：
    /// 本地 settings 的设备级当前供应商存在于数据库时优先；否则清理本地值并回退到 is_current。
    pub fn effective_current_provider(&self, app_type: &AppType) -> Option<String> {
        if let Some(local_id) = crate::settings::get_current_provider(app_type) {
            if self.all.contains_key(&local_id) {
                return Some(local_id);
            }
            log::warn!(
                "本地 settings 中的供应商 {} ({}) 在数据库中不存在，将清理并 fallback 到数据库",
                local_id,
                app_type.as_str()
            );
            let _ = crate::settings::set_current_provider(app_type, None);
        }
        self.db_current.clone()
    }
}

/// 某一配置代数下的运行时配置快照（条目按需加载）
#[derive(Default)]
pub struct RuntimeConfigSnapshot {
    generation: u64,
    app_configs: Mutex<HashMap<String, AppProxyConfig>>,
    processing: Mutex<Option<Arc<ProcessingConfigs>>>,
    app_providers: Mutex<HashMap<String, Arc<AppProviders>>>,
    default_cost_multipliers: Mutex<HashMap<String, String>>,
    pricing_model_sources: Mutex<HashMap<String, String>>,
    model_pricing: Mutex<HashMap<String, Option<ModelPricing>>>,
    claude_desktop_gateway_token: Mutex<Option<String>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// 先查缓存，未命中时加载；仅缓存加载成功的结果。
fn cached<V: Clone>(
    map: &Mutex<HashMap<String, V>>,
    key: &str,
    load: impl FnOnce() -> Result<V, AppError>,
) -> Result<V, AppError> {
    if let Some(value) = lock(map).get(key) {
        return Ok(value.clone());
    }
    let value = load()?;
    lock(map).insert(key.to_string(), value.clone());
    Ok(value)
}

impl RuntimeConfigSnapshot {
    fn empty(generation: u64) -> Self {
        Self {
            generation,
            ..Self::default()
        }
    }

    /// 构建该快照时的配置代数
    #[cfg(test)]
    pub fn generation(&self) -> u64 {
        self.generation
    }
}

/// 运行时配置快照的持有者（`ProxyState.runtime_config`）
pub struct RuntimeConfigStore {
    db: Arc<Database>,
    current: ArcSwap<RuntimeConfigSnapshot>,
}

impl RuntimeConfigStore {
    pub fn new(db: Arc<Database>) -> Self {
        // u64::MAX 永远不等于真实代数，首次访问即换成有效快照
        Self {
            db,
            current: ArcSwap::from_pointee(RuntimeConfigSnapshot::empty(u64::MAX)),
        }
    }

    #[cfg(test)]
    pub(crate) fn db(&self) -> &Arc<Database> {
        &self.db
    }

    /// 当前有效的快照；数据库配置代数变化后换成空快照。
    ///
    /// 先读代数再加载数据：加载期间若有新写入，代数已前进，下次访问会再次换新。
    pub fn snapshot(&self) -> Arc<RuntimeConfigSnapshot> {
        let generation = self.db.config_generation();
        let current = self.current.load_full();
        if current.generation == generation {
            return current;
        }
        let fresh = Arc::new(RuntimeConfigSnapshot::empty(generation));
        self.current.store(fresh.clone());
        fresh
    }

    /// 主动让快照失效（例如 `apply_runtime_config` / `update_config` 之后）。
    pub fn invalidate(&self) {
        self.db.mark_config_changed();
        self.current.store(Arc::new(RuntimeConfigSnapshot::empty(
            self.db.config_generation(),
        )));
    }

    /// 应用级代理配置（等价于 `Database::get_proxy_config_for_app`）
    pub async fn app_proxy_config(&self, app_type: &str) -> Result<AppProxyConfig, AppError> {
        let snapshot = self.snapshot();
        if let Some(config) = lock(&snapshot.app_configs).get(app_type) {
            return Ok(config.clone());
        }
        let config = self.db.get_proxy_config_for_app(app_type).await?;
        lock(&snapshot.app_configs).insert(app_type.to_string(), config.clone());
        Ok(config)
    }

    /// 整流器 / 优化器配置；读取失败的项使用默认值（与原先 `unwrap_or_default` 一致），
    /// 且此时不缓存，下次访问重试。
    pub fn processing_configs(&self) -> Arc<ProcessingConfigs> {
        let snapshot = self.snapshot();
        if let Some(configs) = lock(&snapshot.processing).as_ref() {
            return configs.clone();
        }
        let rectifier = self.db.get_rectifier_config();
        let optimizer = self.db.get_optimizer_config();
        let copilot_optimizer = self.db.get_copilot_optimizer_config();
        let all_loaded = rectifier.is_ok() && optimizer.is_ok() && copilot_optimizer.is_ok();
        let configs = Arc::new(ProcessingConfigs {
            rectifier: rectifier.unwrap_or_default(),
            optimizer: optimizer.unwrap_or_default(),
            copilot_optimizer: copilot_optimizer.unwrap_or_default(),
        });
        if all_loaded {
            *lock(&snapshot.processing) = Some(configs.clone());
        }
        configs
    }

    /// 某个应用的供应商、故障转移队列与 is_current
    pub fn app_providers(&self, app_type: &str) -> Result<Arc<AppProviders>, AppError> {
        let snapshot = self.snapshot();
        cached(&snapshot.app_providers, app_type, || {
            let all = self.db.get_all_providers(app_type)?;
            let failover_queue = self
                .db
                .get_failover_queue(app_type)?
                .into_iter()
                .map(|item| item.provider_id)
                .collect();
            let db_current = self.db.get_current_provider(app_type)?;
            Ok(Arc::new(AppProviders {
                all,
                failover_queue,
                db_current,
            }))
        })
    }

    /// 全局默认成本倍率（等价于 `Database::get_default_cost_multiplier`）
    pub async fn default_cost_multiplier(&self, app_type: &str) -> Result<String, AppError> {
        let snapshot = self.snapshot();
        if let Some(value) = lock(&snapshot.default_cost_multipliers).get(app_type) {
            return Ok(value.clone());
        }
        let value = self.db.get_default_cost_multiplier(app_type).await?;
        lock(&snapshot.default_cost_multipliers).insert(app_type.to_string(), value.clone());
        Ok(value)
    }

    /// 全局默认计费模式来源（等价于 `Database::get_pricing_model_source`）
    pub async fn pricing_model_source(&self, app_type: &str) -> Result<String, AppError> {
        let snapshot = self.snapshot();
        if let Some(value) = lock(&snapshot.pricing_model_sources).get(app_type) {
            return Ok(value.clone());
        }
        let value = self.db.get_pricing_model_source(app_type).await?;
        lock(&snapshot.pricing_model_sources).insert(app_type.to_string(), value.clone());
        Ok(value)
    }

    /// 模型定价（含「未找到」的结果）；`load` 为未命中时的数据库查找。
    pub fn model_pricing(
        &self,
        model_id: &str,
        load: impl FnOnce() -> Result<Option<ModelPricing>, AppError>,
    ) -> Result<Option<ModelPricing>, AppError> {
        let snapshot = self.snapshot();
        cached(&snapshot.model_pricing, model_id, load)
    }

    /// Claude Desktop gateway 鉴权 token（等价于 `get_or_create_gateway_token`）
    pub fn claude_desktop_gateway_token(&self) -> Result<String, AppError> {
        let snapshot = self.snapshot();
        if let Some(token) = lock(&snapshot.claude_desktop_gateway_token).as_ref() {
            return Ok(token.clone());
        }
        let token = crate::claude_desktop_config::get_or_create_gateway_token(&self.db)?;
        *lock(&snapshot.claude_desktop_gateway_token) = Some(token.clone());
        Ok(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cached_reads_hit_the_database_once_per_generation() {
        let db = Arc::new(Database::memory().expect("memory db"));
        let store = RuntimeConfigStore::new(db.clone());

        let first = store.app_proxy_config("claude").await.expect("config");
        let generation = store.snapshot().generation();
        let again = store.app_proxy_config("claude").await.expect("config");
        assert_eq!(first.max_retries, again.max_retries);
        assert_eq!(store.snapshot().generation(), generation);

        // 写入配置表推进代数，下一次读取拿到新值
        let mut updated = first.clone();
        updated.max_retries = first.max_retries + 2;
        db.update_proxy_config_for_app(updated)
            .await
            .expect("update config");
        assert_ne!(store.snapshot().generation(), generation);
        let refreshed = store.app_proxy_config("claude").await.expect("config");
        assert_eq!(refreshed.max_retries, first.max_retries + 2);
    }

    #[tokio::test]
    async fn high_churn_tables_do_not_invalidate_the_snapshot() {
        let db = Arc::new(Database::memory().expect("memory db"));
        let provider = Provider::with_id(
            "p1".to_string(),
            "P1".to_string(),
            serde_json::json!({}),
            None,
        );
        db.save_provider("claude", &provider)
            .expect("save provider");
        let store = RuntimeConfigStore::new(db.clone());
        let providers = store.app_providers("claude").expect("providers");
        let generation = store.snapshot().generation();

        db.update_provider_health_with_threshold("p1", "claude", false, None, 3)
            .await
            .expect("health update");
        assert_eq!(store.snapshot().generation(), generation);
        assert!(Arc::ptr_eq(
            &providers,
            &store.app_providers("claude").expect("providers")
        ));
    }

    #[tokio::test]
    async fn provider_writes_and_invalidate_refresh_the_snapshot() {
        let db = Arc::new(Database::memory().expect("memory db"));
        let store = RuntimeConfigStore::new(db.clone());
        assert!(store
            .app_providers("claude")
            .expect("providers")
            .all
            .is_empty());

        let provider = Provider::with_id(
            "p1".to_string(),
            "P1".to_string(),
            serde_json::json!({}),
            None,
        );
        db.save_provider("claude", &provider)
            .expect("save provider");
        db.add_to_failover_queue("claude", "p1").expect("queue");
        let providers = store.app_providers("claude").expect("providers");
        assert!(providers.all.contains_key("p1"));
        assert_eq!(providers.failover_queue, vec!["p1".to_string()]);

        let before = store.snapshot().generation();
        store.invalidate();
        assert_ne!(store.snapshot().generation(), before);
        assert!(!Arc::ptr_eq(
            &providers,
            &store.app_providers("claude").expect("providers")
        ));
    }

    #[test]
    fn claude_desktop_gateway_token_is_cached_until_settings_change() {
        let db = Arc::new(Database::memory().expect("memory db"));
        let store = RuntimeConfigStore::new(db.clone());
        let token = store.claude_desktop_gateway_token().expect("token");
        assert_eq!(store.claude_desktop_gateway_token().expect("token"), token);

        db.set_setting("claude_desktop_gateway_token", "ccs-rotated")
            .expect("rotate token");
        assert_eq!(
            store.claude_desktop_gateway_token().expect("token"),
            "ccs-rotated"
        );
    }

    #[test]
    fn processing_configs_follow_settings_writes() {
        let db = Arc::new(Database::memory().expect("memory db"));
        let store = RuntimeConfigStore::new(db.clone());
        let before = store.processing_configs();

        let mut rectifier = before.rectifier.clone();
        rectifier.enabled = !rectifier.enabled;
        db.set_rectifier_config(&rectifier).expect("save rectifier");

        assert_eq!(
            store.processing_configs().rectifier.enabled,
            rectifier.enabled
        );
    }
}
