//! 供应商连通性检查服务（reachability）
//!
//! 仅探测供应商 `base_url` 是否可达，**不发送真实大模型请求**：
//! - 收到任意 HTTP 响应（200/4xx/5xx）即判定"可达"（端口通、网关存活）；
//! - 仅 DNS / 连接被拒 / TLS / 超时等网络级错误判定"不可达"；
//! - 延迟 = 收到响应头的耗时（TTFB，真实往返）。
//!
//! ## 设计取舍：可达 ≠ 配置正确
//!
//! 本检查刻意不验证鉴权或模型，因此不会被第三方供应商的鉴权拦截 / 模型校验
//! 误判为"不可用"。代价是它无法告诉你鉴权对不对、模型存不存在。
//!
//! ## 与故障转移的关系（重要不变量）
//!
//! 连通性检查 **绝不** 触碰故障转移熔断器：一个返回 403/401 的供应商在本检查里
//! 算"可达"，但它对真实流量是坏的。熔断器只由 `proxy/forwarder.rs` 转发真实流量
//! 的成败驱动（被动）。两者职责分离——可达性回答"能不能到"，真实流量回答"能不能用"。

use reqwest::header::HeaderValue;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Instant;

use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::Provider;
use crate::proxy::providers::get_adapter;

/// 健康状态枚举
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Operational,
    Degraded,
    Failed,
}

/// 连通性检查配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamCheckConfig {
    /// 单次探测超时（秒）
    pub timeout_secs: u64,
    /// 超时类失败的最大重试次数
    pub max_retries: u32,
    /// 降级阈值（毫秒）：可达但 TTFB 超过该值判定为"较慢"
    pub degraded_threshold_ms: u64,
}

impl Default for StreamCheckConfig {
    fn default() -> Self {
        // 可达性探测打的是 base_url 的小请求（仅读响应头），不等待模型生成，故超时远小于
        // 旧的真实请求检查（45s → 8s）；降级阈值沿用旧尺度 6000ms——探测 TTFB 一般远低于
        // 此，仅在确实很慢时才标"较慢"，避免把 1 秒多的正常延迟误判为降级。
        Self {
            timeout_secs: 8,
            max_retries: 1,
            degraded_threshold_ms: 6000,
        }
    }
}

/// 连通性检查结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamCheckResult {
    pub status: HealthStatus,
    pub success: bool,
    pub message: String,
    pub response_time_ms: Option<u64>,
    pub http_status: Option<u16>,
    /// 保留字段以兼容 `stream_check_logs` 表结构；连通性检查恒为空串。
    pub model_used: String,
    pub tested_at: i64,
    pub retry_count: u32,
    /// 细粒度错误分类；连通性检查不再细分，恒为 None。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_category: Option<String>,
}

/// 连通性检查服务
pub struct StreamCheckService;

impl StreamCheckService {
    /// 执行连通性检查（仅对超时类失败重试）。
    ///
    /// `base_url_override`：用于 Copilot 等需要从 OAuth 管理器动态解析端点的供应商，
    /// 由命令层预先解析后传入；其余供应商传 `None`，由本服务从 `settings_config` 提取。
    pub async fn check_with_retry(
        app_type: &AppType,
        provider: &Provider,
        config: &StreamCheckConfig,
        base_url_override: Option<String>,
    ) -> Result<StreamCheckResult, AppError> {
        let mut last_result: Option<StreamCheckResult> = None;
        for attempt in 0..=config.max_retries {
            let start = Instant::now();
            let result =
                Self::check_once(app_type, provider, config, base_url_override.clone(), start)
                    .await?;

            if result.success {
                return Ok(StreamCheckResult {
                    retry_count: attempt,
                    ..result
                });
            }

            // 仅超时 / abort 类网络抖动值得重试；连接被拒、DNS 失败等立即返回。
            if Self::should_retry(&result.message) && attempt < config.max_retries {
                last_result = Some(result);
                continue;
            }
            return Ok(StreamCheckResult {
                retry_count: attempt,
                ..result
            });
        }

        Ok(last_result.unwrap_or_else(|| StreamCheckResult {
            status: HealthStatus::Failed,
            success: false,
            message: "Check failed".to_string(),
            response_time_ms: None,
            http_status: None,
            model_used: String::new(),
            tested_at: chrono::Utc::now().timestamp(),
            retry_count: config.max_retries,
            error_category: None,
        }))
    }

    /// 熔断器手动探测（"立即探测"）使用的最小请求。
    ///
    /// 与连通性检查共用同一个轻量 GET（不发真实大模型请求、不消耗 token），但判定更严格：
    /// 网络级错误、5xx 与 429 视为供应商仍不可用；其余响应（含 base_url 常见的 401/404）
    /// 说明网关存活，视为已恢复。该函数本身不碰熔断器——由调用方经 HalfOpen 路径记录结果
    /// （见 `ProviderRouter::probe_provider`）。
    pub async fn probe_circuit_target(
        app_type: &AppType,
        provider: &Provider,
        config: &StreamCheckConfig,
        base_url_override: Option<String>,
    ) -> Result<(), String> {
        let base_url = match base_url_override {
            Some(b) => b,
            None => Self::resolve_base_url(app_type, provider).map_err(|e| e.to_string())?,
        };
        let client = crate::proxy::http_client::get();
        let timeout = std::time::Duration::from_secs(config.timeout_secs);
        let ua = Self::custom_user_agent(provider);

        let status = Self::probe_reachability(&client, &base_url, timeout, ua)
            .await
            .map_err(|e| e.to_string())?;
        Self::probe_status_outcome(status)
    }

    /// 探测响应状态码 → 熔断器探测结果
    fn probe_status_outcome(status: u16) -> Result<(), String> {
        if status >= 500 || status == 429 {
            Err(format!("Upstream HTTP {status}"))
        } else {
            Ok(())
        }
    }

    /// 单次连通性探测。
    async fn check_once(
        app_type: &AppType,
        provider: &Provider,
        config: &StreamCheckConfig,
        base_url_override: Option<String>,
        start: Instant,
    ) -> Result<StreamCheckResult, AppError> {
        let base_url = match base_url_override {
            Some(b) => b,
            None => Self::resolve_base_url(app_type, provider)?,
        };

        let client = crate::proxy::http_client::get();
        let timeout = std::time::Duration::from_secs(config.timeout_secs);
        let ua = Self::custom_user_agent(provider);

        let result = Self::probe_reachability(&client, &base_url, timeout, ua).await;
        let response_time = start.elapsed().as_millis() as u64;
        Ok(Self::build_result(
            result,
            response_time,
            config.degraded_threshold_ms,
        ))
    }

    /// 解析供应商 `base_url`。
    ///
    /// 连通性探测只需打到 base（origin 或用户配置的 base 路径）即可——任何 HTTP
    /// 响应都证明端口可达，因此无需像旧的真实请求检查那样解析具体 API 路径
    /// （`/v1/messages` vs `/chat/completions` vs `:streamGenerateContent`）。
    ///
    /// 官方供应商（`category == "official"`）base_url 故意留空（走客户端默认/OAuth 端点），
    /// 没有 cc-switch 能可靠探测的目标——这类供应商的连通检测按钮在前端已隐藏
    /// （见 `ProviderCard.tsx`），故此处对其提取失败直接报错即可，不做官方端点回退。
    fn resolve_base_url(app_type: &AppType, provider: &Provider) -> Result<String, AppError> {
        if provider.category.as_deref() == Some("official") {
            return Err(AppError::Message(
                "Official providers do not expose a reachability-check target".to_string(),
            ));
        }

        match crate::apps::adapter(app_type).probe_base_url(provider) {
            Some(base_url) => base_url,
            None => get_adapter(app_type)
                .extract_base_url(provider)
                .map_err(|e| AppError::Message(format!("Failed to extract base_url: {e}"))),
        }
    }

    /// 轻量可达性探测：GET `base_url`，收到任意 HTTP 响应即可达。
    ///
    /// - `send()` 在收到响应头时即返回，故计时天然是 TTFB；不读 body。
    /// - reqwest 对任何 HTTP 状态码都返回 `Ok`，只有网络级错误进 `Err`——
    ///   这正是"任何响应都算可达、只有连不上才算失败"的语义。
    async fn probe_reachability(
        client: &Client,
        base_url: &str,
        timeout: std::time::Duration,
        custom_ua: Option<HeaderValue>,
    ) -> Result<u16, AppError> {
        let url = base_url.trim();
        if url.is_empty() {
            return Err(AppError::Message("base_url 为空".to_string()));
        }

        let mut req = client
            .get(url)
            .timeout(timeout)
            .header("accept", "*/*")
            .header("accept-encoding", "identity");
        // 复用供应商自定义 UA（部分网关按 UA 白名单放行），与转发路径口径一致。
        if let Some(ua) = custom_ua {
            req = req.header("user-agent", ua);
        }

        match req.send().await {
            Ok(resp) => Ok(resp.status().as_u16()),
            Err(e) => Err(Self::map_request_error(e)),
        }
    }

    /// 将探测原始结果包装成 `StreamCheckResult`。
    fn build_result(
        result: Result<u16, AppError>,
        response_time: u64,
        degraded_threshold_ms: u64,
    ) -> StreamCheckResult {
        let tested_at = chrono::Utc::now().timestamp();
        match result {
            Ok(status) => StreamCheckResult {
                status: Self::determine_status(response_time, degraded_threshold_ms),
                success: true,
                message: "Reachable".to_string(),
                response_time_ms: Some(response_time),
                http_status: Some(status),
                model_used: String::new(),
                tested_at,
                retry_count: 0,
                error_category: None,
            },
            Err(e) => StreamCheckResult {
                status: HealthStatus::Failed,
                success: false,
                message: e.to_string(),
                response_time_ms: Some(response_time),
                http_status: None,
                model_used: String::new(),
                tested_at,
                retry_count: 0,
                error_category: None,
            },
        }
    }

    fn determine_status(latency_ms: u64, threshold: u64) -> HealthStatus {
        if latency_ms <= threshold {
            HealthStatus::Operational
        } else {
            HealthStatus::Degraded
        }
    }

    fn should_retry(msg: &str) -> bool {
        let lower = msg.to_lowercase();
        lower.contains("timeout") || lower.contains("abort") || lower.contains("timed out")
    }

    fn map_request_error(e: reqwest::Error) -> AppError {
        if e.is_timeout() {
            AppError::Message("Request timeout".to_string())
        } else if e.is_connect() {
            AppError::Message(format!("Connection failed: {e}"))
        } else {
            AppError::Message(e.to_string())
        }
    }

    /// Provider 级自定义 User-Agent（`meta.customUserAgent`），与转发路径共用单一口径：
    /// trim、空串视为未设置、非法值静默忽略（返回 `None`）。
    fn custom_user_agent(provider: &Provider) -> Option<HeaderValue> {
        provider
            .meta
            .as_ref()
            .and_then(|meta| meta.custom_user_agent_header().ok().flatten())
    }
}

/// 命令层入口：读取配置与供应商、解析 Copilot 端点并记录检查日志（CCS-181）
impl StreamCheckService {
    /// 连通性检查（单个供应商）
    pub async fn check_provider(
        state: &crate::store::AppState,
        copilot_state: &crate::commands::CopilotAuthState,
        app_type: &AppType,
        provider_id: &str,
    ) -> Result<StreamCheckResult, AppError> {
        let config = state.db.get_stream_check_config()?;
        let providers = state.db.get_all_providers(app_type.as_str())?;
        let provider = providers
            .get(provider_id)
            .ok_or_else(|| AppError::Message(format!("供应商 {provider_id} 不存在")))?;

        // Copilot 端点是动态的（随 OAuth token 解析），需预先取出 host 再探测；
        // 其余供应商传 None，由服务层从 settings_config 提取 base_url。无需鉴权。
        let base_url_override =
            Self::resolve_copilot_base_url_override(provider, copilot_state).await?;
        let result = Self::check_with_retry(app_type, provider, &config, base_url_override).await?;

        let _ =
            state
                .db
                .save_stream_check_log(provider_id, &provider.name, app_type.as_str(), &result);
        Ok(result)
    }

    /// 批量连通性检查；`proxy_targets_only` 时只检查当前供应商与故障转移队列
    pub async fn check_all_providers(
        state: &crate::store::AppState,
        copilot_state: &crate::commands::CopilotAuthState,
        app_type: &AppType,
        proxy_targets_only: bool,
    ) -> Result<Vec<(String, StreamCheckResult)>, AppError> {
        let config = state.db.get_stream_check_config()?;
        let providers = state.db.get_all_providers(app_type.as_str())?;
        let allowed_ids = proxy_targets_only.then(|| Self::proxy_target_ids(state, app_type));

        let mut results = Vec::new();
        for (id, provider) in providers {
            // Official OAuth providers intentionally have no user-configured probe
            // target. Never turn their runtime adapter defaults into unauthenticated
            // network probes against first-party endpoints.
            if provider.category.as_deref() == Some("official") {
                continue;
            }
            if let Some(ids) = &allowed_ids {
                if !ids.contains(&id) {
                    continue;
                }
            }

            let base_url_override =
                Self::resolve_copilot_base_url_override(&provider, copilot_state).await?;
            let result = Self::check_with_retry(app_type, &provider, &config, base_url_override)
                .await
                .unwrap_or_else(|e| StreamCheckResult {
                    status: HealthStatus::Failed,
                    success: false,
                    message: e.to_string(),
                    response_time_ms: None,
                    http_status: None,
                    model_used: String::new(),
                    tested_at: chrono::Utc::now().timestamp(),
                    retry_count: 0,
                    error_category: None,
                });

            let _ = state
                .db
                .save_stream_check_log(&id, &provider.name, app_type.as_str(), &result);
            results.push((id, result));
        }
        Ok(results)
    }

    /// 代理实际会路由到的供应商：当前供应商 + 故障转移队列
    fn proxy_target_ids(
        state: &crate::store::AppState,
        app_type: &AppType,
    ) -> std::collections::HashSet<String> {
        let mut ids = std::collections::HashSet::new();
        if let Ok(Some(current_id)) = state.db.get_current_provider(app_type.as_str()) {
            ids.insert(current_id);
        }
        if let Ok(queue) = state.db.get_failover_queue(app_type.as_str()) {
            ids.extend(queue.into_iter().map(|item| item.provider_id));
        }
        ids
    }

    /// 连通性检查配置
    pub fn config(state: &crate::store::AppState) -> Result<StreamCheckConfig, AppError> {
        state.db.get_stream_check_config()
    }

    /// 保存连通性检查配置
    pub fn save_config(
        state: &crate::store::AppState,
        config: &StreamCheckConfig,
    ) -> Result<(), AppError> {
        state.db.save_stream_check_config(config)
    }

    /// Copilot 供应商的 base_url 需要从 OAuth 管理器动态解析（按账号或默认端点）。
    /// `is_full_url` 的供应商已是完整地址，无需解析。
    pub async fn resolve_copilot_base_url_override(
        provider: &Provider,
        copilot_state: &crate::commands::CopilotAuthState,
    ) -> Result<Option<String>, AppError> {
        let is_full_url = provider
            .meta
            .as_ref()
            .and_then(|meta| meta.is_full_url)
            .unwrap_or(false);
        if !is_copilot_provider(provider) || is_full_url {
            return Ok(None);
        }

        let auth_manager = copilot_state.0.read().await;
        let account_id = provider
            .meta
            .as_ref()
            .and_then(|meta| meta.managed_account_id_for("github_copilot"));
        let endpoint = match account_id.as_deref() {
            Some(id) => auth_manager.get_api_endpoint(id).await,
            None => auth_manager.get_default_api_endpoint().await,
        };
        Ok(Some(endpoint))
    }
}

fn is_copilot_provider(provider: &Provider) -> bool {
    provider
        .meta
        .as_ref()
        .and_then(|meta| meta.provider_type.as_deref())
        == Some("github_copilot")
        || provider
            .settings_config
            .pointer("/env/ANTHROPIC_BASE_URL")
            .and_then(|value| value.as_str())
            .map(|url| url.contains("githubcopilot.com"))
            .unwrap_or(false)
}

#[cfg(test)]
mod copilot_detection_tests {
    use super::is_copilot_provider;
    use crate::provider::{Provider, ProviderMeta};
    use serde_json::json;

    #[test]
    fn copilot_provider_detection_accepts_provider_type_or_base_url() {
        let typed_provider = Provider {
            id: "p1".to_string(),
            name: "typed".to_string(),
            settings_config: json!({}),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: Some(ProviderMeta {
                provider_type: Some("github_copilot".to_string()),
                ..Default::default()
            }),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };
        assert!(is_copilot_provider(&typed_provider));

        let url_provider = Provider {
            id: "p2".to_string(),
            name: "url".to_string(),
            settings_config: json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.githubcopilot.com"
                }
            }),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };
        assert!(is_copilot_provider(&url_provider));
    }

    #[test]
    fn copilot_full_url_metadata_is_available_for_override_guard() {
        let provider = Provider {
            id: "p3".to_string(),
            name: "relay".to_string(),
            settings_config: json!({}),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: Some(ProviderMeta {
                provider_type: Some("github_copilot".to_string()),
                is_full_url: Some(true),
                ..Default::default()
            }),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };

        assert!(is_copilot_provider(&provider));
        assert_eq!(
            provider.meta.as_ref().and_then(|meta| meta.is_full_url),
            Some(true)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_provider(settings_config: serde_json::Value) -> Provider {
        Provider::with_id(
            "test".to_string(),
            "Test".to_string(),
            settings_config,
            None,
        )
    }

    #[test]
    fn test_default_config_uses_reachability_friendly_values() {
        let config = StreamCheckConfig::default();
        assert_eq!(config.timeout_secs, 8);
        assert_eq!(config.max_retries, 1);
        // 降级阈值沿用旧尺度，避免把 1 秒多的正常延迟误判为"较慢"
        assert_eq!(config.degraded_threshold_ms, 6000);
    }

    #[test]
    fn test_circuit_probe_treats_5xx_and_429_as_unavailable() {
        for status in [200, 204, 301, 401, 403, 404, 405] {
            assert!(
                StreamCheckService::probe_status_outcome(status).is_ok(),
                "HTTP {status} means the gateway is alive"
            );
        }
        for status in [429, 500, 502, 503, 504] {
            assert!(
                StreamCheckService::probe_status_outcome(status).is_err(),
                "HTTP {status} keeps the breaker open"
            );
        }
    }

    #[test]
    fn test_determine_status() {
        assert_eq!(
            StreamCheckService::determine_status(1000, 1500),
            HealthStatus::Operational
        );
        assert_eq!(
            StreamCheckService::determine_status(1500, 1500),
            HealthStatus::Operational
        );
        assert_eq!(
            StreamCheckService::determine_status(1501, 1500),
            HealthStatus::Degraded
        );
    }

    #[test]
    fn test_should_retry_only_on_timeout_like_errors() {
        assert!(StreamCheckService::should_retry("Request timeout"));
        assert!(StreamCheckService::should_retry("request timed out"));
        assert!(StreamCheckService::should_retry("connection abort"));
        // 连接被拒 / DNS 失败不重试
        assert!(!StreamCheckService::should_retry(
            "Connection failed: dns error"
        ));
        assert!(!StreamCheckService::should_retry("Reachable"));
    }

    #[test]
    fn test_build_result_any_http_status_is_reachable() {
        // 任何 HTTP 状态码都算可达（success=true）
        for status in [200u16, 401, 403, 404, 429, 500, 503] {
            let r = StreamCheckService::build_result(Ok(status), 100, 1500);
            assert!(r.success, "status {status} should be reachable");
            assert_eq!(r.status, HealthStatus::Operational);
            assert_eq!(r.http_status, Some(status));
            assert!(r.model_used.is_empty());
            assert!(r.error_category.is_none());
        }
    }

    #[test]
    fn test_build_result_network_error_is_unreachable() {
        let r = StreamCheckService::build_result(
            Err(AppError::Message("Connection failed: refused".to_string())),
            5,
            1500,
        );
        assert!(!r.success);
        assert_eq!(r.status, HealthStatus::Failed);
        assert!(r.http_status.is_none());
    }

    #[test]
    fn test_build_result_slow_response_is_degraded() {
        let r = StreamCheckService::build_result(Ok(200), 3000, 1500);
        assert!(r.success);
        assert_eq!(r.status, HealthStatus::Degraded);
    }

    #[test]
    fn test_resolve_opencode_base_url_explicit_wins() {
        let p = make_provider(serde_json::json!({
            "npm": "@ai-sdk/openai",
            "options": { "baseURL": "https://proxy.local/v1", "apiKey": "k" },
            "models": {},
        }));
        let resolved = StreamCheckService::resolve_base_url(&AppType::OpenCode, &p).unwrap();
        assert_eq!(resolved, "https://proxy.local/v1");
    }

    #[test]
    fn test_resolve_opencode_base_url_falls_back_for_known_npm() {
        let p = make_provider(serde_json::json!({
            "npm": "@ai-sdk/anthropic",
            "options": { "apiKey": "k" },
            "models": {},
        }));
        let resolved = StreamCheckService::resolve_base_url(&AppType::OpenCode, &p).unwrap();
        assert_eq!(resolved, "https://api.anthropic.com");
    }

    #[test]
    fn test_resolve_opencode_base_url_errors_for_openai_compatible_without_url() {
        let p = make_provider(serde_json::json!({
            "npm": "@ai-sdk/openai-compatible",
            "options": { "apiKey": "k" },
            "models": {},
        }));
        let result = StreamCheckService::resolve_base_url(&AppType::OpenCode, &p);
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_openclaw_base_url_missing_errors() {
        let p = make_provider(serde_json::json!({ "apiKey": "k", "api": "openai-completions" }));
        assert!(StreamCheckService::resolve_base_url(&AppType::OpenClaw, &p).is_err());

        let p2 = make_provider(serde_json::json!({ "baseUrl": "https://api.deepseek.com/v1" }));
        assert_eq!(
            StreamCheckService::resolve_base_url(&AppType::OpenClaw, &p2).unwrap(),
            "https://api.deepseek.com/v1"
        );
    }

    #[test]
    fn test_resolve_base_url_dispatches_through_app_adapters() {
        let hermes =
            make_provider(serde_json::json!({ "base_url": " https://hermes.example/v1 " }));
        assert_eq!(
            StreamCheckService::resolve_base_url(&AppType::Hermes, &hermes).unwrap(),
            "https://hermes.example/v1"
        );
        let hermes_camel = make_provider(serde_json::json!({ "baseUrl": "https://x.example" }));
        assert!(StreamCheckService::resolve_base_url(&AppType::Hermes, &hermes_camel).is_err());

        let anthropic = make_provider(
            serde_json::json!({ "env": { "ANTHROPIC_BASE_URL": "https://relay.example" } }),
        );
        assert_eq!(
            StreamCheckService::resolve_base_url(&AppType::ClaudeDesktop, &anthropic).unwrap(),
            "https://relay.example"
        );

        let gemini = make_provider(serde_json::json!({
            "env": { "GOOGLE_GEMINI_BASE_URL": "https://gemini.example", "GEMINI_API_KEY": "k" }
        }));
        assert_eq!(
            StreamCheckService::resolve_base_url(&AppType::Gemini, &gemini).unwrap(),
            "https://gemini.example"
        );
    }

    #[test]
    fn test_resolve_base_url_uses_explicit_url_or_errors_when_missing() {
        // 有显式 base_url → 直接用
        let p = make_provider(
            serde_json::json!({ "env": { "ANTHROPIC_BASE_URL": "https://relay.example/v1" } }),
        );
        assert_eq!(
            StreamCheckService::resolve_base_url(&AppType::Claude, &p).unwrap(),
            "https://relay.example/v1"
        );

        // 缺 base_url（官方留空 / 用户忘填）→ 报错。官方供应商的检测按钮在前端已隐藏，
        // 不会走到这里；不做官方端点回退（避免给忘填地址的第三方误显绿灯）。
        let empty = make_provider(serde_json::json!({ "env": {} }));
        assert!(StreamCheckService::resolve_base_url(&AppType::Claude, &empty).is_err());

        let mut official = make_provider(serde_json::json!({ "auth": {}, "config": "" }));
        official.id = crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string();
        official.category = Some("official".to_string());
        assert!(StreamCheckService::resolve_base_url(&AppType::Codex, &official).is_err());
    }
}
