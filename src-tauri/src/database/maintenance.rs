//! 数据库定期维护：按 [`RetentionPolicy`] 汇总 / 清理使用量数据与备份（CCS-161）
//!
//! 启动时（[`Database::init`]）与运行期间的每日定时任务
//! （[`Database::periodic_backup_if_needed`]）都调用 [`Maintenance::run`]，保留期限
//! 来自设置（[`crate::settings::effective_retention_policy`]），不再散落硬编码的天数。
//!
//! 各步骤相互独立：某一步失败只记录告警，不阻断其余步骤。

use super::{lock_conn, Database};
use crate::config::get_app_config_dir;
use crate::error::AppError;
use crate::settings::RetentionPolicy;
use std::path::Path;

/// 一次维护运行的结果（各步骤删除 / 汇总的行数与文件数；失败的步骤计 0）
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MaintenanceReport {
    /// 删除的过期 `stream_check_logs` 行数
    pub stream_check_logs_deleted: u64,
    /// 汇总进 `usage_daily_rollups` 后删除的 `proxy_request_logs` 明细行数
    pub detail_rows_rolled_up: u64,
    /// 删除的过期 `usage_daily_rollups` 行数
    pub rollup_rows_deleted: u64,
    /// 删除的旧数据库备份文件数
    pub backups_deleted: u64,
}

impl MaintenanceReport {
    /// 删除的数据库行数（决定是否需要 incremental vacuum）
    pub fn reclaimed_rows(&self) -> u64 {
        self.stream_check_logs_deleted + self.detail_rows_rolled_up + self.rollup_rows_deleted
    }
}

/// 数据库维护入口
pub struct Maintenance;

impl Maintenance {
    /// `stream_check_logs`（连通性检测记录）保留天数
    pub const STREAM_CHECK_LOG_RETENTION_DAYS: i64 = 7;

    /// 按 `policy` 执行一次维护，备份目录为应用配置目录下的 `backups/`
    pub fn run(db: &Database, policy: &RetentionPolicy) -> MaintenanceReport {
        let backup_dir = get_app_config_dir().join("backups");
        Self::run_with_backup_dir(db, policy, &backup_dir)
    }

    /// 按 `policy` 执行一次维护：
    /// 1. 删除过期的 `stream_check_logs`；
    /// 2. 早于 `detail_days` 的明细汇总进 rollup 后删除；
    /// 3. 设置了 `rollup_days` 时删除更早的 rollup 行；
    /// 4. `backup_dir` 中只保留最新的 `backups_keep` 个数据库备份；
    /// 5. 删除过数据行时执行 incremental vacuum 回收空间。
    pub(crate) fn run_with_backup_dir(
        db: &Database,
        policy: &RetentionPolicy,
        backup_dir: &Path,
    ) -> MaintenanceReport {
        let mut report = MaintenanceReport::default();

        match db.cleanup_old_stream_check_logs(Self::STREAM_CHECK_LOG_RETENTION_DAYS) {
            Ok(deleted) => report.stream_check_logs_deleted = deleted,
            Err(e) => log::warn!("Maintenance: stream_check_logs cleanup failed: {e}"),
        }
        match db.rollup_and_prune(i64::from(policy.detail_days)) {
            Ok(deleted) => report.detail_rows_rolled_up = deleted,
            Err(e) => log::warn!("Maintenance: rollup_and_prune failed: {e}"),
        }
        if let Some(rollup_days) = policy.rollup_days {
            match db.prune_old_rollups(rollup_days) {
                Ok(deleted) => report.rollup_rows_deleted = deleted,
                Err(e) => log::warn!("Maintenance: rollup retention failed: {e}"),
            }
        }
        match Database::cleanup_db_backups(backup_dir, policy.backups_keep) {
            Ok(deleted) => report.backups_deleted = deleted,
            Err(e) => log::warn!("Maintenance: backup retention failed: {e}"),
        }

        if report.reclaimed_rows() > 0 {
            if let Err(e) = Self::incremental_vacuum(db) {
                log::warn!("Maintenance: incremental vacuum failed: {e}");
            }
        }

        report
    }

    fn incremental_vacuum(db: &Database) -> Result<(), AppError> {
        let conn = lock_conn!(db.conn);
        conn.execute_batch("PRAGMA incremental_vacuum;")
            .map_err(|e| AppError::Database(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::{Maintenance, MaintenanceReport};
    use crate::database::{lock_conn, Database};
    use crate::error::AppError;
    use crate::settings::RetentionPolicy;
    use chrono::{Duration, Local};

    fn insert_log(db: &Database, id: &str, created_at: i64) -> Result<(), AppError> {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO proxy_request_logs (
                request_id, provider_id, app_type, model,
                input_tokens, output_tokens, total_cost_usd,
                latency_ms, status_code, created_at
            ) VALUES (?1, 'p1', 'claude', 'claude-3', 10, 5, '0.01', 100, 200, ?2)",
            rusqlite::params![id, created_at],
        )?;
        Ok(())
    }

    fn insert_rollup(db: &Database, days_ago: i64) -> Result<(), AppError> {
        let date = (Local::now().date_naive() - Duration::days(days_ago))
            .format("%Y-%m-%d")
            .to_string();
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO usage_daily_rollups
                (date, app_type, provider_id, model, request_count, success_count,
                 input_tokens, output_tokens, total_cost_usd, total_cost_micros, avg_latency_ms)
             VALUES (?1, 'claude', 'p-rollup', 'claude-3', 1, 1, 10, 5, '0.010000', 10000, 100)",
            [&date],
        )?;
        Ok(())
    }

    fn count(db: &Database, sql: &str) -> Result<i64, AppError> {
        let conn = lock_conn!(db.conn);
        Ok(conn.query_row(sql, [], |row| row.get(0))?)
    }

    fn touch_backup(dir: &std::path::Path, name: &str, age_secs: u64) {
        use std::io::Write;
        let mtime = std::time::SystemTime::now() - std::time::Duration::from_secs(age_secs);
        // 在同一个可写句柄上设置时间戳：Windows 需要 FILE_WRITE_ATTRIBUTES，只读句柄会被拒绝
        let mut file = std::fs::File::create(dir.join(name)).expect("create backup");
        file.write_all(b"backup").expect("write backup");
        file.set_modified(mtime).expect("set backup mtime");
    }

    #[test]
    fn maintenance_prunes_per_retention_policy() -> Result<(), AppError> {
        let db = Database::memory()?;
        let now = chrono::Utc::now().timestamp();
        insert_log(&db, "detail-15d", now - 15 * 86400)?;
        insert_log(&db, "detail-5d", now - 5 * 86400)?;
        insert_rollup(&db, 200)?;
        insert_rollup(&db, 60)?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO stream_check_logs
                    (provider_id, provider_name, app_type, status, success, message, tested_at)
                 VALUES ('p1', 'P1', 'claude', 'operational', 1, 'ok', ?1)",
                [now - 10 * 86400],
            )?;
        }

        let backups = tempfile::tempdir().expect("tempdir");
        for (i, age) in [500u64, 400, 300, 200, 100].iter().enumerate() {
            touch_backup(backups.path(), &format!("db_backup_{i}.db"), *age);
        }
        // Non-.db files are never counted or removed
        touch_backup(backups.path(), "db_backup_0.config.json", 1000);

        let policy = RetentionPolicy {
            detail_days: 10,
            rollup_days: Some(90),
            backups_keep: 2,
        };
        let report = Maintenance::run_with_backup_dir(&db, &policy, backups.path());

        assert_eq!(
            report,
            MaintenanceReport {
                stream_check_logs_deleted: 1,
                detail_rows_rolled_up: 1,
                rollup_rows_deleted: 1,
                backups_deleted: 3,
            }
        );
        // detail older than detail_days was rolled up; the 5-day-old row stays
        assert_eq!(
            count(&db, "SELECT COUNT(*) FROM proxy_request_logs")?,
            1,
            "only the 5-day-old detail row remains"
        );
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM usage_daily_rollups WHERE provider_id = 'p1'"
            )?,
            1,
            "the 15-day-old detail row was rolled up"
        );
        // rollup older than rollup_days removed, the 60-day-old one kept
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM usage_daily_rollups WHERE provider_id = 'p-rollup'"
            )?,
            1
        );
        let mut left: Vec<String> = std::fs::read_dir(backups.path())
            .expect("read backups")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(
            left,
            vec![
                "db_backup_0.config.json".to_string(),
                "db_backup_3.db".to_string(),
                "db_backup_4.db".to_string(),
            ],
            "the newest backups_keep backups are kept"
        );

        // A second run is a no-op
        assert_eq!(
            Maintenance::run_with_backup_dir(&db, &policy, backups.path()),
            MaintenanceReport::default()
        );
        Ok(())
    }

    #[test]
    fn maintenance_keeps_rollups_forever_without_rollup_days() -> Result<(), AppError> {
        let db = Database::memory()?;
        insert_rollup(&db, 3650)?;
        let backups = tempfile::tempdir().expect("tempdir");

        let report =
            Maintenance::run_with_backup_dir(&db, &RetentionPolicy::default(), backups.path());

        assert_eq!(report.rollup_rows_deleted, 0);
        assert_eq!(count(&db, "SELECT COUNT(*) FROM usage_daily_rollups")?, 1);
        Ok(())
    }

    #[test]
    fn maintenance_honours_detail_days() -> Result<(), AppError> {
        let db = Database::memory()?;
        let now = chrono::Utc::now().timestamp();
        insert_log(&db, "detail-40d", now - 40 * 86400)?;
        let backups = tempfile::tempdir().expect("tempdir");

        let keep_longer = RetentionPolicy {
            detail_days: 60,
            ..RetentionPolicy::default()
        };
        let report = Maintenance::run_with_backup_dir(&db, &keep_longer, backups.path());
        assert_eq!(
            report.detail_rows_rolled_up, 0,
            "40 < 60 days: kept as detail"
        );

        let report =
            Maintenance::run_with_backup_dir(&db, &RetentionPolicy::default(), backups.path());
        assert_eq!(report.detail_rows_rolled_up, 1, "40 > 30 days: rolled up");
        assert_eq!(count(&db, "SELECT COUNT(*) FROM proxy_request_logs")?, 0);
        Ok(())
    }
}
