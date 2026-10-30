//! 启动流水线（StartupStep pipeline）与启动诊断报告。
//!
//! `setup` 中每个可能失败的初始化/导入步骤都包装成 [`StartupStep`]：统一计时、
//! 记录结果，失败只记入报告并继续后续步骤（与此前"仅记日志"的容错语义一致）。
//! 报告经 `get_startup_report` 命令提供给前端，迁移或 live 导入失败不再表现为
//! 无声的"供应商丢失"。

use std::sync::{Arc, OnceLock, RwLock};
use std::time::Instant;

use serde::Serialize;
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::Manager;

use crate::error::{AppError, ErrorCode, IpcError};
use crate::store::AppState;

// ============================================================
// 报告模型与步骤执行器
// ============================================================

/// 单个启动步骤的结果状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "lowercase")]
pub enum StartupStepStatus {
    Ok,
    /// 条件不满足、无需执行（如表非空、无旧配置）
    Skipped,
    Failed,
}

/// 报告中的一条步骤记录
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct StartupStepRecord {
    /// 稳定的步骤 id（snake_case），不做本地化，便于跨语言排查
    pub id: String,
    pub status: StartupStepStatus,
    /// 失败时的错误码
    pub code: Option<ErrorCode>,
    /// 成功时的摘要、跳过原因或失败信息
    pub message: Option<String>,
    pub duration_ms: u32,
}

/// 启动诊断报告：按执行顺序排列的步骤记录
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct StartupReport {
    pub steps: Vec<StartupStepRecord>,
}

impl StartupReport {
    /// 执行一个步骤并记录结果
    pub fn run(&mut self, id: &str, step: impl FnOnce() -> StepResult) -> StartupStepStatus {
        let record = execute(id, step);
        let status = record.status;
        self.steps.push(record);
        status
    }

    /// 依次执行一组步骤；单步失败不中断后续步骤
    pub fn run_steps<C: ?Sized>(&mut self, ctx: &C, steps: &[StartupStep<C>]) {
        for step in steps {
            self.run(step.id, || (step.run)(ctx));
        }
    }

    pub fn failed(&self) -> impl Iterator<Item = &StartupStepRecord> {
        self.steps
            .iter()
            .filter(|s| s.status == StartupStepStatus::Failed)
    }
}

/// 启动步骤：稳定 id + 执行函数
pub struct StartupStep<C: ?Sized> {
    pub id: &'static str,
    pub run: fn(&C) -> StepResult,
}

/// 步骤成功时的结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepOutcome {
    Done(Option<String>),
    Skipped(String),
}

impl StepOutcome {
    pub fn done() -> Self {
        Self::Done(None)
    }

    pub fn done_with(summary: impl Into<String>) -> Self {
        Self::Done(Some(summary.into()))
    }

    pub fn skipped(reason: impl Into<String>) -> Self {
        Self::Skipped(reason.into())
    }
}

/// 步骤失败：错误码 + 信息
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepError {
    pub code: ErrorCode,
    pub message: String,
}

impl StepError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl From<AppError> for StepError {
    fn from(err: AppError) -> Self {
        let ipc = IpcError::from(err);
        Self::new(ipc.code, ipc.message)
    }
}

pub type StepResult = Result<StepOutcome, StepError>;

fn execute(id: &str, step: impl FnOnce() -> StepResult) -> StartupStepRecord {
    let started = Instant::now();
    let result = step();
    let duration_ms = u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX);
    let (status, code, message) = match result {
        Ok(StepOutcome::Done(summary)) => (StartupStepStatus::Ok, None, summary),
        Ok(StepOutcome::Skipped(reason)) => (StartupStepStatus::Skipped, None, Some(reason)),
        Err(err) => {
            log::warn!("✗ Startup step {id} failed [{}]: {}", err.code, err.message);
            (StartupStepStatus::Failed, Some(err.code), Some(err.message))
        }
    };
    log::debug!("Startup step {id}: {status:?} ({duration_ms}ms)");
    StartupStepRecord {
        id: id.to_string(),
        status,
        code,
        message,
        duration_ms,
    }
}

// ============================================================
// 全局报告（供 get_startup_report 读取）
// ============================================================

static REPORT: OnceLock<RwLock<StartupReport>> = OnceLock::new();

fn report_cell() -> &'static RwLock<StartupReport> {
    REPORT.get_or_init(|| RwLock::new(StartupReport::default()))
}

/// 把 `setup` 中同步执行的步骤记录追加到全局报告
pub fn publish(report: StartupReport) {
    if let Ok(mut guard) = report_cell().write() {
        guard.steps.extend(report.steps);
    }
}

/// 执行一个后台启动步骤并直接记入全局报告
pub fn run_background_step(id: &str, step: impl FnOnce() -> StepResult) {
    let record = execute(id, step);
    if let Ok(mut guard) = report_cell().write() {
        guard.steps.push(record);
    }
}

/// 当前的启动诊断报告快照
pub fn startup_report() -> StartupReport {
    report_cell()
        .read()
        .map(|guard| guard.clone())
        .unwrap_or_default()
}

// ============================================================
// 数据导入步骤（AppState 就绪后按顺序执行）
// ============================================================

/// 按表独立判断的导入逻辑：各类数据独立检查，互不影响
const DATA_IMPORT_STEPS: &[StartupStep<AppState>] = &[
    StartupStep {
        id: "default_skill_repos",
        run: init_default_skill_repos,
    },
    StartupStep {
        id: "skills_ssot_migration",
        run: migrate_skills_ssot,
    },
    StartupStep {
        id: "secrets_migration",
        run: migrate_secrets,
    },
    StartupStep {
        id: "live_config_import",
        run: import_live_configs,
    },
    StartupStep {
        id: "official_provider_seed",
        run: seed_official_providers,
    },
    StartupStep {
        id: "opencode_live_sync",
        run: sync_opencode_live,
    },
    StartupStep {
        id: "openclaw_live_sync",
        run: sync_openclaw_live,
    },
    StartupStep {
        id: "hermes_live_sync",
        run: sync_hermes_live,
    },
    StartupStep {
        id: "omo_import",
        run: import_omo,
    },
    StartupStep {
        id: "omo_slim_import",
        run: import_omo_slim,
    },
    StartupStep {
        id: "mcp_import",
        run: import_mcp_servers,
    },
    StartupStep {
        id: "prompt_import",
        run: import_prompts,
    },
];

/// 初始化默认 Skills 仓库（已有内置检查：表非空则跳过）
fn init_default_skill_repos(state: &AppState) -> StepResult {
    match state.db.init_default_skill_repos()? {
        0 => Ok(StepOutcome::skipped("skill repos already configured")),
        count => {
            log::info!("✓ Initialized {count} default skill repositories");
            Ok(StepOutcome::done_with(format!("{count} repo(s)")))
        }
    }
}

/// Skills 统一管理迁移：当数据库迁移到 v3 结构后，自动从各应用目录导入到 SSOT。
/// 触发条件由 schema 迁移设置 settings.skills_ssot_migration_pending = true 控制。
fn migrate_skills_ssot(state: &AppState) -> StepResult {
    match state.db.get_setting("skills_ssot_migration_pending")? {
        Some(flag) if flag == "true" || flag == "1" => {}
        _ => return Ok(StepOutcome::skipped("no pending migration")),
    }

    // 安全保护：如果用户已经有 v3 结构的 Skills 数据，就不要自动清空重建。
    let has_existing = state
        .db
        .get_all_installed_skills()
        .map(|skills| !skills.is_empty())
        .unwrap_or(false);

    if has_existing {
        log::info!(
            "Detected skills_ssot_migration_pending but skills table not empty; skipping auto import."
        );
        let _ = state
            .db
            .set_setting("skills_ssot_migration_pending", "false");
        return Ok(StepOutcome::skipped("skills table not empty"));
    }

    match crate::services::skill::migrate_skills_to_ssot(&state.db) {
        Ok(count) => {
            log::info!("✓ Auto imported {count} skill(s) into SSOT");
            if count > 0 {
                crate::init_status::set_skills_migration_result(count);
            }
            let _ = state
                .db
                .set_setting("skills_ssot_migration_pending", "false");
            Ok(StepOutcome::done_with(format!("{count} skill(s)")))
        }
        Err(e) => {
            crate::init_status::set_skills_migration_error(e.to_string());
            // 保留 pending 标志，方便下次启动重试
            Err(StepError::new(ErrorCode::Unknown, e.to_string()))
        }
    }
}

/// 把 settings.json 中的明文 WebDAV/S3 凭据迁入系统钥匙串（或 0600 的 secrets.json）
fn migrate_secrets(_state: &AppState) -> StepResult {
    if crate::settings::migrate_secrets()? {
        log::info!("✓ WebDAV/S3 credentials moved to the secret store");
        Ok(StepOutcome::done())
    } else {
        Ok(StepOutcome::skipped("no plaintext credentials"))
    }
}

/// live 配置不存在或该应用不支持通用导入：属于"无可导入"，不算失败
fn is_absent_live_config(err: &AppError) -> bool {
    match err {
        AppError::Unsupported { .. } => true,
        AppError::Localized { key, .. } => {
            key.ends_with(".missing") || key.ends_with(".import_unsupported")
        }
        _ => false,
    }
}

/// 自动导入 live 配置为 "default" 供应商（Claude / Codex / Gemini 等）
///
/// 先 import 后 seed 是有意为之：先把用户手动配置的 settings.json / auth.json / .env
/// 落成 "default" provider 设为 current，再追加官方预设（is_current=false）。
/// 这样用户切到官方预设时，回填机制会保护原 live 配置不丢失。
///
/// live 文件存在但读不懂（JSON 损坏、被代理接管等）记为失败，否则用户只会看到
/// 供应商列表为空。
fn import_live_configs(state: &AppState) -> StepResult {
    let mut imported = Vec::new();
    let mut failures: Vec<(String, StepError)> = Vec::new();

    for app_type in crate::app_config::AppType::all().filter(|t| !t.is_additive_mode()) {
        if !crate::services::provider::should_import_default_config_on_startup(state, &app_type)
            .unwrap_or(false)
        {
            log::debug!(
                "○ {} already has providers; live import skipped",
                app_type.as_str()
            );
            continue;
        }

        match crate::services::provider::import_default_config(state, app_type.clone()) {
            Ok(true) => {
                log::info!(
                    "✓ Imported live config for {} as default provider",
                    app_type.as_str()
                );
                imported.push(app_type.as_str().to_string());
            }
            Ok(false) => log::debug!(
                "○ {} already has providers; live import skipped",
                app_type.as_str()
            ),
            Err(e) if is_absent_live_config(&e) => {
                log::debug!("○ No live config to import for {}: {e}", app_type.as_str())
            }
            Err(e) => failures.push((app_type.as_str().to_string(), e.into())),
        }
    }

    if let Some(err) = combine_failures(failures) {
        return Err(err);
    }
    if imported.is_empty() {
        Ok(StepOutcome::skipped("nothing to import"))
    } else {
        Ok(StepOutcome::done_with(imported.join(", ")))
    }
}

fn seed_official_providers(state: &AppState) -> StepResult {
    match state.db.init_default_official_providers()? {
        0 => Ok(StepOutcome::skipped("official providers already seeded")),
        count => {
            log::info!("✓ Seeded {count} official provider(s)");
            Ok(StepOutcome::done_with(format!("{count} provider(s)")))
        }
    }
}

// 自动同步 OpenCode / OpenClaw / Hermes 的 live providers 到数据库
//
// additive 模式的 import 函数按 id 幂等——新 id 执行导入，已有 id 则更新 settings
// 和 display name，所以每次启动都跑是安全的：既保证新装用户开箱可见 live 中的
// 供应商，也让外部修改的 live 文件能在重启后同步到数据库。
//
// 底层 read_*_config 在文件不存在时返回默认空配置，因此新装且无 live 文件的
// 用户走 Ok(0) 路径，不会产生错误日志噪音。
fn additive_sync_outcome(name: &str, count: usize) -> StepOutcome {
    if count > 0 {
        log::info!("✓ Synced {count} {name} provider(s) from live config");
        StepOutcome::done_with(format!("{count} provider(s)"))
    } else {
        log::debug!("○ No {name} provider changes from live config");
        StepOutcome::skipped("no changes")
    }
}

fn sync_opencode_live(state: &AppState) -> StepResult {
    let count = crate::services::provider::import_opencode_providers_from_live(state)?;
    Ok(additive_sync_outcome("OpenCode", count))
}

fn sync_openclaw_live(state: &AppState) -> StepResult {
    let count = crate::services::provider::import_openclaw_providers_from_live(state)?;
    Ok(additive_sync_outcome("OpenClaw", count))
}

fn sync_hermes_live(state: &AppState) -> StepResult {
    let count = crate::services::provider::import_hermes_providers_from_live(state)?;
    Ok(additive_sync_outcome("Hermes", count))
}

/// OMO 配置导入（数据库中无对应分类的 provider 时，从本地文件导入）
fn import_omo_variant(
    state: &AppState,
    variant: &crate::services::omo::OmoVariant,
    category: &str,
    name: &str,
) -> StepResult {
    let has_variant = state
        .db
        .get_all_providers("opencode")
        .map(|providers| {
            providers
                .values()
                .any(|p| p.category.as_deref() == Some(category))
        })
        .unwrap_or(false);
    if has_variant {
        return Ok(StepOutcome::skipped("provider already exists"));
    }

    match crate::services::OmoService::import_from_local(state, variant) {
        Ok(provider) => {
            log::info!(
                "✓ Imported {name} config from local as provider '{}'",
                provider.name
            );
            Ok(StepOutcome::done_with(provider.name))
        }
        Err(AppError::OmoConfigNotFound) => {
            log::debug!("○ No {name} config to import");
            Ok(StepOutcome::skipped("no local config"))
        }
        Err(e) => Err(e.into()),
    }
}

fn import_omo(state: &AppState) -> StepResult {
    import_omo_variant(state, &crate::services::omo::STANDARD, "omo", "OMO")
}

fn import_omo_slim(state: &AppState) -> StepResult {
    import_omo_variant(state, &crate::services::omo::SLIM, "omo-slim", "OMO Slim")
}

/// 导入 MCP 服务器配置（表空时触发）
fn import_mcp_servers(state: &AppState) -> StepResult {
    if !state.db.is_mcp_table_empty().unwrap_or(false) {
        return Ok(StepOutcome::skipped("MCP table not empty"));
    }
    log::info!("MCP table empty, importing from live configurations...");

    let mut total = 0;
    let mut failures = Vec::new();
    for descriptor in crate::apps::registry::with_caps(crate::apps::AppCaps::MCP) {
        let name = descriptor.display_name;
        match crate::services::mcp::McpService::import_from_app(state, &descriptor.app) {
            Ok(count) if count > 0 => {
                log::info!("✓ Imported {count} MCP server(s) from {name}");
                total += count;
            }
            Ok(_) => log::debug!("○ No {name} MCP servers found to import"),
            Err(e) => failures.push((name.to_string(), StepError::from(e))),
        }
    }
    finish_per_app(total, failures, "MCP server(s)")
}

/// 导入提示词文件（表空时触发）
fn import_prompts(state: &AppState) -> StepResult {
    if !state.db.is_prompts_table_empty().unwrap_or(false) {
        return Ok(StepOutcome::skipped("prompts table not empty"));
    }
    log::info!("Prompts table empty, importing from live configurations...");

    let mut total = 0;
    let mut failures = Vec::new();
    for app in
        crate::apps::registry::with_caps(crate::apps::AppCaps::PROMPTS).map(|d| d.app.clone())
    {
        match crate::services::prompt::PromptService::import_from_file_on_first_launch(
            state,
            app.clone(),
        ) {
            Ok(count) if count > 0 => {
                log::info!("✓ Imported {count} prompt(s) for {}", app.as_str());
                total += count;
            }
            Ok(_) => log::debug!("○ No prompt file found for {}", app.as_str()),
            Err(e) => failures.push((app.as_str().to_string(), StepError::from(e))),
        }
    }
    finish_per_app(total, failures, "prompt(s)")
}

/// 合并逐应用失败：错误码取第一个失败，信息逐应用列出
fn combine_failures(failures: Vec<(String, StepError)>) -> Option<StepError> {
    let code = failures.first()?.1.code;
    let message = failures
        .iter()
        .map(|(app, err)| format!("{app}: {}", err.message))
        .collect::<Vec<_>>()
        .join("; ");
    Some(StepError::new(code, message))
}

/// 汇总逐应用导入：任一应用失败即整步失败
fn finish_per_app(total: usize, failures: Vec<(String, StepError)>, unit: &str) -> StepResult {
    match combine_failures(failures) {
        Some(err) => Err(err),
        None => Ok(StepOutcome::done_with(format!("{total} {unit}"))),
    }
}

// ============================================================
// Tauri setup
// ============================================================

/// `tauri::Builder::setup` 的实现
pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    // 预先刷新 Store 覆盖配置，确保后续路径读取正确（日志/数据库等）
    crate::app_store::refresh_app_config_dir_override(app.handle());
    crate::panic_hook::init_app_config_dir(crate::config::get_app_config_dir());
    #[cfg(target_os = "windows")]
    crate::set_windows_app_user_model_id(app.handle());

    // 注册 Updater 插件（桌面端）
    #[cfg(desktop)]
    {
        if let Err(e) = app
            .handle()
            .plugin(tauri_plugin_updater::Builder::new().build())
        {
            // 若配置不完整（如缺少 pubkey），跳过 Updater 而不中断应用
            log::warn!("初始化 Updater 插件失败，已跳过：{e}");
        }
    }
    init_logging(app)?;

    // 注入 AppHandle 给 usage_events，让无 AppHandle 持有的写日志路径
    // 也能向前端推送 `usage-log-recorded`。
    // 放在日志系统初始化之后，确保 init 的日志能正常输出。
    crate::usage_events::init(app.handle().clone());

    let mut report = StartupReport::default();
    let Some(app_state) = init_app_state(app, &mut report) else {
        // 数据库版本过新：进入恢复界面，不再执行后续步骤
        publish(report);
        return Ok(());
    };

    // 设置 AppHandle 用于代理故障转移时的 UI 更新
    app_state.proxy_service.set_app_handle(app.handle().clone());

    // 捕获首次运行快照：所有全新装用户都会看到欢迎弹窗介绍 CC Switch 的工作方式。
    // 读失败时默认不弹，宁可漏弹也不要因为故障打扰用户。
    let first_run_already_confirmed = crate::settings::get_settings()
        .first_run_notice_confirmed
        .unwrap_or(false);
    let fresh_install_at_startup = app_state.db.is_providers_empty().unwrap_or(false);

    report.run_steps(&app_state, DATA_IMPORT_STEPS);

    // 老用户 / 已确认的路径由 `fresh_install_at_startup` 自行拦截，这里不做写入。
    // 字段只由前端在用户点击"我知道了"时 save_settings 回写，语义是"用户显式确认过"。
    if !first_run_already_confirmed && fresh_install_at_startup {
        log::info!("✓ First-run welcome notice pending");
    }

    spawn_db_backfills(&app_state);

    // 迁移旧的 app_config_dir 配置到 Store
    report.run("app_config_dir_migration", || {
        crate::app_store::migrate_app_config_dir_from_settings(app.handle())?;
        Ok(StepOutcome::done())
    });
    publish(report);

    // 启动阶段不再无条件保存,避免意外覆盖用户配置。

    super::run_events::register_deep_links(app);

    build_tray(app, &app_state)?;
    crate::services::webdav_auto_sync::start_worker(app_state.db.clone(), app.handle().clone());
    crate::services::s3_auto_sync::start_worker(app_state.db.clone(), app.handle().clone());
    // 将同一个实例注入到全局状态，避免重复创建导致的不一致
    app.manage(app_state);
    app.manage(crate::services::drift::watcher::DriftWatcherState::default());

    manage_runtime_services(app);

    // 异常退出恢复 + 代理状态自动恢复
    spawn_post_setup_tasks(app.handle().clone());

    show_main_window(app);

    Ok(())
}

/// 初始化日志（单文件输出到 <app_config_dir>/logs/cc-switch.log）
fn init_logging(app: &tauri::App) -> Result<(), tauri::Error> {
    use tauri_plugin_log::{RotationStrategy, Target, TargetKind, TimezoneStrategy};

    let log_dir = crate::panic_hook::get_log_dir();

    // 确保日志目录存在
    if let Err(e) = std::fs::create_dir_all(&log_dir) {
        eprintln!("创建日志目录失败: {e}");
    }

    // 启动时删除旧日志文件，实现单文件覆盖效果
    let log_file_path = log_dir.join("cc-switch.log");
    let _ = std::fs::remove_file(&log_file_path);

    app.handle().plugin(
        tauri_plugin_log::Builder::default()
            // 初始化为 Trace，允许后续通过 log::set_max_level() 动态调整级别
            .level(log::LevelFilter::Trace)
            .targets([
                Target::new(TargetKind::Stdout),
                Target::new(TargetKind::Folder {
                    path: log_dir,
                    file_name: Some("cc-switch".into()),
                }),
            ])
            // 单文件模式：启动时删除旧文件，达到大小时轮转
            // 注意：KeepSome(n) 内部会做 n-2 运算，n=1 会导致 usize 下溢
            // KeepSome(2) 是最小安全值，表示不保留轮转文件
            .rotation_strategy(RotationStrategy::KeepSome(2))
            // 单文件大小限制 1GB
            .max_file_size(1024 * 1024 * 1024)
            .timezone_strategy(TimezoneStrategy::UseLocal)
            .build(),
    )
}

/// 旧配置校验 → 数据库版本预检 → 数据库初始化 → config.json 迁移
///
/// 返回 `None` 表示数据库版本过新，已进入恢复界面。
fn init_app_state(app: &tauri::App, report: &mut StartupReport) -> Option<AppState> {
    let app_config_dir = crate::config::get_app_config_dir();
    let db_path = app_config_dir.join("cc-switch.db");
    let json_path = app_config_dir.join("config.json");

    // 检查是否需要从 config.json 迁移到 SQLite
    let has_json = json_path.exists();
    let has_db = db_path.exists();

    // 如果需要迁移，先验证 config.json 是否可以加载（在创建数据库之前）
    // 这样如果加载失败用户选择退出，数据库文件还没被创建，下次可以正常重试
    let mut legacy_json_backup_id = None;
    let mut migration_config = None;
    report.run("legacy_config_load", || {
        if has_db || !has_json {
            return Ok(StepOutcome::skipped("no legacy config.json"));
        }
        log::info!("检测到旧版配置文件，验证配置文件...");

        // 加载会就地升级并重写 config.json，必须先原样备份
        match crate::database::Database::backup_legacy_config_json() {
            Ok(id) => legacy_json_backup_id = id,
            Err(e) => log::warn!("迁移前备份 config.json 失败，继续迁移: {e}"),
        }

        // 循环：支持用户重试加载配置文件
        loop {
            match crate::app_config::MultiAppConfig::load() {
                Ok(config) => {
                    log::info!("✓ 配置文件加载成功");
                    migration_config = Some(config);
                    return Ok(StepOutcome::done());
                }
                Err(e) => {
                    log::error!("加载旧配置文件失败: {e}");
                    // 弹出系统对话框让用户选择
                    if !crate::show_migration_error_dialog(app.handle(), &e.to_string()) {
                        // 用户选择退出（此时数据库还没创建，下次启动可以重试）
                        log::info!("用户选择退出程序");
                        std::process::exit(1);
                    }
                    // 用户选择重试，继续循环
                    log::info!("用户选择重试加载配置文件");
                }
            }
        }
    });

    // 现在创建数据库（包含 Schema 迁移）
    //
    // 说明：从 v3.8.* 升级的用户通常会走到这里的 SQLite schema 迁移，
    // 若迁移失败（数据库损坏/权限不足/user_version 过新等），需要给用户明确提示，
    // 否则表现可能只是“应用打不开/闪退”。
    //
    // 预检：数据库版本过新时，必须先于任何 schema 写操作（迁移步骤内含
    // DROP/ALTER 等 DDL）进入恢复界面，避免旧应用对读不懂的更新版 DB 落写。
    let mut db_too_new = false;
    report.run("db_version_check", || {
        match crate::database::Database::stored_user_version_exceeds_supported(&db_path) {
            Ok(Some(version)) => {
                log::warn!("数据库版本过新（v{version}），引导用户在应用内升级应用");
                db_too_new = true;
                // 附带可恢复的迁移前备份，恢复界面据此提供降级恢复（见 docs/dev/DB_COMPATIBILITY.md）
                let payload = crate::init_status::db_version_too_new_payload(&db_path, version);
                let message = payload.error.clone();
                crate::init_status::set_init_error(payload);
                Err(StepError::new(ErrorCode::DbVersionTooNew, message))
            }
            Ok(None) => Ok(StepOutcome::done()),
            Err(e) => {
                log::warn!("预检数据库版本失败，继续正常初始化流程: {e}");
                Err(e.into())
            }
        }
    });
    if db_too_new {
        // 主窗口默认 visible:false，恢复界面必须强制显示
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.set_focus();
        }
        return None;
    }

    let mut db = None;
    report.run("database_init", || loop {
        match crate::database::Database::init() {
            Ok(opened) => {
                db = Some(Arc::new(opened));
                return Ok(StepOutcome::done());
            }
            Err(e) => {
                log::error!("Failed to init database: {e}");

                if !crate::show_database_init_error_dialog(app.handle(), &db_path, &e.to_string()) {
                    log::info!("用户选择退出程序");
                    std::process::exit(1);
                }

                log::info!("用户选择重试初始化数据库");
            }
        }
    });
    #[allow(clippy::expect_used)]
    let db = db.expect("database_init either opens the database or exits");

    // 如果有预加载的配置，执行迁移
    report.run("legacy_json_migration", || {
        let Some(config) = migration_config else {
            return Ok(StepOutcome::skipped("no legacy config.json"));
        };
        log::info!("开始执行数据迁移...");

        match db.migrate_from_json(&config) {
            Ok(_) => {
                log::info!("✓ 配置迁移成功");
                // 标记迁移成功，供前端显示 Toast
                crate::init_status::set_migration_success();
                // 归档旧配置文件（重命名而非删除，便于用户恢复）
                let archive_path = json_path.with_extension("json.migrated");
                if let Err(e) = std::fs::rename(&json_path, &archive_path) {
                    log::warn!("归档旧配置文件失败: {e}");
                } else {
                    log::info!("✓ 旧配置已归档为 config.json.migrated");
                }
                Ok(StepOutcome::done())
            }
            Err(e) => {
                // 配置加载成功但迁移失败的情况极少（磁盘满等），记入报告后从现有配置导入
                let mut err = StepError::from(e);
                if let Some(id) = &legacy_json_backup_id {
                    err.message = format!("{}（迁移前备份 ID：{id}）", err.message);
                }
                Err(err)
            }
        }
    });

    Some(AppState::new(db))
}

/// 后台数据回填与 Codex 历史迁移，不阻塞启动
fn spawn_db_backfills(app_state: &AppState) {
    // 跨源去重标记 superseded_by 的分块回填：后台进行，不阻塞启动；
    // 完成前 Dashboard 查询回落到 EXISTS，结果不变
    let db_for_superseded_backfill = app_state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(e) = db_for_superseded_backfill
            .run_superseded_backfill(crate::usage_events::notify_backfill_progress)
        {
            log::warn!("✗ superseded_by backfill failed: {e}");
        }
    });

    let db = app_state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        use crate::codex_history_migration as migration;

        run_background_step("codex_history_bucket_migration", || {
            let outcome = migration::maybe_migrate_codex_third_party_history_provider_bucket(&db)?;
            if let Some(reason) = outcome.skipped_reason {
                log::debug!("○ Codex history provider bucket migration skipped: {reason}");
                return Ok(StepOutcome::skipped(reason));
            }
            log::info!(
                "✓ Codex history provider bucket migration completed: sources={}, jsonl_files={}, state_rows={}",
                outcome.source_provider_ids.len(),
                outcome.migrated_jsonl_files,
                outcome.migrated_state_rows
            );
            Ok(StepOutcome::done())
        });

        run_background_step("codex_template_bucket_migration", || {
            let outcome = migration::maybe_migrate_codex_provider_template_bucket(&db)?;
            if let Some(reason) = outcome.skipped_reason {
                log::debug!("○ Codex provider template bucket migration skipped: {reason}");
                return Ok(StepOutcome::skipped(reason));
            }
            if !outcome.migrated_provider_ids.is_empty() {
                log::info!(
                    "✓ Codex provider template bucket migration completed: providers={}",
                    outcome.migrated_provider_ids.len()
                );
            }
            Ok(StepOutcome::done())
        });

        // 统一会话开关的官方历史迁移：开关开启但上次未完成（如文件被占用
        // 中途失败）时在启动期重试；函数内部自门控，开关关闭时直接跳过。
        run_background_step("codex_official_history_unify", || {
            let outcome = migration::maybe_migrate_codex_official_history_to_unified_bucket()?;
            if let Some(reason) = outcome.skipped_reason {
                log::debug!("○ Codex official history unify migration skipped: {reason}");
                return Ok(StepOutcome::skipped(reason));
            }
            log::info!(
                "✓ Codex official history unify migration completed: jsonl_files={}, state_rows={}",
                outcome.migrated_jsonl_files,
                outcome.migrated_state_rows
            );
            Ok(StepOutcome::done())
        });
    });
}

/// 创建动态托盘菜单并构建托盘图标
fn build_tray(app: &tauri::App, app_state: &AppState) -> Result<(), Box<dyn std::error::Error>> {
    let menu = crate::tray::create_tray_menu(app.handle(), app_state)?;

    let mut tray_builder = TrayIconBuilder::with_id(crate::tray::TRAY_ID)
        .tooltip("CC Switch") // 鼠标悬停提示
        .on_tray_icon_event(|tray, event| match event {
            // 鼠标悬停/点击到托盘图标时，后台异步刷新用量缓存，
            // 让用户下一次（或快速打开菜单的那一刻）看到较新的数字。
            // refresh_all_usage_in_tray 内部有 10 秒防抖。
            TrayIconEvent::Enter { .. } | TrayIconEvent::Click { .. } => {
                let app = tray.app_handle().clone();
                tauri::async_runtime::spawn(async move {
                    crate::tray::refresh_all_usage_in_tray(&app).await;
                });
            }
            _ => log::debug!("unhandled event {event:?}"),
        })
        .menu(&menu)
        .on_menu_event(|app, event| {
            crate::tray::handle_tray_menu_event(app, &event.id.0);
        })
        .show_menu_on_left_click(true);

    // 使用平台对应的托盘图标（macOS 使用模板图标适配深浅色）
    #[cfg(target_os = "macos")]
    {
        if let Some(icon) = crate::macos_tray_icon() {
            tray_builder = tray_builder.icon(icon).icon_as_template(true);
        } else if let Some(icon) = app.default_window_icon() {
            log::warn!("Falling back to default window icon for tray");
            tray_builder = tray_builder.icon(icon.clone());
        } else {
            log::warn!("Failed to load macOS tray icon for tray");
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        if let Some(icon) = app.default_window_icon() {
            tray_builder = tray_builder.icon(icon.clone());
        } else {
            log::warn!("Failed to get default window icon for tray");
        }
    }

    let _tray = tray_builder.build(app)?;
    Ok(())
}

/// AppState 注册后：日志级别、认证管理器与全局出站代理客户端
fn manage_runtime_services(app: &tauri::App) {
    // 从数据库加载日志配置并应用
    {
        let db = &app.state::<AppState>().db;
        if let Ok(log_config) = db.get_log_config() {
            log::set_max_level(log_config.to_level_filter());
            log::info!(
                "已加载日志配置: enabled={}, level={}",
                log_config.enabled,
                log_config.level
            );
        }
    }

    // 初始化 SkillService
    let skill_service = crate::SkillService::new();
    app.manage(crate::commands::skill::SkillServiceState(Arc::new(
        skill_service,
    )));

    // 初始化 CopilotAuthManager
    {
        use crate::commands::CopilotAuthState;
        use crate::proxy::providers::copilot_auth::CopilotAuthManager;
        use tokio::sync::RwLock;

        let app_config_dir = crate::config::get_app_config_dir();
        let copilot_auth_manager = CopilotAuthManager::new(app_config_dir);
        app.manage(CopilotAuthState(Arc::new(RwLock::new(
            copilot_auth_manager,
        ))));
        log::info!("✓ CopilotAuthManager initialized");
    }

    // 初始化 CodexOAuthManager (ChatGPT Plus/Pro 反代)
    {
        use crate::commands::CodexOAuthState;
        use crate::proxy::providers::codex_oauth_auth::CodexOAuthManager;
        use tokio::sync::RwLock;

        let app_config_dir = crate::config::get_app_config_dir();
        let codex_oauth_manager = CodexOAuthManager::new(app_config_dir);
        app.manage(CodexOAuthState(Arc::new(RwLock::new(codex_oauth_manager))));
        log::info!("✓ CodexOAuthManager initialized");
    }

    // 初始化全局出站代理 HTTP 客户端
    {
        let db = &app.state::<AppState>().db;
        let proxy_url = db.get_global_proxy_url().ok().flatten();

        if let Err(e) = crate::proxy::http_client::init(proxy_url.as_deref()) {
            log::error!("[GlobalProxy] [GP-005] Failed to initialize with saved config: {e}");

            // 清除无效的代理配置
            if proxy_url.is_some() {
                log::warn!("[GlobalProxy] [GP-006] Clearing invalid proxy config from database");
                if let Err(clear_err) = db.set_global_proxy_url(None) {
                    log::error!(
                        "[GlobalProxy] [GP-007] Failed to clear invalid config: {clear_err}"
                    );
                }
            }

            // 使用直连模式重新初始化
            if let Err(fallback_err) = crate::proxy::http_client::init(None) {
                log::error!(
                    "[GlobalProxy] [GP-008] Failed to initialize direct connection: {fallback_err}"
                );
            }
        }
    }
}

/// 异常退出恢复、代理状态恢复、漂移监听与周期任务
fn spawn_post_setup_tasks(app_handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = app_handle.state::<AppState>();

        // 检查是否有 Live 备份（表示上次异常退出时可能处于接管状态）
        let has_backups = match state.db.has_any_live_backup().await {
            Ok(v) => v,
            Err(e) => {
                log::error!("检查 Live 备份失败: {e}");
                false
            }
        };
        // 检查 Live 配置是否仍处于被接管状态（包含占位符）
        let live_taken_over = state.proxy_service.detect_takeover_in_live_configs();

        if has_backups || live_taken_over {
            log::warn!("检测到上次异常退出（存在接管残留），正在恢复 Live 配置...");
            let result = state.proxy_service.recover_from_crash().await;
            run_background_step("live_takeover_recovery", || match result {
                Ok(()) => {
                    log::info!("Live 配置已恢复");
                    Ok(StepOutcome::done())
                }
                Err(e) => {
                    log::error!("恢复 Live 配置失败: {e}");
                    Err(StepError::new(ErrorCode::Unknown, e.to_string()))
                }
            });
        }

        crate::initialize_common_config_snippets(&state);

        // 检查 settings 表中的代理状态，自动恢复代理服务
        crate::restore_proxy_state_on_startup(&state).await;

        // live 配置漂移检测：恢复完成后检测一次，并监听应用外的改动（CCS-166）
        let drift_handle = app_handle.clone();
        match tauri::async_runtime::spawn_blocking(move || {
            crate::services::drift::watcher::start(drift_handle)
        })
        .await
        {
            Ok(Ok(watcher)) => {
                if let Some(slot) =
                    app_handle.try_state::<crate::services::drift::watcher::DriftWatcherState>()
                {
                    if let Ok(mut slot) = slot.0.lock() {
                        *slot = Some(watcher);
                    }
                }
            }
            Ok(Err(e)) => log::warn!("启动 live 配置漂移监听失败: {e}"),
            Err(e) => log::warn!("启动 live 配置漂移监听任务失败: {e}"),
        }

        // Periodic backup check (on startup)
        if let Err(e) = state.db.periodic_backup_if_needed() {
            log::warn!("Periodic backup failed on startup: {e}");
        }

        // Periodic maintenance timer: run once per day while the app is running
        let db_for_timer = state.db.clone();
        tauri::async_runtime::spawn(async move {
            const PERIODIC_MAINTENANCE_INTERVAL_SECS: u64 = 24 * 60 * 60;
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(
                PERIODIC_MAINTENANCE_INTERVAL_SECS,
            ));
            interval.tick().await; // skip immediate first tick (already checked above)
            loop {
                interval.tick().await;
                if let Err(e) = db_for_timer.periodic_backup_if_needed() {
                    log::warn!("Periodic maintenance timer failed: {e}");
                }
            }
        });

        // Session log usage sync: 启动时同步一次，之后每 60 秒检查
        tauri::async_runtime::spawn(session_usage_sync_loop(state.db.clone()));
    });
}

/// 会话日志用量同步：启动时同步一次，之后每 60 秒检查
async fn session_usage_sync_loop(db: Arc<crate::database::Database>) {
    const SESSION_SYNC_INTERVAL_SECS: u64 = 60;

    // 每个步骤都包含文件扫描和同步 Mutex 数据库操作，
    // 放到阻塞线程池中执行，避免占用 tokio 工作线程
    async fn run_step<T: Send + 'static>(
        name: &'static str,
        db: &Arc<crate::database::Database>,
        step: fn(&crate::database::Database) -> Result<T, AppError>,
    ) {
        let db = db.clone();
        let result =
            crate::services::session_usage::run_sync_blocking(name, move || step(&db)).await;
        if let Err(e) = result {
            log::warn!("{name} failed: {e}");
        }
    }

    let db = &db;

    // 首次同步
    run_step(
        "Usage cost startup backfill",
        db,
        crate::database::Database::backfill_missing_usage_costs,
    )
    .await;
    run_step(
        "Session usage initial sync",
        db,
        crate::services::session_usage::sync_claude_session_logs,
    )
    .await;
    run_step(
        "Codex usage initial sync",
        db,
        crate::services::session_usage_codex::sync_codex_usage,
    )
    .await;
    run_step(
        "Gemini usage initial sync",
        db,
        crate::services::session_usage_gemini::sync_gemini_usage,
    )
    .await;
    run_step(
        "OpenCode usage initial sync",
        db,
        crate::services::session_usage_opencode::sync_opencode_usage,
    )
    .await;

    // 定期同步
    let mut interval =
        tokio::time::interval(std::time::Duration::from_secs(SESSION_SYNC_INTERVAL_SECS));
    interval.tick().await; // skip immediate first tick
    loop {
        interval.tick().await;
        run_step(
            "Session usage periodic sync",
            db,
            crate::services::session_usage::sync_claude_session_logs,
        )
        .await;
        run_step(
            "Codex usage periodic sync",
            db,
            crate::services::session_usage_codex::sync_codex_usage,
        )
        .await;
        run_step(
            "Gemini usage periodic sync",
            db,
            crate::services::session_usage_gemini::sync_gemini_usage,
        )
        .await;
        run_step(
            "OpenCode usage periodic sync",
            db,
            crate::services::session_usage_opencode::sync_opencode_usage,
        )
        .await;
    }
}

/// 静默启动：根据设置决定是否显示主窗口
fn show_main_window(app: &tauri::App) {
    // Linux: 禁用 WebKitGTK 硬件加速，防止 EGL 初始化失败导致白屏
    #[cfg(target_os = "linux")]
    {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.with_webview(|webview| {
                use webkit2gtk::{HardwareAccelerationPolicy, SettingsExt, WebViewExt};
                let wk_webview = webview.inner();
                if let Some(settings) = WebViewExt::settings(&wk_webview) {
                    SettingsExt::set_hardware_acceleration_policy(
                        &settings,
                        HardwareAccelerationPolicy::Never,
                    );
                    log::info!("已禁用 WebKitGTK 硬件加速");
                }
            });
        }
    }

    let settings = crate::settings::get_settings();
    if let Some(window) = app.get_webview_window("main") {
        // 在窗口首次显示前同步装饰状态，避免前端加载后再切换导致标题栏闪烁
        // 仅 Linux 生效：解决 Wayland 下系统窗口按钮不可用的问题
        #[cfg(target_os = "linux")]
        let _ = window.set_decorations(!settings.use_app_window_controls);
        if settings.silent_startup {
            // 静默启动模式：保持窗口隐藏
            let _ = window.hide();
            #[cfg(target_os = "windows")]
            let _ = window.set_skip_taskbar(true);
            #[cfg(target_os = "macos")]
            crate::tray::apply_tray_policy(app.handle(), false);
            log::info!("静默启动模式：主窗口已隐藏");
        } else {
            // 正常启动模式：显示窗口
            let _ = window.show();
            log::info!("正常启动模式：主窗口已显示");

            // Linux: 解决首次启动 UI 无响应问题（Tauri #10746 + wry #637）。
            // 启动时 webview 未获取焦点 + surface 尺寸协商失败，导致点击无效。
            // 这里做 set_focus + 伪 resize，等价于无视觉版本的"最大化-还原"。
            #[cfg(target_os = "linux")]
            {
                crate::linux_fix::nudge_main_window(window.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    fn ok_step(_: &()) -> StepResult {
        Ok(StepOutcome::done_with("fine"))
    }

    fn skipped_step(_: &()) -> StepResult {
        Ok(StepOutcome::skipped("nothing to do"))
    }

    fn failing_step(_: &()) -> StepResult {
        Err(AppError::Database("disk I/O error".into()).into())
    }

    #[test]
    fn failing_step_is_reported_with_its_error_code_and_later_steps_still_run() {
        let steps = [
            StartupStep {
                id: "first",
                run: ok_step,
            },
            StartupStep {
                id: "broken",
                run: failing_step,
            },
            StartupStep {
                id: "after",
                run: skipped_step,
            },
        ];
        let mut report = StartupReport::default();
        report.run_steps(&(), &steps);

        let ids: Vec<_> = report.steps.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["first", "broken", "after"]);

        let failed: Vec<_> = report.failed().collect();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].id, "broken");
        assert_eq!(failed[0].status, StartupStepStatus::Failed);
        assert_eq!(failed[0].code, Some(ErrorCode::Database));
        assert!(failed[0]
            .message
            .as_deref()
            .is_some_and(|m| m.contains("disk I/O error")));

        assert_eq!(report.steps[0].status, StartupStepStatus::Ok);
        assert_eq!(report.steps[0].code, None);
        assert_eq!(report.steps[2].status, StartupStepStatus::Skipped);
        assert_eq!(report.steps[2].message.as_deref(), Some("nothing to do"));
    }

    #[test]
    fn report_serializes_for_the_frontend() {
        let mut report = StartupReport::default();
        report.run("db_version_check", || {
            Err(StepError::new(ErrorCode::DbVersionTooNew, "too new"))
        });
        let value = serde_json::to_value(&report).expect("serialize report");
        let step = &value["steps"][0];
        assert_eq!(step["id"], "db_version_check");
        assert_eq!(step["status"], "failed");
        assert_eq!(step["code"], "DB_VERSION_TOO_NEW");
        assert_eq!(step["message"], "too new");
        assert!(step["durationMs"].is_number());
    }

    #[test]
    fn published_steps_are_visible_in_the_global_report() {
        let mut report = StartupReport::default();
        report.run("bootstrap_test_published_step", || {
            Err(StepError::new(ErrorCode::Io, "unreadable"))
        });
        publish(report);
        run_background_step("bootstrap_test_background_step", || Ok(StepOutcome::done()));

        let global = startup_report();
        let published = global
            .steps
            .iter()
            .find(|s| s.id == "bootstrap_test_published_step")
            .expect("published step");
        assert_eq!(published.code, Some(ErrorCode::Io));
        assert!(
            global
                .steps
                .iter()
                .any(|s| s.id == "bootstrap_test_background_step"
                    && s.status == StartupStepStatus::Ok)
        );
    }

    #[test]
    fn missing_or_unsupported_live_config_is_not_a_failure() {
        assert!(is_absent_live_config(&AppError::localized(
            "claude.live.missing",
            "缺失",
            "missing"
        )));
        assert!(is_absent_live_config(&AppError::localized(
            "claude_desktop.import_unsupported",
            "不支持",
            "unsupported"
        )));
        assert!(!is_absent_live_config(&AppError::Config("bad".into())));
        assert!(!is_absent_live_config(&AppError::localized(
            "provider.import.live_taken_over",
            "接管",
            "taken over"
        )));
    }

    fn state_with_home(home: &std::path::Path) -> (AppState, crate::paths::ScopedPaths) {
        let paths = crate::paths::scoped(crate::paths::Paths::with_home(home));
        let db = Arc::new(Database::memory().expect("in-memory database"));
        (AppState::new(db), paths)
    }

    #[test]
    fn live_import_without_live_files_is_skipped() {
        let home = tempfile::tempdir().expect("tempdir");
        let (state, _paths) = state_with_home(home.path());

        let mut report = StartupReport::default();
        report.run("live_config_import", || import_live_configs(&state));

        assert_eq!(report.steps[0].status, StartupStepStatus::Skipped);
        assert_eq!(report.failed().count(), 0);
    }

    /// 回归：损坏的 live 配置此前只记一条 debug 日志，用户只看到"没有供应商"
    #[test]
    fn corrupt_live_config_fails_the_import_step_with_its_error_code() {
        let home = tempfile::tempdir().expect("tempdir");
        let claude_dir = home.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).expect("create claude dir");
        std::fs::write(claude_dir.join("settings.json"), "{ not json").expect("write settings");
        let (state, _paths) = state_with_home(home.path());

        let mut report = StartupReport::default();
        report.run_steps(
            &state,
            &[StartupStep {
                id: "live_config_import",
                run: import_live_configs,
            }],
        );

        let failed: Vec<_> = report.failed().collect();
        assert_eq!(failed.len(), 1, "report: {report:?}");
        assert_eq!(failed[0].id, "live_config_import");
        assert_eq!(failed[0].code, Some(ErrorCode::Json));
        assert!(failed[0]
            .message
            .as_deref()
            .is_some_and(|m| m.starts_with("claude: ")));
        assert!(state
            .db
            .get_all_providers("claude")
            .expect("read providers")
            .is_empty());
    }
}
