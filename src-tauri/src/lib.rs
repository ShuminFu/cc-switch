// 生产代码由 lib 目标按 clippy.toml 检查；单元测试可直接用 std::fs::write 准备夹具
#![cfg_attr(test, allow(clippy::disallowed_methods))]

mod app_config;
mod app_shell;
mod app_store;
mod apps;
mod auto_launch;
#[cfg(test)]
mod bindings;
mod commands;
mod config;
mod database;
mod deeplink;
mod error;
mod gemini_mcp;
mod init_status;
mod lightweight;
#[cfg(target_os = "linux")]
mod linux_fix;
#[cfg(any(test, feature = "testkit"))]
#[doc(hidden)]
pub mod live_config_testkit;
mod mcp;
mod model_capabilities;
mod panic_hook;
pub mod paths;
mod picked_paths;
mod platform;
mod pricing;
mod prompt;
mod prompt_files;
mod provider;
mod provider_defaults;
mod proxy;
/// End-to-end proxy harness, used by `tests/proxy_e2e.rs`.
#[cfg(feature = "testkit")]
#[doc(hidden)]
pub mod proxy_harness;
mod secret_store;
mod services;
mod session_manager;
mod settings;
mod store;

mod tray;
mod usage_events;
mod usage_script;

// 配置读写代码已迁入 apps/<app>/；保留旧路径别名，调用方无需改动
use apps::claude::claude_json as claude_mcp;
use apps::claude::plugin as claude_plugin;
use apps::claude_desktop::config as claude_desktop_config;
use apps::codex::config as codex_config;
use apps::codex::history_migration as codex_history_migration;
use apps::codex::state_db as codex_state_db;
use apps::gemini::config as gemini_config;
use apps::grok::config as grok_config;
pub use apps::hermes::config as hermes_config;
use apps::openclaw::config as openclaw_config;
use apps::opencode::config as opencode_config;

pub use app_config::{AppSet, AppType, InstalledSkill, McpServer, MultiAppConfig};
pub use codex_config::{get_codex_auth_path, get_codex_config_path, write_codex_live_atomic};
pub use commands::open_provider_terminal;
pub use commands::*;
pub use config::{get_claude_mcp_path, get_claude_settings_path, read_json_file};
pub use database::{Database, Profile, SCHEMA_VERSION};
pub use deeplink::{import_provider_from_deeplink, parse_deeplink_url, DeepLinkImportRequest};
pub use error::AppError;
pub use mcp::{
    import_from_claude, import_from_codex, import_from_gemini, import_from_grokbuild,
    remove_server_from_claude, remove_server_from_codex, remove_server_from_gemini,
    remove_server_from_grokbuild, sync_enabled_to_claude, sync_enabled_to_codex,
    sync_enabled_to_gemini, sync_single_server_to_claude, sync_single_server_to_codex,
    sync_single_server_to_gemini, sync_single_server_to_grokbuild,
};
pub use prompt::Prompt;
pub use provider::{Provider, ProviderMeta};
pub use services::{
    profile::{ProfilePayload, ProfileScope, ProfileService},
    provider::{
        reapply_current_codex_official_live, LiveFilePreview, LivePreviewSkipReason,
        LiveWritePreview,
    },
    skill::{migrate_skills_to_ssot, ImportSkillSelection},
    ConfigService, EndpointLatency, McpService, PromptService, ProviderService, ProxyService,
    SkillService, SpeedtestService,
};
pub use settings::{scoped_settings_store, update_settings, AppSettings, ScopedSettingsStore};
pub use store::AppState;

/// Golden bridge testkit, used by `tests/proxy_golden.rs`.
#[cfg(any(test, feature = "testkit"))]
#[doc(hidden)]
pub use proxy::providers::testkit as proxy_testkit;

use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

#[cfg(target_os = "macos")]
use tauri::image::Image;
use tauri::Manager;
use tauri_plugin_window_state::{AppHandleExt, StateFlags};

#[cfg(target_os = "windows")]
fn set_windows_app_user_model_id(app: &tauri::AppHandle) {
    let app_id = app.config().identifier.clone();
    let wide_app_id: Vec<u16> = app_id.encode_utf16().chain(std::iter::once(0)).collect();

    let result = unsafe {
        windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(wide_app_id.as_ptr())
    };

    if result < 0 {
        log::warn!("设置 Windows AppUserModelID 失败: 0x{result:08X}");
    } else {
        log::debug!("Windows AppUserModelID 已设置为 {app_id}");
    }
}

#[tauri::command]
async fn update_tray_menu(app: tauri::AppHandle) -> Result<bool, String> {
    match tray::rebuild_tray_menu(&app).await {
        Ok(updated) => Ok(updated),
        Err(err) => {
            log::error!("创建托盘菜单失败: {err}");
            Ok(false)
        }
    }
}

#[cfg(target_os = "macos")]
fn macos_tray_icon() -> Option<Image<'static>> {
    const ICON_BYTES: &[u8] = include_bytes!("../icons/tray/macos/statusbar_template_3x.png");

    match Image::from_bytes(ICON_BYTES) {
        Ok(icon) => Some(icon),
        Err(err) => {
            log::warn!("Failed to load macOS tray icon: {err}");
            None
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 设置 panic hook，在应用崩溃时记录日志到 <app_config_dir>/crash.log（默认 ~/.cc-switch/crash.log）
    panic_hook::setup_panic_hook();

    let mut builder = tauri::Builder::default();

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(
            app_shell::run_events::on_second_instance,
        ));
    }

    let builder = builder
        // 注册 deep-link 插件（处理 macOS AppleEvent 和其他平台的深链接）
        .plugin(tauri_plugin_deep_link::init())
        // 拦截窗口关闭：根据设置决定是否最小化到托盘
        .on_window_event(app_shell::run_events::on_window_event)
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(window_state_flags())
                .build(),
        )
        .setup(app_shell::bootstrap::setup)
        .invoke_handler(invoke_handler());

    let app = builder
        .build(tauri::generate_context!())
        .expect("error while running tauri application");

    app.run(app_shell::run_events::on_run_event);
}

/// 全部已注册的 Tauri 命令（`bindings.rs` 从此列表解析 `COMMANDS`）
fn invoke_handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        commands::get_app_registry,
        commands::get_providers,
        commands::get_current_provider,
        commands::add_provider,
        commands::update_provider,
        commands::preview_live_write,
        commands::delete_provider,
        commands::remove_provider_from_live_config,
        commands::switch_provider,
        commands::revert_last_switch,
        commands::import_default_config,
        commands::get_claude_desktop_status,
        commands::get_claude_desktop_default_routes,
        commands::import_claude_desktop_providers_from_claude,
        commands::ensure_claude_desktop_official_provider,
        commands::ensure_codex_official_provider,
        commands::get_claude_code_config_path,
        commands::get_config_dir,
        commands::open_config_folder,
        commands::pick_directory,
        commands::open_external,
        commands::get_init_error,
        commands::get_migration_result,
        commands::get_skills_migration_result,
        commands::get_startup_report,
        commands::get_app_config_path,
        commands::open_app_config_folder,
        commands::get_claude_common_config_snippet,
        commands::set_claude_common_config_snippet,
        commands::get_common_config_snippet,
        commands::set_common_config_snippet,
        commands::update_toml_common_config_snippet,
        commands::extract_common_config_snippet,
        commands::read_live_provider_settings,
        commands::get_settings,
        commands::save_settings,
        commands::get_settings_load_failure,
        commands::acknowledge_settings_load_failure,
        commands::has_codex_unify_history_backup,
        commands::restore_codex_unified_history,
        commands::get_rectifier_config,
        commands::set_rectifier_config,
        commands::get_optimizer_config,
        commands::set_optimizer_config,
        commands::get_log_config,
        commands::set_log_config,
        commands::restart_app,
        commands::install_update_and_restart,
        commands::check_app_update_available,
        commands::check_for_updates,
        commands::is_portable_mode,
        commands::copy_text_to_clipboard,
        commands::apply_claude_plugin_config,
        commands::apply_claude_onboarding_skip,
        commands::clear_claude_onboarding_skip,
        // Claude MCP management
        commands::get_claude_mcp_status,
        commands::read_claude_mcp_config,
        commands::upsert_claude_mcp_server,
        commands::delete_claude_mcp_server,
        commands::validate_mcp_command,
        // usage query
        commands::query_provider_usage,
        commands::test_usage_script,
        // subscription quota
        commands::get_subscription_quota,
        commands::get_codex_oauth_quota,
        commands::get_codex_oauth_models,
        commands::get_coding_plan_quota,
        commands::get_balance,
        // New MCP via config.json (SSOT)
        commands::get_mcp_config,
        commands::upsert_mcp_server_in_config,
        commands::delete_mcp_server_in_config,
        commands::set_mcp_enabled,
        // Unified MCP management
        commands::get_mcp_servers,
        commands::upsert_mcp_server,
        commands::delete_mcp_server,
        commands::toggle_mcp_app,
        commands::import_mcp_from_apps,
        // Prompt management
        commands::get_prompts,
        commands::upsert_prompt,
        commands::delete_prompt,
        commands::enable_prompt,
        commands::import_prompt_from_file,
        commands::get_current_prompt_file_content,
        // Profile management (项目配置方案)
        commands::list_profiles,
        commands::create_profile,
        commands::update_profile,
        commands::delete_profile,
        commands::clear_current_profile,
        commands::apply_profile,
        // model list fetch (OpenAI-compatible /v1/models)
        commands::fetch_models_for_config,
        // ours: endpoint speed test + custom endpoint management
        commands::test_api_endpoints,
        commands::get_custom_endpoints,
        commands::add_custom_endpoint,
        commands::remove_custom_endpoint,
        commands::update_endpoint_last_used,
        // app_config_dir override via Store
        commands::get_app_config_dir_override,
        commands::set_app_config_dir_override,
        // provider sort order management
        commands::update_providers_sort_order,
        // theirs: config import/export and dialogs
        commands::export_config_to_file,
        commands::import_config_from_file,
        commands::webdav_test_connection,
        commands::webdav_sync_upload,
        commands::webdav_sync_download,
        commands::webdav_sync_save_settings,
        commands::webdav_sync_fetch_remote_info,
        commands::s3_test_connection,
        commands::s3_sync_upload,
        commands::s3_sync_download,
        commands::s3_sync_save_settings,
        commands::s3_sync_fetch_remote_info,
        commands::get_sync_snapshot_key_status,
        commands::set_sync_snapshot_key,
        commands::save_file_dialog,
        commands::save_csv_file_dialog,
        commands::write_csv_export,
        commands::open_file_dialog,
        commands::open_zip_file_dialog,
        commands::create_db_backup,
        commands::list_db_backups,
        commands::restore_db_backup,
        commands::restore_backup_over_newer_db,
        commands::rename_db_backup,
        commands::delete_db_backup,
        commands::get_db_storage_stats,
        commands::run_db_maintenance,
        commands::sync_current_providers_live,
        // Deep link import
        commands::merge_deeplink_config,
        commands::preview_deeplink_import,
        commands::import_from_deeplink_unified,
        update_tray_menu,
        // Environment variable management
        commands::check_env_conflicts,
        commands::delete_env_vars,
        commands::restore_env_backup,
        // Skill management (v3.10.0+ unified)
        commands::get_installed_skills,
        commands::get_skill_backups,
        commands::delete_skill_backup,
        commands::install_skill_unified,
        commands::uninstall_skill_unified,
        commands::restore_skill_backup,
        commands::toggle_skill_app,
        commands::scan_unmanaged_skills,
        commands::import_skills_from_apps,
        commands::discover_available_skills,
        commands::check_skill_updates,
        commands::update_skill,
        commands::migrate_skill_storage,
        commands::search_skills_sh,
        // Skill management (legacy API compatibility)
        commands::get_skills,
        commands::get_skills_for_app,
        commands::install_skill,
        commands::install_skill_for_app,
        commands::uninstall_skill,
        commands::uninstall_skill_for_app,
        commands::get_skill_repos,
        commands::add_skill_repo,
        commands::remove_skill_repo,
        commands::install_skills_from_zip,
        // Auto launch
        commands::set_auto_launch,
        commands::get_auto_launch_status,
        // Proxy server management
        commands::start_proxy_server,
        commands::stop_proxy_server,
        commands::stop_proxy_with_restore,
        commands::get_proxy_takeover_status,
        commands::set_proxy_takeover_for_app,
        commands::get_live_config_drift,
        commands::get_all_live_config_drift,
        commands::resolve_live_config_drift,
        commands::get_proxy_status,
        commands::get_proxy_config,
        commands::update_proxy_config,
        // Bug report capture
        commands::start_proxy_capture,
        commands::stop_proxy_capture,
        commands::get_proxy_capture_status,
        commands::export_proxy_bug_report,
        // Request trace inspector
        commands::get_proxy_request_trace,
        // Global & Per-App Config
        commands::get_global_proxy_config,
        commands::update_global_proxy_config,
        commands::get_proxy_config_for_app,
        commands::update_proxy_config_for_app,
        commands::get_default_cost_multiplier,
        commands::set_default_cost_multiplier,
        commands::get_pricing_model_source,
        commands::set_pricing_model_source,
        commands::is_proxy_running,
        commands::is_live_takeover_active,
        commands::switch_proxy_provider,
        // Proxy failover commands
        commands::get_provider_health,
        commands::reset_circuit_breaker,
        commands::get_circuit_breaker_config,
        commands::update_circuit_breaker_config,
        commands::get_circuit_breaker_stats,
        commands::probe_circuit_breaker,
        // Failover queue management
        commands::get_failover_queue,
        commands::get_available_providers_for_failover,
        commands::add_to_failover_queue,
        commands::remove_from_failover_queue,
        commands::get_auto_failover_enabled,
        commands::set_auto_failover_enabled,
        // Usage statistics
        commands::get_usage_summary,
        commands::get_usage_summary_by_app,
        commands::get_usage_trends,
        commands::get_provider_stats,
        commands::get_model_stats,
        commands::get_request_logs,
        commands::get_unpriced_model_stats,
        commands::get_request_detail,
        commands::get_model_pricing,
        commands::update_model_pricing,
        commands::delete_model_pricing,
        commands::check_provider_limits,
        // Session usage sync
        commands::sync_session_usage,
        commands::rescan_session_usage,
        commands::get_usage_data_sources,
        // Stream health check
        commands::stream_check_provider,
        commands::stream_check_all_providers,
        commands::get_stream_check_config,
        commands::save_stream_check_config,
        // Session manager
        commands::list_sessions,
        commands::get_session_messages,
        commands::delete_session,
        commands::delete_sessions,
        commands::launch_session_terminal,
        commands::get_tool_versions,
        commands::run_tool_lifecycle_action,
        commands::probe_tool_installations,
        // Provider terminal
        commands::open_provider_terminal,
        // Universal Provider management
        commands::get_universal_providers,
        commands::get_universal_provider,
        commands::upsert_universal_provider,
        commands::delete_universal_provider,
        commands::sync_universal_provider,
        // OpenCode specific
        commands::import_opencode_providers_from_live,
        commands::get_opencode_live_provider_ids,
        // OpenClaw specific
        commands::import_openclaw_providers_from_live,
        commands::get_openclaw_live_provider_ids,
        commands::get_openclaw_live_provider,
        commands::scan_openclaw_config_health,
        commands::get_openclaw_default_model,
        commands::set_openclaw_default_model,
        commands::get_openclaw_model_catalog,
        commands::set_openclaw_model_catalog,
        commands::get_openclaw_agents_defaults,
        commands::set_openclaw_agents_defaults,
        commands::get_openclaw_env,
        commands::set_openclaw_env,
        commands::get_openclaw_tools,
        commands::set_openclaw_tools,
        // Hermes specific
        commands::import_hermes_providers_from_live,
        commands::get_hermes_live_provider_ids,
        commands::get_hermes_model_config,
        commands::open_hermes_web_ui,
        commands::launch_hermes_dashboard,
        commands::get_hermes_memory,
        commands::set_hermes_memory,
        commands::get_hermes_memory_limits,
        commands::set_hermes_memory_enabled,
        // Global upstream proxy
        commands::get_global_proxy_url,
        commands::set_global_proxy_url,
        commands::test_proxy_url,
        commands::get_upstream_proxy_status,
        commands::scan_local_proxies,
        // Window theme control
        commands::set_window_theme,
        // Generic managed auth commands
        commands::auth_start_login,
        commands::auth_poll_for_account,
        commands::auth_list_accounts,
        commands::auth_get_status,
        commands::auth_remove_account,
        commands::auth_set_default_account,
        commands::auth_logout,
        // Copilot OAuth commands (multi-account support)
        commands::copilot_start_device_flow,
        commands::copilot_poll_for_auth,
        commands::copilot_poll_for_account,
        commands::copilot_list_accounts,
        commands::copilot_remove_account,
        commands::copilot_set_default_account,
        commands::copilot_get_auth_status,
        commands::copilot_logout,
        commands::copilot_is_authenticated,
        commands::copilot_get_token,
        commands::copilot_get_token_for_account,
        commands::copilot_get_models,
        commands::copilot_get_models_for_account,
        commands::copilot_get_usage,
        commands::copilot_get_usage_for_account,
        // OMO commands
        commands::read_omo_local_file,
        commands::get_current_omo_provider_id,
        commands::disable_current_omo,
        commands::read_omo_slim_local_file,
        commands::get_current_omo_slim_provider_id,
        commands::disable_current_omo_slim,
        // Workspace files (OpenClaw)
        commands::read_workspace_file,
        commands::write_workspace_file,
        // Daily memory files (OpenClaw workspace)
        commands::list_daily_memory_files,
        commands::read_daily_memory_file,
        commands::write_daily_memory_file,
        commands::delete_daily_memory_file,
        commands::search_daily_memory_files,
        commands::open_workspace_directory,
    ]
}

// ============================================================
// 应用退出清理
// ============================================================

/// 应用退出前的清理工作
///
/// 在应用退出前检查代理服务器状态，如果正在运行则停止代理并恢复 Live 配置。
/// 确保 Claude Code/Codex/Gemini 的配置不会处于损坏状态。
/// 使用 stop_with_restore_keep_state 保留 settings 表中的代理状态，下次启动时自动恢复。
pub async fn cleanup_before_exit(app_handle: &tauri::AppHandle) {
    if let Some(state) = app_handle.try_state::<store::AppState>() {
        let proxy_service = &state.proxy_service;

        // 退出时也需要兜底：代理可能已崩溃/未运行，但 Live 接管残留仍在（占位符/备份）。
        let has_backups = match state.db.has_any_live_backup().await {
            Ok(v) => v,
            Err(e) => {
                log::error!("退出时检查 Live 备份失败: {e}");
                false
            }
        };
        let live_taken_over = proxy_service.detect_takeover_in_live_configs();
        let needs_restore = has_backups || live_taken_over;

        if needs_restore {
            log::info!("检测到接管残留，开始恢复 Live 配置（保留代理状态）...");
            // 使用 keep_state 版本，保留 settings 表中的代理状态
            if let Err(e) = proxy_service.stop_with_restore_keep_state().await {
                log::error!("退出时恢复 Live 配置失败: {e}");
            } else {
                log::info!("已恢复 Live 配置（代理状态已保留，下次启动将自动恢复）");
            }
            return;
        }

        // 非接管模式：代理在运行则仅停止代理
        if proxy_service.is_running().await {
            log::info!("检测到代理服务器正在运行，开始停止...");
            if let Err(e) = proxy_service.stop().await {
                log::error!("退出时停止代理失败: {e}");
            }
            log::info!("代理服务器清理完成");
        }
    }
}

/// 主动从系统托盘移除托盘图标。
///
/// `std::process::exit` 会绕过 Tauri 运行时，触发不了 `TrayIcon::drop()`，
/// 也就不会向 Windows Shell 发 `NIM_DELETE`。结果是进程退出后托盘里
/// 仍保留一个死图标的缓存占位（Shell 不会主动重绘，需要鼠标悬停才刷新）。
///
/// 通过 `set_visible(false)` 走 `WM_USER_HIDE_TRAYICON` 消息路径，
/// 触发 tray-icon 内部的 `remove_tray_icon` → `Shell_NotifyIconW(NIM_DELETE)`，
/// 在进程结束前干净地把图标摘掉。其它平台 `set_visible(false)` 也是
/// 正常的隐藏/移除语义，作为跨平台兜底也安全。
pub(crate) fn remove_tray_icon_before_exit(app_handle: &tauri::AppHandle) {
    if let Some(tray) = app_handle.tray_by_id(tray::TRAY_ID) {
        if let Err(e) = tray.set_visible(false) {
            log::warn!("退出时移除托盘图标失败: {e}");
        } else {
            log::info!("已显式从系统托盘移除图标");
        }
    }
}

// ============================================================
// 启动时恢复代理状态
// ============================================================

/// 启动时根据 proxy_config 表中的代理状态自动恢复代理服务
///
/// 检查 `proxy_config.enabled` 字段，如果有任一应用的状态为 `true`，
/// 则自动启动代理服务并接管对应应用的 Live 配置。
async fn enabled_proxy_apps_on_startup(db: &database::Database) -> Vec<&'static str> {
    let mut apps = Vec::new();
    for app_type in apps::registry::with_caps(apps::AppCaps::PROXY_TAKEOVER).map(|d| d.slug) {
        if db
            .get_proxy_config_for_app(app_type)
            .await
            .is_ok_and(|config| config.enabled)
        {
            apps.push(app_type);
        }
    }
    apps
}

async fn restore_proxy_state_on_startup(state: &store::AppState) {
    // 收集需要恢复接管的应用列表（从 proxy_config.enabled 读取）
    let apps_to_restore = enabled_proxy_apps_on_startup(&state.db).await;

    if apps_to_restore.is_empty() {
        log::debug!("启动时无需恢复代理状态");
        return;
    }

    log::info!("检测到上次代理状态需要恢复，应用列表: {apps_to_restore:?}");

    // 逐个恢复接管状态
    for app_type in apps_to_restore {
        match state
            .proxy_service
            .set_takeover_for_app(app_type, true)
            .await
        {
            Ok(()) => {
                log::info!("✓ 已恢复 {app_type} 的代理接管状态");
            }
            Err(e) => {
                log::error!("✗ 恢复 {app_type} 的代理接管状态失败: {e}");
                // 失败时清除该应用的状态，避免下次启动再次尝试
                if let Err(clear_err) = state
                    .proxy_service
                    .set_takeover_for_app(app_type, false)
                    .await
                {
                    log::error!("清除 {app_type} 代理状态失败: {clear_err}");
                }
            }
        }
    }
}

fn initialize_common_config_snippets(state: &store::AppState) {
    // Auto-extract common config snippets from clean live files when snippet is missing.
    // This must run before proxy takeover is restored on startup, otherwise we'd read
    // proxy-placeholder configs instead of the user's actual live settings.
    for app_type in crate::app_config::AppType::all() {
        if !state
            .db
            .should_auto_extract_config_snippet(app_type.as_str())
            .unwrap_or(false)
        {
            continue;
        }

        let settings = match crate::services::provider::ProviderService::read_live_settings(
            app_type.clone(),
        ) {
            Ok(s) => s,
            Err(_) => continue,
        };

        match crate::services::provider::ProviderService::extract_common_config_snippet_from_settings(
            app_type.clone(),
            &settings,
        ) {
            Ok(snippet) if !snippet.is_empty() && snippet != "{}" => {
                match state.db.set_config_snippet(app_type.as_str(), Some(snippet)) {
                    Ok(()) => {
                        let _ = state.db.set_config_snippet_cleared(app_type.as_str(), false);
                        log::info!(
                            "✓ Auto-extracted common config snippet for {}",
                            app_type.as_str()
                        );
                    }
                    Err(e) => log::warn!(
                        "✗ Failed to save config snippet for {}: {e}",
                        app_type.as_str()
                    ),
                }
            }
            Ok(_) => log::debug!(
                "○ Live config for {} has no extractable common fields",
                app_type.as_str()
            ),
            Err(e) => log::warn!(
                "✗ Failed to extract config snippet for {}: {e}",
                app_type.as_str()
            ),
        }
    }

    let should_run_legacy_migration = state
        .db
        .is_legacy_common_config_migrated()
        .map(|done| !done)
        .unwrap_or(true);

    if should_run_legacy_migration {
        for app_type in [
            crate::app_config::AppType::Claude,
            crate::app_config::AppType::Codex,
            crate::app_config::AppType::Gemini,
        ] {
            if let Err(e) = crate::services::provider::ProviderService::migrate_legacy_common_config_usage_if_needed(
                state,
                app_type.clone(),
            ) {
                log::warn!(
                    "✗ Failed to migrate legacy common-config usage for {}: {e}",
                    app_type.as_str()
                );
            }
        }

        if let Err(e) = state.db.set_legacy_common_config_migrated(true) {
            log::warn!("✗ Failed to persist legacy common-config migration flag: {e}");
        }
    }
}

// ============================================================
// 迁移错误对话框辅助函数
// ============================================================

/// 检测是否为中文环境
fn is_chinese_locale() -> bool {
    std::env::var("LANG")
        .or_else(|_| std::env::var("LC_ALL"))
        .or_else(|_| std::env::var("LC_MESSAGES"))
        .map(|lang| lang.starts_with("zh"))
        .unwrap_or(false)
}

/// 显示迁移错误对话框
/// 返回 true 表示用户选择重试，false 表示用户选择退出
fn show_migration_error_dialog(app: &tauri::AppHandle, error: &str) -> bool {
    let title = if is_chinese_locale() {
        "配置迁移失败"
    } else {
        "Migration Failed"
    };

    let message = if is_chinese_locale() {
        format!(
            "从旧版本迁移配置时发生错误：\n\n{error}\n\n\
            您的数据尚未丢失，旧配置文件仍然保留。\n\
            建议回退到旧版本 CC Switch 以保护数据。\n\n\
            点击「重试」重新尝试迁移\n\
            点击「退出」关闭程序（可回退版本后重新打开）"
        )
    } else {
        format!(
            "An error occurred while migrating configuration:\n\n{error}\n\n\
            Your data is NOT lost - the old config file is still preserved.\n\
            Consider rolling back to an older CC Switch version.\n\n\
            Click 'Retry' to attempt migration again\n\
            Click 'Exit' to close the program"
        )
    };

    let retry_text = if is_chinese_locale() {
        "重试"
    } else {
        "Retry"
    };
    let exit_text = if is_chinese_locale() {
        "退出"
    } else {
        "Exit"
    };

    // 使用 blocking_show 同步等待用户响应
    // OkCancelCustom: 第一个按钮（重试）返回 true，第二个按钮（退出）返回 false
    app.dialog()
        .message(&message)
        .title(title)
        .kind(MessageDialogKind::Error)
        .buttons(MessageDialogButtons::OkCancelCustom(
            retry_text.to_string(),
            exit_text.to_string(),
        ))
        .blocking_show()
}

/// 显示数据库初始化/Schema 迁移失败对话框
/// 返回 true 表示用户选择重试，false 表示用户选择退出
fn show_database_init_error_dialog(
    app: &tauri::AppHandle,
    db_path: &std::path::Path,
    error: &str,
) -> bool {
    let title = if is_chinese_locale() {
        "数据库初始化失败"
    } else {
        "Database Initialization Failed"
    };

    let message = if is_chinese_locale() {
        format!(
            "初始化数据库或迁移数据库结构时发生错误：\n\n{error}\n\n\
            数据库文件路径：\n{db}\n\n\
            您的数据尚未丢失，应用不会自动删除数据库文件。\n\
            常见原因包括：数据库版本过新、文件损坏、权限不足、磁盘空间不足等。\n\n\
            建议：\n\
            1) 先备份整个配置目录（包含 cc-switch.db）\n\
            2) 如果提示“数据库版本过新”，请升级到更新版本\n\
            3) 如果刚升级出现异常，可回退旧版本导出/备份后再升级\n\n\
            点击「重试」重新尝试初始化\n\
            点击「退出」关闭程序",
            db = db_path.display()
        )
    } else {
        format!(
            "An error occurred while initializing or migrating the database:\n\n{error}\n\n\
            Database file path:\n{db}\n\n\
            Your data is NOT lost - the app will not delete the database automatically.\n\
            Common causes include: newer database version, corrupted file, permission issues, or low disk space.\n\n\
            Suggestions:\n\
            1) Back up the entire config directory (including cc-switch.db)\n\
            2) If you see “database version is newer”, please upgrade CC Switch\n\
            3) If this happened right after upgrading, consider rolling back to export/backup then upgrade again\n\n\
            Click 'Retry' to attempt initialization again\n\
            Click 'Exit' to close the program",
            db = db_path.display()
        )
    };

    let retry_text = if is_chinese_locale() {
        "重试"
    } else {
        "Retry"
    };
    let exit_text = if is_chinese_locale() {
        "退出"
    } else {
        "Exit"
    };

    app.dialog()
        .message(&message)
        .title(title)
        .kind(MessageDialogKind::Error)
        .buttons(MessageDialogButtons::OkCancelCustom(
            retry_text.to_string(),
            exit_text.to_string(),
        ))
        .blocking_show()
}

// ============================================================
// 在应用主动退出前显式持久化窗口状态
// ============================================================

fn window_state_flags() -> StateFlags {
    StateFlags::POSITION | StateFlags::SIZE | StateFlags::MAXIMIZED
}

/// 当前应用的退出路径会拦截 `ExitRequested` 并最终直接 `std::process::exit(0)`，
/// 这里需要在真正结束进程前手动落盘，避免 window-state 插件的默认退出钩子被绕过。
pub fn save_window_state_before_exit(app_handle: &tauri::AppHandle) {
    if let Err(err) = app_handle.save_window_state(window_state_flags()) {
        log::error!("退出前保存窗口状态失败: {err}");
    } else {
        log::info!("已在退出前保存窗口状态");
    }
}

/// 主动释放 single-instance 锁。
///
/// macOS single-instance 使用 `/tmp/{identifier}.sock`。我们有若干路径会直接
/// `std::process::exit(0)`，不会触发插件挂在 `RunEvent::Exit` 上的清理钩子。
/// 重启前主动 destroy 可以避免新进程误连旧 listener 后自行退出。
pub fn destroy_single_instance_lock(app_handle: &tauri::AppHandle) {
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    tauri_plugin_single_instance::destroy(app_handle);
}

/// 清理托盘图标、释放 single-instance 锁后重启当前应用。
///
/// 直接走 `tauri::process::restart`（spawn 新进程 + `exit(0)`），不经过事件
/// 循环退出，因此 Tauri 内部的 `cleanup_before_exit` 和各插件的
/// `RunEvent::Exit` 钩子都不会执行。需要的清理由调用方与本函数显式补偿：
/// 窗口状态、代理/Live 恢复（调用方）；托盘图标、single-instance 锁（本函数）。
///
/// 有意不调 `AppHandle::cleanup_before_exit()`：它会在调用线程上 Drop 托盘
/// 图标，而 macOS 的 NSStatusItem 操作要求主线程；`set_visible(false)` 走
/// `run_item_main_thread` 代理，跨线程安全（见 `remove_tray_icon_before_exit`）。
pub fn restart_process(app_handle: &tauri::AppHandle) -> ! {
    remove_tray_icon_before_exit(app_handle);
    destroy_single_instance_lock(app_handle);
    tauri::process::restart(&app_handle.env());
}

#[cfg(test)]
mod tests {
    use super::enabled_proxy_apps_on_startup;
    use crate::database::Database;

    #[tokio::test]
    async fn startup_restore_includes_enabled_grokbuild_route() {
        let db = Database::memory().expect("initialize database");
        let mut config = db
            .get_proxy_config_for_app("grokbuild")
            .await
            .expect("read Grok Build proxy config");
        config.enabled = true;
        db.update_proxy_config_for_app(config)
            .await
            .expect("enable Grok Build proxy config");

        let apps = enabled_proxy_apps_on_startup(&db).await;

        assert_eq!(apps, vec!["grokbuild"]);
    }
}
