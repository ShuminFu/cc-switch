//! 托盘菜单管理模块
//!
//! 负责系统托盘图标和菜单的创建、更新和事件处理。

use std::sync::LazyLock;
use tauri::menu::{CheckMenuItem, Menu, MenuBuilder, MenuItem, Submenu, SubmenuBuilder};
use tauri::{Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

use crate::app_config::AppType;
use crate::error::AppError;
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

/// 每个 app 分区的子菜单句柄，用于 usage 更新时就地改 label 而非整菜单重建。
/// `create_tray_menu` 每次重建都会整表覆盖写入，保证句柄始终指向当前活跃菜单。
static TRAY_SECTION_SUBMENUS: LazyLock<
    std::sync::Mutex<std::collections::HashMap<AppType, Submenu<tauri::Wry>>>,
> = LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// 托盘菜单文本（国际化）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrayTexts {
    pub show_main: &'static str,
    pub open_website: &'static str,
    pub no_providers_label: &'static str,
    pub lightweight_mode: &'static str,
    pub quit: &'static str,
    pub _auto_label: &'static str,
    pub projects_label: &'static str,
    pub no_project_label: &'static str,
}

/// 将系统区域标识映射为托盘支持的语言码。
///
/// 镜像前端 `i18n/getInitialLanguage` 的判定顺序，确保首次安装
/// （`settings.language` 尚未写入）时托盘语言与界面语言一致：
/// 繁中系统（zh-TW/HK/MO/Hant）→ `zh-TW`，其余 zh → `zh`，
/// 日文 → `ja`，英文 → `en`，其余未知区域（de/fr/ko 等）回退到 `en`
/// （与前端默认一致）。
fn map_locale_to_tray_language(locale: &str) -> &'static str {
    // 部分平台返回 POSIX 风格的 `zh_TW`，统一成 BCP 47 的连字符形式。
    let locale = locale.to_lowercase().replace('_', "-");
    if locale == "zh" {
        "zh"
    } else if locale.starts_with("zh-tw")
        || locale.starts_with("zh-hk")
        || locale.starts_with("zh-mo")
        || locale.starts_with("zh-hant")
    {
        "zh-TW"
    } else if locale.starts_with("zh") {
        "zh"
    } else if locale.starts_with("ja") {
        "ja"
    } else {
        // 英文以及所有未知区域
        "en"
    }
}

/// 读取系统区域并映射为应用语言码；取不到区域时回退到 `en`。
///
/// 后端其它在 `settings.language` 未设置时需要选择提示语言的地方也复用它。
pub(crate) fn detect_system_tray_language() -> &'static str {
    sys_locale::get_locale()
        .as_deref()
        .map(map_locale_to_tray_language)
        .unwrap_or("en")
}

/// The one backend language source (CMD-17): `settings.language` when set,
/// otherwise the system locale. Used by the tray and the startup dialogs.
pub(crate) fn backend_language() -> String {
    crate::settings::get_settings()
        .language
        .filter(|l| !l.trim().is_empty())
        .unwrap_or_else(|| detect_system_tray_language().to_string())
}

impl TrayTexts {
    pub fn from_language(language: &str) -> Self {
        match language {
            "en" => Self {
                show_main: "Open main window",
                open_website: "Open Official Website",
                no_providers_label: "(no providers)",
                lightweight_mode: "Lightweight Mode",
                quit: "Quit",
                _auto_label: "Auto (Failover)",
                projects_label: "Projects",
                no_project_label: "No project",
            },
            "ja" => Self {
                show_main: "メインウィンドウを開く",
                open_website: "公式サイトを開く",
                no_providers_label: "(プロバイダーなし)",
                lightweight_mode: "軽量モード",
                quit: "終了",
                _auto_label: "自動 (フェイルオーバー)",
                projects_label: "プロジェクト",
                no_project_label: "プロジェクトを使用しない",
            },
            "zh-TW" => Self {
                show_main: "開啟主介面",
                open_website: "開啟官方網站",
                no_providers_label: "(無供應商)",
                lightweight_mode: "輕量模式",
                quit: "退出",
                _auto_label: "自動 (故障轉移)",
                projects_label: "專案",
                no_project_label: "不使用專案",
            },
            _ => Self {
                show_main: "打开主界面",
                open_website: "打开官方网站",
                no_providers_label: "(无供应商)",
                lightweight_mode: "轻量模式",
                quit: "退出",
                _auto_label: "自动 (故障转移)",
                projects_label: "项目",
                no_project_label: "不使用项目",
            },
        }
    }
}

/// 托盘应用分区配置
pub struct TrayAppSection {
    pub app_type: AppType,
    pub prefix: &'static str,
    pub empty_id: &'static str,
    pub header_label: &'static str,
    pub log_name: &'static str,
}

/// Auto 菜单项后缀
pub const AUTO_SUFFIX: &str = "auto";
pub const TRAY_ID: &str = "cc-switch";

pub const TRAY_SECTIONS: [TrayAppSection; 5] = [
    TrayAppSection {
        app_type: AppType::Claude,
        prefix: "claude_",
        empty_id: "claude_empty",
        header_label: "Claude",
        log_name: "Claude",
    },
    // PRODUCT-15: Claude Desktop is an exclusive (one current provider) app like
    // Claude Code, so it switches through the same ProviderService::switch path.
    TrayAppSection {
        app_type: AppType::ClaudeDesktop,
        prefix: "claudedesktop_",
        empty_id: "claudedesktop_empty",
        header_label: "Claude Desktop",
        log_name: "Claude Desktop",
    },
    TrayAppSection {
        app_type: AppType::Codex,
        prefix: "codex_",
        empty_id: "codex_empty",
        header_label: "Codex",
        log_name: "Codex",
    },
    TrayAppSection {
        app_type: AppType::Gemini,
        prefix: "gemini_",
        empty_id: "gemini_empty",
        header_label: "Gemini",
        log_name: "Gemini",
    },
    TrayAppSection {
        app_type: AppType::GrokBuild,
        prefix: "grokbuild_",
        empty_id: "grokbuild_empty",
        header_label: "Grok Build",
        log_name: "Grok Build",
    },
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

fn format_usage_suffix(
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

/// 对供应商列表排序：sort_index → created_at → name
fn sort_providers(
    providers: &indexmap::IndexMap<String, crate::provider::Provider>,
) -> Vec<(&String, &crate::provider::Provider)> {
    let mut sorted: Vec<_> = providers.iter().collect();
    sorted.sort_by(|(_, a), (_, b)| {
        match (a.sort_index, b.sort_index) {
            (Some(idx_a), Some(idx_b)) => return idx_a.cmp(&idx_b),
            (Some(_), None) => return std::cmp::Ordering::Less,
            (None, Some(_)) => return std::cmp::Ordering::Greater,
            _ => {}
        }

        match (a.created_at, b.created_at) {
            (Some(time_a), Some(time_b)) => return time_a.cmp(&time_b),
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            _ => {}
        }

        a.name.cmp(&b.name)
    });
    sorted
}

/// 处理项目 Profile 托盘事件，返回是否已处理
///
/// 事件 id 形如 `profile_<scope>_<uuid>`（同一项目在各分组子菜单里各有一项，
/// 应用时只作用于该分组）；`profile_none_<scope>` 表示某分组"不使用项目"
/// （只清该分组标记，不动配置）。
pub fn handle_profile_tray_event(app: &tauri::AppHandle, event_id: &str) -> bool {
    let Some(suffix) = event_id.strip_prefix("profile_") else {
        return false;
    };

    if let Some(scope_str) = suffix.strip_prefix("none_") {
        let Ok(scope) = crate::services::profile::ProfileScope::parse(scope_str) else {
            log::error!("未知的项目分组托盘事件: {event_id}");
            return true;
        };
        // 清除当前项目；若有首次应用前的基线则恢复它（会写 live 文件，放到阻塞线程池）。
        // 收尾事件里 profileId=null 表示该分组已清除当前项目。
        let app_handle = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let Some(app_state) = app_handle.try_state::<AppState>() else {
                return;
            };
            match crate::services::profile::ProfileService::clear_current(app_state.inner(), scope)
            {
                Ok((warnings, should_stop_proxy)) => {
                    for warning in &warnings {
                        log::warn!("[Profile] 清除当前项目警告: {warning}");
                    }
                    if should_stop_proxy {
                        let app_handle2 = app_handle.clone();
                        let proxy_service = app_state.proxy_service.clone();
                        tauri::async_runtime::spawn(async move {
                            if let Err(e) = proxy_service.stop().await {
                                log::warn!("托盘清除项目后停止代理服务失败: {e}");
                            }
                            if let Some(state) = app_handle2.try_state::<AppState>() {
                                crate::commands::emit_profile_events(
                                    &app_handle2,
                                    state.inner(),
                                    None,
                                    scope,
                                );
                            }
                        });
                    } else {
                        crate::commands::emit_profile_events(
                            &app_handle,
                            app_state.inner(),
                            None,
                            scope,
                        );
                    }
                }
                Err(e) => {
                    log::error!("清除当前项目失败: {e}");
                    refresh_tray_menu(&app_handle);
                }
            }
        });
        return true;
    }

    // scope 是固定枚举字符串（不含下划线），uuid 只含连字符，首个下划线即分界
    let Some((scope_str, profile_id)) = suffix.split_once('_') else {
        log::error!("无法解析项目托盘事件: {event_id}");
        return true;
    };
    let Ok(scope) = crate::services::profile::ProfileScope::parse(scope_str) else {
        log::error!("未知的项目分组托盘事件: {event_id}");
        return true;
    };

    log::info!("应用项目: {profile_id}（{scope_str} 组）");
    let app_handle = app.clone();
    let profile_id = profile_id.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(app_state) = app_handle.try_state::<AppState>() else {
            return;
        };
        match crate::services::profile::ProfileService::apply(app_state.inner(), &profile_id, scope)
        {
            Ok((warnings, should_stop_proxy)) => {
                for warning in &warnings {
                    log::warn!("[Profile] 应用项目 {profile_id} 警告: {warning}");
                }

                if should_stop_proxy {
                    let app_handle2 = app_handle.clone();
                    let proxy_service = app_state.proxy_service.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = proxy_service.stop().await {
                            log::warn!("托盘切换项目后停止代理服务失败: {e}");
                        }
                        if let Some(state) = app_handle2.try_state::<AppState>() {
                            crate::commands::emit_profile_apply_events(
                                &app_handle2,
                                state.inner(),
                                &profile_id,
                                scope,
                            );
                        }
                    });
                } else {
                    crate::commands::emit_profile_apply_events(
                        &app_handle,
                        app_state.inner(),
                        &profile_id,
                        scope,
                    );
                }
            }
            Err(e) => {
                log::error!("应用项目 {profile_id} 失败: {e}");
                refresh_tray_menu(&app_handle);
            }
        }
    });
    true
}

/// 处理供应商托盘事件
pub fn handle_provider_tray_event(app: &tauri::AppHandle, event_id: &str) -> bool {
    for section in TRAY_SECTIONS.iter() {
        if let Some(suffix) = event_id.strip_prefix(section.prefix) {
            // 处理 Auto 点击
            if suffix == AUTO_SUFFIX {
                log::info!("切换到{} Auto模式", section.log_name);
                let app_handle = app.clone();
                let app_type = section.app_type.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = handle_auto_click(&app_handle, &app_type).await {
                        log::error!("切换{}Auto模式失败: {e}", section.log_name);
                        // 原生勾选状态已被点击切换，重建以恢复真实状态
                        request_tray_rebuild(&app_handle, true);
                    }
                });
                return true;
            }

            // 处理供应商点击
            log::info!("切换到{}供应商: {suffix}", section.log_name);
            let app_handle = app.clone();
            let provider_id = suffix.to_string();
            let app_type = section.app_type.clone();
            tauri::async_runtime::spawn_blocking(move || {
                if let Err(e) = handle_provider_click(&app_handle, &app_type, &provider_id) {
                    log::error!("切换{}供应商失败: {e}", section.log_name);
                    // 原生勾选状态已被点击切换，重建以恢复真实状态
                    request_tray_rebuild(&app_handle, true);
                }
            });
            return true;
        }
    }
    false
}

/// 处理 Auto 点击：启用 proxy 和 auto_failover
///
/// 代理服务的状态读写都在 tokio 锁之后，因此这里是 async 函数，由托盘事件
/// 通过 `tauri::async_runtime::spawn` 驱动，而不是在阻塞线程里
/// 阻塞式 `block_on` 等待 tokio 锁（CMD-07）。
/// Whether the tray "Auto" item may turn on the proxy/failover at all: the
/// UI only offers the proxy once the user enabled the local proxy feature.
fn tray_auto_allowed(settings: &crate::settings::AppSettings) -> bool {
    settings.enable_local_proxy
}

async fn handle_auto_click(app: &tauri::AppHandle, app_type: &AppType) -> Result<(), AppError> {
    let Some(app_state) = app.try_state::<AppState>() else {
        return Ok(());
    };
    let app_type_str = app_type.as_str();

    if !tray_auto_allowed(&crate::settings::get_settings()) {
        return Err(AppError::Message(
            "本地代理功能未启用，无法从托盘开启 Auto 模式".to_string(),
        ));
    }

    let proxy_service = &app_state.proxy_service;

    // 1) 托盘的便捷路径：该 app 尚未接管时先启动代理并接管 Live 配置
    //    （set_takeover_for_app 会在代理未运行时自动启动）。
    let took_over_here = !app_state
        .db
        .get_proxy_config_for_app(app_type_str)
        .await?
        .enabled;
    if took_over_here || !proxy_service.is_running().await {
        log::info!("[Tray] Auto 模式：对 {app_type_str} 执行接管");
        if let Err(e) = proxy_service.set_takeover_for_app(app_type_str, true).await {
            log::error!("[Tray] 执行接管失败: {e}");
            return Err(AppError::Message(format!("执行接管失败: {e}")));
        }
    }

    // 2) 与 UI 命令共用：切到队列 P1 成功后才持久化 auto_failover_enabled，
    //    失败时移除自动加入的队列项（CMD-44）。
    let p1_provider_id =
        match crate::commands::enable_auto_failover(app_state.inner(), app_type).await {
            Ok(id) => id,
            Err(e) => {
                log::error!("[Tray] Auto 模式开启失败: {e}");
                // 回滚本次点击做的接管，恢复点击前的状态
                if took_over_here {
                    if let Err(undo) = proxy_service
                        .set_takeover_for_app(app_type_str, false)
                        .await
                    {
                        log::error!("[Tray] 回滚 {app_type_str} 接管失败: {undo}");
                    }
                }
                return Err(AppError::Message(format!("Auto 模式开启失败: {e}")));
            }
        };

    // 4) 更新托盘菜单
    request_tray_rebuild(app, true);

    // 5) 发射事件到前端
    let event_data = serde_json::json!({
        "appType": app_type_str,
        "proxyEnabled": true,
        "autoFailoverEnabled": true,
        "providerId": p1_provider_id
    });
    if let Err(e) = app.emit(crate::events::PROXY_FLAGS_CHANGED, event_data.clone()) {
        log::error!("发射 proxy-flags-changed 事件失败: {e}");
    }
    // 发射 provider-switched 事件（保持向后兼容，Auto 切换也算一种切换）
    if let Err(e) = app.emit(crate::events::PROVIDER_SWITCHED, event_data) {
        log::error!("发射 provider-switched 事件失败: {e}");
    }
    Ok(())
}

/// 处理供应商点击：关闭 auto_failover + 切换供应商
fn handle_provider_click(
    app: &tauri::AppHandle,
    app_type: &AppType,
    provider_id: &str,
) -> Result<(), AppError> {
    if let Some(app_state) = app.try_state::<AppState>() {
        let app_type_str = app_type.as_str();

        // 获取当前 proxy 状态，保持 enabled 不变，只关闭 auto_failover
        let (proxy_enabled, _) = app_state.db.get_proxy_flags_sync(app_type_str);
        app_state
            .db
            .set_proxy_flags_sync(app_type_str, proxy_enabled, false)?;

        // 切换供应商。需要本地路由的供应商也不在这里自动启动代理，
        // 由用户在页面/设置中手动开启。
        crate::services::ProviderService::switch(app_state.inner(), app_type.clone(), provider_id)?;

        // 更新托盘菜单
        request_tray_rebuild(app, true);

        // 发射事件到前端
        let event_data = serde_json::json!({
            "appType": app_type_str,
            "proxyEnabled": proxy_enabled,
            "autoFailoverEnabled": false,
            "providerId": provider_id
        });
        if let Err(e) = app.emit(crate::events::PROXY_FLAGS_CHANGED, event_data.clone()) {
            log::error!("发射 proxy-flags-changed 事件失败: {e}");
        }
        // 发射 provider-switched 事件（保持向后兼容）
        if let Err(e) = app.emit(crate::events::PROVIDER_SWITCHED, event_data) {
            log::error!("发射 provider-switched 事件失败: {e}");
        }
    }
    Ok(())
}

// ============================================================
// 托盘菜单模型（PERF-16）
//
// 重建分两步：先从 DB / 缓存收集一个纯数据的 `TrayModel`，再据此构建
// 原生菜单。模型可比较：内容未变时跳过 `set_menu`（macOS 上 `set_menu`
// 会关闭用户正打开的菜单），并且所有异步状态都以非阻塞方式读取，
// 不再用阻塞式 `block_on` 等待 tokio 锁（CMD-07）。
// ============================================================

#[derive(Debug, Clone, PartialEq)]
struct TrayProviderItem {
    id: String,
    label: String,
    enabled: bool,
    checked: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum TraySectionBody {
    /// 没有供应商：显示禁用的提示项
    Empty { label: String },
    /// 子菜单：标题 + 供应商列表
    Providers {
        label: String,
        items: Vec<TrayProviderItem>,
    },
}

#[derive(Debug, Clone, PartialEq)]
struct TraySectionModel {
    /// `TRAY_SECTIONS` 下标
    section: usize,
    body: TraySectionBody,
}

#[derive(Debug, Clone, PartialEq)]
struct TrayProfileItem {
    id: String,
    name: String,
    checked: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct TrayProfileScopeModel {
    scope: &'static str,
    label: &'static str,
    profiles: Vec<TrayProfileItem>,
    none_checked: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct TrayModel {
    texts: TrayTexts,
    sections: Vec<TraySectionModel>,
    profile_scopes: Vec<TrayProfileScopeModel>,
    lightweight: bool,
}

/// 最近一次装到托盘上的菜单对应的模型（`None` 表示托盘上是骨架菜单或尚未构建）。
static LAST_TRAY_MODEL: std::sync::Mutex<Option<TrayModel>> = std::sync::Mutex::new(None);

/// 串行化"收集模型 + 构建菜单 + set_menu"，避免并发重建按错误顺序落到托盘上。
static TRAY_REBUILD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 上一次成功读到的代理运行状态；锁被占用（启动/停止进行中）时沿用它。
static LAST_PROXY_RUNNING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// 非阻塞读取代理运行状态。
///
/// `ProxyService::is_running` 等待的是 tokio `RwLock`：锁空闲时 future 首次 poll
/// 即完成；锁被写者占用时返回 `None`（调用方沿用上次的值并安排一次补刷），
/// 绝不在当前线程上阻塞等待。
fn proxy_running_snapshot(app_state: &AppState) -> (bool, bool) {
    use futures::FutureExt;
    use std::sync::atomic::Ordering;

    match app_state.proxy_service.is_running().now_or_never() {
        Some(running) => {
            LAST_PROXY_RUNNING.store(running, Ordering::Release);
            (running, false)
        }
        None => (LAST_PROXY_RUNNING.load(Ordering::Acquire), true),
    }
}

/// Live 配置文件的指纹：每个文件的 (修改时间, 大小)，文件不存在为 `None`。
type LiveFingerprint = Vec<Option<(std::time::SystemTime, u64)>>;

/// 按应用缓存"Live 配置是否含接管占位符"的检测结果，文件指纹不变时不再读取解析。
static LIVE_TAKEOVER_CACHE: LazyLock<
    std::sync::Mutex<std::collections::HashMap<AppType, (LiveFingerprint, bool)>>,
> = LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

fn live_config_paths(app_type: &AppType) -> Vec<std::path::PathBuf> {
    match app_type {
        AppType::Claude => vec![crate::config::get_claude_settings_path()],
        AppType::Codex => vec![
            crate::codex_config::get_codex_auth_path(),
            crate::codex_config::get_codex_config_path(),
        ],
        AppType::Gemini => vec![crate::gemini_config::get_gemini_env_path()],
        AppType::GrokBuild => vec![crate::grok_config::get_grok_config_path()],
        _ => Vec::new(),
    }
}

fn live_fingerprint(paths: &[std::path::PathBuf]) -> LiveFingerprint {
    paths
        .iter()
        .map(|path| {
            let meta = std::fs::metadata(path).ok()?;
            Some((meta.modified().ok()?, meta.len()))
        })
        .collect()
}

/// 读取缓存；指纹匹配时返回缓存值，否则调用 `detect` 并写回缓存。
fn cached_takeover_detection(
    app_type: &AppType,
    fingerprint: LiveFingerprint,
    detect: impl FnOnce() -> bool,
) -> bool {
    {
        let cache = LIVE_TAKEOVER_CACHE
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if let Some((cached_fp, value)) = cache.get(app_type) {
            if *cached_fp == fingerprint {
                return *value;
            }
        }
    }
    let value = detect();
    LIVE_TAKEOVER_CACHE
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(app_type.clone(), (fingerprint, value));
    value
}

fn live_config_taken_over(app_state: &AppState, app_type: &AppType) -> bool {
    let paths = live_config_paths(app_type);
    if paths.is_empty() {
        return app_state
            .proxy_service
            .detect_takeover_in_live_config_for_app(app_type);
    }
    cached_takeover_detection(app_type, live_fingerprint(&paths), || {
        app_state
            .proxy_service
            .detect_takeover_in_live_config_for_app(app_type)
    })
}

/// 与 `settings::get_effective_current_provider` 语义一致，但复用已加载的供应商表，
/// 不再为验证存在性重新扫描一遍整张表。
fn effective_current_provider_from(
    app_state: &AppState,
    app_type: &AppType,
    providers: &indexmap::IndexMap<String, crate::provider::Provider>,
) -> Result<Option<String>, AppError> {
    if let Some(local_id) = crate::settings::get_current_provider(app_type) {
        if providers.contains_key(&local_id) {
            return Ok(Some(local_id));
        }
        log::warn!(
            "本地 settings 中的供应商 {} ({}) 在数据库中不存在，将清理并 fallback 到数据库",
            local_id,
            app_type.as_str()
        );
        let _ = crate::settings::set_current_provider(app_type, None);
    }
    app_state.db.get_current_provider(app_type.as_str())
}

fn section_label(
    section: &TrayAppSection,
    provider: &crate::provider::Provider,
    suffix: &str,
) -> String {
    format!("{} · {}{}", section.header_label, provider.name, suffix)
}

/// 收集托盘菜单所需的全部数据。返回值第二项为 `true` 表示代理状态锁被占用，
/// 本次用了上次的值，调用方应稍后补刷一次。
fn collect_tray_model(app_state: &AppState) -> Result<(TrayModel, bool), AppError> {
    collect_tray_model_with(app_state, crate::settings::get_settings())
}

/// [`collect_tray_model`] with the settings passed in, so tests can build the
/// model for any language / visibility without touching the global settings
/// cache (TEST-16, TEST-09).
fn collect_tray_model_with(
    app_state: &AppState,
    app_settings: crate::settings::AppSettings,
) -> Result<(TrayModel, bool), AppError> {
    // 用户未显式设置语言（首次安装）时，按系统区域回退而非硬编码简体，
    // 否则繁中系统的托盘会固定显示简体直到用户手动切换一次。
    let language: &str = match app_settings.language.as_deref() {
        Some(lang) => lang,
        None => detect_system_tray_language(),
    };
    let texts = TrayTexts::from_language(language);

    // Get visible apps setting, default to all visible
    let visible_apps = app_settings.visible_apps.unwrap_or_default();

    // Pre-compute proxy running state (used to disable official providers in tray menu)
    let (is_proxy_running, proxy_state_stale) = proxy_running_snapshot(app_state);

    let mut sections = Vec::new();
    for (index, section) in TRAY_SECTIONS.iter().enumerate() {
        if !visible_apps.is_visible(&section.app_type) {
            continue;
        }

        let app_type_str = section.app_type.as_str();
        let providers = app_state.db.get_all_providers(app_type_str)?;

        if providers.is_empty() {
            sections.push(TraySectionModel {
                section: index,
                body: TraySectionBody::Empty {
                    label: format!("{} {}", section.header_label, texts.no_providers_label),
                },
            });
            continue;
        }

        let current_id = effective_current_provider_from(app_state, &section.app_type, &providers)?
            .unwrap_or_default();

        let label = match providers.get(&current_id) {
            Some(p) => {
                let suffix = format_usage_suffix(app_state, &section.app_type, p, &current_id)
                    .unwrap_or_default();
                section_label(section, p, &suffix)
            }
            None => section.header_label.to_string(),
        };

        // Check if this app is under proxy takeover (for disabling official providers).
        // `get_live_backup` 只持有 std 互斥锁、内部没有 await 点，首次 poll 即完成。
        let is_app_taken_over = is_proxy_running && {
            use futures::FutureExt;
            let has_backup = app_state
                .db
                .get_live_backup(app_type_str)
                .now_or_never()
                .and_then(|r| r.ok())
                .flatten()
                .is_some();
            has_backup || live_config_taken_over(app_state, &section.app_type)
        };

        let items = sort_providers(&providers)
            .into_iter()
            .map(|(id, provider)| {
                let is_official_blocked = is_app_taken_over
                    && provider.category.as_deref() == Some("official")
                    && !crate::services::provider::official_provider_supports_proxy_takeover(
                        &section.app_type,
                        provider,
                    );
                TrayProviderItem {
                    id: id.clone(),
                    label: if is_official_blocked {
                        format!("{} \u{26D4}", &provider.name) // ⛔ emoji
                    } else {
                        provider.name.clone()
                    },
                    enabled: !is_official_blocked, // disabled when blocked
                    checked: current_id == *id,
                }
            })
            .collect();

        sections.push(TraySectionModel {
            section: index,
            body: TraySectionBody::Providers { label, items },
        });
    }

    // 项目 Profile 子菜单：项目列表全应用共享，按分组嵌套子菜单各自勾选/应用
    // （组内应用可见且存在项目时才显示该组）
    let mut profile_scopes = Vec::new();
    {
        use crate::services::profile::ProfileScope;

        let scope_visible = |scope: &ProfileScope| {
            scope
                .apps()
                .iter()
                .any(|app_type| visible_apps.is_visible(app_type))
        };
        let profiles = if ProfileScope::ALL.iter().any(scope_visible) {
            app_state.db.get_all_profiles()?
        } else {
            Vec::new()
        };

        if !profiles.is_empty() {
            for scope in ProfileScope::ALL {
                if !scope_visible(&scope) {
                    continue;
                }
                let current_profile_id = app_state
                    .db
                    .get_current_profile_id(scope.as_str())?
                    .unwrap_or_default();
                // 分组标签用产品名，不进 i18n
                let label = scope.label();
                profile_scopes.push(TrayProfileScopeModel {
                    scope: scope.as_str(),
                    label,
                    profiles: profiles
                        .iter()
                        .map(|profile| TrayProfileItem {
                            id: profile.id.clone(),
                            name: profile.name.clone(),
                            checked: current_profile_id == profile.id,
                        })
                        .collect(),
                    none_checked: current_profile_id.is_empty(),
                });
            }
        }
    }

    Ok((
        TrayModel {
            texts,
            sections,
            profile_scopes,
            lightweight: crate::lightweight::is_lightweight_mode(),
        },
        proxy_state_stale,
    ))
}

type SectionHandles = std::collections::HashMap<AppType, Submenu<tauri::Wry>>;

fn menu_err(what: &str) -> impl Fn(tauri::Error) -> AppError + '_ {
    move |e| AppError::Message(format!("{what}: {e}"))
}

/// 由模型构建原生菜单，同时返回各 app 分区子菜单句柄（用于就地改标题）。
fn build_tray_menu(
    app: &tauri::AppHandle,
    model: &TrayModel,
) -> Result<(Menu<tauri::Wry>, SectionHandles), AppError> {
    let texts = &model.texts;
    let mut menu_builder = MenuBuilder::new(app);
    let mut section_handles = SectionHandles::new();

    // 顶部：打开主界面 / 打开官方网站
    let show_main_item = MenuItem::with_id(app, "show_main", texts.show_main, true, None::<&str>)
        .map_err(menu_err("创建打开主界面菜单失败"))?;
    let open_website_item =
        MenuItem::with_id(app, "open_website", texts.open_website, true, None::<&str>)
            .map_err(menu_err("创建打开官方网站菜单失败"))?;
    menu_builder = menu_builder
        .item(&show_main_item)
        .item(&open_website_item)
        .separator();

    // 每个应用类型折叠为子菜单，避免供应商过多时菜单过长
    for section_model in &model.sections {
        let section = &TRAY_SECTIONS[section_model.section];
        match &section_model.body {
            TraySectionBody::Empty { label } => {
                // 空供应商：显示禁用的菜单项
                let empty_item =
                    MenuItem::with_id(app, section.empty_id, label, false, None::<&str>).map_err(
                        |e| AppError::Message(format!("创建{}空提示失败: {e}", section.log_name)),
                    )?;
                menu_builder = menu_builder.item(&empty_item);
            }
            TraySectionBody::Providers { label, items } => {
                let submenu_id = format!("submenu_{}", section.app_type.as_str());
                let mut submenu_builder = SubmenuBuilder::with_id(app, &submenu_id, label);
                for item in items {
                    let menu_item = CheckMenuItem::with_id(
                        app,
                        format!("{}{}", section.prefix, item.id),
                        &item.label,
                        item.enabled,
                        item.checked,
                        None::<&str>,
                    )
                    .map_err(|e| {
                        AppError::Message(format!("创建{}菜单项失败: {e}", section.log_name))
                    })?;
                    submenu_builder = submenu_builder.item(&menu_item);
                }
                let submenu = submenu_builder.build().map_err(|e| {
                    AppError::Message(format!("构建{}子菜单失败: {e}", section.log_name))
                })?;
                section_handles.insert(section.app_type.clone(), submenu.clone());
                menu_builder = menu_builder.item(&submenu);
            }
        }
        menu_builder = menu_builder.separator();
    }

    if !model.profile_scopes.is_empty() {
        let mut scope_submenus = Vec::new();
        for scope in &model.profile_scopes {
            let mut scope_builder = SubmenuBuilder::with_id(
                app,
                format!("submenu_profiles_{}", scope.scope),
                scope.label,
            );
            for profile in &scope.profiles {
                let item = CheckMenuItem::with_id(
                    app,
                    format!("profile_{}_{}", scope.scope, profile.id),
                    &profile.name,
                    true,
                    profile.checked,
                    None::<&str>,
                )
                .map_err(menu_err("创建项目菜单项失败"))?;
                scope_builder = scope_builder.item(&item);
            }
            let none_item = CheckMenuItem::with_id(
                app,
                format!("profile_none_{}", scope.scope),
                texts.no_project_label,
                true,
                scope.none_checked,
                None::<&str>,
            )
            .map_err(menu_err("创建不使用项目菜单项失败"))?;
            let scope_submenu = scope_builder
                .separator()
                .item(&none_item)
                .build()
                .map_err(menu_err("构建项目分组子菜单失败"))?;
            scope_submenus.push(scope_submenu);
        }

        let mut profiles_builder =
            SubmenuBuilder::with_id(app, "submenu_profiles", texts.projects_label);
        for scope_submenu in &scope_submenus {
            profiles_builder = profiles_builder.item(scope_submenu);
        }
        let profiles_submenu = profiles_builder
            .build()
            .map_err(menu_err("构建项目子菜单失败"))?;
        menu_builder = menu_builder.item(&profiles_submenu).separator();
    }

    let lightweight_item = CheckMenuItem::with_id(
        app,
        "lightweight_mode",
        texts.lightweight_mode,
        true,
        model.lightweight,
        None::<&str>,
    )
    .map_err(menu_err("创建轻量模式菜单失败"))?;

    menu_builder = menu_builder.item(&lightweight_item).separator();

    // 退出菜单（分隔符已在上面的 section 循环中添加）
    let quit_item = MenuItem::with_id(app, "quit", texts.quit, true, None::<&str>)
        .map_err(menu_err("创建退出菜单失败"))?;

    let menu = menu_builder
        .item(&quit_item)
        .build()
        .map_err(menu_err("构建菜单失败"))?;

    Ok((menu, section_handles))
}

/// 记录"当前装在托盘上的菜单"对应的模型与子菜单句柄。
fn commit_tray_model(model: Option<TrayModel>, handles: SectionHandles) {
    *TRAY_SECTION_SUBMENUS
        .lock()
        .unwrap_or_else(|p| p.into_inner()) = handles;
    *LAST_TRAY_MODEL.lock().unwrap_or_else(|p| p.into_inner()) = model;
}

// 完整菜单只由 `rebuild_tray_menu_now` 构建（经 `request_tray_rebuild` 去抖），
// 不再有各处直接 `create_tray_menu` + `set_menu` 的副本（CMD-16）。

/// 启动早期使用的骨架菜单：只含不依赖数据库的固定项，供应商/项目分区由
/// 后台初始化完成后的重建补齐。
pub fn create_startup_tray_menu(app: &tauri::AppHandle) -> Result<Menu<tauri::Wry>, AppError> {
    let language = backend_language();
    let model = TrayModel {
        texts: TrayTexts::from_language(&language),
        sections: Vec::new(),
        profile_scopes: Vec::new(),
        lightweight: crate::lightweight::is_lightweight_mode(),
    };
    let (menu, handles) = build_tray_menu(app, &model)?;
    // 不记录模型：下一次重建无论内容如何都必须换上完整菜单
    commit_tray_model(None, handles);
    Ok(menu)
}

/// 立即重建托盘菜单。`force = false` 时模型未变化就跳过 `set_menu`。
///
/// 返回是否真的换上了新菜单。
fn rebuild_tray_menu_now(app: &tauri::AppHandle, force: bool) -> bool {
    let Some(state) = app.try_state::<AppState>() else {
        return false;
    };
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return false;
    };

    let started = std::time::Instant::now();
    let _guard = TRAY_REBUILD_LOCK.lock().unwrap_or_else(|p| p.into_inner());

    let (model, stale) = match collect_tray_model(state.inner()) {
        Ok(v) => v,
        Err(e) => {
            log::error!("刷新托盘菜单失败: {e}");
            return false;
        }
    };
    if stale {
        // 代理正在启动/停止：先用上次的状态，稍后再补一次
        request_tray_rebuild(app, false);
    }

    let unchanged = LAST_TRAY_MODEL
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
        == Some(&model);
    if unchanged && !force {
        log::debug!(
            "[Tray] menu unchanged, rebuild skipped ({} ms)",
            started.elapsed().as_millis()
        );
        return false;
    }

    let (menu, handles) = match build_tray_menu(app, &model) {
        Ok(v) => v,
        Err(e) => {
            log::error!("刷新托盘菜单失败: {e}");
            return false;
        }
    };
    if let Err(e) = tray.set_menu(Some(menu)) {
        log::error!("刷新托盘菜单失败: {e}");
        return false;
    }
    commit_tray_model(Some(model), handles);

    let elapsed = started.elapsed();
    if elapsed >= std::time::Duration::from_millis(50) {
        log::info!("[Tray] menu rebuilt in {} ms", elapsed.as_millis());
    } else {
        log::debug!("[Tray] menu rebuilt in {} ms", elapsed.as_millis());
    }
    true
}

/// 合并一段时间窗口内的多次重建请求。
///
/// `request` 返回 `true` 表示调用方需要安排一次执行；执行前调用 `begin`
/// 取出窗口内是否有"强制重建"的请求。
struct RebuildCoalescer {
    pending: std::sync::atomic::AtomicBool,
    force: std::sync::atomic::AtomicBool,
}

impl RebuildCoalescer {
    const fn new() -> Self {
        Self {
            pending: std::sync::atomic::AtomicBool::new(false),
            force: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn request(&self, force: bool) -> bool {
        use std::sync::atomic::Ordering;
        if force {
            self.force.store(true, Ordering::SeqCst);
        }
        !self.pending.swap(true, Ordering::SeqCst)
    }

    /// 窗口结束：允许新的请求安排下一次执行，并返回本轮是否需要强制重建。
    fn begin(&self) -> bool {
        use std::sync::atomic::Ordering;
        self.pending.store(false, Ordering::SeqCst);
        self.force.swap(false, Ordering::SeqCst)
    }
}

static TRAY_REBUILD_COALESCER: RebuildCoalescer = RebuildCoalescer::new();

/// 去抖窗口：供应商切换 / 项目应用 / 故障转移等事件往往成串到达
/// （后端改完 + 前端 mutation 回调再调一次 `update_tray_menu`），合并为一次重建。
const TRAY_REBUILD_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(80);

/// 请求一次（去抖后的）托盘菜单重建。
///
/// - `force = true`：即使模型未变化也重新装载菜单。用于托盘点击路径——
///   原生 `CheckMenuItem` 被点击时会自行切换勾选状态，需要用重建把它纠正回来。
/// - `force = false`：模型没变就什么都不做（不会关闭用户正打开的菜单）。
pub fn request_tray_rebuild(app: &tauri::AppHandle, force: bool) {
    if !TRAY_REBUILD_COALESCER.request(force) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(TRAY_REBUILD_DEBOUNCE).await;
        let force = TRAY_REBUILD_COALESCER.begin();
        let app_for_rebuild = app.clone();
        if let Err(e) = tauri::async_runtime::spawn_blocking(move || {
            rebuild_tray_menu_now(&app_for_rebuild, force);
        })
        .await
        {
            log::error!("托盘菜单重建任务失败: {e}");
        }
    });
}

/// 后台初始化完成后立即装上完整托盘菜单（不走去抖，替换启动骨架菜单）。
pub(crate) fn rebuild_tray_menu_for_startup(app: &tauri::AppHandle) {
    rebuild_tray_menu_now(app, true);
}

/// 刷新托盘菜单（去抖、强制）。保持原有"状态变化后整菜单刷新"的语义，
/// 但一串连续调用只会触发一次重建。
pub fn refresh_tray_menu(app: &tauri::AppHandle) {
    request_tray_rebuild(app, true);
}

/// 就地更新各 app 分区子菜单的标题（usage 后缀变化时走这条），
/// 避免 `set_menu` 导致用户打开中的菜单被关闭。
/// 句柄由上一次 `create_tray_menu` 填充；为空（从未构建过菜单）时无事发生。
///
/// 只按 id 读取当前供应商，不再加载整张供应商表；新标题同步写回
/// `LAST_TRAY_MODEL`，使后续"模型未变则跳过"的比较与托盘上的实际标题一致。
fn update_tray_usage_labels(app: &tauri::AppHandle) {
    let Some(app_state) = app.try_state::<AppState>() else {
        return;
    };
    // 与整菜单重建互斥，避免把标题写到即将被替换的旧子菜单上
    let _rebuild_guard = TRAY_REBUILD_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let handles = match TRAY_SECTION_SUBMENUS.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };

    for (index, section) in TRAY_SECTIONS.iter().enumerate() {
        let Some(submenu) = handles.get(&section.app_type) else {
            continue;
        };
        let Some((current_id, provider)) =
            current_provider_for_label(&app_state, &section.app_type)
        else {
            continue;
        };
        let suffix = format_usage_suffix(&app_state, &section.app_type, &provider, &current_id)
            .unwrap_or_default();
        let new_label = section_label(section, &provider, &suffix);
        if let Err(e) = submenu.set_text(&new_label) {
            log::debug!("[Tray] 更新{}子菜单标题失败: {e}", section.log_name);
            continue;
        }
        if let Some(model) = LAST_TRAY_MODEL
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_mut()
        {
            for section_model in model.sections.iter_mut() {
                if section_model.section != index {
                    continue;
                }
                if let TraySectionBody::Providers { label, .. } = &mut section_model.body {
                    *label = new_label.clone();
                }
            }
        }
    }
}

/// 解析某个分区的当前供应商（按 id 读取，不扫描整表）。
fn current_provider_for_label(
    app_state: &AppState,
    app_type: &AppType,
) -> Option<(String, crate::provider::Provider)> {
    let app_type_str = app_type.as_str();
    if let Some(local_id) = crate::settings::get_current_provider(app_type) {
        if let Ok(Some(provider)) = app_state.db.get_provider_by_id(&local_id, app_type_str) {
            return Some((local_id, provider));
        }
    }
    // 本地 settings 缺失/失效：回退到完整语义（含清理失效的本地 id）
    let current_id = crate::settings::get_effective_current_provider(&app_state.db, app_type)
        .ok()
        .flatten()?;
    let provider = app_state
        .db
        .get_provider_by_id(&current_id, app_type_str)
        .ok()
        .flatten()?;
    Some((current_id, provider))
}

#[cfg(target_os = "macos")]
pub fn apply_tray_policy(app: &tauri::AppHandle, dock_visible: bool) {
    use tauri::ActivationPolicy;

    let desired_policy = if dock_visible {
        ActivationPolicy::Regular
    } else {
        ActivationPolicy::Accessory
    };

    if let Err(err) = app.set_dock_visibility(dock_visible) {
        log::warn!("设置 Dock 显示状态失败: {err}");
    }

    if let Err(err) = app.set_activation_policy(desired_policy) {
        log::warn!("设置激活策略失败: {err}");
    }
}

/// 处理托盘菜单事件
pub fn handle_tray_menu_event(app: &tauri::AppHandle, event_id: &str) {
    log::info!("处理托盘菜单事件: {event_id}");

    match event_id {
        "show_main" => {
            crate::main_window::show_or_recreate_main_window(app);
        }
        "open_website" => {
            if let Err(e) = app.opener().open_url("https://ccswitch.io", None::<String>) {
                log::error!("打开官方网站失败: {e}");
            }
        }
        "lightweight_mode" => {
            if crate::lightweight::is_lightweight_mode() {
                if let Err(e) = crate::lightweight::exit_lightweight_mode(app) {
                    log::error!("退出轻量模式失败: {e}");
                }
            } else if let Err(e) = crate::lightweight::enter_lightweight_mode(app) {
                log::error!("进入轻量模式失败: {e}");
            }
        }
        "quit" => {
            log::info!("退出应用");
            app.exit(0);
        }
        _ => {
            if handle_profile_tray_event(app, event_id) {
                return;
            }
            if handle_provider_tray_event(app, event_id) {
                return;
            }
            log::warn!("未处理的菜单事件: {event_id}");
        }
    }
}

static LAST_TRAY_USAGE_REFRESH: std::sync::Mutex<Option<std::time::Instant>> =
    std::sync::Mutex::new(None);
const MIN_TRAY_USAGE_REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);

/// 合并多次快速触发的"usage 标题软更新"：批量刷新期间多个 usage 命令
/// 同时成功时，只会产生一次就地 `set_text` 批量调用。走软更新而不是
/// `refresh_tray_menu` 整建，避免用户打开中的菜单被 macOS 系统关闭。
static TRAY_REBUILD_SCHEDULED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub fn schedule_tray_refresh(app: &tauri::AppHandle) {
    use std::sync::atomic::Ordering;
    if TRAY_REBUILD_SCHEDULED.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    // 50ms 合窗：让同一轮 React Query / 托盘批量刷新触发的多个写入共享一次
    // 标题更新。用 tokio 计时器等待，不占用阻塞线程池（CMD-28）。
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        TRAY_REBUILD_SCHEDULED.store(false, Ordering::Release);
        if let Err(e) =
            tauri::async_runtime::spawn_blocking(move || update_tray_usage_labels(&app)).await
        {
            log::error!("托盘用量标题更新任务失败: {e}");
        }
    });
}

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
                if let Err(e) = crate::commands::queryProviderUsage(
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
    use super::{format_script_summary, format_subscription_summary, TRAY_ID, TRAY_SECTIONS};
    use crate::app_config::AppType;
    use crate::provider::{UsageData, UsageResult};
    use crate::services::subscription::{
        CredentialStatus, QuotaTier, SubscriptionQuota, TIER_FIVE_HOUR, TIER_GEMINI_FLASH,
        TIER_GEMINI_FLASH_LITE, TIER_GEMINI_PRO, TIER_MONTHLY, TIER_SEVEN_DAY, TIER_SEVEN_DAY_OPUS,
        TIER_SEVEN_DAY_SONNET, TIER_THIRTY_DAY, TIER_WEEKLY_LIMIT,
    };

    #[test]
    fn rebuild_requests_in_one_window_coalesce_into_one_run() {
        let c = super::RebuildCoalescer::new();
        // 第一次请求负责安排执行，窗口内其余请求只做合并
        assert!(c.request(false));
        for _ in 0..100 {
            assert!(!c.request(false));
        }
        assert!(!c.request(true));
        // 窗口结束：窗口内出现过强制请求 -> 本轮强制
        assert!(c.begin());
        // 新窗口重新开始
        assert!(c.request(false));
        assert!(!c.begin(), "force flag must not leak into the next window");
    }

    #[test]
    fn request_arriving_during_a_run_schedules_another_run() {
        let c = super::RebuildCoalescer::new();
        assert!(c.request(false));
        let _ = c.begin();
        // 正在重建时到达的请求必须再安排一轮，否则会丢失这次变化
        assert!(c.request(true));
        assert!(c.begin());
    }

    #[test]
    fn takeover_detection_is_cached_until_live_fingerprint_changes() {
        use std::cell::Cell;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{}").expect("write");
        let paths = vec![path.clone()];
        let calls = Cell::new(0);
        let detect = |value: bool| {
            calls.set(calls.get() + 1);
            value
        };
        // 测试专用的 app 类型（不在托盘分区中），避免与其它测试共享缓存条目
        let app = AppType::OpenClaw;

        let fp = super::live_fingerprint(&paths);
        assert!(!super::cached_takeover_detection(&app, fp.clone(), || {
            detect(false)
        }));
        assert!(!super::cached_takeover_detection(&app, fp, || detect(true)));
        assert_eq!(calls.get(), 1, "unchanged file must not be re-read");

        std::fs::write(
            &path,
            r#"{"env":{"ANTHROPIC_BASE_URL":"http://127.0.0.1"}}"#,
        )
        .expect("rewrite");
        let fp = super::live_fingerprint(&paths);
        assert!(super::cached_takeover_detection(&app, fp, || detect(true)));
        assert_eq!(calls.get(), 2, "changed file must be re-detected");

        std::fs::remove_file(&path).expect("remove");
        let fp = super::live_fingerprint(&paths);
        assert_eq!(fp, vec![None]);
        assert!(!super::cached_takeover_detection(&app, fp, || detect(
            false
        )));
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn contended_tokio_lock_is_reported_instead_of_blocking() {
        use futures::FutureExt;
        let lock = tokio::sync::RwLock::new(Some(1));
        assert_eq!(lock.read().now_or_never().map(|g| g.is_some()), Some(true));
        let _writer = lock.try_write().expect("write lock");
        assert!(lock.read().now_or_never().is_none());
    }

    #[test]
    fn proxy_running_snapshot_reads_idle_proxy_without_blocking() {
        let db = std::sync::Arc::new(crate::database::Database::memory().expect("memory db"));
        let state = crate::store::AppState::new(db);
        assert_eq!(super::proxy_running_snapshot(&state), (false, false));
    }

    #[test]
    fn tray_id_is_unique_to_app() {
        assert_eq!(TRAY_ID, "cc-switch");
        assert_ne!(TRAY_ID, "main");
    }

    #[test]
    fn locale_maps_traditional_chinese_variants_to_zh_tw() {
        use super::map_locale_to_tray_language;
        for locale in [
            "zh-TW",
            "zh-HK",
            "zh-MO",
            "zh-Hant",
            "zh-Hant-TW",
            "zh-hant-hk",
        ] {
            assert_eq!(
                map_locale_to_tray_language(locale),
                "zh-TW",
                "expected {locale} -> zh-TW"
            );
        }
    }

    #[test]
    fn locale_maps_simplified_chinese_variants_to_zh() {
        use super::map_locale_to_tray_language;
        for locale in ["zh", "zh-CN", "zh-SG", "zh-Hans", "zh-Hans-CN"] {
            assert_eq!(
                map_locale_to_tray_language(locale),
                "zh",
                "expected {locale} -> zh"
            );
        }
    }

    #[test]
    fn locale_maps_japanese_and_english() {
        use super::map_locale_to_tray_language;
        assert_eq!(map_locale_to_tray_language("ja-JP"), "ja");
        assert_eq!(map_locale_to_tray_language("ja"), "ja");
        assert_eq!(map_locale_to_tray_language("en-US"), "en");
        assert_eq!(map_locale_to_tray_language("en"), "en");
    }

    #[test]
    fn locale_unknown_falls_back_to_en() {
        use super::map_locale_to_tray_language;
        // 与前端 getInitialLanguage 的默认值保持一致：未知区域回退到英文。
        for locale in ["de-DE", "fr", "ko-KR", "es-ES", "pt-BR", "ru-RU", ""] {
            assert_eq!(
                map_locale_to_tray_language(locale),
                "en",
                "expected {locale} -> en (default)"
            );
        }
    }

    #[test]
    fn locale_accepts_posix_underscore_form() {
        use super::map_locale_to_tray_language;
        assert_eq!(map_locale_to_tray_language("zh_TW"), "zh-TW");
        assert_eq!(map_locale_to_tray_language("zh_CN"), "zh");
        assert_eq!(map_locale_to_tray_language("ja_JP"), "ja");
        assert_eq!(map_locale_to_tray_language("de_DE"), "en");
    }

    #[test]
    fn tray_sections_include_claude_desktop_with_a_non_overlapping_prefix() {
        let section = TRAY_SECTIONS
            .iter()
            .find(|section| section.app_type == AppType::ClaudeDesktop)
            .expect("Claude Desktop tray section should exist");
        assert_eq!(section.prefix, "claudedesktop_");
        // No section prefix may be a prefix of another, or events would be
        // routed to the wrong app.
        for a in TRAY_SECTIONS.iter() {
            for b in TRAY_SECTIONS.iter() {
                if a.app_type != b.app_type {
                    assert!(
                        !b.prefix.starts_with(a.prefix),
                        "{} shadows {}",
                        a.prefix,
                        b.prefix
                    );
                }
            }
        }
    }

    #[test]
    fn tray_sections_include_grokbuild_provider_switching() {
        let section = TRAY_SECTIONS
            .iter()
            .find(|section| section.app_type == AppType::GrokBuild)
            .expect("Grok Build tray section should exist");

        assert_eq!(section.prefix, "grokbuild_");
        assert_eq!(section.empty_id, "grokbuild_empty");
        assert_eq!(section.header_label, "Grok Build");
    }

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

    // ---- TEST-16: tray menu model snapshots ---------------------------------
    //
    // The model (sections, items, check marks, labels in every language) is
    // built from an in-memory DB and explicit settings, then compared with
    // `src-tauri/tests/fixtures/tray/<case>.txt` (`UPDATE_GOLDEN=1` rewrites).

    mod model_snapshots {
        use super::super::{collect_tray_model_with, TrayModel};
        use crate::database::{Database, Profile};
        use crate::provider::Provider;
        use crate::settings::{AppSettings, VisibleApps};
        use crate::store::AppState;
        use serde_json::json;
        use serial_test::serial;
        use std::path::PathBuf;
        use std::sync::Arc;

        const LANGUAGES: [&str; 4] = ["en", "zh", "zh-TW", "ja"];

        fn visibility(name: &str) -> VisibleApps {
            let mut v = VisibleApps::default();
            match name {
                "all" => {
                    v.hermes = true;
                }
                "default" => {}
                "claude-only" => {
                    v = VisibleApps {
                        claude: true,
                        claude_desktop: false,
                        codex: false,
                        gemini: false,
                        grokbuild: false,
                        opencode: false,
                        openclaw: false,
                        hermes: false,
                    };
                }
                other => panic!("unknown visibility {other}"),
            }
            v
        }

        fn state(with_data: bool) -> AppState {
            let db = Arc::new(Database::memory().expect("memory db"));
            if with_data {
                for (app, id, name, category, sort) in [
                    ("claude", "official", "Claude Official", Some("official"), 0),
                    ("claude", "relay", "Relay B", Some("custom"), 1),
                    ("codex", "codex-a", "Codex A", None, 0),
                ] {
                    let mut p = Provider::with_id(id.into(), name.into(), json!({}), None);
                    p.category = category.map(str::to_string);
                    p.sort_index = Some(sort);
                    db.save_provider(app, &p).unwrap();
                }
                db.set_current_provider("claude", "relay").unwrap();
                db.save_profile(&Profile {
                    id: "work".into(),
                    name: "Work".into(),
                    payload: "{}".into(),
                    sort_order: Some(0),
                    created_at: Some(0),
                    updated_at: Some(0),
                })
                .unwrap();
                db.set_current_profile_id("claude", Some("work")).unwrap();
            }
            AppState::new(db)
        }

        fn model(language: &str, visible: &str, with_data: bool) -> TrayModel {
            let settings = AppSettings {
                language: Some(language.to_string()),
                visible_apps: Some(visibility(visible)),
                ..AppSettings::default()
            };
            collect_tray_model_with(&state(with_data), settings)
                .expect("collect tray model")
                .0
        }

        fn fixture(name: &str) -> PathBuf {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/tray")
                .join(format!("{name}.txt"))
        }

        fn assert_snapshot(name: &str, model: &TrayModel) {
            let actual = format!("{model:#?}\n");
            let path = fixture(name);
            if std::env::var_os("UPDATE_GOLDEN").is_some() || !path.exists() {
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, &actual).unwrap();
                return;
            }
            let expected = std::fs::read_to_string(&path).unwrap();
            assert_eq!(
                actual,
                expected.replace("\r\n", "\n"),
                "tray model snapshot {name} changed; rerun with UPDATE_GOLDEN=1 after review"
            );
        }

        #[test]
        #[serial]
        fn tray_model_snapshots_per_language_visibility_and_data() {
            let mut count = 0;
            for language in LANGUAGES {
                for visible in ["all", "default", "claude-only"] {
                    for with_data in [false, true] {
                        let name = format!(
                            "{language}__{visible}__{}",
                            if with_data { "data" } else { "empty" }
                        );
                        assert_snapshot(&name, &model(language, visible, with_data));
                        count += 1;
                    }
                }
            }
            assert_eq!(count, 24);
        }

        #[test]
        #[serial]
        fn current_provider_is_checked_and_sorted_by_index() {
            let m = model("en", "claude-only", true);
            assert_eq!(m.sections.len(), 1, "only Claude is visible");
            let rendered = format!("{m:?}");
            assert!(rendered.contains("Claude · Relay B"), "{rendered}");
            match &m.sections[0].body {
                super::super::TraySectionBody::Providers { items, .. } => {
                    let ids: Vec<_> = items.iter().map(|i| i.id.as_str()).collect();
                    assert_eq!(ids, ["official", "relay"]);
                    assert!(!items[0].checked && items[1].checked);
                    assert!(items.iter().all(|i| i.enabled), "proxy is not running");
                }
                other => panic!("expected providers, got {other:?}"),
            }
            assert_eq!(
                m.profile_scopes.len(),
                1,
                "only the Claude scope is visible"
            );
            assert!(m.profile_scopes[0].profiles[0].checked);
            assert!(!m.profile_scopes[0].none_checked);
        }

        #[test]
        #[serial]
        fn every_language_has_its_own_texts() {
            let texts: Vec<_> = LANGUAGES
                .iter()
                .map(|l| model(l, "default", false).texts.no_providers_label)
                .collect();
            let unique: std::collections::BTreeSet<_> = texts.iter().collect();
            assert_eq!(unique.len(), LANGUAGES.len(), "{texts:?}");
            // An empty app section shows the localized hint.
            let m = model("ja", "claude-only", false);
            assert!(format!("{m:?}").contains("(プロバイダーなし)"));
        }
    }
}
