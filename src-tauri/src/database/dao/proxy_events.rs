//! 代理事件时间线 DAO
//!
//! 记录「为什么我现在在 P2」这类事后需要回看的事实：故障转移切换、熔断器状态
//! 变化与重置、接管开关、配额规则切换、代理启停。设备本地，不参与云同步，
//! 只保留最近 [`PROXY_EVENTS_RETAIN`] 条。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::{params, Row};
use serde::{Deserialize, Serialize};

/// 全局保留条数（按 id 倒序）
pub const PROXY_EVENTS_RETAIN: i64 = 1000;

/// 事件种类；前端按同名 i18n key 展示
pub mod kind {
    pub const PROXY_START: &str = "proxy_start";
    pub const PROXY_STOP: &str = "proxy_stop";
    pub const TAKEOVER_ON: &str = "takeover_on";
    pub const TAKEOVER_OFF: &str = "takeover_off";
    pub const FAILOVER_SWITCH: &str = "failover_switch";
    pub const BREAKER_OPEN: &str = "breaker_open";
    pub const BREAKER_HALF_OPEN: &str = "breaker_half_open";
    pub const BREAKER_CLOSED: &str = "breaker_closed";
    pub const BREAKER_RESET: &str = "breaker_reset";
    pub const RULE_SWITCH: &str = "rule_switch";
    pub const RULE_REVERT: &str = "rule_revert";
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyEvent {
    pub id: i64,
    pub created_at: i64,
    pub app_type: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

fn row_to_event(row: &Row<'_>) -> rusqlite::Result<ProxyEvent> {
    Ok(ProxyEvent {
        id: row.get(0)?,
        created_at: row.get(1)?,
        app_type: row.get(2)?,
        kind: row.get(3)?,
        provider_id: row.get(4)?,
        provider_name: row.get(5)?,
        detail: row.get(6)?,
    })
}

impl Database {
    /// 追加一条事件并裁剪到保留上限；返回事件 id
    pub fn record_proxy_event(
        &self,
        app_type: &str,
        kind: &str,
        provider_id: Option<&str>,
        provider_name: Option<&str>,
        detail: Option<&str>,
    ) -> Result<i64, AppError> {
        let conn = lock_conn!(self.conn);
        let now = chrono::Utc::now().timestamp();
        conn.execute(
            "INSERT INTO proxy_events (created_at, app_type, kind, provider_id, provider_name, detail)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![now, app_type, kind, provider_id, provider_name, detail],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        let id = conn.last_insert_rowid();
        conn.execute(
            "DELETE FROM proxy_events
             WHERE id NOT IN (SELECT id FROM proxy_events ORDER BY id DESC LIMIT ?1)",
            params![PROXY_EVENTS_RETAIN],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(id)
    }

    /// 最近的事件（新→旧），可按应用过滤
    pub fn list_proxy_events(
        &self,
        app_type: Option<&str>,
        limit: u32,
    ) -> Result<Vec<ProxyEvent>, AppError> {
        let conn = lock_conn!(self.conn);
        let limit = limit.clamp(1, PROXY_EVENTS_RETAIN as u32) as i64;
        let mut stmt = conn
            .prepare(
                "SELECT id, created_at, app_type, kind, provider_id, provider_name, detail
                 FROM proxy_events
                 WHERE (?1 IS NULL OR app_type = ?1)
                 ORDER BY id DESC
                 LIMIT ?2",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        let events = stmt
            .query_map(params![app_type, limit], row_to_event)
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(events)
    }

    pub fn clear_proxy_events(&self, app_type: Option<&str>) -> Result<usize, AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "DELETE FROM proxy_events WHERE (?1 IS NULL OR app_type = ?1)",
            params![app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_lists_filters_and_prunes() -> Result<(), AppError> {
        let db = Database::memory()?;
        db.record_proxy_event("claude", kind::PROXY_START, None, None, None)?;
        db.record_proxy_event(
            "claude",
            kind::FAILOVER_SWITCH,
            Some("p2"),
            Some("Backup"),
            None,
        )?;
        db.record_proxy_event(
            "codex",
            kind::BREAKER_OPEN,
            Some("c1"),
            None,
            Some("5 failures"),
        )?;

        let all = db.list_proxy_events(None, 50)?;
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].kind, kind::BREAKER_OPEN, "newest first");
        assert_eq!(all[0].detail.as_deref(), Some("5 failures"));

        let claude = db.list_proxy_events(Some("claude"), 50)?;
        assert_eq!(claude.len(), 2);
        assert_eq!(claude[0].provider_name.as_deref(), Some("Backup"));

        assert_eq!(db.list_proxy_events(None, 1)?.len(), 1);

        assert_eq!(db.clear_proxy_events(Some("codex"))?, 1);
        assert_eq!(db.list_proxy_events(None, 50)?.len(), 2);
        assert_eq!(db.clear_proxy_events(None)?, 2);
        assert!(db.list_proxy_events(None, 50)?.is_empty());
        Ok(())
    }

    #[test]
    fn keeps_only_the_newest_events() -> Result<(), AppError> {
        let db = Database::memory()?;
        for i in 0..(PROXY_EVENTS_RETAIN + 25) {
            db.record_proxy_event(
                "claude",
                kind::BREAKER_CLOSED,
                None,
                None,
                Some(&i.to_string()),
            )?;
        }
        let events = db.list_proxy_events(None, PROXY_EVENTS_RETAIN as u32)?;
        assert_eq!(events.len(), PROXY_EVENTS_RETAIN as usize);
        assert_eq!(
            events[0].detail.as_deref(),
            Some((PROXY_EVENTS_RETAIN + 24).to_string().as_str())
        );
        assert_eq!(events.last().unwrap().detail.as_deref(), Some("25"));
        Ok(())
    }
}
