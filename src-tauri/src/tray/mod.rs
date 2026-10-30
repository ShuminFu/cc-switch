//! 托盘菜单管理模块
//!
//! 负责系统托盘图标和菜单的创建、更新和事件处理，拆分为：
//! - [`model`]：由应用注册表与状态快照派生的纯数据菜单模型（可单测）；
//! - [`menu`]：采集状态快照、把模型渲染为 Tauri 菜单；
//! - [`events`]：托盘事件总线（驱动重建 / 标题软更新）与菜单点击处理；
//! - [`usage`]：用量后缀格式化与悬停刷新。
//!
//! 菜单构建全程同步且不等待异步任务：代理运行态 / 接管态由事件总线
//! 在异步上下文中探测后写入模型快照。

use std::sync::LazyLock;

use crate::app_config::AppType;

mod events;
mod menu;
mod model;
mod usage;

pub use events::{handle_tray_menu_event, refresh_tray_menu, schedule_tray_refresh};
pub use menu::{create_tray_menu, rebuild_tray_menu};
pub(crate) use usage::refresh_all_usage_in_tray;

/// 托盘菜单文本（国际化）
#[derive(Clone, Copy)]
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
/// 日文 → `ja`，英文 → `en`，未知区域回退到 `zh`（与前端默认一致）。
fn map_locale_to_tray_language(locale: &str) -> &'static str {
    let locale = locale.to_lowercase();
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
    } else if locale.starts_with("en") {
        "en"
    } else {
        "zh"
    }
}

/// 读取系统区域并映射为托盘语言码；取不到区域时回退到 `zh`。
pub(crate) fn detect_system_tray_language() -> &'static str {
    sys_locale::get_locale()
        .as_deref()
        .map(map_locale_to_tray_language)
        .unwrap_or("zh")
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
    pub prefix: String,
    pub empty_id: String,
    pub header_label: &'static str,
    pub log_name: &'static str,
}

/// Auto 菜单项后缀
pub const AUTO_SUFFIX: &str = "auto";
pub const TRAY_ID: &str = "cc-switch";

/// 托盘分区：由应用注册表中具备 `TRAY` 能力的应用派生
pub static TRAY_SECTIONS: LazyLock<Vec<TrayAppSection>> = LazyLock::new(|| {
    crate::apps::registry::with_caps(crate::apps::AppCaps::TRAY)
        .map(|d| TrayAppSection {
            app_type: d.app.clone(),
            prefix: format!("{}_", d.slug),
            empty_id: format!("{}_empty", d.slug),
            header_label: d.display_name,
            log_name: d.display_name,
        })
        .collect()
});

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

#[cfg(test)]
mod tests {
    use super::{TRAY_ID, TRAY_SECTIONS};
    use crate::app_config::AppType;

    /// 菜单构建 / 事件处理不得同步等待异步任务（易在主线程上死锁或卡顿），
    /// 运行态由事件总线异步探测后写入模型快照。
    #[test]
    fn tray_sources_never_wait_synchronously_on_futures() {
        let forbidden = concat!("block", "_on");
        for (name, src) in [
            ("mod.rs", include_str!("mod.rs")),
            ("model.rs", include_str!("model.rs")),
            ("menu.rs", include_str!("menu.rs")),
            ("events.rs", include_str!("events.rs")),
            ("usage.rs", include_str!("usage.rs")),
        ] {
            assert!(
                !src.contains(forbidden),
                "tray/{name} must not wait synchronously on async work"
            );
        }
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
    fn locale_unknown_falls_back_to_zh() {
        use super::map_locale_to_tray_language;
        // 与前端 getInitialLanguage 的默认值保持一致。
        for locale in ["de-DE", "fr", "ko-KR", ""] {
            assert_eq!(
                map_locale_to_tray_language(locale),
                "zh",
                "expected {locale} -> zh (default)"
            );
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
}
