//! 托盘菜单渲染：采集状态快照 → [`model::build_menu_model`] → Tauri 菜单。
//!
//! 菜单构建只读取同步数据源（数据库 / 设置 / 用量缓存）与事件总线写入的
//! [`TrayRuntime`] 快照；需要异步探测的代理运行态由 [`rebuild_tray_menu`]
//! 在构建前 `.await` 取得。

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use tauri::menu::Submenu;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, MenuItemKind, PredefinedMenuItem};
use tauri::Manager;

use super::model::{
    self, MenuEntry, ProfileScopeInput, SectionInput, TrayMenuModel, TrayModelInput, TrayRuntime,
};
use super::usage::format_usage_suffix;
use super::{detect_system_tray_language, TrayTexts, TRAY_ID, TRAY_SECTIONS};
use crate::app_config::AppType;
use crate::error::AppError;
use crate::store::AppState;

/// 每个 app 分区的子菜单句柄，用于 usage 更新时就地改 label 而非整菜单重建。
/// `create_tray_menu` 每次重建都会整表覆盖写入，保证句柄始终指向当前活跃菜单。
static TRAY_SECTION_SUBMENUS: LazyLock<Mutex<HashMap<AppType, Submenu<tauri::Wry>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 最近一次探测到的代理运行态；首次探测前为默认值（代理未运行）。
static TRAY_RUNTIME: LazyLock<Mutex<TrayRuntime>> =
    LazyLock::new(|| Mutex::new(TrayRuntime::default()));

fn current_runtime() -> TrayRuntime {
    TRAY_RUNTIME
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
}

fn store_runtime(runtime: TrayRuntime) {
    *TRAY_RUNTIME.lock().unwrap_or_else(|p| p.into_inner()) = runtime;
}

/// 异步探测代理运行态与各可见应用的接管态
async fn probe_runtime(app_state: &AppState) -> TrayRuntime {
    let proxy_running = app_state.proxy_service.is_running().await;
    let mut taken_over = Vec::new();
    if proxy_running {
        let visible_apps = crate::settings::get_settings()
            .visible_apps
            .unwrap_or_default();
        for section in model::visible_sections(&visible_apps) {
            let has_backup = app_state
                .db
                .get_live_backup(section.app_type.as_str())
                .await
                .ok()
                .flatten()
                .is_some();
            if has_backup
                || app_state
                    .proxy_service
                    .detect_takeover_in_live_config_for_app(&section.app_type)
            {
                taken_over.push(section.app_type.clone());
            }
        }
    }
    TrayRuntime {
        proxy_running,
        taken_over,
    }
}

fn tray_texts() -> (TrayTexts, crate::settings::VisibleApps) {
    let app_settings = crate::settings::get_settings();
    // 用户未显式设置语言（首次安装）时，按系统区域回退而非硬编码简体，
    // 否则繁中系统的托盘会固定显示简体直到用户手动切换一次。
    let language: &str = match app_settings.language.as_deref() {
        Some(lang) => lang,
        None => detect_system_tray_language(),
    };
    let texts = TrayTexts::from_language(language);
    // Get visible apps setting, default to all visible
    (texts, app_settings.visible_apps.unwrap_or_default())
}

/// 从数据库 / 设置 / 用量缓存采集快照并派生菜单模型（同步，无异步等待）
fn collect_menu_model(
    app_state: &AppState,
    runtime: &TrayRuntime,
) -> Result<TrayMenuModel, AppError> {
    let (texts, visible_apps) = tray_texts();

    let mut sections = Vec::new();
    for section in model::visible_sections(&visible_apps) {
        let providers = app_state.db.get_all_providers(section.app_type.as_str())?;
        let current_id =
            crate::settings::get_effective_current_provider(&app_state.db, &section.app_type)?
                .unwrap_or_default();
        let usage_suffix = providers
            .get(&current_id)
            .and_then(|p| format_usage_suffix(app_state, &section.app_type, p, &current_id));
        sections.push(SectionInput {
            section,
            providers,
            current_id,
            usage_suffix,
        });
    }

    // 项目 Profile：组内应用可见且存在项目时才显示该组
    let scopes = model::visible_profile_scopes(&visible_apps);
    let profiles = if scopes.is_empty() {
        Vec::new()
    } else {
        app_state.db.get_all_profiles()?
    };
    let mut profile_scopes = Vec::new();
    if !profiles.is_empty() {
        for scope in scopes {
            let current_profile_id = app_state
                .db
                .get_current_profile_id(scope.as_str())?
                .unwrap_or_default();
            profile_scopes.push(ProfileScopeInput {
                scope,
                current_profile_id,
            });
        }
    }

    Ok(model::build_menu_model(&TrayModelInput {
        texts,
        sections,
        profiles: &profiles,
        profile_scopes,
        lightweight: app_state.is_lightweight_mode(),
        runtime,
    }))
}

fn render_entry(
    app: &tauri::AppHandle,
    entry: &MenuEntry,
    submenus: &mut HashMap<String, Submenu<tauri::Wry>>,
) -> Result<MenuItemKind<tauri::Wry>, AppError> {
    let err = |e: tauri::Error| {
        AppError::Message(format!(
            "创建托盘菜单项 {} 失败: {e}",
            entry.id().unwrap_or("separator")
        ))
    };
    Ok(match entry {
        MenuEntry::Item { id, label, enabled } => MenuItemKind::MenuItem(
            MenuItem::with_id(app, id.as_str(), label, *enabled, None::<&str>).map_err(err)?,
        ),
        MenuEntry::Check {
            id,
            label,
            enabled,
            checked,
        } => MenuItemKind::Check(
            CheckMenuItem::with_id(app, id.as_str(), label, *enabled, *checked, None::<&str>)
                .map_err(err)?,
        ),
        MenuEntry::Submenu { id, label, entries } => {
            let children = render_entries(app, entries, submenus)?;
            let refs: Vec<&dyn IsMenuItem<tauri::Wry>> = children
                .iter()
                .map(|c| c as &dyn IsMenuItem<tauri::Wry>)
                .collect();
            let submenu =
                Submenu::with_id_and_items(app, id.as_str(), label, true, &refs).map_err(err)?;
            submenus.insert(id.clone(), submenu.clone());
            MenuItemKind::Submenu(submenu)
        }
        MenuEntry::Separator => {
            MenuItemKind::Predefined(PredefinedMenuItem::separator(app).map_err(err)?)
        }
    })
}

fn render_entries(
    app: &tauri::AppHandle,
    entries: &[MenuEntry],
    submenus: &mut HashMap<String, Submenu<tauri::Wry>>,
) -> Result<Vec<MenuItemKind<tauri::Wry>>, AppError> {
    entries
        .iter()
        .map(|entry| render_entry(app, entry, submenus))
        .collect()
}

/// 把菜单模型渲染为 Tauri 菜单，并刷新各 app 分区子菜单句柄
fn render_menu(
    app: &tauri::AppHandle,
    model: &TrayMenuModel,
) -> Result<Menu<tauri::Wry>, AppError> {
    let mut submenus = HashMap::new();
    let items = render_entries(app, &model.entries, &mut submenus)?;
    let refs: Vec<&dyn IsMenuItem<tauri::Wry>> = items
        .iter()
        .map(|i| i as &dyn IsMenuItem<tauri::Wry>)
        .collect();
    let menu = Menu::with_items(app, &refs)
        .map_err(|e| AppError::Message(format!("构建菜单失败: {e}")))?;

    let section_handles: HashMap<AppType, Submenu<tauri::Wry>> = TRAY_SECTIONS
        .iter()
        .filter_map(|section| {
            submenus
                .remove(&model::section_submenu_id(&section.app_type))
                .map(|submenu| (section.app_type.clone(), submenu))
        })
        .collect();
    *TRAY_SECTION_SUBMENUS
        .lock()
        .unwrap_or_else(|p| p.into_inner()) = section_handles;

    Ok(menu)
}

/// 创建动态托盘菜单（同步）。代理运行态取自最近一次探测的快照。
pub fn create_tray_menu(
    app: &tauri::AppHandle,
    app_state: &AppState,
) -> Result<Menu<tauri::Wry>, AppError> {
    let model = collect_menu_model(app_state, &current_runtime())?;
    render_menu(app, &model)
}

/// 探测代理运行态 → 写入快照 → 重建并替换托盘菜单。
/// 返回是否找到托盘并完成替换。
pub async fn rebuild_tray_menu(app: &tauri::AppHandle) -> Result<bool, AppError> {
    let Some(app_state) = app.try_state::<AppState>() else {
        return Ok(false);
    };
    store_runtime(probe_runtime(app_state.inner()).await);

    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(app_state) = app.try_state::<AppState>() else {
            return Ok(false);
        };
        let menu = create_tray_menu(&app, app_state.inner())?;
        match app.tray_by_id(TRAY_ID) {
            Some(tray) => {
                tray.set_menu(Some(menu))
                    .map_err(|e| AppError::Message(format!("更新托盘菜单失败: {e}")))?;
                Ok(true)
            }
            None => Ok(false),
        }
    })
    .await
    .map_err(|e| AppError::Message(format!("托盘菜单重建任务失败: {e}")))?
}

/// 就地更新各 app 分区子菜单的标题（usage 后缀变化时走这条），
/// 避免 `set_menu` 导致用户打开中的菜单被关闭。
/// 句柄由上一次 `create_tray_menu` 填充；为空（从未构建过菜单）时无事发生。
pub(super) fn update_tray_usage_labels(app: &tauri::AppHandle) {
    let Some(app_state) = app.try_state::<AppState>() else {
        return;
    };
    let handles = match TRAY_SECTION_SUBMENUS.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };

    for section in TRAY_SECTIONS.iter() {
        let Some(submenu) = handles.get(&section.app_type) else {
            continue;
        };
        let Ok(providers) = app_state.db.get_all_providers(section.app_type.as_str()) else {
            continue;
        };
        let Ok(Some(current_id)) =
            crate::settings::get_effective_current_provider(&app_state.db, &section.app_type)
        else {
            continue;
        };
        let Some(provider) = providers.get(&current_id) else {
            continue;
        };
        let suffix = format_usage_suffix(&app_state, &section.app_type, provider, &current_id)
            .unwrap_or_default();
        let new_label = model::section_label(section, &provider.name, &suffix);
        if let Err(e) = submenu.set_text(&new_label) {
            log::debug!("[Tray] 更新{}子菜单标题失败: {e}", section.log_name);
        }
    }
}
