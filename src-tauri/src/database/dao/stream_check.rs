//! 流式健康检查日志 DAO

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::services::stream_check::{HealthStatus, StreamCheckConfig, StreamCheckResult};

/// The value stored in `stream_check_logs.status` (DB-29): pinned strings, not
/// the enum's `Debug` output, so renaming a variant cannot change stored data.
pub(crate) fn health_status_db_str(status: &HealthStatus) -> &'static str {
    match status {
        HealthStatus::Operational => "operational",
        HealthStatus::Degraded => "degraded",
        HealthStatus::Failed => "failed",
    }
}

/// Inverse of [`health_status_db_str`].
#[allow(dead_code)]
pub(crate) fn health_status_from_db_str(value: &str) -> Option<HealthStatus> {
    match value {
        "operational" => Some(HealthStatus::Operational),
        "degraded" => Some(HealthStatus::Degraded),
        "failed" => Some(HealthStatus::Failed),
        _ => None,
    }
}

impl Database {
    /// 保存流式检查日志
    pub fn save_stream_check_log(
        &self,
        provider_id: &str,
        provider_name: &str,
        app_type: &str,
        result: &StreamCheckResult,
    ) -> Result<i64, AppError> {
        let conn = lock_conn!(self.conn);

        conn.execute(
            "INSERT INTO stream_check_logs 
             (provider_id, provider_name, app_type, status, success, message, 
              response_time_ms, http_status, model_used, retry_count, tested_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                provider_id,
                provider_name,
                app_type,
                health_status_db_str(&result.status),
                result.success,
                result.message,
                result.response_time_ms.map(|t| t as i64),
                result.http_status.map(|s| s as i64),
                result.model_used,
                result.retry_count as i64,
                result.tested_at,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(conn.last_insert_rowid())
    }

    /// 获取流式检查配置
    pub fn get_stream_check_config(&self) -> Result<StreamCheckConfig, AppError> {
        match self.get_setting("stream_check_config")? {
            Some(json) => serde_json::from_str(&json)
                .map_err(|e| AppError::Message(format!("解析配置失败: {e}"))),
            None => Ok(StreamCheckConfig::default()),
        }
    }

    /// Delete stream check logs older than `retain_days` days.
    /// Returns the number of deleted rows.
    pub fn cleanup_old_stream_check_logs(&self, retain_days: i64) -> Result<u64, AppError> {
        let cutoff = chrono::Utc::now().timestamp() - retain_days * 86400;
        let conn = lock_conn!(self.conn);
        let deleted = conn
            .execute(
                "DELETE FROM stream_check_logs WHERE tested_at < ?1",
                [cutoff],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        if deleted > 0 {
            log::info!("Cleaned up {deleted} stream_check_logs older than {retain_days} days");
        }
        Ok(deleted as u64)
    }

    /// 保存流式检查配置
    pub fn save_stream_check_config(&self, config: &StreamCheckConfig) -> Result<(), AppError> {
        let json = serde_json::to_string(config)
            .map_err(|e| AppError::Message(format!("序列化配置失败: {e}")))?;
        self.set_setting("stream_check_config", &json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_status_strings_are_pinned() {
        for (status, stored) in [
            (HealthStatus::Operational, "operational"),
            (HealthStatus::Degraded, "degraded"),
            (HealthStatus::Failed, "failed"),
        ] {
            assert_eq!(health_status_db_str(&status), stored);
            assert_eq!(health_status_from_db_str(stored), Some(status));
        }
        assert_eq!(health_status_from_db_str("Operational"), None);
    }
}
