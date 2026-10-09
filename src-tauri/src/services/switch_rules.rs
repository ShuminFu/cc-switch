//! 配额规则：订阅 / Coding Plan 窗口越过阈值时自动切换供应商，并在窗口重置后回切。
//!
//! 评估挂在两个已有的配额写入点上（官方订阅查询、Coding Plan 用量查询），不自带
//! 调度器。切换始终走 `ProviderService::switch`（接管模式下它内部走热切换），
//! 任何一次切换都会清掉该应用的待回切状态，规则触发后再写回自己的状态，因此
//! 手动 / 托盘 / 项目切换天然取消待回切。

use crate::app_config::AppType;
use crate::database::{SwitchRule, SwitchRuleState};
use crate::services::subscription::SubscriptionQuota;
use crate::store::AppState;
use std::collections::HashMap;
use std::str::FromStr;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

pub use crate::database::{SOURCE_CODING_PLAN, SOURCE_SUBSCRIPTION};

/// 快照超过此时长视为过期，不触发规则
pub const MAX_SNAPSHOT_AGE_SECS: i64 = 15 * 60;
/// 同一规则两次触发之间的最短间隔（防抖）
pub const MIN_COOLDOWN_SECS: i64 = 10 * 60;
/// resets_at 缺失时的兜底回切等待
pub const FALLBACK_REVERT_SECS: i64 = 5 * 60 * 60;
/// 窗口重置后再多等一会儿，避免供应商侧尚未真正清零
pub const REVERT_GRACE_SECS: i64 = 30;

#[derive(Debug, Clone, PartialEq)]
pub struct FireDecision {
    pub rule: SwitchRule,
    pub utilization: f64,
    pub resets_at: Option<i64>,
    pub reason: String,
}

pub fn now_secs() -> i64 {
    chrono::Utc::now().timestamp()
}

/// 解析 tier 的 ISO 8601 重置时间为 unix 秒
pub fn parse_resets_at(value: Option<&str>) -> Option<i64> {
    let raw = value?.trim();
    if raw.is_empty() {
        return None;
    }
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.timestamp())
}

/// 纯函数：根据规则、已触发状态与一份新鲜快照，决定哪些规则应当触发。
///
/// - 失败快照、过期快照不触发；
/// - 已有待回切状态的规则不重复触发（回切或手动切换清掉状态后才会再次评估）；
/// - 距上次触发不足 `MIN_COOLDOWN_SECS` 不触发。
pub fn evaluate(
    rules: &[SwitchRule],
    states: &[SwitchRuleState],
    app_type: &str,
    watched_provider_id: Option<&str>,
    quota: &SubscriptionQuota,
    now: i64,
) -> Vec<FireDecision> {
    if !quota.success {
        return Vec::new();
    }
    if let Some(queried_at_ms) = quota.queried_at {
        let age = now - queried_at_ms / 1000;
        if age > MAX_SNAPSHOT_AGE_SECS {
            return Vec::new();
        }
    }
    let armed: HashMap<&str, &SwitchRuleState> =
        states.iter().map(|s| (s.rule_id.as_str(), s)).collect();
    let expected_source = if watched_provider_id.is_some() {
        SOURCE_CODING_PLAN
    } else {
        SOURCE_SUBSCRIPTION
    };

    rules
        .iter()
        .filter(|rule| rule.enabled && rule.app_type == app_type)
        .filter(|rule| rule.source == expected_source)
        .filter(|rule| rule.watched_provider_id.as_deref() == watched_provider_id)
        .filter(|rule| !armed.contains_key(rule.id.as_str()))
        .filter(|rule| {
            rule.last_fired_at
                .map(|last| now - last >= MIN_COOLDOWN_SECS)
                .unwrap_or(true)
        })
        .filter_map(|rule| {
            let tier = quota.tiers.iter().find(|t| t.name == rule.tier_name)?;
            if tier.utilization < rule.threshold_pct {
                return None;
            }
            Some(FireDecision {
                rule: rule.clone(),
                utilization: tier.utilization,
                resets_at: parse_resets_at(tier.resets_at.as_deref()),
                reason: format!("{} {:.0}%", tier.name, tier.utilization),
            })
        })
        .collect()
}

/// 规则触发后的回切时刻
pub fn revert_at(fired_at: i64, resets_at: Option<i64>) -> i64 {
    match resets_at {
        Some(at) if at > fired_at => at + REVERT_GRACE_SECS,
        _ => fired_at + FALLBACK_REVERT_SECS,
    }
}

fn rules_enabled() -> bool {
    crate::settings::get_settings().switch_rules_enabled
}

/// 后台评估（fire-and-forget）：供配额写入点调用，不阻塞用量查询本身
pub fn spawn_evaluation(
    app: AppHandle,
    app_type: AppType,
    watched_provider_id: Option<String>,
    quota: SubscriptionQuota,
) {
    if !rules_enabled() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        match on_quota_snapshot(&app, app_type, watched_provider_id.as_deref(), &quota).await {
            Ok(fired) if fired > 0 => log::info!("[SwitchRules] 触发了 {fired} 条规则"),
            Ok(_) => {}
            Err(e) => log::warn!("[SwitchRules] 评估失败: {e}"),
        }
    });
}

/// 用一份新鲜快照评估并执行匹配的规则，返回触发条数
pub async fn on_quota_snapshot(
    app: &AppHandle,
    app_type: AppType,
    watched_provider_id: Option<&str>,
    quota: &SubscriptionQuota,
) -> Result<u32, String> {
    if !rules_enabled() {
        return Ok(0);
    }
    if app_type.is_additive_mode() {
        return Ok(0);
    }
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| "AppState unavailable".to_string())?;
    let app_key = app_type.as_str();
    let rules = state
        .db
        .list_switch_rules(Some(app_key))
        .map_err(|e| e.to_string())?;
    if rules.is_empty() {
        return Ok(0);
    }
    let states = state
        .db
        .list_switch_rule_states()
        .map_err(|e| e.to_string())?;
    let now = now_secs();
    let decisions = evaluate(&rules, &states, app_key, watched_provider_id, quota, now);
    if decisions.is_empty() {
        return Ok(0);
    }

    let providers = state
        .db
        .get_all_providers(app_key)
        .map_err(|e| e.to_string())?;
    let current = crate::services::ProviderService::current(state.inner(), app_type.clone())
        .map_err(|e| e.to_string())?;
    // 只有「正在使用被监控的额度」时才切换：官方订阅 ↔ 当前为官方供应商，
    // Coding Plan ↔ 当前就是该供应商。否则额度耗尽与当前流量无关。
    let current_is_watched = match watched_provider_id {
        Some(id) => current == id,
        None => providers
            .get(&current)
            .and_then(|p| p.category.as_deref())
            .map(|c| c == "official")
            .unwrap_or(false),
    };
    if !current_is_watched {
        log::debug!("[SwitchRules] {app_key} 当前供应商 {current} 不是被监控的额度来源，跳过");
        return Ok(0);
    }

    let mut fired = 0u32;
    for decision in decisions {
        let target = decision.rule.target_provider_id.clone();
        if target == current {
            continue;
        }
        if !providers.contains_key(&target) {
            log::warn!(
                "[SwitchRules] 规则 {} 的目标供应商 {target} 不存在，跳过",
                decision.rule.id
            );
            continue;
        }
        log::info!(
            "[SwitchRules] {app_key}: {} ≥ {:.0}% → 切换 {current} → {target}",
            decision.reason,
            decision.rule.threshold_pct
        );
        if let Err(e) = perform_switch(app, app_type.clone(), &target).await {
            log::error!("[SwitchRules] 切换到 {target} 失败: {e}");
            continue;
        }
        let fired_at = now_secs();
        let rule_state = SwitchRuleState {
            rule_id: decision.rule.id.clone(),
            fired_at,
            resets_at: decision.resets_at,
            switched_from: current.clone(),
            switched_to: target.clone(),
            last_utilization: decision.utilization,
            reason: Some(decision.reason.clone()),
        };
        if let Err(e) = state.db.set_switch_rule_state(&rule_state) {
            log::warn!("[SwitchRules] 记录规则状态失败: {e}");
        }
        if let Err(e) = state
            .db
            .set_switch_rule_last_fired(&decision.rule.id, fired_at)
        {
            log::warn!("[SwitchRules] 记录触发时间失败: {e}");
        }
        let provider_name = providers
            .get(&target)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| target.clone());
        emit_switched(
            app,
            app_key,
            &target,
            &provider_name,
            "rule",
            &decision.reason,
            &decision.rule.id,
        );
        crate::tray::schedule_tray_refresh(app);
        if decision.rule.revert_on_reset {
            arm_revert(
                app.clone(),
                decision.rule.id.clone(),
                fired_at,
                revert_at(fired_at, decision.resets_at),
            );
        }
        fired += 1;
        // 同一应用一次只执行一条规则：后续规则看到的 current 已经变了
        break;
    }
    Ok(fired)
}

fn emit_switched(
    app: &AppHandle,
    app_type: &str,
    provider_id: &str,
    provider_name: &str,
    source: &str,
    reason: &str,
    rule_id: &str,
) {
    let payload = serde_json::json!({
        "appType": app_type,
        "providerId": provider_id,
        "providerName": provider_name,
        "source": source,
        "reason": reason,
        "ruleId": rule_id,
    });
    if let Err(e) = app.emit("provider-switched", payload) {
        log::error!("[SwitchRules] 发射 provider-switched 失败: {e}");
    }
}

/// `ProviderService::switch` 内部用 block_on，必须放到阻塞线程池
async fn perform_switch(app: &AppHandle, app_type: AppType, target: &str) -> Result<(), String> {
    let handle = app.clone();
    let target = target.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let state = handle
            .try_state::<AppState>()
            .ok_or_else(|| "AppState unavailable".to_string())?;
        crate::services::ProviderService::switch(state.inner(), app_type, &target)
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("switch task failed: {e}"))?
}

/// 到点回切。`fired_at` 用于识别状态是否仍是本次触发写下的那一份。
pub fn arm_revert(app: AppHandle, rule_id: String, fired_at: i64, revert_at: i64) {
    tauri::async_runtime::spawn(async move {
        let wait = revert_at - now_secs();
        if wait > 0 {
            tokio::time::sleep(Duration::from_secs(wait as u64)).await;
        }
        match revert_rule(&app, &rule_id, fired_at).await {
            Ok(true) => log::info!("[SwitchRules] 规则 {rule_id} 已回切"),
            Ok(false) => log::debug!("[SwitchRules] 规则 {rule_id} 无需回切"),
            Err(e) => log::warn!("[SwitchRules] 规则 {rule_id} 回切失败: {e}"),
        }
    });
}

/// 回切：状态仍在、仍是同一次触发、当前供应商仍是规则目标时才执行
pub async fn revert_rule(app: &AppHandle, rule_id: &str, fired_at: i64) -> Result<bool, String> {
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| "AppState unavailable".to_string())?;
    let Some(rule_state) = state
        .db
        .get_switch_rule_state(rule_id)
        .map_err(|e| e.to_string())?
    else {
        return Ok(false);
    };
    if rule_state.fired_at != fired_at {
        return Ok(false);
    }
    let Some(rule) = state
        .db
        .get_switch_rule(rule_id)
        .map_err(|e| e.to_string())?
    else {
        let _ = state.db.clear_switch_rule_state(rule_id);
        return Ok(false);
    };
    let app_type = AppType::from_str(&rule.app_type).map_err(|e| e.to_string())?;
    let current = crate::services::ProviderService::current(state.inner(), app_type.clone())
        .map_err(|e| e.to_string())?;
    if current != rule_state.switched_to {
        // 用户（或故障转移）已经切走了，不再干预
        let _ = state.db.clear_switch_rule_state(rule_id);
        return Ok(false);
    }
    let from = rule_state.switched_from.clone();
    let providers = state
        .db
        .get_all_providers(&rule.app_type)
        .map_err(|e| e.to_string())?;
    if !providers.contains_key(&from) {
        let _ = state.db.clear_switch_rule_state(rule_id);
        return Err(format!("原供应商 {from} 已不存在"));
    }
    perform_switch(app, app_type, &from).await?;
    let _ = state.db.clear_switch_rule_state(rule_id);
    let provider_name = providers
        .get(&from)
        .map(|p| p.name.clone())
        .unwrap_or_else(|| from.clone());
    let reason = rule_state
        .reason
        .clone()
        .unwrap_or_else(|| rule.tier_name.clone());
    emit_switched(
        app,
        &rule.app_type,
        &from,
        &provider_name,
        "rule-revert",
        &reason,
        rule_id,
    );
    crate::tray::schedule_tray_refresh(app);
    Ok(true)
}

/// 启动时重新武装上次运行期间触发、尚未回切的规则
pub fn rearm_pending_reverts(app: AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let states = match state.db.list_switch_rule_states() {
        Ok(s) => s,
        Err(e) => {
            log::warn!("[SwitchRules] 读取待回切状态失败: {e}");
            return;
        }
    };
    for rule_state in states {
        let revert_on_reset = state
            .db
            .get_switch_rule(&rule_state.rule_id)
            .ok()
            .flatten()
            .map(|r| r.revert_on_reset)
            .unwrap_or(false);
        if !revert_on_reset {
            continue;
        }
        arm_revert(
            app.clone(),
            rule_state.rule_id.clone(),
            rule_state.fired_at,
            revert_at(rule_state.fired_at, rule_state.resets_at),
        );
    }
}

/// 规则校验（命令层调用）
pub fn validate_rule(rule: &SwitchRule) -> Result<(), String> {
    let app_type = AppType::from_str(&rule.app_type)
        .map_err(|_| format!("Unknown app type: {}", rule.app_type))?;
    if app_type.is_additive_mode() {
        return Err(format!(
            "{} has no current provider; switch rules are not supported",
            rule.app_type
        ));
    }
    if rule.tier_name.trim().is_empty() {
        return Err("Tier name is required".to_string());
    }
    if !(1.0..=100.0).contains(&rule.threshold_pct) {
        return Err("Threshold must be between 1 and 100".to_string());
    }
    if rule.target_provider_id.trim().is_empty() {
        return Err("Target provider is required".to_string());
    }
    if rule.watched_provider_id.as_deref() == Some(rule.target_provider_id.as_str()) {
        return Err("Target provider must differ from the watched provider".to_string());
    }
    match rule.source.as_str() {
        SOURCE_SUBSCRIPTION => {
            if rule.watched_provider_id.is_some() {
                return Err(
                    "Subscription rules watch the official plan, not a provider".to_string()
                );
            }
        }
        SOURCE_CODING_PLAN => {
            if rule.watched_provider_id.is_none() {
                return Err("Coding plan rules need a watched provider".to_string());
            }
        }
        other => return Err(format!("Unknown rule source: {other}")),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::subscription::{CredentialStatus, QuotaTier};

    fn rule(id: &str, tier: &str, threshold: f64) -> SwitchRule {
        SwitchRule {
            id: id.to_string(),
            app_type: "claude".to_string(),
            watched_provider_id: None,
            source: SOURCE_SUBSCRIPTION.to_string(),
            tier_name: tier.to_string(),
            threshold_pct: threshold,
            target_provider_id: "backup".to_string(),
            revert_on_reset: true,
            enabled: true,
            sort_index: 0,
            last_fired_at: None,
            created_at: 1,
        }
    }

    fn quota(now: i64, tiers: Vec<(&str, f64, Option<&str>)>) -> SubscriptionQuota {
        SubscriptionQuota {
            tool: "claude".to_string(),
            credential_status: CredentialStatus::Valid,
            credential_message: None,
            success: true,
            tiers: tiers
                .into_iter()
                .map(|(name, utilization, resets_at)| QuotaTier {
                    name: name.to_string(),
                    utilization,
                    resets_at: resets_at.map(|s| s.to_string()),
                    used_value_usd: None,
                    max_value_usd: None,
                })
                .collect(),
            extra_usage: None,
            error: None,
            queried_at: Some(now * 1000),
        }
    }

    const NOW: i64 = 1_800_000_000;

    #[test]
    fn fires_once_at_or_above_threshold() {
        let rules = vec![rule("r1", "five_hour", 90.0), rule("r2", "seven_day", 90.0)];
        let q = quota(
            NOW,
            vec![
                ("five_hour", 92.0, Some("2027-01-01T10:00:00Z")),
                ("seven_day", 40.0, None),
            ],
        );
        let decisions = evaluate(&rules, &[], "claude", None, &q, NOW);
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].rule.id, "r1");
        assert_eq!(decisions[0].reason, "five_hour 92%");
        assert_eq!(decisions[0].resets_at, Some(1_798_797_600));

        // exactly at threshold also fires
        let q = quota(NOW, vec![("five_hour", 90.0, None)]);
        assert_eq!(evaluate(&rules, &[], "claude", None, &q, NOW).len(), 1);
    }

    #[test]
    fn does_not_refire_while_armed_or_within_cooldown() {
        let rules = vec![rule("r1", "five_hour", 90.0)];
        let q = quota(NOW, vec![("five_hour", 99.0, None)]);
        let armed = SwitchRuleState {
            rule_id: "r1".to_string(),
            fired_at: NOW - 60,
            resets_at: None,
            switched_from: "official".to_string(),
            switched_to: "backup".to_string(),
            last_utilization: 95.0,
            reason: None,
        };
        assert!(evaluate(&rules, &[armed], "claude", None, &q, NOW).is_empty());

        let mut recent = rule("r1", "five_hour", 90.0);
        recent.last_fired_at = Some(NOW - MIN_COOLDOWN_SECS + 1);
        assert!(evaluate(&[recent.clone()], &[], "claude", None, &q, NOW).is_empty());
        recent.last_fired_at = Some(NOW - MIN_COOLDOWN_SECS);
        assert_eq!(evaluate(&[recent], &[], "claude", None, &q, NOW).len(), 1);
    }

    #[test]
    fn ignores_failed_stale_disabled_and_foreign_snapshots() {
        let rules = vec![rule("r1", "five_hour", 90.0)];
        let mut failed = quota(NOW, vec![("five_hour", 99.0, None)]);
        failed.success = false;
        assert!(evaluate(&rules, &[], "claude", None, &failed, NOW).is_empty());

        let stale = quota(
            NOW - MAX_SNAPSHOT_AGE_SECS - 1,
            vec![("five_hour", 99.0, None)],
        );
        assert!(evaluate(&rules, &[], "claude", None, &stale, NOW).is_empty());

        let fresh = quota(NOW, vec![("five_hour", 99.0, None)]);
        let mut disabled = rule("r1", "five_hour", 90.0);
        disabled.enabled = false;
        assert!(evaluate(&[disabled], &[], "claude", None, &fresh, NOW).is_empty());

        // other app, or a coding-plan snapshot for a subscription rule
        assert!(evaluate(&rules, &[], "codex", None, &fresh, NOW).is_empty());
        assert!(evaluate(&rules, &[], "claude", Some("kimi"), &fresh, NOW).is_empty());

        // coding-plan rule matches only its watched provider
        let mut plan_rule = rule("r2", "weekly_limit", 80.0);
        plan_rule.source = SOURCE_CODING_PLAN.to_string();
        plan_rule.watched_provider_id = Some("kimi".to_string());
        let plan_quota = quota(NOW, vec![("weekly_limit", 85.0, None)]);
        assert_eq!(
            evaluate(
                &[plan_rule.clone()],
                &[],
                "claude",
                Some("kimi"),
                &plan_quota,
                NOW
            )
            .len(),
            1
        );
        assert!(evaluate(&[plan_rule], &[], "claude", Some("other"), &plan_quota, NOW).is_empty());
    }

    #[test]
    fn computes_revert_time_from_resets_at_with_fallback() {
        assert_eq!(
            parse_resets_at(Some("2027-01-01T10:00:00+00:00")),
            Some(1_798_797_600)
        );
        assert_eq!(parse_resets_at(Some("not-a-date")), None);
        assert_eq!(parse_resets_at(None), None);
        assert_eq!(
            revert_at(NOW, Some(NOW + 100)),
            NOW + 100 + REVERT_GRACE_SECS
        );
        assert_eq!(revert_at(NOW, Some(NOW - 100)), NOW + FALLBACK_REVERT_SECS);
        assert_eq!(revert_at(NOW, None), NOW + FALLBACK_REVERT_SECS);
    }

    #[test]
    fn validates_rules() {
        assert!(validate_rule(&rule("r1", "five_hour", 90.0)).is_ok());
        let mut bad = rule("r1", "five_hour", 0.0);
        assert!(validate_rule(&bad).is_err());
        bad.threshold_pct = 90.0;
        bad.app_type = "opencode".to_string();
        assert!(validate_rule(&bad).is_err());
        bad.app_type = "claude".to_string();
        bad.watched_provider_id = Some("backup".to_string());
        bad.source = SOURCE_CODING_PLAN.to_string();
        assert!(validate_rule(&bad).is_err(), "target equals watched");
        bad.watched_provider_id = Some("kimi".to_string());
        assert!(validate_rule(&bad).is_ok());
        bad.source = SOURCE_SUBSCRIPTION.to_string();
        assert!(
            validate_rule(&bad).is_err(),
            "subscription rule with watched provider"
        );
    }
}
