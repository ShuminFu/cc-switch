//! 代理相关的持久化设置（CCS-181）
//!
//! 全局 / 应用级代理配置、计费倍率与计费模式、熔断器配置、整流器 / 优化器 /
//! 日志配置，以及代理模式下的供应商热切换校验。写入后需要热更新运行中代理
//! 的步骤在这里一并完成，命令层只做参数解析与错误转换。

use std::str::FromStr;

use crate::app_config::AppType;
use crate::error::AppError;
use crate::proxy::types::{
    AppProxyConfig, GlobalProxyConfig, LogConfig, OptimizerConfig, RectifierConfig,
};
use crate::proxy::CircuitBreakerConfig;
use crate::store::AppState;

pub struct ProxySettingsService;

impl ProxySettingsService {
    /// 全局代理配置（代理开关、监听地址、端口、日志开关）
    pub async fn global_config(state: &AppState) -> Result<GlobalProxyConfig, AppError> {
        state.db.get_global_proxy_config().await
    }

    /// 更新全局代理配置（同时写入各应用行）
    pub async fn update_global_config(
        state: &AppState,
        config: GlobalProxyConfig,
    ) -> Result<(), AppError> {
        state.db.update_global_proxy_config(config).await
    }

    /// 应用级代理配置（enabled、auto_failover、超时、熔断器等）
    pub async fn app_config(state: &AppState, app_type: &str) -> Result<AppProxyConfig, AppError> {
        state.db.get_proxy_config_for_app(app_type).await
    }

    /// 更新应用级代理配置，并热更新该应用运行中的熔断器配置
    pub async fn update_app_config(
        state: &AppState,
        config: AppProxyConfig,
    ) -> Result<(), AppError> {
        let app_type = config.app_type.clone();
        let circuit_config = CircuitBreakerConfig::from(&config);
        state.db.update_proxy_config_for_app(config).await?;
        state
            .proxy_service
            .update_circuit_breaker_config_for_app(&app_type, circuit_config)
            .await
            .map_err(AppError::Message)
    }

    /// 默认成本倍率
    pub async fn default_cost_multiplier(
        state: &AppState,
        app_type: &str,
    ) -> Result<String, AppError> {
        state.db.get_default_cost_multiplier(app_type).await
    }

    pub async fn set_default_cost_multiplier(
        state: &AppState,
        app_type: &str,
        value: &str,
    ) -> Result<(), AppError> {
        state.db.set_default_cost_multiplier(app_type, value).await
    }

    /// 计费模式来源（按请求模型或响应模型计价）
    pub async fn pricing_model_source(
        state: &AppState,
        app_type: &str,
    ) -> Result<String, AppError> {
        state.db.get_pricing_model_source(app_type).await
    }

    pub async fn set_pricing_model_source(
        state: &AppState,
        app_type: &str,
        value: &str,
    ) -> Result<(), AppError> {
        state.db.set_pricing_model_source(app_type, value).await
    }

    /// 熔断器配置
    pub async fn circuit_breaker_config(
        state: &AppState,
    ) -> Result<CircuitBreakerConfig, AppError> {
        state.db.get_circuit_breaker_config().await
    }

    /// 更新熔断器配置；代理运行中时热更新内存中的熔断器
    pub async fn update_circuit_breaker_config(
        state: &AppState,
        config: CircuitBreakerConfig,
    ) -> Result<(), AppError> {
        state.db.update_circuit_breaker_config(&config).await?;
        state
            .proxy_service
            .update_circuit_breaker_configs(config)
            .await
            .map_err(AppError::Message)
    }

    pub fn rectifier_config(state: &AppState) -> Result<RectifierConfig, AppError> {
        state.db.get_rectifier_config()
    }

    pub fn set_rectifier_config(
        state: &AppState,
        config: &RectifierConfig,
    ) -> Result<(), AppError> {
        state.db.set_rectifier_config(config)
    }

    pub fn optimizer_config(state: &AppState) -> Result<OptimizerConfig, AppError> {
        state.db.get_optimizer_config()
    }

    pub fn set_optimizer_config(
        state: &AppState,
        config: &OptimizerConfig,
    ) -> Result<(), AppError> {
        state.db.set_optimizer_config(config)
    }

    pub fn log_config(state: &AppState) -> Result<LogConfig, AppError> {
        state.db.get_log_config()
    }

    /// 保存日志配置并立即应用到当前进程的日志级别
    pub fn set_log_config(state: &AppState, config: &LogConfig) -> Result<(), AppError> {
        state.db.set_log_config(config)?;
        log::set_max_level(config.to_level_filter());
        log::info!(
            "日志配置已更新: enabled={}, level={}",
            config.enabled,
            config.level
        );
        Ok(())
    }

    /// 代理模式下切换供应商（热切换）
    ///
    /// Codex 内置的官方供应商可在接管模式下沿用客户端自身的 OpenAI 登录，
    /// 其他官方供应商仍不允许切入。
    pub async fn switch_proxy_provider(
        state: &AppState,
        app_type: &str,
        provider_id: &str,
    ) -> Result<(), AppError> {
        let provider = state
            .db
            .get_provider_by_id(provider_id, app_type)
            .map_err(|e| AppError::Message(format!("读取供应商失败: {e}")))?
            .ok_or_else(|| AppError::Message(format!("供应商不存在: {provider_id}")))?;
        let app = AppType::from_str(app_type)
            .map_err(|e| AppError::Message(format!("无效的应用类型: {e}")))?;
        if provider.category.as_deref() == Some("official")
            && !crate::services::provider::official_provider_supports_proxy_takeover(
                &app, &provider,
            )
        {
            return Err(AppError::Message(
                "代理接管模式下不能切换到官方供应商 (Cannot switch to official provider during proxy takeover)"
                    .to_string(),
            ));
        }

        state
            .proxy_service
            .switch_proxy_target(app_type, provider_id)
            .await
            .map_err(AppError::Message)
    }
}
