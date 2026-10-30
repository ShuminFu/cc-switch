//! 托盘菜单模型：纯数据描述，由应用注册表（[`TRAY_SECTIONS`]）与状态快照派生。
//!
//! 这里不接触 Tauri 菜单句柄、数据库或异步任务，便于针对每种状态单测；
//! [`super::menu`] 负责采集快照并把模型渲染成真正的菜单。

use indexmap::IndexMap;

use super::{TrayAppSection, TrayTexts, TRAY_SECTIONS};
use crate::app_config::AppType;
use crate::database::Profile;
use crate::provider::Provider;
use crate::services::profile::ProfileScope;
use crate::settings::VisibleApps;

/// 顶层菜单项 id
pub const SHOW_MAIN_ID: &str = "show_main";
pub const OPEN_WEBSITE_ID: &str = "open_website";
pub const LIGHTWEIGHT_MODE_ID: &str = "lightweight_mode";
pub const QUIT_ID: &str = "quit";
pub const PROFILES_SUBMENU_ID: &str = "submenu_profiles";

/// 代理运行态快照：由事件总线在异步上下文中探测后写入，菜单构建只读不等待。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrayRuntime {
    pub proxy_running: bool,
    /// 当前被代理接管 Live 配置的应用（仅在代理运行时有意义）
    pub taken_over: Vec<AppType>,
}

impl TrayRuntime {
    pub fn is_taken_over(&self, app_type: &AppType) -> bool {
        self.proxy_running && self.taken_over.contains(app_type)
    }
}

/// 菜单条目
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuEntry {
    Item {
        id: String,
        label: String,
        enabled: bool,
    },
    Check {
        id: String,
        label: String,
        enabled: bool,
        checked: bool,
    },
    Submenu {
        id: String,
        label: String,
        entries: Vec<MenuEntry>,
    },
    Separator,
}

impl MenuEntry {
    pub fn id(&self) -> Option<&str> {
        match self {
            MenuEntry::Item { id, .. }
            | MenuEntry::Check { id, .. }
            | MenuEntry::Submenu { id, .. } => Some(id),
            MenuEntry::Separator => None,
        }
    }

    #[cfg(test)]
    pub fn label(&self) -> Option<&str> {
        match self {
            MenuEntry::Item { label, .. }
            | MenuEntry::Check { label, .. }
            | MenuEntry::Submenu { label, .. } => Some(label),
            MenuEntry::Separator => None,
        }
    }
}

/// 整个托盘菜单
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayMenuModel {
    pub entries: Vec<MenuEntry>,
}

impl TrayMenuModel {
    /// 递归按 id 查找条目
    #[cfg(test)]
    pub fn find(&self, id: &str) -> Option<&MenuEntry> {
        fn walk<'a>(entries: &'a [MenuEntry], id: &str) -> Option<&'a MenuEntry> {
            for entry in entries {
                if entry.id() == Some(id) {
                    return Some(entry);
                }
                if let MenuEntry::Submenu { entries, .. } = entry {
                    if let Some(found) = walk(entries, id) {
                        return Some(found);
                    }
                }
            }
            None
        }
        walk(&self.entries, id)
    }
}

/// 单个应用分区的输入快照（仅可见应用）
pub struct SectionInput {
    pub section: &'static TrayAppSection,
    pub providers: IndexMap<String, Provider>,
    /// 当前供应商 id；未设置时为空串
    pub current_id: String,
    /// 当前供应商的用量后缀（形如 ` · 🟢 h9%`）
    pub usage_suffix: Option<String>,
}

/// 项目分组的输入快照（仅可见分组）
pub struct ProfileScopeInput {
    pub scope: ProfileScope,
    /// 当前项目 id；未设置时为空串
    pub current_profile_id: String,
}

/// 构建菜单模型所需的全部输入
pub struct TrayModelInput<'a> {
    pub texts: TrayTexts,
    pub sections: Vec<SectionInput>,
    pub profiles: &'a [Profile],
    pub profile_scopes: Vec<ProfileScopeInput>,
    pub lightweight: bool,
    pub runtime: &'a TrayRuntime,
}

/// 可见的应用分区（注册表顺序）
pub fn visible_sections(
    visible_apps: &VisibleApps,
) -> impl Iterator<Item = &'static TrayAppSection> + '_ {
    TRAY_SECTIONS
        .iter()
        .filter(move |section| visible_apps.is_visible(&section.app_type))
}

/// 组内存在可见应用的项目分组
pub fn visible_profile_scopes(visible_apps: &VisibleApps) -> Vec<ProfileScope> {
    ProfileScope::ALL
        .into_iter()
        .filter(|scope| {
            scope
                .apps()
                .iter()
                .any(|app_type| visible_apps.is_visible(app_type))
        })
        .collect()
}

/// 应用分区子菜单 id
pub fn section_submenu_id(app_type: &AppType) -> String {
    format!("submenu_{}", app_type.as_str())
}

/// 应用分区子菜单标题：`<应用> · <当前供应商><用量后缀>`
pub fn section_label(section: &TrayAppSection, provider_name: &str, usage_suffix: &str) -> String {
    format!(
        "{} · {}{}",
        section.header_label, provider_name, usage_suffix
    )
}

/// 对供应商列表排序：sort_index → created_at → name
pub fn sort_providers(providers: &IndexMap<String, Provider>) -> Vec<(&String, &Provider)> {
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

fn section_entry(input: &SectionInput, texts: &TrayTexts, runtime: &TrayRuntime) -> MenuEntry {
    let section = input.section;
    if input.providers.is_empty() {
        // 空供应商：显示禁用的菜单项
        return MenuEntry::Item {
            id: section.empty_id.clone(),
            label: format!("{} {}", section.header_label, texts.no_providers_label),
            enabled: false,
        };
    }

    let label = match input.providers.get(&input.current_id) {
        Some(p) => section_label(
            section,
            &p.name,
            input.usage_suffix.as_deref().unwrap_or_default(),
        ),
        None => section.header_label.to_string(),
    };

    // 应用被代理接管时，不支持接管的官方供应商置灰（⛔）
    let is_app_taken_over = runtime.is_taken_over(&section.app_type);
    let entries = sort_providers(&input.providers)
        .into_iter()
        .map(|(id, provider)| {
            let is_official_blocked = is_app_taken_over
                && provider.category.as_deref() == Some("official")
                && !crate::services::provider::official_provider_supports_proxy_takeover(
                    &section.app_type,
                    provider,
                );
            let label = if is_official_blocked {
                format!("{} \u{26D4}", &provider.name) // ⛔ emoji
            } else {
                provider.name.clone()
            };
            MenuEntry::Check {
                id: format!("{}{}", section.prefix, id),
                label,
                enabled: !is_official_blocked,
                checked: input.current_id == *id,
            }
        })
        .collect();

    MenuEntry::Submenu {
        id: section_submenu_id(&section.app_type),
        label,
        entries,
    }
}

fn profiles_entry(
    profiles: &[Profile],
    scopes: &[ProfileScopeInput],
    texts: &TrayTexts,
) -> Option<MenuEntry> {
    if profiles.is_empty() || scopes.is_empty() {
        return None;
    }

    let scope_entries = scopes
        .iter()
        .map(|input| {
            let scope = input.scope;
            // 分组标签用产品名，不进 i18n
            let scope_label = match scope {
                ProfileScope::Claude => "Claude Code",
                ProfileScope::ClaudeDesktop => "Claude Desktop",
                ProfileScope::Codex => "Codex",
            };
            let mut entries: Vec<MenuEntry> = profiles
                .iter()
                .map(|profile| MenuEntry::Check {
                    id: format!("profile_{}_{}", scope.as_str(), profile.id),
                    label: profile.name.clone(),
                    enabled: true,
                    checked: input.current_profile_id == profile.id,
                })
                .collect();
            entries.push(MenuEntry::Separator);
            entries.push(MenuEntry::Check {
                id: format!("profile_none_{}", scope.as_str()),
                label: texts.no_project_label.to_string(),
                enabled: true,
                checked: input.current_profile_id.is_empty(),
            });
            MenuEntry::Submenu {
                id: format!("{PROFILES_SUBMENU_ID}_{}", scope.as_str()),
                label: scope_label.to_string(),
                entries,
            }
        })
        .collect();

    Some(MenuEntry::Submenu {
        id: PROFILES_SUBMENU_ID.to_string(),
        label: texts.projects_label.to_string(),
        entries: scope_entries,
    })
}

/// 由输入快照派生完整菜单模型
pub fn build_menu_model(input: &TrayModelInput<'_>) -> TrayMenuModel {
    let texts = &input.texts;
    let mut entries = vec![
        // 顶部：打开主界面 / 打开官方网站
        MenuEntry::Item {
            id: SHOW_MAIN_ID.to_string(),
            label: texts.show_main.to_string(),
            enabled: true,
        },
        MenuEntry::Item {
            id: OPEN_WEBSITE_ID.to_string(),
            label: texts.open_website.to_string(),
            enabled: true,
        },
        MenuEntry::Separator,
    ];

    // 每个应用类型折叠为子菜单，避免供应商过多时菜单过长
    for section in &input.sections {
        entries.push(section_entry(section, texts, input.runtime));
        entries.push(MenuEntry::Separator);
    }

    // 项目 Profile 子菜单：项目列表全应用共享，按分组嵌套子菜单各自勾选/应用
    if let Some(profiles) = profiles_entry(input.profiles, &input.profile_scopes, texts) {
        entries.push(profiles);
        entries.push(MenuEntry::Separator);
    }

    entries.push(MenuEntry::Check {
        id: LIGHTWEIGHT_MODE_ID.to_string(),
        label: texts.lightweight_mode.to_string(),
        enabled: true,
        checked: input.lightweight,
    });
    entries.push(MenuEntry::Separator);
    entries.push(MenuEntry::Item {
        id: QUIT_ID.to_string(),
        label: texts.quit.to_string(),
        enabled: true,
    });

    TrayMenuModel { entries }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(app_type: AppType) -> &'static TrayAppSection {
        TRAY_SECTIONS
            .iter()
            .find(|s| s.app_type == app_type)
            .expect("tray section should exist")
    }

    fn provider(id: &str, name: &str, sort_index: usize) -> Provider {
        let mut p = Provider::with_id(
            id.to_string(),
            name.to_string(),
            serde_json::json!({}),
            None,
        );
        p.sort_index = Some(sort_index);
        p
    }

    fn official(id: &str, name: &str, sort_index: usize) -> Provider {
        let mut p = provider(id, name, sort_index);
        p.category = Some("official".to_string());
        p
    }

    fn providers(list: Vec<Provider>) -> IndexMap<String, Provider> {
        list.into_iter().map(|p| (p.id.clone(), p)).collect()
    }

    fn profile(id: &str, name: &str) -> Profile {
        Profile {
            id: id.to_string(),
            name: name.to_string(),
            payload: "{}".to_string(),
            sort_order: None,
            created_at: None,
            updated_at: None,
        }
    }

    fn input<'a>(
        sections: Vec<SectionInput>,
        profiles: &'a [Profile],
        profile_scopes: Vec<ProfileScopeInput>,
        runtime: &'a TrayRuntime,
    ) -> TrayModelInput<'a> {
        TrayModelInput {
            texts: TrayTexts::from_language("en"),
            sections,
            profiles,
            profile_scopes,
            lightweight: false,
            runtime,
        }
    }

    fn ids(entries: &[MenuEntry]) -> Vec<Option<&str>> {
        entries.iter().map(MenuEntry::id).collect()
    }

    fn claude_section(list: Vec<Provider>, current: &str, suffix: Option<&str>) -> SectionInput {
        SectionInput {
            section: section(AppType::Claude),
            providers: providers(list),
            current_id: current.to_string(),
            usage_suffix: suffix.map(String::from),
        }
    }

    fn submenu_entries<'a>(model: &'a TrayMenuModel, id: &str) -> (&'a str, &'a [MenuEntry]) {
        match model.find(id) {
            Some(MenuEntry::Submenu { label, entries, .. }) => (label, entries),
            other => panic!("expected submenu {id}, got {other:?}"),
        }
    }

    fn check(entry: &MenuEntry) -> (&str, bool, bool) {
        match entry {
            MenuEntry::Check {
                label,
                enabled,
                checked,
                ..
            } => (label, *enabled, *checked),
            other => panic!("expected check item, got {other:?}"),
        }
    }

    #[test]
    fn minimal_menu_has_fixed_items_only() {
        let runtime = TrayRuntime::default();
        let model = build_menu_model(&input(vec![], &[], vec![], &runtime));
        assert_eq!(
            ids(&model.entries),
            vec![
                Some(SHOW_MAIN_ID),
                Some(OPEN_WEBSITE_ID),
                None,
                Some(LIGHTWEIGHT_MODE_ID),
                None,
                Some(QUIT_ID),
            ]
        );
        assert_eq!(model.entries[0].label(), Some("Open main window"));
        assert_eq!(model.entries[5].label(), Some("Quit"));
        assert!(matches!(
            model.find(LIGHTWEIGHT_MODE_ID),
            Some(MenuEntry::Check { checked: false, .. })
        ));
    }

    #[test]
    fn lightweight_mode_is_checked_when_active() {
        let runtime = TrayRuntime::default();
        let mut model_input = input(vec![], &[], vec![], &runtime);
        model_input.lightweight = true;
        let model = build_menu_model(&model_input);
        assert!(matches!(
            model.find(LIGHTWEIGHT_MODE_ID),
            Some(MenuEntry::Check { checked: true, .. })
        ));
    }

    #[test]
    fn empty_section_renders_disabled_placeholder() {
        let runtime = TrayRuntime::default();
        let claude = section(AppType::Claude);
        let model = build_menu_model(&input(
            vec![claude_section(vec![], "", None)],
            &[],
            vec![],
            &runtime,
        ));
        assert_eq!(
            model.find(&claude.empty_id),
            Some(&MenuEntry::Item {
                id: claude.empty_id.clone(),
                label: format!("{} (no providers)", claude.header_label),
                enabled: false,
            })
        );
        assert!(model.find(&section_submenu_id(&AppType::Claude)).is_none());
    }

    #[test]
    fn section_lists_sorted_providers_and_checks_current() {
        let runtime = TrayRuntime::default();
        let claude = section(AppType::Claude);
        let model = build_menu_model(&input(
            vec![claude_section(
                vec![provider("b", "Beta", 2), provider("a", "Alpha", 1)],
                "b",
                Some(" · \u{1F7E2} h9%"),
            )],
            &[],
            vec![],
            &runtime,
        ));
        let (label, entries) = submenu_entries(&model, &section_submenu_id(&AppType::Claude));
        assert_eq!(
            label,
            format!("{} · Beta · \u{1F7E2} h9%", claude.header_label)
        );
        assert_eq!(
            ids(entries),
            vec![
                Some(format!("{}a", claude.prefix).as_str()),
                Some(format!("{}b", claude.prefix).as_str()),
            ]
        );
        assert_eq!(check(&entries[0]), ("Alpha", true, false));
        assert_eq!(check(&entries[1]), ("Beta", true, true));
    }

    #[test]
    fn section_without_current_provider_uses_header_label() {
        let runtime = TrayRuntime::default();
        let claude = section(AppType::Claude);
        let model = build_menu_model(&input(
            vec![claude_section(vec![provider("a", "Alpha", 1)], "", None)],
            &[],
            vec![],
            &runtime,
        ));
        let (label, entries) = submenu_entries(&model, &section_submenu_id(&AppType::Claude));
        assert_eq!(label, claude.header_label);
        assert_eq!(check(&entries[0]), ("Alpha", true, false));
    }

    #[test]
    fn official_provider_is_blocked_while_app_is_taken_over() {
        let runtime = TrayRuntime {
            proxy_running: true,
            taken_over: vec![AppType::Claude],
        };
        let model = build_menu_model(&input(
            vec![claude_section(
                vec![provider("a", "Relay", 1), official("o", "Official", 2)],
                "a",
                None,
            )],
            &[],
            vec![],
            &runtime,
        ));
        let (_, entries) = submenu_entries(&model, &section_submenu_id(&AppType::Claude));
        assert_eq!(check(&entries[0]), ("Relay", true, true));
        assert_eq!(check(&entries[1]), ("Official \u{26D4}", false, false));
    }

    #[test]
    fn official_provider_is_selectable_without_running_proxy_or_takeover() {
        for runtime in [
            // 接管标记残留但代理未运行
            TrayRuntime {
                proxy_running: false,
                taken_over: vec![AppType::Claude],
            },
            // 代理运行但该应用未被接管
            TrayRuntime {
                proxy_running: true,
                taken_over: vec![AppType::Codex],
            },
        ] {
            let model = build_menu_model(&input(
                vec![claude_section(
                    vec![official("o", "Official", 1)],
                    "o",
                    None,
                )],
                &[],
                vec![],
                &runtime,
            ));
            let (_, entries) = submenu_entries(&model, &section_submenu_id(&AppType::Claude));
            assert_eq!(check(&entries[0]), ("Official", true, true), "{runtime:?}");
        }
    }

    #[test]
    fn profiles_submenu_per_scope_with_current_and_none_items() {
        let runtime = TrayRuntime::default();
        let profiles = [profile("p1", "Work"), profile("p2", "Home")];
        let model = build_menu_model(&input(
            vec![],
            &profiles,
            vec![
                ProfileScopeInput {
                    scope: ProfileScope::Claude,
                    current_profile_id: "p2".to_string(),
                },
                ProfileScopeInput {
                    scope: ProfileScope::Codex,
                    current_profile_id: String::new(),
                },
            ],
            &runtime,
        ));

        let (label, scopes) = submenu_entries(&model, PROFILES_SUBMENU_ID);
        assert_eq!(label, "Projects");
        assert_eq!(
            ids(scopes),
            vec![
                Some("submenu_profiles_claude"),
                Some("submenu_profiles_codex"),
            ]
        );

        let (claude_label, claude) = submenu_entries(&model, "submenu_profiles_claude");
        assert_eq!(claude_label, "Claude Code");
        assert_eq!(
            ids(claude),
            vec![
                Some("profile_claude_p1"),
                Some("profile_claude_p2"),
                None,
                Some("profile_none_claude"),
            ]
        );
        assert_eq!(check(&claude[0]), ("Work", true, false));
        assert_eq!(check(&claude[1]), ("Home", true, true));
        assert_eq!(check(&claude[3]), ("No project", true, false));

        let (_, codex) = submenu_entries(&model, "submenu_profiles_codex");
        assert_eq!(check(&codex[3]), ("No project", true, true));
    }

    #[test]
    fn profiles_submenu_hidden_without_profiles_or_scopes() {
        let runtime = TrayRuntime::default();
        let no_profiles = build_menu_model(&input(
            vec![],
            &[],
            vec![ProfileScopeInput {
                scope: ProfileScope::Claude,
                current_profile_id: String::new(),
            }],
            &runtime,
        ));
        assert!(no_profiles.find(PROFILES_SUBMENU_ID).is_none());

        let profiles = [profile("p1", "Work")];
        let no_scopes = build_menu_model(&input(vec![], &profiles, vec![], &runtime));
        assert!(no_scopes.find(PROFILES_SUBMENU_ID).is_none());
    }

    #[test]
    fn visibility_filters_sections_and_profile_scopes() {
        let mut visible = VisibleApps::default();
        for section in TRAY_SECTIONS.iter() {
            visible.set_visible(&section.app_type, false);
        }
        visible.set_visible(&AppType::ClaudeDesktop, false);
        visible.set_visible(&AppType::Codex, true);

        let sections: Vec<_> = visible_sections(&visible)
            .map(|s| s.app_type.clone())
            .collect();
        assert_eq!(sections, vec![AppType::Codex]);
        assert_eq!(visible_profile_scopes(&visible), vec![ProfileScope::Codex]);
    }

    #[test]
    fn section_order_follows_registry() {
        let runtime = TrayRuntime::default();
        let sections: Vec<SectionInput> = TRAY_SECTIONS
            .iter()
            .map(|section| SectionInput {
                section,
                providers: providers(vec![provider("x", "X", 1)]),
                current_id: "x".to_string(),
                usage_suffix: None,
            })
            .collect();
        let model = build_menu_model(&input(sections, &[], vec![], &runtime));
        let submenu_ids: Vec<String> = model
            .entries
            .iter()
            .filter_map(|e| match e {
                MenuEntry::Submenu { id, .. } => Some(id.clone()),
                _ => None,
            })
            .collect();
        let expected: Vec<String> = TRAY_SECTIONS
            .iter()
            .map(|s| section_submenu_id(&s.app_type))
            .collect();
        assert_eq!(submenu_ids, expected);
    }
}
