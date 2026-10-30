//! 供应商用量查询（CCS-181 自命令层迁入）
//!
//! 按用量脚本的模板类型分派：GitHub Copilot 配额、Coding Plan、官方余额、
//! 官方订阅额度，其余走通用 JS 脚本。

use std::str::FromStr;

use crate::app_config::AppType;
use crate::commands::CopilotAuthState;
use crate::ctx::Ctx;
use crate::provider::{Provider, UsageResult};
use crate::services::ProviderService;

const TEMPLATE_TYPE_GITHUB_COPILOT: &str = "github_copilot";
const TEMPLATE_TYPE_TOKEN_PLAN: &str = "token_plan";
const TEMPLATE_TYPE_BALANCE: &str = "balance";
const TEMPLATE_TYPE_OFFICIAL_SUBSCRIPTION: &str = "official_subscription";
const COPILOT_UNIT_PREMIUM: &str = "requests";

/// 查询用量并在成功拿到快照时刷新用量缓存、通知前端、刷新托盘
///
/// 查询可能以两种形式失败：
///   1) 返回 Ok(UsageResult { success: false, .. }) —— 确定性失败（401、脚本
///      报错、未知供应商等）。写进 UsageCache 并刷新托盘，让
///      format_script_summary 的 success 守卫生效、suffix 自然消失。
///   2) 返回 Err(String) —— 瞬时传输失败（网络/超时）及 DB/Copilot fetch 等。
///      不写失败快照、不 emit：保留上一份托盘快照，与前端 react-query reject
///      保留上次 data 的语义一致；否则失败快照会经 useUsageCacheBridge 盲写
///      回 query 缓存，抹掉 reject 本该保留的旧值。
pub async fn refresh_provider_usage(
    ctx: &Ctx<'_>,
    copilot_state: &CopilotAuthState,
    app_type: AppType,
    provider_id: &str,
) -> Result<UsageResult, String> {
    let snapshot =
        query_provider_usage(ctx.state, copilot_state, app_type.clone(), provider_id).await?;
    ctx.events.emit(
        "usage-cache-updated",
        serde_json::json!({
            "kind": "script",
            "appType": app_type.as_str(),
            "providerId": provider_id,
            "data": &snapshot,
        }),
    );
    ctx.state
        .usage_cache
        .put_script(app_type, provider_id.to_string(), snapshot.clone());
    ctx.events.schedule_tray_refresh();
    Ok(snapshot)
}

/// 查询官方订阅额度
///
/// 读取 CLI 工具已有的 OAuth 凭据并调用官方 API 获取使用额度。
/// `Ok`（成功或确定性失败）写入 `UsageCache`、通知托盘刷新并 emit
/// `usage-cache-updated`，让前端 React Query 与托盘共享同一份最新数据；失败
/// 快照写入后 `format_subscription_summary` 会通过 `success=false` 守卫返回
/// `None`，托盘 suffix 自然消失，避免长期滞留旧配额数字。
/// `Err`（瞬时传输失败）不写快照、不 emit：保留上一份托盘快照，与前端
/// react-query reject 保留上次 data 的语义一致（emit 失败快照会经
/// `useUsageCacheBridge` 盲写回 query 缓存，抹掉本该保留的旧值）。
pub async fn refresh_subscription_quota(
    ctx: &Ctx<'_>,
    tool: &str,
) -> Result<crate::services::subscription::SubscriptionQuota, String> {
    let snapshot = crate::services::subscription::get_subscription_quota(tool).await?;
    if let Ok(app_type) = AppType::from_str(tool) {
        ctx.events.emit(
            "usage-cache-updated",
            serde_json::json!({
                "kind": "subscription",
                "appType": app_type.as_str(),
                "data": &snapshot,
            }),
        );
        ctx.state
            .usage_cache
            .put_subscription(app_type, snapshot.clone());
        ctx.events.schedule_tray_refresh();
    }
    Ok(snapshot)
}

/// Resolve `(base_url, api_key)` for native usage queries, delegating to the
/// per-app resolver on `Provider`. Missing provider → empty credentials.
fn resolve_native_credentials(app_type: &AppType, provider: Option<&Provider>) -> (String, String) {
    provider
        .map(|p| p.resolve_usage_credentials(app_type))
        .unwrap_or_default()
}

fn resolve_coding_plan_credentials(
    app_type: &AppType,
    provider: Option<&Provider>,
    usage_script: Option<&crate::provider::UsageScript>,
) -> (String, String) {
    let is_zenmux = usage_script
        .and_then(|s| s.coding_plan_provider.as_deref())
        .map(|provider| provider.eq_ignore_ascii_case("zenmux"))
        .unwrap_or(false);

    if !is_zenmux {
        return resolve_native_credentials(app_type, provider);
    }

    let script_base_url = usage_script
        .and_then(|s| s.base_url.as_deref())
        .unwrap_or("")
        .trim_end_matches('/')
        .to_string();
    let script_api_key = usage_script
        .and_then(|s| s.api_key.as_deref())
        .unwrap_or("")
        .to_string();

    if !script_base_url.is_empty() && !script_api_key.is_empty() {
        return (script_base_url, script_api_key);
    }

    let native = resolve_native_credentials(app_type, provider);
    if !native.0.is_empty() && !native.1.is_empty() {
        native
    } else {
        (script_base_url, script_api_key)
    }
}

pub async fn query_provider_usage(
    state: &crate::store::AppState,
    copilot_state: &CopilotAuthState,
    app_type: AppType,
    provider_id: &str,
) -> Result<crate::provider::UsageResult, String> {
    // 从数据库读取供应商信息，检查特殊模板类型
    let providers = state
        .db
        .get_all_providers(app_type.as_str())
        .map_err(|e| format!("Failed to get providers: {e}"))?;
    let provider = providers.get(provider_id);
    let usage_script = provider
        .and_then(|p| p.meta.as_ref())
        .and_then(|m| m.usage_script.as_ref());
    let template_type = usage_script
        .and_then(|s| s.template_type.as_deref())
        .unwrap_or("");

    // ── GitHub Copilot 专用路径 ──
    if template_type == TEMPLATE_TYPE_GITHUB_COPILOT {
        let copilot_account_id = provider
            .and_then(|p| p.meta.as_ref())
            .and_then(|m| m.managed_account_id_for(TEMPLATE_TYPE_GITHUB_COPILOT));

        let auth_manager = copilot_state.0.read().await;
        let usage = match copilot_account_id.as_deref() {
            Some(account_id) => auth_manager
                .fetch_usage_for_account(account_id)
                .await
                .map_err(|e| format!("Failed to fetch Copilot usage: {e}"))?,
            None => auth_manager
                .fetch_usage()
                .await
                .map_err(|e| format!("Failed to fetch Copilot usage: {e}"))?,
        };
        let premium = &usage.quota_snapshots.premium_interactions;
        let used = premium.entitlement - premium.remaining;

        return Ok(crate::provider::UsageResult {
            success: true,
            data: Some(vec![crate::provider::UsageData {
                plan_name: Some(usage.copilot_plan),
                remaining: Some(premium.remaining as f64),
                total: Some(premium.entitlement as f64),
                used: Some(used as f64),
                unit: Some(COPILOT_UNIT_PREMIUM.to_string()),
                is_valid: Some(true),
                invalid_message: None,
                extra: Some(format!("Reset: {}", usage.quota_reset_date)),
            }]),
            error: None,
        });
    }

    // ── Coding Plan 专用路径 ──
    if template_type == TEMPLATE_TYPE_TOKEN_PLAN {
        let (base_url, api_key) =
            resolve_coding_plan_credentials(&app_type, provider, usage_script);

        // 火山方舟用账号 AK/SK 签名查询用量（存于 usage_script，与推理 api_key 分离）；
        // 其他供应商为 None，service 层沿用 api_key。
        let access_key_id = usage_script.and_then(|s| s.access_key_id.clone());
        let secret_access_key = usage_script.and_then(|s| s.secret_access_key.clone());
        // 智谱团队版：显式 provider 标识 + 组织/项目 ID（与个人版智谱 base_url 相同，
        // 靠 coding_plan_provider == "zhipu_team" 在 service 层路由）。
        let coding_plan_provider = usage_script.and_then(|s| s.coding_plan_provider.clone());
        let team_organization_id = usage_script.and_then(|s| s.team_organization_id.clone());
        let team_project_id = usage_script.and_then(|s| s.team_project_id.clone());

        let quota = crate::services::coding_plan::get_coding_plan_quota(
            &base_url,
            &api_key,
            access_key_id.as_deref(),
            secret_access_key.as_deref(),
            coding_plan_provider.as_deref(),
            team_organization_id.as_deref(),
            team_project_id.as_deref(),
        )
        .await
        .map_err(|e| format!("Failed to query coding plan: {e}"))?;

        // 将 SubscriptionQuota 转换为 UsageResult
        if !quota.success {
            return Ok(crate::provider::UsageResult {
                success: false,
                data: None,
                error: quota.error,
            });
        }

        // ZenMux 的 tier 携带 USD 额度信息，需要编码为 JSON extra
        let has_usd = quota
            .tiers
            .first()
            .map(|t| t.used_value_usd.is_some())
            .unwrap_or(false);
        let plan_label = quota
            .credential_message
            .as_deref()
            .and_then(|msg| msg.split(' ').next())
            .map(|tier| format!("ZenMux·{}", tier.to_uppercase()));
        let mut first_tier = true;

        let data: Vec<crate::provider::UsageData> = quota
            .tiers
            .iter()
            .map(|tier| {
                let total = 100.0;
                let used = tier.utilization;
                let remaining = total - used;
                let extra = if has_usd {
                    let mut extra_json = serde_json::json!({
                        "resetsAt": tier.resets_at,
                    });
                    if let Some(v) = tier.used_value_usd {
                        extra_json["usedValueUsd"] = serde_json::json!(v);
                    }
                    if let Some(v) = tier.max_value_usd {
                        extra_json["maxValueUsd"] = serde_json::json!(v);
                    }
                    if first_tier {
                        if let Some(ref label) = plan_label {
                            extra_json["planLabel"] = serde_json::json!(label);
                        }
                        first_tier = false;
                    }
                    Some(extra_json.to_string())
                } else {
                    tier.resets_at.clone()
                };
                crate::provider::UsageData {
                    plan_name: Some(tier.name.clone()),
                    remaining: Some(remaining),
                    total: Some(total),
                    used: Some(used),
                    unit: Some("%".to_string()),
                    is_valid: Some(true),
                    invalid_message: None,
                    extra,
                }
            })
            .collect();

        return Ok(crate::provider::UsageResult {
            success: true,
            data: if data.is_empty() { None } else { Some(data) },
            error: None,
        });
    }

    // ── 官方余额查询路径 ──
    if template_type == TEMPLATE_TYPE_BALANCE {
        // 按 app 区分的凭据存储格式提取 Base URL 与 API Key
        let (base_url, api_key) = resolve_native_credentials(&app_type, provider);

        return crate::services::balance::get_balance(&base_url, &api_key)
            .await
            .map_err(|e| format!("Failed to query balance: {e}"));
    }

    // ── 官方订阅额度查询路径 ──
    if template_type == TEMPLATE_TYPE_OFFICIAL_SUBSCRIPTION {
        if !usage_script.map(|s| s.enabled).unwrap_or(false) {
            return Ok(crate::provider::UsageResult {
                success: false,
                data: None,
                error: Some("Usage query is disabled".to_string()),
            });
        }

        let quota = crate::services::subscription::get_subscription_quota(app_type.as_str())
            .await
            .map_err(|e| format!("Failed to query subscription quota: {e}"))?;

        if !quota.success {
            return Ok(crate::provider::UsageResult {
                success: false,
                data: None,
                error: quota.error.or(quota.credential_message),
            });
        }

        let data: Vec<crate::provider::UsageData> = quota
            .tiers
            .iter()
            .map(|tier| crate::provider::UsageData {
                plan_name: Some(tier.name.clone()),
                remaining: Some(100.0 - tier.utilization),
                total: Some(100.0),
                used: Some(tier.utilization),
                unit: Some("%".to_string()),
                is_valid: Some(true),
                invalid_message: None,
                extra: tier.resets_at.clone(),
            })
            .collect();

        return Ok(crate::provider::UsageResult {
            success: true,
            data: if data.is_empty() { None } else { Some(data) },
            error: None,
        });
    }

    // ── 通用 JS 脚本路径 ──
    ProviderService::query_usage(state, app_type, provider_id)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod native_query_credentials_tests {
    use super::{
        refresh_provider_usage, resolve_coding_plan_credentials, resolve_native_credentials,
        CopilotAuthState, Ctx, TEMPLATE_TYPE_OFFICIAL_SUBSCRIPTION,
    };
    use crate::app_config::AppType;
    use crate::provider::{Provider, UsageScript};
    use serde_json::json;

    fn usage_script(
        coding_plan_provider: Option<&str>,
        base_url: Option<&str>,
        api_key: Option<&str>,
    ) -> UsageScript {
        UsageScript {
            enabled: true,
            language: "javascript".to_string(),
            code: String::new(),
            timeout: Some(10),
            api_key: api_key.map(str::to_string),
            base_url: base_url.map(str::to_string),
            access_token: None,
            user_id: None,
            template_type: Some("token_plan".to_string()),
            auto_query_interval: None,
            coding_plan_provider: coding_plan_provider.map(str::to_string),
            access_key_id: None,
            secret_access_key: None,
            team_organization_id: None,
            team_project_id: None,
        }
    }

    #[test]
    fn delegates_to_provider_for_codex() {
        let provider = Provider::with_id(
            "test".to_string(),
            "Test".to_string(),
            json!({
                "auth": { "OPENAI_API_KEY": "sk-codex" },
                "config": "model_provider = \"deepseek\"\n\
                           [model_providers.deepseek]\n\
                           base_url = \"https://api.deepseek.com\"\n",
            }),
            None,
        );
        let (base_url, api_key) = resolve_native_credentials(&AppType::Codex, Some(&provider));
        assert_eq!(base_url, "https://api.deepseek.com");
        assert_eq!(api_key, "sk-codex");
    }

    #[test]
    fn missing_provider_yields_empty() {
        let (base_url, api_key) = resolve_native_credentials(&AppType::Codex, None);
        assert!(base_url.is_empty());
        assert!(api_key.is_empty());
    }

    #[test]
    fn zenmux_coding_plan_uses_script_credentials_first() {
        let provider = Provider::with_id(
            "test".to_string(),
            "Test".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://provider.zenmux.example/v1",
                    "ANTHROPIC_AUTH_TOKEN": "sk-provider"
                }
            }),
            None,
        );
        let script = usage_script(
            Some("zenmux"),
            Some("https://script.zenmux.example/api/usage/"),
            Some("sk-script"),
        );

        let (base_url, api_key) =
            resolve_coding_plan_credentials(&AppType::Claude, Some(&provider), Some(&script));

        assert_eq!(base_url, "https://script.zenmux.example/api/usage");
        assert_eq!(api_key, "sk-script");
    }

    #[test]
    fn zenmux_coding_plan_falls_back_to_provider_credentials() {
        let provider = Provider::with_id(
            "test".to_string(),
            "Test".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://provider.zenmux.example/v1",
                    "ANTHROPIC_AUTH_TOKEN": "sk-provider"
                }
            }),
            None,
        );
        let script = usage_script(Some("zenmux"), Some("https://script.zenmux.example"), None);

        let (base_url, api_key) =
            resolve_coding_plan_credentials(&AppType::Claude, Some(&provider), Some(&script));

        assert_eq!(base_url, "https://provider.zenmux.example/v1");
        assert_eq!(api_key, "sk-provider");
    }

    fn ctx_state() -> crate::store::AppState {
        crate::store::AppState::new(std::sync::Arc::new(
            crate::database::Database::memory().expect("memory db"),
        ))
    }

    fn copilot_state(dir: &std::path::Path) -> CopilotAuthState {
        CopilotAuthState(std::sync::Arc::new(tokio::sync::RwLock::new(
            crate::proxy::providers::copilot_auth::CopilotAuthManager::new(dir.to_path_buf()),
        )))
    }

    #[tokio::test]
    async fn deterministic_failure_is_cached_and_announced() {
        let state = ctx_state();
        let mut script = usage_script(None, None, None);
        script.enabled = false;
        script.template_type = Some(TEMPLATE_TYPE_OFFICIAL_SUBSCRIPTION.to_string());
        let mut provider = Provider::with_id("p1".to_string(), "P1".to_string(), json!({}), None);
        provider.meta = Some(crate::provider::ProviderMeta {
            usage_script: Some(script),
            ..Default::default()
        });
        state
            .db
            .save_provider(AppType::Claude.as_str(), &provider)
            .expect("save provider");
        let dir = tempfile::tempdir().expect("tempdir");
        let sink = crate::ctx::RecordingEventSink::new();

        let result = refresh_provider_usage(
            &Ctx::new(&state, &sink),
            &copilot_state(dir.path()),
            AppType::Claude,
            "p1",
        )
        .await
        .expect("disabled query is a deterministic result");

        assert!(!result.success);
        assert_eq!(result.error.as_deref(), Some("Usage query is disabled"));
        let events = sink.events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "usage-cache-updated");
        assert_eq!(events[0].1["kind"], "script");
        assert_eq!(events[0].1["providerId"], "p1");
        assert_eq!(sink.tray_refreshes(), 1);
        assert_eq!(
            state
                .usage_cache
                .with_script(&AppType::Claude, "p1", |r| r.success),
            Some(false)
        );
    }

    #[tokio::test]
    async fn transient_failure_keeps_the_previous_snapshot() {
        let state = ctx_state();
        let dir = tempfile::tempdir().expect("tempdir");
        let sink = crate::ctx::RecordingEventSink::new();

        // 未知供应商走通用脚本路径并返回 Err：不写缓存、不发事件、不刷新托盘
        let result = refresh_provider_usage(
            &Ctx::new(&state, &sink),
            &copilot_state(dir.path()),
            AppType::Claude,
            "missing",
        )
        .await;

        assert!(result.is_err());
        assert!(sink.events().is_empty());
        assert_eq!(sink.tray_refreshes(), 0);
        assert!(state
            .usage_cache
            .with_script(&AppType::Claude, "missing", |_| ())
            .is_none());
    }
}
