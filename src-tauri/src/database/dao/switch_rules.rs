//! 配额规则 DAO
//!
//! `switch_rules`：当订阅 / Coding Plan 的某个窗口利用率越过阈值时，把应用切到
//! 备用供应商，并在窗口重置后回切。规则随 providers 同步；`switch_rule_state`
//! 记录「已触发、等待回切」的设备本地状态，不参与云同步。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

pub const SOURCE_SUBSCRIPTION: &str = "subscription";
pub const SOURCE_CODING_PLAN: &str = "coding_plan";

fn default_source() -> String {
    SOURCE_SUBSCRIPTION.to_string()
}

fn default_true() -> bool {
    true
}

/// 一条配额规则
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchRule {
    pub id: String,
    pub app_type: String,
    /// None = 该应用的官方订阅；Some = 带 Coding Plan 用量脚本的供应商
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watched_provider_id: Option<String>,
    /// "subscription" | "coding_plan"
    #[serde(default = "default_source")]
    pub source: String,
    /// 窗口名：five_hour / seven_day / weekly_limit / monthly ...
    pub tier_name: String,
    /// 触发阈值（0-100）
    pub threshold_pct: f64,
    pub target_provider_id: String,
    #[serde(default = "default_true")]
    pub revert_on_reset: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub sort_index: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_fired_at: Option<i64>,
    #[serde(default)]
    pub created_at: i64,
}

/// 规则触发后的待回切状态
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchRuleState {
    pub rule_id: String,
    pub fired_at: i64,
    /// 窗口重置时间（unix 秒）；缺失时按兜底时长回切
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<i64>,
    pub switched_from: String,
    pub switched_to: String,
    pub last_utilization: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

fn row_to_rule(row: &Row<'_>) -> rusqlite::Result<SwitchRule> {
    Ok(SwitchRule {
        id: row.get(0)?,
        app_type: row.get(1)?,
        watched_provider_id: row.get(2)?,
        source: row.get(3)?,
        tier_name: row.get(4)?,
        threshold_pct: row.get(5)?,
        target_provider_id: row.get(6)?,
        revert_on_reset: row.get::<_, i64>(7)? != 0,
        enabled: row.get::<_, i64>(8)? != 0,
        sort_index: row.get(9)?,
        last_fired_at: row.get(10)?,
        created_at: row.get(11)?,
    })
}

fn row_to_state(row: &Row<'_>) -> rusqlite::Result<SwitchRuleState> {
    Ok(SwitchRuleState {
        rule_id: row.get(0)?,
        fired_at: row.get(1)?,
        resets_at: row.get(2)?,
        switched_from: row.get(3)?,
        switched_to: row.get(4)?,
        last_utilization: row.get(5)?,
        reason: row.get(6)?,
    })
}

const RULE_COLUMNS: &str = "id, app_type, watched_provider_id, source, tier_name, threshold_pct,
        target_provider_id, revert_on_reset, enabled, sort_index, last_fired_at, created_at";
const STATE_COLUMNS: &str =
    "rule_id, fired_at, resets_at, switched_from, switched_to, last_utilization, reason";

impl Database {
    /// 列出规则（可按应用过滤），按 app_type / sort_index / created_at 排序
    pub fn list_switch_rules(&self, app_type: Option<&str>) -> Result<Vec<SwitchRule>, AppError> {
        let conn = lock_conn!(self.conn);
        let sql = format!(
            "SELECT {RULE_COLUMNS} FROM switch_rules
             WHERE (?1 IS NULL OR app_type = ?1)
             ORDER BY app_type ASC, sort_index ASC, created_at ASC, id ASC"
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| AppError::Database(e.to_string()))?;
        let rules = stmt
            .query_map(params![app_type], row_to_rule)
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(rules)
    }

    pub fn get_switch_rule(&self, id: &str) -> Result<Option<SwitchRule>, AppError> {
        let conn = lock_conn!(self.conn);
        let sql = format!("SELECT {RULE_COLUMNS} FROM switch_rules WHERE id = ?1");
        conn.query_row(&sql, params![id], row_to_rule)
            .optional()
            .map_err(|e| AppError::Database(e.to_string()))
    }

    /// 新增或更新规则（按 id）；created_at 为 0 时取当前时间
    pub fn upsert_switch_rule(&self, rule: &SwitchRule) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        let now = chrono::Utc::now().timestamp();
        let created_at = if rule.created_at > 0 {
            rule.created_at
        } else {
            now
        };
        conn.execute(
            "INSERT INTO switch_rules (
                id, app_type, watched_provider_id, source, tier_name, threshold_pct,
                target_provider_id, revert_on_reset, enabled, sort_index, last_fired_at,
                created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(id) DO UPDATE SET
                app_type = excluded.app_type,
                watched_provider_id = excluded.watched_provider_id,
                source = excluded.source,
                tier_name = excluded.tier_name,
                threshold_pct = excluded.threshold_pct,
                target_provider_id = excluded.target_provider_id,
                revert_on_reset = excluded.revert_on_reset,
                enabled = excluded.enabled,
                sort_index = excluded.sort_index,
                updated_at = excluded.updated_at",
            params![
                rule.id,
                rule.app_type,
                rule.watched_provider_id,
                rule.source,
                rule.tier_name,
                rule.threshold_pct,
                rule.target_provider_id,
                rule.revert_on_reset as i64,
                rule.enabled as i64,
                rule.sort_index,
                rule.last_fired_at,
                created_at,
                now,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 删除规则及其状态；返回是否存在
    pub fn delete_switch_rule(&self, id: &str) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "DELETE FROM switch_rule_state WHERE rule_id = ?1",
            params![id],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        let affected = conn
            .execute("DELETE FROM switch_rules WHERE id = ?1", params![id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(affected > 0)
    }

    /// 启用 / 停用规则；停用时同时清除待回切状态
    pub fn set_switch_rule_enabled(&self, id: &str, enabled: bool) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let affected = conn
            .execute(
                "UPDATE switch_rules SET enabled = ?2, updated_at = strftime('%s', 'now') WHERE id = ?1",
                params![id, enabled as i64],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        if !enabled {
            conn.execute(
                "DELETE FROM switch_rule_state WHERE rule_id = ?1",
                params![id],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        }
        Ok(affected > 0)
    }

    pub fn set_switch_rule_last_fired(&self, id: &str, fired_at: i64) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE switch_rules SET last_fired_at = ?2 WHERE id = ?1",
            params![id, fired_at],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn list_switch_rule_states(&self) -> Result<Vec<SwitchRuleState>, AppError> {
        let conn = lock_conn!(self.conn);
        let sql = format!("SELECT {STATE_COLUMNS} FROM switch_rule_state ORDER BY fired_at ASC");
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| AppError::Database(e.to_string()))?;
        let states = stmt
            .query_map([], row_to_state)
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(states)
    }

    pub fn get_switch_rule_state(
        &self,
        rule_id: &str,
    ) -> Result<Option<SwitchRuleState>, AppError> {
        let conn = lock_conn!(self.conn);
        let sql = format!("SELECT {STATE_COLUMNS} FROM switch_rule_state WHERE rule_id = ?1");
        conn.query_row(&sql, params![rule_id], row_to_state)
            .optional()
            .map_err(|e| AppError::Database(e.to_string()))
    }

    pub fn set_switch_rule_state(&self, state: &SwitchRuleState) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO switch_rule_state (
                rule_id, fired_at, resets_at, switched_from, switched_to, last_utilization, reason
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                state.rule_id,
                state.fired_at,
                state.resets_at,
                state.switched_from,
                state.switched_to,
                state.last_utilization,
                state.reason,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn clear_switch_rule_state(&self, rule_id: &str) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let affected = conn
            .execute(
                "DELETE FROM switch_rule_state WHERE rule_id = ?1",
                params![rule_id],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(affected > 0)
    }

    /// 清除某应用所有规则的待回切状态（任何手动 / 托盘 / 项目切换都会调用）
    pub fn clear_switch_rule_states_for_app(&self, app_type: &str) -> Result<usize, AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "DELETE FROM switch_rule_state
             WHERE rule_id IN (SELECT id FROM switch_rules WHERE app_type = ?1)",
            params![app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(id: &str, app_type: &str) -> SwitchRule {
        SwitchRule {
            id: id.to_string(),
            app_type: app_type.to_string(),
            watched_provider_id: None,
            source: SOURCE_SUBSCRIPTION.to_string(),
            tier_name: "five_hour".to_string(),
            threshold_pct: 90.0,
            target_provider_id: "backup".to_string(),
            revert_on_reset: true,
            enabled: true,
            sort_index: 0,
            last_fired_at: None,
            created_at: 0,
        }
    }

    #[test]
    fn rules_round_trip_and_states_follow_rules() -> Result<(), AppError> {
        let db = Database::memory()?;
        db.upsert_switch_rule(&rule("r1", "claude"))?;
        db.upsert_switch_rule(&rule("r2", "codex"))?;

        let all = db.list_switch_rules(None)?;
        assert_eq!(all.len(), 2);
        assert!(all[0].created_at > 0);
        assert_eq!(db.list_switch_rules(Some("claude"))?.len(), 1);

        let mut updated = rule("r1", "claude");
        updated.threshold_pct = 75.0;
        updated.enabled = false;
        db.upsert_switch_rule(&updated)?;
        let fetched = db.get_switch_rule("r1")?.expect("r1");
        assert_eq!(fetched.threshold_pct, 75.0);
        assert!(!fetched.enabled);
        assert_eq!(
            fetched.created_at, all[0].created_at,
            "created_at is kept on update"
        );

        db.set_switch_rule_state(&SwitchRuleState {
            rule_id: "r1".to_string(),
            fired_at: 100,
            resets_at: Some(200),
            switched_from: "official".to_string(),
            switched_to: "backup".to_string(),
            last_utilization: 92.5,
            reason: Some("five_hour 92%".to_string()),
        })?;
        db.set_switch_rule_state(&SwitchRuleState {
            rule_id: "r2".to_string(),
            fired_at: 101,
            resets_at: None,
            switched_from: "a".to_string(),
            switched_to: "b".to_string(),
            last_utilization: 95.0,
            reason: None,
        })?;
        assert_eq!(db.list_switch_rule_states()?.len(), 2);
        assert_eq!(db.clear_switch_rule_states_for_app("claude")?, 1);
        assert!(db.get_switch_rule_state("r1")?.is_none());
        assert!(db.get_switch_rule_state("r2")?.is_some());

        assert!(db.set_switch_rule_enabled("r2", false)?);
        assert!(
            db.get_switch_rule_state("r2")?.is_none(),
            "disabling clears state"
        );

        assert!(db.delete_switch_rule("r1")?);
        assert!(!db.delete_switch_rule("r1")?);
        assert_eq!(db.list_switch_rules(None)?.len(), 1);
        Ok(())
    }
}
