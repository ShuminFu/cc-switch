use serde::Serialize;
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct InitErrorPayload {
    pub path: String,
    pub error: String,
    /// 错误类别。`Some("db_version_too_new")` 表示数据库版本过新（应用过旧），
    /// 前端据此展示「升级应用」恢复界面而非直接退出。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// 磁盘上数据库的 user_version（数据库版本过新时填充）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub db_version: Option<i32>,
    /// 当前应用支持的 SCHEMA_VERSION（数据库版本过新时填充）。
    /// 当升级到最新版后 db_version 仍 > supported_version，说明可能由第三方客户端创建。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported_version: Option<i32>,
}

static INIT_ERROR: OnceLock<RwLock<Option<InitErrorPayload>>> = OnceLock::new();

fn cell() -> &'static RwLock<Option<InitErrorPayload>> {
    INIT_ERROR.get_or_init(|| RwLock::new(None))
}

pub fn set_init_error(payload: InitErrorPayload) {
    #[allow(clippy::unwrap_used)]
    if let Ok(mut guard) = cell().write() {
        *guard = Some(payload);
    }
}

pub fn get_init_error() -> Option<InitErrorPayload> {
    cell().read().ok()?.clone()
}

#[cfg(test)]
mod startup_warning_tests {
    use super::{push_startup_warning, startup_warnings, StartupWarning};

    #[test]
    fn warnings_are_recorded_once_and_not_consumed() {
        let warning = StartupWarning {
            kind: "test_warning_kind".to_string(),
            app_type: None,
            path: Some("/x".to_string()),
            message: "m".to_string(),
        };
        push_startup_warning(warning.clone());
        push_startup_warning(warning.clone());
        let count = |w: &[StartupWarning]| w.iter().filter(|x| **x == warning).count();
        assert_eq!(count(&startup_warnings()), 1);
        assert_eq!(count(&startup_warnings()), 1);
    }
}

// ============================================================
// 启动告警（非致命，前端以横幅/Toast 提示）
// ============================================================

/// A non-fatal problem found during startup that the UI should explain:
/// `settings_corrupt` (settings.json could not be parsed and was moved aside),
/// `config_dir_unavailable` (the data-directory override is missing),
/// `proxy_restore_failed` (takeover restore failed after a retry, CMD-46).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StartupWarning {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub message: String,
}

static STARTUP_WARNINGS: RwLock<Vec<StartupWarning>> = RwLock::new(Vec::new());

/// Record a startup warning (duplicates are ignored).
pub fn push_startup_warning(warning: StartupWarning) {
    if let Ok(mut guard) = STARTUP_WARNINGS.write() {
        if !guard.contains(&warning) {
            guard.push(warning);
        }
    }
}

/// All startup warnings recorded so far (not consumed: the UI may ask again
/// after a reload).
pub fn startup_warnings() -> Vec<StartupWarning> {
    STARTUP_WARNINGS
        .read()
        .map(|guard| guard.clone())
        .unwrap_or_default()
}

// ============================================================
// 迁移结果状态
// ============================================================

static MIGRATION_SUCCESS: OnceLock<RwLock<bool>> = OnceLock::new();

fn migration_cell() -> &'static RwLock<bool> {
    MIGRATION_SUCCESS.get_or_init(|| RwLock::new(false))
}

pub fn set_migration_success() {
    if let Ok(mut guard) = migration_cell().write() {
        *guard = true;
    }
}

/// 获取并消费迁移成功状态（只返回一次 true，之后返回 false）
pub fn take_migration_success() -> bool {
    if let Ok(mut guard) = migration_cell().write() {
        let val = *guard;
        *guard = false;
        val
    } else {
        false
    }
}

// ============================================================
// Skills SSOT 迁移结果状态
// ============================================================

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct SkillsMigrationPayload {
    pub count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

static SKILLS_MIGRATION_RESULT: OnceLock<RwLock<Option<SkillsMigrationPayload>>> = OnceLock::new();

fn skills_migration_cell() -> &'static RwLock<Option<SkillsMigrationPayload>> {
    SKILLS_MIGRATION_RESULT.get_or_init(|| RwLock::new(None))
}

pub fn set_skills_migration_result(count: usize) {
    if let Ok(mut guard) = skills_migration_cell().write() {
        *guard = Some(SkillsMigrationPayload { count, error: None });
    }
}

pub fn set_skills_migration_error(error: String) {
    if let Ok(mut guard) = skills_migration_cell().write() {
        *guard = Some(SkillsMigrationPayload {
            count: 0,
            error: Some(error),
        });
    }
}

/// 获取并消费 Skills 迁移结果（只返回一次 Some，之后返回 None）
pub fn take_skills_migration_result() -> Option<SkillsMigrationPayload> {
    if let Ok(mut guard) = skills_migration_cell().write() {
        guard.take()
    } else {
        None
    }
}

// ============================================================
// 启动阶段计时（CMD-09 / PERF-11）
// ============================================================

/// 后台初始化每完成一个阶段发射一次，payload 为 [`StartupPhaseTiming`]。
/// 前端不监听也没关系：它只是进度信号，不影响任何功能。
pub const STARTUP_PROGRESS_EVENT: &str = "startup-progress";
/// 后台初始化全部完成后发射一次，payload 为 [`StartupReadyPayload`]。
pub const STARTUP_READY_EVENT: &str = "startup-ready";

static STARTUP_EPOCH: OnceLock<Instant> = OnceLock::new();

/// 记录启动起点（`run()` 入口处调用；重复调用保持第一次的值）。
pub fn mark_startup_begin() -> Instant {
    *STARTUP_EPOCH.get_or_init(Instant::now)
}

/// 距启动起点经过的毫秒数。
pub fn startup_elapsed_ms() -> u64 {
    duration_ms(mark_startup_begin().elapsed())
}

fn duration_ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

/// 单个启动阶段的耗时。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StartupPhaseTiming {
    pub phase: &'static str,
    /// 该阶段自身耗时
    pub elapsed_ms: u64,
    /// 该阶段结束时距启动起点的耗时
    pub since_start_ms: u64,
}

/// 启动完成事件的 payload。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupReadyPayload {
    pub total_ms: u64,
    pub phases: Vec<StartupPhaseTiming>,
}

/// 阶段计时器：`start` 开始，`finish` 结束并以统一格式写入日志
/// （`startup.phase=<name> ms=<n> since_start_ms=<n>`，便于在 cc-switch.log 中检索）。
pub struct StartupPhase {
    name: &'static str,
    started: Instant,
}

impl StartupPhase {
    pub fn start(name: &'static str) -> Self {
        mark_startup_begin();
        Self {
            name,
            started: Instant::now(),
        }
    }

    pub fn finish(self) -> StartupPhaseTiming {
        let timing = StartupPhaseTiming {
            phase: self.name,
            elapsed_ms: duration_ms(self.started.elapsed()),
            since_start_ms: startup_elapsed_ms(),
        };
        log::info!(
            "startup.phase={} ms={} since_start_ms={}",
            timing.phase,
            timing.elapsed_ms,
            timing.since_start_ms
        );
        timing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_error_roundtrip() {
        let payload = InitErrorPayload {
            path: "/tmp/config.json".into(),
            error: "broken json".into(),
            kind: None,
            db_version: None,
            supported_version: None,
        };
        set_init_error(payload.clone());
        let got = get_init_error().expect("should get payload back");
        assert_eq!(got.path, payload.path);
        assert_eq!(got.error, payload.error);
    }

    #[test]
    fn startup_phase_reports_monotonic_timings_in_camel_case() {
        let begin = mark_startup_begin();
        assert_eq!(mark_startup_begin(), begin, "epoch must be fixed once set");

        let phase = StartupPhase::start("unit_test_phase");
        std::thread::sleep(Duration::from_millis(5));
        let timing = phase.finish();
        assert_eq!(timing.phase, "unit_test_phase");
        assert!(timing.elapsed_ms >= 5);
        assert!(timing.since_start_ms >= timing.elapsed_ms);

        let json = serde_json::to_value(StartupReadyPayload {
            total_ms: 7,
            phases: vec![timing],
        })
        .expect("serialize");
        assert_eq!(json["totalMs"], 7);
        assert_eq!(json["phases"][0]["phase"], "unit_test_phase");
        assert!(json["phases"][0]["elapsedMs"].is_u64());
        assert!(json["phases"][0]["sinceStartMs"].is_u64());
    }
}
