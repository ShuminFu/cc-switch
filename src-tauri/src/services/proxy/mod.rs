//! 代理服务业务逻辑层
//!
//! 提供代理服务器的启动、停止和配置管理

use crate::app_config::AppType;
use crate::database::Database;
use crate::provider::Provider;
use crate::proxy::inbound_auth::bind_inbound_token;
use crate::proxy::server::ProxyServer;
use crate::proxy::switch_lock::SwitchLockManager;
use crate::proxy::types::*;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;

mod backup;
mod config;
mod hot_switch;
mod lifecycle;
mod takeover;

pub(crate) use takeover::TakeoverMode;

/// 用于接管 Live 配置时的占位符（避免客户端提示缺少 key，同时不泄露真实 Token）
///
/// 接管投影先写入该哨兵，落盘前由 `bind_inbound_token` 替换为本安装的代理 token。
const PROXY_TOKEN_PLACEHOLDER: &str = crate::proxy::inbound_auth::LEGACY_PROXY_TOKEN;

#[derive(Clone)]
pub struct ProxyService {
    db: Arc<Database>,
    server: Arc<RwLock<Option<ProxyServer>>>,
    /// 串行化 start/stop/重启等生命周期操作，避免并发启动绑定多个端口；
    /// `server` 读写锁只在取放实例时短暂持有，不跨越 stop() 等待。
    lifecycle: lifecycle::LifecycleMutex,
    /// AppHandle，用于传递给 ProxyServer 以支持故障转移时的 UI 更新
    app_handle: Arc<RwLock<Option<tauri::AppHandle>>>,
    switch_locks: SwitchLockManager,
    /// 测试用：stop() 停止服务器后额外等待的毫秒数，用于模拟慢速停止
    #[cfg(test)]
    stop_delay_ms_for_test: Arc<std::sync::atomic::AtomicU64>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct HotSwitchOutcome {
    pub logical_target_changed: bool,
}

impl ProxyService {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            server: Arc::new(RwLock::new(None)),
            lifecycle: Arc::new(tokio::sync::Mutex::new(())),
            app_handle: Arc::new(RwLock::new(None)),
            switch_locks: SwitchLockManager::new(),
            #[cfg(test)]
            stop_delay_ms_for_test: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    pub(in crate::services::proxy) fn get_current_provider_for_app(
        &self,
        app_type: &AppType,
    ) -> Result<Option<Provider>, String> {
        let Some(current_id) = crate::settings::get_effective_current_provider(&self.db, app_type)
            .map_err(|e| format!("获取 {app_type:?} 当前供应商失败: {e}"))?
        else {
            return Ok(None);
        };

        self.db
            .get_provider_by_id(&current_id, app_type.as_str())
            .map_err(|e| format!("读取 {app_type:?} 当前供应商失败: {e}"))
    }

    pub(in crate::services::proxy) fn require_current_provider_for_app(
        &self,
        app_type: &AppType,
    ) -> Result<Provider, String> {
        self.get_current_provider_for_app(app_type)?
            .ok_or_else(|| format!("{app_type:?} 当前供应商不存在，无法接管 Live 配置"))
    }

    /// 设置 AppHandle（在应用初始化时调用）
    pub fn set_app_handle(&self, handle: tauri::AppHandle) {
        futures::executor::block_on(async {
            *self.app_handle.write().await = Some(handle);
        });
    }

    pub(crate) async fn lock_switch_for_app(
        &self,
        app_type: &str,
    ) -> tokio::sync::OwnedMutexGuard<()> {
        self.switch_locks.lock_for_app(app_type).await
    }

    #[cfg(test)]
    pub(in crate::services::proxy) async fn lock_switch_for_test(
        &self,
        app_type: &str,
    ) -> tokio::sync::OwnedMutexGuard<()> {
        self.switch_locks.lock_for_app(app_type).await
    }

    /// 把接管投影中的 token 哨兵替换为本安装的代理 token（无哨兵时原样返回）
    pub(in crate::services::proxy) fn bind_proxy_token(
        &self,
        config: &Value,
    ) -> Result<Value, String> {
        let mut bound = config.clone();
        if config.to_string().contains(PROXY_TOKEN_PLACEHOLDER) {
            let token = crate::proxy::inbound_auth::get_or_create_inbound_token(&self.db)
                .map_err(|e| format!("读取代理 token 失败: {e}"))?;
            bind_inbound_token(&mut bound, &token);
        }
        Ok(bound)
    }
}

#[cfg(test)]
mod tests;
