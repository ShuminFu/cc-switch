//! 托盘用量后缀：把 `UsageCache` 中的订阅 / 脚本结果格式化为子菜单标题后缀，
//! 以及悬停托盘时的后台用量刷新。

use tauri::Manager;

use super::TRAY_SECTIONS;
use crate::app_config::AppType;
use crate::store::AppState;

const TEMPLATE_TYPE_OFFICIAL_SUBSCRIPTION: &str = "official_subscription";
const H_TIER_NAMES: &[&str] = &[crate::services::subscription::TIER_FIVE_HOUR];
const W_TIER_NAMES: &[&str] = &[
    crate::services::subscription::TIER_WEEKLY_LIMIT,
    crate::services::subscription::TIER_SEVEN_DAY,
    crate::services::subscription::TIER_SEVEN_DAY_OPUS,
    crate::services::subscription::TIER_SEVEN_DAY_SONNET,
];
// 月窗口分组：火山方舟 Agent/Coding Plan 的月窗口（5h/周/月 三档），
// 以及 Codex 免费方案的 30 天窗口（#3651）——两者都归入 "m" 档，避免免费
// Codex 账号在托盘里空白（前端 footer 能看到、托盘却不显示的不对称）。
const M_TIER_NAMES: &[&str] = &[
    crate::services::subscription::TIER_MONTHLY,
    crate::services::subscription::TIER_THIRTY_DAY,
];
const GEMINI_PRO_TIER_NAMES: &[&str] = &[crate::services::subscription::TIER_GEMINI_PRO];
const GEMINI_FLASH_TIER_NAMES: &[&str] = &[crate::services::subscription::TIER_GEMINI_FLASH];
const GEMINI_FLASH_LITE_TIER_NAMES: &[&str] =
    &[crate::services::subscription::TIER_GEMINI_FLASH_LITE];
const TIER_LABEL_GROUPS: &[(&str, &[&str])] = &[
    ("h", H_TIER_NAMES),
    ("w", W_TIER_NAMES),
    ("m", M_TIER_NAMES),
    ("p", GEMINI_PRO_TIER_NAMES),
    ("f", GEMINI_FLASH_TIER_NAMES),
    ("l", GEMINI_FLASH_LITE_TIER_NAMES),
];

/// 配色阈值（与前端 `utilizationColor` 语义一致）。
const UTIL_WARN_PCT: f64 = 70.0;
const UTIL_DANGER_PCT: f64 = 90.0;

fn emoji_for_utilization(pct: f64) -> &'static str {
    if pct >= UTIL_DANGER_PCT {
        "\u{1F534}" // 🔴
    } else if pct >= UTIL_WARN_PCT {
        "\u{1F7E0}" // 🟠
    } else {
        "\u{1F7E2}" // 🟢
    }
}

fn format_subscription_summary(
    quota: &crate::services::subscription::SubscriptionQuota,
) -> Option<String> {
    if !quota.success {
        return None;
    }

    let entries: Vec<(&str, f64)> = quota
        .tiers
        .iter()
        .map(|tier| (tier.name.as_str(), tier.utilization))
        .collect();
    let parts = labeled_tier_parts(&entries);

    if parts.is_empty() {
        return None;
    }

    // 色标取所有已选 tier 里最高的利用率——用户更关心"离上限多近"。
    let worst = parts
        .iter()
        .map(|(_, u)| *u)
        .fold(f64::NEG_INFINITY, f64::max);
    if !worst.is_finite() {
        return None;
    }

    let emoji = emoji_for_utilization(worst);
    let body = parts
        .iter()
        .map(|(label, u)| format!("{label}{}%", u.round() as i64))
        .collect::<Vec<_>>()
        .join(" ");
    Some(format!("{emoji} {body}"))
}

fn labeled_tier_parts(entries: &[(&str, f64)]) -> Vec<(&'static str, f64)> {
    let mut parts = Vec::new();
    for &(label, tier_names) in TIER_LABEL_GROUPS {
        let max_utilization = entries
            .iter()
            .filter(|(name, _)| tier_names.contains(name))
            .map(|(_, utilization)| *utilization)
            .filter(|utilization| utilization.is_finite())
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        if let Some(utilization) = max_utilization {
            parts.push((label, utilization));
        }
    }
    parts
}

fn tier_pct(data: &crate::provider::UsageData) -> Option<f64> {
    match (data.used, data.total) {
        (Some(used), Some(total)) if total > 0.0 => Some(used / total * 100.0),
        _ => None,
    }
}

fn format_script_summary(result: &crate::provider::UsageResult) -> Option<String> {
    if !result.success {
        return None;
    }
    let data = result.data.as_ref()?;
    if data.is_empty() {
        return None;
    }

    // commands::provider 的 token_plan / official_subscription 分支都会把
    // SubscriptionQuota 的每个 tier 扁平化为一条 UsageData（plan_name 承载
    // tier 名），所以这里按 plan_name 恢复托盘短标签。其余 usage 结果
    //（Copilot / balance / 自定义脚本）走 fallback。
    let entries: Vec<(&str, f64)> = data
        .iter()
        .filter_map(|d| Some((d.plan_name.as_deref()?, tier_pct(d)?)))
        .collect();
    let parts = labeled_tier_parts(&entries);
    if !parts.is_empty() {
        let worst = parts
            .iter()
            .map(|(_, u)| *u)
            .fold(f64::NEG_INFINITY, f64::max);
        let emoji = emoji_for_utilization(worst);
        let body = parts
            .iter()
            .map(|(label, u)| format!("{label}{}%", u.round() as i64))
            .collect::<Vec<_>>()
            .join(" ");
        return Some(format!("{emoji} {body}"));
    }

    let first = data.first()?;
    let pct = tier_pct(first)?;
    let emoji = emoji_for_utilization(pct);
    let plan = first.plan_name.as_deref().unwrap_or("");
    let rounded = pct.round() as i64;
    if plan.is_empty() {
        Some(format!("{} {}%", emoji, rounded))
    } else {
        Some(format!("{} {} {}%", emoji, plan, rounded))
    }
}

fn provider_uses_official_subscription(provider: &crate::provider::Provider) -> bool {
    provider
        .meta
        .as_ref()
        .and_then(|m| m.usage_script.as_ref())
        .map(|script| {
            script.enabled
                && script.template_type.as_deref() == Some(TEMPLATE_TYPE_OFFICIAL_SUBSCRIPTION)
        })
        .unwrap_or(false)
}

pub(super) fn format_usage_suffix(
    app_state: &AppState,
    app_type: &AppType,
    provider: &crate::provider::Provider,
    provider_id: &str,
) -> Option<String> {
    // 当前脚本是否启用：禁用/删除时不再沿用旧 UsageCache 结果，
    // 并顺手 invalidate，防止后续重建继续命中过期数据。
    let is_official_provider = provider.category.as_deref() == Some("official");
    let can_use_script = provider.has_usage_script_enabled()
        && (!is_official_provider || provider_uses_official_subscription(provider));
    if can_use_script {
        // 脚本缓存优先（覆盖 Copilot/coding_plan/balance/自定义脚本），借用访问避免克隆整条 UsageResult。
        if let Some(Some(s)) =
            app_state
                .usage_cache
                .with_script(app_type, provider_id, format_script_summary)
        {
            return Some(format!(" · {s}"));
        }
        if provider_uses_official_subscription(provider) {
            if let Some(Some(s)) = app_state
                .usage_cache
                .with_subscription(app_type, format_subscription_summary)
            {
                return Some(format!(" · {s}"));
            }
        }
    } else {
        app_state
            .usage_cache
            .invalidate_script(app_type, provider_id);
    }

    if !provider_uses_official_subscription(provider) {
        app_state.usage_cache.invalidate_subscription(app_type);
    }
    None
}

static LAST_TRAY_USAGE_REFRESH: std::sync::Mutex<Option<std::time::Instant>> =
    std::sync::Mutex::new(None);
const MIN_TRAY_USAGE_REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);

/// 并行刷新每个可见 app "当前 provider" 的用量；成功 / 失败结果都通过各
/// command 的 write-through 逻辑写入 `UsageCache`，单次重建菜单由
/// `schedule_tray_refresh` 做合并。内部 10 秒节流防止鼠标悬停反复进出时
/// 雪崩请求；互斥锁被毒化时以上次状态为准继续推进，不会永久阻塞。
///
/// 刷新面与 `format_usage_suffix` 的展示面严格对齐 —— 每次悬停最多发
/// `TRAY_SECTIONS.len()` 次外部请求；只有显式启用的用量查询（含官方订阅、
/// coding_plan / balance / Copilot / 自定义脚本）才会发请求。
pub(crate) async fn refresh_all_usage_in_tray(app: &tauri::AppHandle) {
    use crate::commands::CopilotAuthState;
    use futures::future::join_all;

    {
        let mut guard = LAST_TRAY_USAGE_REFRESH
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let now = std::time::Instant::now();
        if let Some(last) = *guard {
            if now.duration_since(last) < MIN_TRAY_USAGE_REFRESH_INTERVAL {
                return;
            }
        }
        *guard = Some(now);
    }

    let Some(app_state) = app.try_state::<AppState>() else {
        return;
    };

    // 与 `create_tray_menu` 保持一致：用户隐藏的 app 不参与外部 API 查询，
    // 避免在未使用的 app 上浪费请求、撞 rate limit 或反复触发鉴权失败日志。
    let visible_apps = crate::settings::get_settings()
        .visible_apps
        .unwrap_or_default();

    let mut script_futures = Vec::new();

    for section in TRAY_SECTIONS.iter() {
        if !visible_apps.is_visible(&section.app_type) {
            continue;
        }

        let app_type_str = section.app_type.as_str();
        let log_name = section.log_name;

        // 解析 effective current provider；未设置 / 出错都静默跳过，
        // 与 create_tray_menu 的行为保持一致。
        let current_id =
            match crate::settings::get_effective_current_provider(&app_state.db, &section.app_type)
            {
                Ok(Some(id)) => id,
                Ok(None) => continue,
                Err(e) => {
                    log::warn!("[Tray] 读取{log_name}当前供应商失败: {e}");
                    continue;
                }
            };
        // 只需当前 provider —— by-id 查询避免把整个 app 的 provider 列表加载
        // 进内存（每次悬停 × 3 sections 的热路径）。
        let current = match app_state.db.get_provider_by_id(&current_id, app_type_str) {
            Ok(Some(p)) => p,
            Ok(None) => continue,
            Err(e) => {
                log::warn!("[Tray] 读取{log_name}当前供应商失败: {e}");
                continue;
            }
        };

        // 与 format_usage_suffix 同一优先级：只有显式启用的用量查询才发请求。
        let is_official_provider = current.category.as_deref() == Some("official");
        if current.has_usage_script_enabled()
            && (!is_official_provider || provider_uses_official_subscription(&current))
        {
            let app_clone = app.clone();
            let state = app.state::<AppState>();
            let copilot_state = app.state::<CopilotAuthState>();
            let provider_id = current_id.clone();
            let app_str = app_type_str.to_string();
            script_futures.push(async move {
                if let Err(e) = crate::commands::query_provider_usage(
                    app_clone,
                    state,
                    copilot_state,
                    provider_id.clone(),
                    app_str,
                )
                .await
                {
                    log::debug!("[Tray] 刷新{log_name}供应商 {provider_id} 用量失败: {e}");
                }
            });
        }
    }

    join_all(script_futures).await;
}

#[cfg(test)]
mod tests {
    use super::{format_script_summary, format_subscription_summary};
    use crate::provider::{UsageData, UsageResult};
    use crate::services::subscription::{
        CredentialStatus, QuotaTier, SubscriptionQuota, TIER_FIVE_HOUR, TIER_GEMINI_FLASH,
        TIER_GEMINI_FLASH_LITE, TIER_GEMINI_PRO, TIER_MONTHLY, TIER_SEVEN_DAY, TIER_SEVEN_DAY_OPUS,
        TIER_SEVEN_DAY_SONNET, TIER_THIRTY_DAY, TIER_WEEKLY_LIMIT,
    };

    fn make_quota(tool: &str, success: bool, tiers: Vec<QuotaTier>) -> SubscriptionQuota {
        SubscriptionQuota {
            tool: tool.to_string(),
            credential_status: CredentialStatus::Valid,
            credential_message: None,
            success,
            tiers,
            extra_usage: None,
            error: None,
            queried_at: Some(0),
        }
    }

    fn tier(name: &str, utilization: f64) -> QuotaTier {
        QuotaTier {
            name: name.to_string(),
            utilization,
            resets_at: None,
            used_value_usd: None,
            max_value_usd: None,
        }
    }

    #[test]
    fn claude_summary_uses_h_and_w_labels() {
        let quota = make_quota(
            "claude",
            true,
            vec![tier("five_hour", 9.0), tier("seven_day", 27.0)],
        );
        let s = format_subscription_summary(&quota).expect("should format");
        assert!(s.contains("h9%"), "expected h9% in {s}");
        assert!(s.contains("w27%"), "expected w27% in {s}");
    }

    #[test]
    fn gemini_summary_uses_p_and_f_labels() {
        let quota = make_quota(
            "gemini",
            true,
            vec![tier("gemini_pro", 15.0), tier("gemini_flash", 42.0)],
        );
        let s = format_subscription_summary(&quota).expect("should format");
        assert!(s.contains("p15%"), "expected p15% in {s}");
        assert!(s.contains("f42%"), "expected f42% in {s}");
    }

    #[test]
    fn gemini_summary_includes_all_three_tiers() {
        let quota = make_quota(
            "gemini",
            true,
            vec![
                tier("gemini_pro", 5.0),
                tier("gemini_flash", 42.0),
                tier("gemini_flash_lite", 80.0),
            ],
        );
        let s = format_subscription_summary(&quota).expect("should format");
        assert!(s.contains("p5%"), "expected p5% in {s}");
        assert!(s.contains("f42%"), "expected f42% in {s}");
        assert!(s.contains("l80%"), "expected l80% in {s}");
    }

    #[test]
    fn gemini_summary_lite_only_still_renders() {
        // flash_lite 如果是 API 返回的唯一 tier，仍应显示（避免前端 footer 能看到、
        // 托盘空白的不对称）。
        let quota = make_quota("gemini", true, vec![tier("gemini_flash_lite", 80.0)]);
        let s = format_subscription_summary(&quota).expect("should format");
        assert!(s.contains("l80%"), "expected l80% in {s}");
    }

    #[test]
    fn codex_summary_thirty_day_only_still_renders() {
        // Codex 免费方案的唯一 tier 是 30 天窗口。前端 footer 已能显示（TIER_I18N_KEYS
        // 有 "30_day"），托盘也必须能显示——否则就是这条不变量要防的非对称：footer
        // 能看到、托盘却空白。30_day 归入 "m" 月分组。见 #3651。
        let quota = make_quota("codex", true, vec![tier(TIER_THIRTY_DAY, 85.0)]);
        let s = format_subscription_summary(&quota).expect("should format");
        assert!(s.contains("m85%"), "expected m85% in {s}");
    }

    #[test]
    fn gemini_summary_emoji_reflects_highest_tier_including_lite() {
        // lite 是利用率最高的那条 → emoji 必须是红色，不能被 pro/flash 掩盖。
        let quota = make_quota(
            "gemini",
            true,
            vec![
                tier("gemini_pro", 10.0),
                tier("gemini_flash", 20.0),
                tier("gemini_flash_lite", 95.0),
            ],
        );
        let s = format_subscription_summary(&quota).unwrap();
        assert!(
            s.starts_with("\u{1F534}"),
            "expected red emoji (lite worst) in {s}"
        );
    }

    #[test]
    fn worst_emoji_reflects_highest_utilization() {
        // 🔴 = \u{1F534}; 任一 tier ≥ 90% 时预期显示红色。
        let quota = make_quota(
            "claude",
            true,
            vec![tier("five_hour", 10.0), tier("seven_day", 95.0)],
        );
        let s = format_subscription_summary(&quota).unwrap();
        assert!(s.starts_with("\u{1F534}"), "expected red emoji in {s}");
    }

    #[test]
    fn subscription_summary_week_aliases_use_highest_utilization() {
        let quota = make_quota(
            "claude",
            true,
            vec![
                tier(TIER_FIVE_HOUR, 10.0),
                tier(TIER_SEVEN_DAY_OPUS, 20.0),
                tier(TIER_SEVEN_DAY_SONNET, 95.0),
            ],
        );
        let s = format_subscription_summary(&quota).unwrap();
        assert!(s.contains("w95%"), "expected w95% in {s}");
        assert!(s.starts_with("\u{1F534}"), "expected red emoji in {s}");
    }

    #[test]
    fn failure_quota_returns_none() {
        let quota = make_quota("claude", false, vec![tier("five_hour", 50.0)]);
        assert!(format_subscription_summary(&quota).is_none());
    }

    #[test]
    fn unknown_tiers_return_none() {
        let quota = make_quota("claude", true, vec![tier("one_hour", 80.0)]);
        assert!(format_subscription_summary(&quota).is_none());
    }

    #[test]
    fn gemini_without_any_known_tiers_returns_none() {
        // 完全没有 pro/flash/flash_lite 三种 tier 的退化响应 → None。
        let quota = make_quota("gemini", true, vec![tier("some_future_tier", 80.0)]);
        assert!(format_subscription_summary(&quota).is_none());
    }

    fn usage_data(plan_name: Option<&str>, utilization: f64) -> UsageData {
        UsageData {
            plan_name: plan_name.map(String::from),
            extra: None,
            is_valid: Some(true),
            invalid_message: None,
            total: Some(100.0),
            used: Some(utilization),
            remaining: Some(100.0 - utilization),
            unit: Some("%".to_string()),
        }
    }

    fn usage_result(success: bool, data: Vec<UsageData>) -> UsageResult {
        UsageResult {
            success,
            data: if data.is_empty() { None } else { Some(data) },
            error: None,
        }
    }

    #[test]
    fn script_summary_token_plan_two_tiers() {
        let r = usage_result(
            true,
            vec![
                usage_data(Some(TIER_FIVE_HOUR), 12.0),
                usage_data(Some(TIER_WEEKLY_LIMIT), 80.0),
            ],
        );
        let s = format_script_summary(&r).expect("should format");
        assert!(s.contains("h12%"), "expected h12% in {s}");
        assert!(s.contains("w80%"), "expected w80% in {s}");
        assert!(s.starts_with("\u{1F7E0}"), "expected orange emoji in {s}");
    }

    #[test]
    fn script_summary_token_plan_worst_drives_emoji() {
        let r = usage_result(
            true,
            vec![
                usage_data(Some(TIER_FIVE_HOUR), 20.0),
                usage_data(Some(TIER_WEEKLY_LIMIT), 95.0),
            ],
        );
        let s = format_script_summary(&r).unwrap();
        assert!(s.starts_with("\u{1F534}"), "expected red emoji in {s}");
    }

    #[test]
    fn script_summary_token_plan_five_hour_only() {
        let r = usage_result(true, vec![usage_data(Some(TIER_FIVE_HOUR), 8.0)]);
        let s = format_script_summary(&r).expect("should format");
        assert!(s.contains("h8%"), "expected h8% in {s}");
        assert!(
            !s.contains("plan_name"),
            "plan_name should not leak into label: {s}"
        );
    }

    #[test]
    fn script_summary_token_plan_weekly_only() {
        let r = usage_result(true, vec![usage_data(Some(TIER_WEEKLY_LIMIT), 50.0)]);
        let s = format_script_summary(&r).expect("should format");
        assert!(s.contains("w50%"), "expected w50% in {s}");
    }

    #[test]
    fn script_summary_token_plan_volcengine_three_tiers_with_monthly() {
        // 火山方舟 Agent Plan 回 5h/周/月三档，托盘应包含 m（月）窗口，
        // 不再静默丢弃。
        let r = usage_result(
            true,
            vec![
                usage_data(Some(TIER_FIVE_HOUR), 25.0),
                usage_data(Some(TIER_WEEKLY_LIMIT), 30.0),
                usage_data(Some(TIER_MONTHLY), 42.0),
            ],
        );
        let s = format_script_summary(&r).expect("should format");
        assert!(s.contains("h25%"), "expected h25% in {s}");
        assert!(s.contains("w30%"), "expected w30% in {s}");
        assert!(s.contains("m42%"), "expected m42% in {s}");
    }

    #[test]
    fn script_summary_token_plan_monthly_only_renders_label_not_raw_name() {
        // 仅月窗口激活时不应回落到原始 "monthly" 机器名，而是走 m 标签。
        let r = usage_result(true, vec![usage_data(Some(TIER_MONTHLY), 60.0)]);
        let s = format_script_summary(&r).expect("should format");
        assert!(s.contains("m60%"), "expected m60% in {s}");
        assert!(
            !s.contains("monthly"),
            "raw tier name should not leak into label: {s}"
        );
    }

    #[test]
    fn script_summary_official_subscription_claude_uses_h_and_w_labels() {
        let r = usage_result(
            true,
            vec![
                usage_data(Some(TIER_FIVE_HOUR), 12.0),
                usage_data(Some(TIER_SEVEN_DAY), 80.0),
            ],
        );
        let s = format_script_summary(&r).expect("should format");
        assert!(s.contains("h12%"), "expected h12% in {s}");
        assert!(s.contains("w80%"), "expected w80% in {s}");
        assert!(
            !s.contains(TIER_SEVEN_DAY),
            "tier machine name should not leak into label: {s}"
        );
    }

    #[test]
    fn script_summary_week_aliases_use_highest_utilization() {
        let r = usage_result(
            true,
            vec![
                usage_data(Some(TIER_FIVE_HOUR), 10.0),
                usage_data(Some(TIER_SEVEN_DAY_OPUS), 20.0),
                usage_data(Some(TIER_SEVEN_DAY_SONNET), 95.0),
            ],
        );
        let s = format_script_summary(&r).unwrap();
        assert!(s.contains("w95%"), "expected w95% in {s}");
        assert!(s.starts_with("\u{1F534}"), "expected red emoji in {s}");
    }

    #[test]
    fn script_summary_official_subscription_gemini_uses_short_labels() {
        let r = usage_result(
            true,
            vec![
                usage_data(Some(TIER_GEMINI_PRO), 15.0),
                usage_data(Some(TIER_GEMINI_FLASH), 42.0),
                usage_data(Some(TIER_GEMINI_FLASH_LITE), 80.0),
            ],
        );
        let s = format_script_summary(&r).expect("should format");
        assert!(s.contains("p15%"), "expected p15% in {s}");
        assert!(s.contains("f42%"), "expected f42% in {s}");
        assert!(s.contains("l80%"), "expected l80% in {s}");
        assert!(
            !s.contains("gemini_"),
            "Gemini tier machine names should not leak into label: {s}"
        );
    }

    #[test]
    fn script_summary_single_bucket_fallback_with_plan_name() {
        let r = usage_result(true, vec![usage_data(Some("Copilot Pro"), 40.0)]);
        let s = format_script_summary(&r).expect("should format");
        assert!(s.contains("Copilot Pro"), "expected plan name in {s}");
        assert!(s.contains("40%"), "expected 40% in {s}");
        assert!(
            !s.contains("h40%"),
            "must not relabel non-token-plan data as h: {s}"
        );
    }

    #[test]
    fn script_summary_single_bucket_fallback_without_plan_name() {
        let r = usage_result(true, vec![usage_data(None, 15.0)]);
        let s = format_script_summary(&r).expect("should format");
        assert_eq!(s, "\u{1F7E2} 15%", "expected emoji + pct only, got {s}");
    }

    #[test]
    fn script_summary_failure_returns_none() {
        let r = usage_result(false, vec![usage_data(Some(TIER_FIVE_HOUR), 12.0)]);
        assert!(format_script_summary(&r).is_none());
    }

    #[test]
    fn script_summary_empty_data_returns_none() {
        let r = usage_result(true, vec![]);
        assert!(format_script_summary(&r).is_none());
    }
}
