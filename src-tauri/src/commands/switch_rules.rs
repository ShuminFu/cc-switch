//! 配额规则命令

use crate::app_config::AppType;
use crate::database::{SwitchRule, SwitchRuleState};
use crate::services::switch_rules;
use crate::store::AppState;
use std::str::FromStr;

#[tauri::command]
pub async fn list_switch_rules(
    state: tauri::State<'_, AppState>,
    app_type: Option<String>,
) -> Result<Vec<SwitchRule>, String> {
    state
        .db
        .list_switch_rules(app_type.as_deref())
        .map_err(|e| e.to_string())
}

/// 新增或更新规则；id 为空时自动生成。返回落库后的规则。
#[tauri::command]
pub async fn upsert_switch_rule(
    state: tauri::State<'_, AppState>,
    rule: SwitchRule,
) -> Result<SwitchRule, String> {
    let mut rule = rule;
    rule.id = rule.id.trim().to_string();
    if rule.id.is_empty() {
        rule.id = uuid::Uuid::new_v4().to_string();
    }
    rule.tier_name = rule.tier_name.trim().to_string();
    rule.target_provider_id = rule.target_provider_id.trim().to_string();
    rule.watched_provider_id = rule
        .watched_provider_id
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    switch_rules::validate_rule(&rule)?;

    let providers = state
        .db
        .get_all_providers(&rule.app_type)
        .map_err(|e| e.to_string())?;
    if !providers.contains_key(&rule.target_provider_id) {
        return Err(format!(
            "Target provider {} does not exist for {}",
            rule.target_provider_id, rule.app_type
        ));
    }
    if let Some(watched) = &rule.watched_provider_id {
        if !providers.contains_key(watched) {
            return Err(format!(
                "Watched provider {watched} does not exist for {}",
                rule.app_type
            ));
        }
    }
    // 编辑规则后旧的待回切状态不再可信
    let _ = state.db.clear_switch_rule_state(&rule.id);
    state
        .db
        .upsert_switch_rule(&rule)
        .map_err(|e| e.to_string())?;
    state
        .db
        .get_switch_rule(&rule.id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Rule vanished after save".to_string())
}

#[tauri::command]
pub async fn delete_switch_rule(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<bool, String> {
    state.db.delete_switch_rule(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_switch_rule_enabled(
    state: tauri::State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<bool, String> {
    state
        .db
        .set_switch_rule_enabled(&id, enabled)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_switch_rule_states(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<SwitchRuleState>, String> {
    state
        .db
        .list_switch_rule_states()
        .map_err(|e| e.to_string())
}

/// 取消某条规则的待回切（保持当前供应商不动）
#[tauri::command]
pub async fn cancel_switch_rule_revert(
    state: tauri::State<'_, AppState>,
    rule_id: String,
) -> Result<bool, String> {
    state
        .db
        .clear_switch_rule_state(&rule_id)
        .map_err(|e| e.to_string())
}

/// 立即用官方订阅额度评估某应用的规则（Coding Plan 规则在该供应商的用量查询时评估）
#[tauri::command]
pub async fn evaluate_switch_rules_now(
    app: tauri::AppHandle,
    app_type: String,
) -> Result<u32, String> {
    let parsed = AppType::from_str(&app_type).map_err(|e| e.to_string())?;
    let quota = crate::services::subscription::get_subscription_quota(parsed.as_str()).await?;
    switch_rules::on_quota_snapshot(&app, parsed, None, &quota).await
}
