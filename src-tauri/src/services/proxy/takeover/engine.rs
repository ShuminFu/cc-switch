//! 统一接管引擎：`takeover(app, Strict | BestEffort)`
//!
//! 取代此前按应用展开的三个接管变体（全部接管 / 严格 / 尽力而为）。各应用只在
//! 投影接管字段与写入 Live 两处有差异；代理地址统一由 [`ProxyService::takeover_base_url`]
//! 依据注册表的 `proxy_route` 与适配器的 [`crate::apps::ProxyTakeover::base_url`] 生成。

use super::claude::ClaudeTakeoverAuthPolicy;
use super::*;
use crate::app_config::AppType;
use crate::apps::registry::{self, AppCaps};
use crate::provider::Provider;
use serde_json::Value;

/// 接管模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TakeoverMode {
    /// 目标应用的 Live 配置必须存在且可读，当前供应商必须存在
    Strict,
    /// Live 配置缺失或不可读时跳过该应用；可以不依赖供应商投影的应用（Claude）
    /// 在缺少当前供应商时按现有 Live 的凭据字段写入占位符
    BestEffort,
}

/// 接管失败原因
///
/// `Display` 输出与旧实现返回的错误字符串一致，命令层与前端提示保持不变。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum TakeoverError {
    /// 应用不支持代理接管（无 `PROXY_TAKEOVER` 能力 / 适配器 / 代理路由）
    #[error("该应用不支持代理功能")]
    Unsupported(AppType),
    /// 无法生成代理地址（读取代理配置失败、端口为 0 且代理未运行等）
    #[error("{0}")]
    ProxyAddress(String),
    /// 严格模式下目标应用的 Live 配置缺失或不可读
    #[error("{message}")]
    LiveUnavailable { app: AppType, message: String },
    /// 当前供应商缺失或读取失败
    #[error("{message}")]
    Provider { app: AppType, message: String },
    /// 生成接管字段失败（配置结构异常等）
    #[error("{message}")]
    Projection { app: AppType, message: String },
    /// 写入 Live 配置失败
    #[error("{message}")]
    Write { app: AppType, message: String },
    /// 旧接管路径（`takeoverEngine = "legacy"`）返回的错误
    #[error("{message}")]
    Legacy {
        app: Option<AppType>,
        message: String,
    },
}

impl From<TakeoverError> for String {
    fn from(err: TakeoverError) -> Self {
        err.to_string()
    }
}

impl ProxyService {
    /// 接管指定应用的 Live 配置（写入代理地址与 token 占位符）
    pub(crate) async fn takeover(
        &self,
        app: &AppType,
        mode: TakeoverMode,
    ) -> Result<(), TakeoverError> {
        // 累加模式应用的条目级接管晚于引擎合并，旧路径没有对应实现
        let entry_level = super::additive::entry_takeover(app).is_some();
        if !entry_level && !crate::settings::takeover_engine().uses_unified_takeover() {
            let legacy = match mode {
                TakeoverMode::Strict => self.takeover_live_config_strict_legacy(app).await,
                TakeoverMode::BestEffort => self.takeover_live_config_best_effort_legacy(app).await,
            };
            return legacy.map_err(|message| TakeoverError::Legacy {
                app: Some(app.clone()),
                message,
            });
        }

        let base_url = self.takeover_base_url(app).await?;
        let mut live_config = match self.read_live_for_takeover(app) {
            Ok(config) => config,
            Err(message) => {
                return match mode {
                    TakeoverMode::Strict => Err(TakeoverError::LiveUnavailable {
                        app: app.clone(),
                        message,
                    }),
                    TakeoverMode::BestEffort => {
                        log::debug!("{} Live 配置不可用，跳过接管: {message}", app.as_str());
                        Ok(())
                    }
                };
            }
        };

        // 累加模式应用只接管选中的供应商条目
        if entry_level {
            return self.takeover_entry(app, mode, &base_url, &live_config);
        }

        let provider = self.takeover_provider(app, mode)?;
        self.project_takeover_live(app, &mut live_config, &base_url, provider.as_ref())?;
        self.write_takeover_live(app, &live_config, provider.as_ref())?;
        log::info!(
            "{} Live 配置已接管，代理地址: {base_url}",
            registry::descriptor(app).display_name
        );
        Ok(())
    }

    /// 尽力接管所有支持代理接管的应用（`start_with_takeover` 使用）
    pub(in crate::services::proxy) async fn takeover_all(&self) -> Result<(), TakeoverError> {
        if !crate::settings::takeover_engine().uses_unified_takeover() {
            self.takeover_live_configs_legacy()
                .await
                .map_err(|message| TakeoverError::Legacy { app: None, message })?;
            for descriptor in registry::with_caps(AppCaps::PROXY_TAKEOVER)
                .filter(|d| super::additive::entry_takeover(&d.app).is_some())
            {
                self.takeover(&descriptor.app, TakeoverMode::BestEffort)
                    .await?;
            }
            return Ok(());
        }

        for descriptor in registry::with_caps(AppCaps::PROXY_TAKEOVER) {
            self.takeover(&descriptor.app, TakeoverMode::BestEffort)
                .await?;
        }
        Ok(())
    }

    /// 接管时写入该应用 Live 的代理基础地址
    pub(crate) async fn takeover_base_url(&self, app: &AppType) -> Result<String, TakeoverError> {
        let descriptor = registry::descriptor(app);
        let route = descriptor
            .proxy_route
            .filter(|_| descriptor.has(AppCaps::PROXY_TAKEOVER));
        let unsupported = || TakeoverError::Unsupported(app.clone());
        let route = route.ok_or_else(unsupported)?;
        if crate::apps::adapter(app).takeover().is_none() {
            return Err(unsupported());
        }
        let origin = self
            .proxy_origin()
            .await
            .map_err(TakeoverError::ProxyAddress)?;
        // 适配器引用（`&dyn ProxyTakeover`，非 `Sync`）不能跨越 await 持有
        crate::apps::adapter(app)
            .takeover()
            .map(|takeover| takeover.base_url(origin.trim_end_matches('/'), route))
            .ok_or_else(unsupported)
    }

    /// 接管投影需要的当前供应商；Gemini / Grok Build 的投影不依赖供应商
    fn takeover_provider(
        &self,
        app: &AppType,
        mode: TakeoverMode,
    ) -> Result<Option<Provider>, TakeoverError> {
        let provider_error = |message| TakeoverError::Provider {
            app: app.clone(),
            message,
        };
        match app {
            AppType::Claude if mode == TakeoverMode::BestEffort => {
                Ok(self.get_current_provider_for_app(app).ok().flatten())
            }
            AppType::Claude | AppType::Codex => self
                .require_current_provider_for_app(app)
                .map(Some)
                .map_err(provider_error),
            _ => Ok(None),
        }
    }

    /// 把接管字段（代理地址、token 占位符、模型映射等）投影到 Live 配置上
    fn project_takeover_live(
        &self,
        app: &AppType,
        live_config: &mut Value,
        base_url: &str,
        provider: Option<&Provider>,
    ) -> Result<(), TakeoverError> {
        let projection_error = |message| TakeoverError::Projection {
            app: app.clone(),
            message,
        };
        match (app, provider) {
            (AppType::Claude, Some(provider)) => {
                let provider = self
                    .claude_provider_with_effective_settings(provider)
                    .map_err(projection_error)?;
                Self::apply_claude_takeover_fields_for_provider(live_config, base_url, &provider);
            }
            (AppType::Claude, None) => Self::apply_claude_takeover_fields_with_policy(
                live_config,
                base_url,
                ClaudeTakeoverAuthPolicy::PreserveExistingOrAuthToken,
            ),
            (AppType::Codex, Some(provider)) => {
                Self::apply_codex_takeover_fields_for_provider(live_config, base_url, provider)
                    .map_err(projection_error)?
            }
            (AppType::Gemini, _) => Self::apply_gemini_takeover_fields(live_config, base_url),
            (AppType::GrokBuild, _) => {
                Self::apply_grok_takeover_fields(live_config, base_url).map_err(projection_error)?
            }
            _ => return Err(TakeoverError::Unsupported(app.clone())),
        }
        Ok(())
    }

    fn write_takeover_live(
        &self,
        app: &AppType,
        live_config: &Value,
        provider: Option<&Provider>,
    ) -> Result<(), TakeoverError> {
        let written = match app {
            AppType::Codex => self.write_codex_takeover_live_for_provider(live_config, provider),
            _ => self.write_live_config_for_app(app, live_config),
        };
        written.map_err(|message| TakeoverError::Write {
            app: app.clone(),
            message,
        })
    }

    /// Live 中写入的代理地址是否与 `expected` 一致
    pub(in crate::services::proxy) fn live_base_url_matches(
        app: &AppType,
        config: &Value,
        expected: &str,
    ) -> bool {
        let env_url_matches = |key: &str| {
            config
                .get("env")
                .and_then(|value| value.get(key))
                .and_then(Value::as_str)
                .is_some_and(|url| Self::proxy_urls_match(url, expected))
        };
        let config_text = config.get("config").and_then(Value::as_str);
        match app {
            AppType::Claude => env_url_matches("ANTHROPIC_BASE_URL"),
            AppType::Gemini => env_url_matches("GOOGLE_GEMINI_BASE_URL"),
            AppType::Codex => config_text.is_some_and(|text| {
                Self::codex_config_has_base_url_matching(text, |url| {
                    Self::proxy_urls_match(url, expected)
                })
            }),
            AppType::GrokBuild => config_text.is_some_and(|text| {
                crate::grok_config::base_url_matches(text, |url| {
                    Self::proxy_urls_match(url, expected)
                })
            }),
            _ => Self::entries_base_url_matches(app, config, expected),
        }
    }
}
