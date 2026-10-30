//! 代理配置读写、重启与熔断器管理

use super::*;
use crate::app_config::AppType;
use crate::proxy::server::ProxyServer;
use std::sync::Arc;

impl ProxyService {
    /// 构造写入 Live 的代理地址（处理 0.0.0.0 / IPv6 等特殊情况）
    pub(in crate::services::proxy) async fn build_proxy_urls(
        &self,
    ) -> Result<(String, String), String> {
        let config = self
            .db
            .get_proxy_config()
            .await
            .map_err(|e| format!("获取代理配置失败: {e}"))?;

        // listen_address 可能是 0.0.0.0（用于监听所有网卡），但客户端无法用 0.0.0.0 连接；
        // 因此写回到各应用配置时，优先使用本机回环地址。
        let connect_host = match config.listen_address.as_str() {
            "0.0.0.0" => "127.0.0.1".to_string(),
            "::" => "::1".to_string(),
            _ => config.listen_address.clone(),
        };
        let connect_host_for_url = if connect_host.contains(':') && !connect_host.starts_with('[') {
            format!("[{connect_host}]")
        } else {
            connect_host
        };

        let mut listen_port = config.listen_port;
        if let Some(server) = self.server.read().await.as_ref() {
            let status = server.get_status().await;
            if status.running {
                listen_port = status.port;
            }
        }
        if listen_port == 0 {
            return Err("代理监听端口为 0，但代理服务器尚未运行，无法生成接管地址".to_string());
        }

        let proxy_origin = format!("http://{}:{}", connect_host_for_url, listen_port);
        let proxy_url = proxy_origin.clone();
        let proxy_codex_base_url = format!("{}/v1", proxy_origin.trim_end_matches('/'));

        Ok((proxy_url, proxy_codex_base_url))
    }

    /// 获取代理配置
    pub async fn get_config(&self) -> Result<ProxyConfig, String> {
        self.db
            .get_proxy_config()
            .await
            .map_err(|e| format!("获取代理配置失败: {e}"))
    }

    /// 更新代理配置
    pub async fn update_config(&self, config: &ProxyConfig) -> Result<(), String> {
        let _lifecycle = self.lifecycle.lock().await;

        // 记录旧配置用于判定是否需要重启
        let previous = self
            .db
            .get_proxy_config()
            .await
            .map_err(|e| format!("获取代理配置失败: {e}"))?;

        // 保存到数据库（保持 live_takeover_active 状态不变）
        let mut new_config = config.clone();
        new_config.live_takeover_active = previous.live_takeover_active;

        self.db
            .update_proxy_config(new_config.clone())
            .await
            .map_err(|e| format!("保存代理配置失败: {e}"))?;

        // 检查服务器当前状态
        if !self.is_running().await {
            return Ok(());
        }

        // 判断是否需要重启（地址或端口变更）
        let require_restart = new_config.listen_address != previous.listen_address
            || new_config.listen_port != previous.listen_port;

        if require_restart {
            if let Err(e) = self.restart_with_config(&new_config, &previous).await {
                // 新配置无法启动：数据库回滚到旧配置，避免下次启动重复失败
                if let Err(db_err) = self.db.update_proxy_config(previous.clone()).await {
                    log::warn!("回滚代理配置失败: {db_err}");
                }
                return Err(e);
            }
            log::info!("代理配置已更新，服务器已自动重启应用最新配置");

            // 如果当前存在任意 app 的 Live 接管，需要同步更新 Live 中的代理地址（否则客户端仍指向旧端口）
            if let Ok(takeover) = self.get_takeover_status().await {
                let mut updated_any = false;

                if takeover.claude {
                    self.takeover_live_config_best_effort(&AppType::Claude)
                        .await?;
                    updated_any = true;
                }
                if takeover.codex {
                    self.takeover_live_config_best_effort(&AppType::Codex)
                        .await?;
                    updated_any = true;
                }
                if takeover.gemini {
                    self.takeover_live_config_best_effort(&AppType::Gemini)
                        .await?;
                    updated_any = true;
                }
                if takeover.grokbuild {
                    self.takeover_live_config_best_effort(&AppType::GrokBuild)
                        .await?;
                    updated_any = true;
                }

                if updated_any {
                    log::info!("已同步更新 Live 配置中的代理地址");
                }
            }

            return Ok(());
        } else if let Some(server) = self.server.read().await.as_ref() {
            server.apply_runtime_config(&new_config).await;
            log::info!("代理配置已实时应用，无需重启代理服务器");
        }

        Ok(())
    }

    /// 以新配置重启代理服务器（调用方须已持有 `lifecycle` 锁）
    ///
    /// 端口变更时先绑定新端口再停止旧服务器，失败时旧服务器保持运行；
    /// 仅地址变更（同端口）时须先停止旧服务器，新配置启动失败则用 `previous` 重新启动。
    pub(in crate::services::proxy) async fn restart_with_config(
        &self,
        new_config: &ProxyConfig,
        previous: &ProxyConfig,
    ) -> Result<(), String> {
        let app_handle = self.app_handle.read().await.clone();

        if new_config.listen_port != previous.listen_port {
            let new_server =
                ProxyServer::new(new_config.clone(), self.db.clone(), app_handle.clone());
            let info = new_server
                .start()
                .await
                .map_err(|e| format!("重启代理服务器失败: {e}"))?;
            if let Err(e) = self
                .persist_ephemeral_listen_port_if_needed(new_config, info.port)
                .await
            {
                let _ = new_server.stop().await;
                // 新服务器启动时已改写全局代理端口，旧服务器仍在运行，需改回旧端口
                if let Some(server) = self.server.read().await.as_ref() {
                    crate::proxy::http_client::set_proxy_port(server.get_status().await.port);
                }
                return Err(e);
            }

            // 单独语句换出实例，避免写锁跨越 stop() 的等待
            let old_server = self.server.write().await.replace(new_server);
            if let Some(server) = old_server {
                if let Err(e) = server.stop().await {
                    log::warn!("停止旧代理服务器失败: {e}");
                }
            }
            return Ok(());
        }

        // 单独语句取出实例，避免写锁跨越 stop() 的等待
        let old_server = self.server.write().await.take();
        if let Some(server) = old_server {
            if let Err(e) = server.stop().await {
                // 关闭信号已发出，旧实例不可再用：继续尝试新配置，失败时再以旧配置恢复
                log::warn!("重启前停止代理服务器失败: {e}");
            }
        }

        let new_server = ProxyServer::new(new_config.clone(), self.db.clone(), app_handle.clone());
        let start_result = match new_server.start().await {
            Ok(info) => match self
                .persist_ephemeral_listen_port_if_needed(new_config, info.port)
                .await
            {
                Ok(()) => Ok(()),
                Err(e) => {
                    let _ = new_server.stop().await;
                    Err(e)
                }
            },
            Err(e) => Err(format!("重启代理服务器失败: {e}")),
        };
        if start_result.is_ok() {
            *self.server.write().await = Some(new_server);
            return start_result;
        }

        // 新配置启动失败：用旧配置恢复服务器，避免已接管的客户端指向已关闭的端口
        let restored = ProxyServer::new(previous.clone(), self.db.clone(), app_handle);
        match restored.start().await {
            Ok(_) => {
                *self.server.write().await = Some(restored);
                log::warn!("新代理配置启动失败，已恢复旧配置");
            }
            Err(e) => log::error!("新代理配置启动失败，且恢复旧配置失败: {e}"),
        }
        start_result
    }

    /// 热更新熔断器配置
    ///
    /// 如果代理服务器正在运行，将新配置应用到所有已创建的熔断器实例
    pub async fn update_circuit_breaker_configs(
        &self,
        config: crate::proxy::CircuitBreakerConfig,
    ) -> Result<(), String> {
        if let Some(server) = self.server.read().await.as_ref() {
            server.update_circuit_breaker_configs(config).await;
            log::info!("已热更新运行中的熔断器配置");
        } else {
            log::debug!("代理服务器未运行，熔断器配置将在下次启动时生效");
        }
        Ok(())
    }

    /// 热更新指定应用的熔断器配置
    pub async fn update_circuit_breaker_config_for_app(
        &self,
        app_type: &str,
        config: crate::proxy::CircuitBreakerConfig,
    ) -> Result<(), String> {
        if let Some(server) = self.server.read().await.as_ref() {
            server
                .update_circuit_breaker_config_for_app(app_type, config)
                .await;
            log::info!("已热更新 {app_type} 运行中的熔断器配置");
        } else {
            log::debug!("{app_type} 熔断器配置将在下次代理启动时生效");
        }
        Ok(())
    }

    /// 重置指定 Provider 的熔断器
    ///
    /// 如果代理服务器正在运行，立即重置内存中的熔断器状态
    pub async fn reset_provider_circuit_breaker(
        &self,
        provider_id: &str,
        app_type: &str,
    ) -> Result<(), String> {
        if let Some(server) = self.server.read().await.as_ref() {
            server
                .reset_provider_circuit_breaker(provider_id, app_type)
                .await;
            log::info!("已重置 Provider {provider_id} (app: {app_type}) 的熔断器");
        }
        Ok(())
    }

    /// 获取指定 Provider 的熔断器状态
    ///
    /// 代理未运行或该 Provider 尚无熔断器实例时返回 `None`
    pub async fn get_provider_circuit_breaker_stats(
        &self,
        provider_id: &str,
        app_type: &str,
    ) -> Option<crate::proxy::CircuitBreakerStats> {
        let router = self.provider_router().await?;
        router
            .get_circuit_breaker_stats(provider_id, app_type)
            .await
    }

    /// 手动探测指定 Provider 的熔断器
    ///
    /// 代理未运行时没有内存中的熔断器，返回 `Ok(None)` 且不发出请求
    pub async fn probe_provider_circuit_breaker<F, Fut>(
        &self,
        provider_id: &str,
        app_type: &str,
        probe: F,
    ) -> Result<Option<crate::proxy::CircuitProbeResult>, crate::error::AppError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<(), String>>,
    {
        let Some(router) = self.provider_router().await else {
            return Ok(None);
        };
        router
            .probe_provider(provider_id, app_type, probe)
            .await
            .map(Some)
    }

    /// 运行中代理的 ProviderRouter（克隆后立即释放服务器读锁）
    pub(in crate::services::proxy) async fn provider_router(
        &self,
    ) -> Option<Arc<crate::proxy::ProviderRouter>> {
        self.server
            .read()
            .await
            .as_ref()
            .map(ProxyServer::provider_router)
    }
}
