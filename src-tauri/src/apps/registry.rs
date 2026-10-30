//! 应用描述符注册表（单一事实来源）
//!
//! 新增一个受管应用时，只需要在 [`APPS`] 中追加一条 [`AppDescriptor`]，
//! `AppType::as_str` / `FromStr` / `is_additive_mode` / `all()` 以及
//! 启动导入、代理接管恢复、托盘分区等调用点都会自动跟随。

use std::ops::BitOr;

use serde::Serialize;

use crate::app_config::AppType;

/// Live 配置写入模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncMode {
    /// 只把当前供应商写入 live 配置（Claude、Codex、Gemini ...）
    Switch,
    /// 把所有供应商都写入 live 配置（OpenCode、OpenClaw、Hermes）
    Additive,
}

/// 上游上报的 `input_tokens` 语义
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InputTokenSemantics {
    /// `input_tokens` 不含缓存部分（Anthropic 风格，也是新应用的安全默认值）
    Fresh,
    /// `input_tokens` 已包含缓存读取部分（OpenAI Responses / Gemini 风格）
    CacheInclusive,
}

/// 应用能力位集合
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppCaps(u16);

impl AppCaps {
    pub const NONE: Self = Self(0);
    /// 支持 CC Switch 管理的 MCP 服务器同步
    pub const MCP: Self = Self(1 << 0);
    /// 支持 CC Switch 管理的 Skills 同步
    pub const SKILLS: Self = Self(1 << 1);
    /// 有提示词文件（首次启动时可导入）
    pub const PROMPTS: Self = Self(1 << 2);
    /// 支持本地代理接管 live 配置（启动时按 proxy_config.enabled 恢复）
    pub const PROXY_TAKEOVER: Self = Self(1 << 3);
    /// 在系统托盘中有独立的供应商分区
    pub const TRAY: Self = Self(1 << 4);
    /// 请求用量会进入使用统计（经本地代理记录，或从会话日志导入）
    pub const USAGE: Self = Self(1 << 5);
    /// 会话管理中可一键在终端恢复会话（`session_manager::build_resume_command`）
    pub const SESSION_RESUME: Self = Self(1 << 6);

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl BitOr for AppCaps {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

/// 单个应用的静态描述
#[derive(Debug)]
pub struct AppDescriptor {
    pub app: AppType,
    /// 规范标识（数据库 `app_type`、配置键、命令参数）
    pub slug: &'static str,
    /// `FromStr` 额外接受的别名（已小写）
    pub aliases: &'static [&'static str],
    /// 前端 i18n 名称键
    pub label_key: &'static str,
    /// 后端（托盘、日志）使用的显示名称
    pub display_name: &'static str,
    pub sync_mode: SyncMode,
    pub caps: AppCaps,
    /// 本地代理上的应用前缀路由（如 `/claude`），不经代理的应用为 `None`
    pub proxy_route: Option<&'static str>,
    pub input_token_semantics: InputTokenSemantics,
    /// 主页面默认是否显示
    pub default_visible: bool,
}

impl AppDescriptor {
    pub const fn has(&self, caps: AppCaps) -> bool {
        self.caps.contains(caps)
    }
}

const CORE_CAPS: AppCaps = AppCaps::MCP.union(AppCaps::SKILLS).union(AppCaps::PROMPTS);
const PROXIED_CAPS: AppCaps = CORE_CAPS
    .union(AppCaps::PROXY_TAKEOVER)
    .union(AppCaps::TRAY)
    .union(AppCaps::USAGE);
/// 有 CLI 会话恢复命令的经代理应用（Claude、Codex、Gemini、Grok Build）
const PROXIED_CLI_CAPS: AppCaps = PROXIED_CAPS.union(AppCaps::SESSION_RESUME);

/// 所有受管应用，顺序即 `AppType::all()` 的顺序。
pub static APPS: &[AppDescriptor] = &[
    AppDescriptor {
        app: AppType::Claude,
        slug: "claude",
        aliases: &[],
        label_key: "apps.claude",
        display_name: "Claude",
        sync_mode: SyncMode::Switch,
        caps: PROXIED_CLI_CAPS,
        proxy_route: Some("/claude"),
        input_token_semantics: InputTokenSemantics::Fresh,
        default_visible: true,
    },
    AppDescriptor {
        app: AppType::ClaudeDesktop,
        slug: "claude-desktop",
        aliases: &["claude_desktop", "claudedesktop"],
        label_key: "apps.claude-desktop",
        display_name: "Claude Desktop",
        sync_mode: SyncMode::Switch,
        // 无 MCP / Skills / 提示词；请求经 /claude-desktop 路由，用量照常记录
        caps: AppCaps::USAGE,
        proxy_route: Some("/claude-desktop"),
        input_token_semantics: InputTokenSemantics::Fresh,
        default_visible: true,
    },
    AppDescriptor {
        app: AppType::Codex,
        slug: "codex",
        aliases: &[],
        label_key: "apps.codex",
        display_name: "Codex",
        sync_mode: SyncMode::Switch,
        caps: PROXIED_CLI_CAPS,
        proxy_route: Some("/codex"),
        input_token_semantics: InputTokenSemantics::CacheInclusive,
        default_visible: true,
    },
    AppDescriptor {
        app: AppType::Gemini,
        slug: "gemini",
        aliases: &[],
        label_key: "apps.gemini",
        display_name: "Gemini",
        sync_mode: SyncMode::Switch,
        caps: PROXIED_CLI_CAPS,
        proxy_route: Some("/gemini"),
        input_token_semantics: InputTokenSemantics::CacheInclusive,
        default_visible: true,
    },
    AppDescriptor {
        app: AppType::GrokBuild,
        slug: "grokbuild",
        aliases: &["grok-build", "grok_build", "grok"],
        label_key: "apps.grokbuild",
        display_name: "Grok Build",
        sync_mode: SyncMode::Switch,
        caps: PROXIED_CLI_CAPS,
        proxy_route: Some("/grokbuild"),
        input_token_semantics: InputTokenSemantics::CacheInclusive,
        default_visible: true,
    },
    AppDescriptor {
        app: AppType::OpenCode,
        slug: "opencode",
        aliases: &[],
        label_key: "apps.opencode",
        display_name: "OpenCode",
        sync_mode: SyncMode::Additive,
        caps: CORE_CAPS
            .union(AppCaps::USAGE)
            .union(AppCaps::SESSION_RESUME),
        proxy_route: None,
        input_token_semantics: InputTokenSemantics::Fresh,
        default_visible: true,
    },
    AppDescriptor {
        app: AppType::OpenClaw,
        slug: "openclaw",
        aliases: &[],
        label_key: "apps.openclaw",
        display_name: "OpenClaw",
        sync_mode: SyncMode::Additive,
        caps: AppCaps::MCP.union(AppCaps::PROMPTS),
        proxy_route: None,
        input_token_semantics: InputTokenSemantics::Fresh,
        default_visible: true,
    },
    AppDescriptor {
        app: AppType::Hermes,
        slug: "hermes",
        aliases: &[],
        label_key: "apps.hermes",
        display_name: "Hermes",
        sync_mode: SyncMode::Additive,
        caps: CORE_CAPS,
        proxy_route: None,
        input_token_semantics: InputTokenSemantics::Fresh,
        // 默认不显示，需用户手动启用
        default_visible: false,
    },
];

/// `get_app_registry` 返回给前端的能力布尔表
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppCapsDto {
    pub mcp: bool,
    pub skills: bool,
    pub prompts: bool,
    pub proxy_takeover: bool,
    pub tray: bool,
    pub usage: bool,
    pub session_resume: bool,
}

impl From<AppCaps> for AppCapsDto {
    fn from(caps: AppCaps) -> Self {
        Self {
            mcp: caps.contains(AppCaps::MCP),
            skills: caps.contains(AppCaps::SKILLS),
            prompts: caps.contains(AppCaps::PROMPTS),
            proxy_takeover: caps.contains(AppCaps::PROXY_TAKEOVER),
            tray: caps.contains(AppCaps::TRAY),
            usage: caps.contains(AppCaps::USAGE),
            session_resume: caps.contains(AppCaps::SESSION_RESUME),
        }
    }
}

/// `get_app_registry` 返回给前端的单个应用描述（可序列化视图）
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppDescriptorDto {
    pub id: &'static str,
    pub aliases: &'static [&'static str],
    pub label_key: &'static str,
    pub display_name: &'static str,
    pub sync_mode: SyncMode,
    pub caps: AppCapsDto,
    pub proxy_route: Option<&'static str>,
    pub input_token_semantics: InputTokenSemantics,
    pub default_visible: bool,
}

impl From<&AppDescriptor> for AppDescriptorDto {
    fn from(d: &AppDescriptor) -> Self {
        Self {
            id: d.slug,
            aliases: d.aliases,
            label_key: d.label_key,
            display_name: d.display_name,
            sync_mode: d.sync_mode,
            caps: d.caps.into(),
            proxy_route: d.proxy_route,
            input_token_semantics: d.input_token_semantics,
            default_visible: d.default_visible,
        }
    }
}

/// 全部应用描述（按 [`APPS`] 顺序），供 `get_app_registry` 命令使用
pub fn registry_dto() -> Vec<AppDescriptorDto> {
    APPS.iter().map(AppDescriptorDto::from).collect()
}

/// 查找应用的描述符
pub fn descriptor(app: &AppType) -> &'static AppDescriptor {
    APPS.iter()
        .find(|d| d.app == *app)
        .expect("every AppType variant must have an AppDescriptor in APPS")
}

/// 按规范标识或别名查找（输入需已 trim + 小写）
pub fn find_by_id(normalized: &str) -> Option<&'static AppDescriptor> {
    APPS.iter()
        .find(|d| d.slug == normalized || d.aliases.contains(&normalized))
}

/// 具备指定能力的应用
pub fn with_caps(caps: AppCaps) -> impl Iterator<Item = &'static AppDescriptor> {
    APPS.iter().filter(move |d| d.has(caps))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::str::FromStr;

    /// 穷举 match：新增 AppType 变体却忘了注册时，这里会编译失败或断言失败。
    fn variant_covered(app: &AppType) -> bool {
        match app {
            AppType::Claude
            | AppType::ClaudeDesktop
            | AppType::Codex
            | AppType::Gemini
            | AppType::GrokBuild
            | AppType::OpenCode
            | AppType::OpenClaw
            | AppType::Hermes => APPS.iter().any(|d| d.app == *app),
        }
    }

    #[test]
    fn every_variant_registered_exactly_once() {
        let apps: Vec<AppType> = AppType::all().collect();
        assert_eq!(apps.len(), APPS.len());
        for app in &apps {
            assert!(variant_covered(app), "{app:?} missing from APPS");
            assert_eq!(APPS.iter().filter(|d| d.app == *app).count(), 1);
        }
    }

    #[test]
    fn slugs_and_aliases_are_unique_and_normalized() {
        let mut seen = HashSet::new();
        for d in APPS {
            for id in std::iter::once(&d.slug).chain(d.aliases.iter()) {
                assert_eq!(*id, id.trim().to_lowercase(), "{id} not normalized");
                assert!(seen.insert(*id), "duplicate app id {id}");
            }
        }
    }

    #[test]
    fn as_str_from_str_round_trip_through_registry() {
        for d in APPS {
            assert_eq!(d.app.as_str(), d.slug);
            assert_eq!(AppType::from_str(d.slug).unwrap(), d.app);
            assert_eq!(
                AppType::from_str(&format!("  {}  ", d.slug.to_uppercase())).unwrap(),
                d.app
            );
            for alias in d.aliases {
                assert_eq!(AppType::from_str(alias).unwrap(), d.app);
            }
        }
        assert!(AppType::from_str("unknown").is_err());
    }

    #[test]
    fn slug_matches_serde_representation() {
        for d in APPS {
            let json = serde_json::to_value(&d.app).unwrap();
            assert_eq!(json, serde_json::Value::String(d.slug.to_string()));
        }
    }

    #[test]
    fn additive_mode_matches_sync_mode() {
        let additive: Vec<&str> = APPS
            .iter()
            .filter(|d| d.app.is_additive_mode())
            .map(|d| d.slug)
            .collect();
        assert_eq!(additive, vec!["opencode", "openclaw", "hermes"]);
    }

    #[test]
    fn capability_sets_match_historical_lists() {
        let slugs = |caps| with_caps(caps).map(|d| d.slug).collect::<Vec<_>>();
        assert_eq!(
            slugs(AppCaps::PROXY_TAKEOVER),
            vec!["claude", "codex", "gemini", "grokbuild"]
        );
        assert_eq!(
            slugs(AppCaps::TRAY),
            vec!["claude", "codex", "gemini", "grokbuild"]
        );
        assert_eq!(
            slugs(AppCaps::PROMPTS),
            vec![
                "claude",
                "codex",
                "gemini",
                "grokbuild",
                "opencode",
                "openclaw",
                "hermes"
            ]
        );
        assert_eq!(
            slugs(AppCaps::MCP),
            vec![
                "claude",
                "codex",
                "gemini",
                "grokbuild",
                "opencode",
                "openclaw",
                "hermes"
            ]
        );
        assert_eq!(
            slugs(AppCaps::SKILLS),
            vec![
                "claude",
                "codex",
                "gemini",
                "grokbuild",
                "opencode",
                "hermes"
            ]
        );
    }

    #[test]
    fn mcp_and_skill_flags_respect_caps() {
        use crate::app_config::AppSet;
        let all: Vec<AppType> = AppType::all().collect();
        let set = AppSet::from_apps(&all);
        for d in APPS {
            // AppSet 同时承载 MCP 与 Skills 的启用状态，接纳具备其一的应用；
            // 只支持其一的应用（OpenClaw 只有 MCP）由关联表按各自能力位过滤持久化
            assert_eq!(
                set.is_enabled_for(&d.app),
                d.has(AppCaps::MCP) || d.has(AppCaps::SKILLS),
                "{}",
                d.slug
            );
        }
    }

    #[test]
    fn default_visible_matches_settings_default() {
        let visible = crate::settings::VisibleApps::default();
        for d in APPS {
            assert_eq!(visible.is_visible(&d.app), d.default_visible, "{}", d.slug);
        }
    }

    #[test]
    fn proxy_routes_are_mounted_by_the_proxy_server() {
        // server.rs 依据 CLIENT_ENDPOINTS 生成代理路由
        let paths: Vec<&str> = crate::proxy::endpoints::CLIENT_ENDPOINTS
            .iter()
            .flat_map(|endpoint| endpoint.paths.iter().copied())
            .collect();
        for d in APPS {
            if let Some(route) = d.proxy_route {
                let prefix = format!("{route}/v1");
                assert!(
                    paths.iter().any(|path| path.starts_with(&prefix)),
                    "{route} not mounted by proxy/endpoints.rs"
                );
            }
        }
    }

    #[test]
    fn label_keys_exist_in_en_locale() {
        let en: serde_json::Value =
            serde_json::from_str(include_str!("../../../src/i18n/locales/en.json")).unwrap();
        for d in APPS {
            let (ns, key) = d.label_key.split_once('.').unwrap();
            assert!(
                en[ns][key].is_string(),
                "missing i18n key {} for {}",
                d.label_key,
                d.slug
            );
        }
    }

    /// 所有 AppType 变体（穷举 match 保证新增变体时这里编译失败，必须补上）
    fn all_variants() -> Vec<AppType> {
        let variants = vec![
            AppType::Claude,
            AppType::ClaudeDesktop,
            AppType::Codex,
            AppType::Gemini,
            AppType::GrokBuild,
            AppType::OpenCode,
            AppType::OpenClaw,
            AppType::Hermes,
        ];
        for v in &variants {
            match v {
                AppType::Claude
                | AppType::ClaudeDesktop
                | AppType::Codex
                | AppType::Gemini
                | AppType::GrokBuild
                | AppType::OpenCode
                | AppType::OpenClaw
                | AppType::Hermes => {}
            }
        }
        variants
    }

    /// 前端资源的只读视图，便于在测试中替换为缺项的假数据
    struct FrontendAssets<'a> {
        /// (语言, 解析后的 locale JSON)
        locales: Vec<(&'a str, serde_json::Value)>,
        /// `src/config/appConfig.tsx`（APP_ICON_MAP）
        icon_map_src: &'a str,
        /// `src/entities/app/registry.ts`（前端静态注册表）
        frontend_registry_src: &'a str,
        /// `src/features/providers/forms`，每个应用一个 `<slug>/presets.ts`
        presets_root: std::path::PathBuf,
    }

    impl FrontendAssets<'static> {
        fn real() -> Self {
            let parse = |src: &str| serde_json::from_str::<serde_json::Value>(src).unwrap();
            Self {
                locales: vec![
                    (
                        "en",
                        parse(include_str!("../../../src/i18n/locales/en.json")),
                    ),
                    (
                        "zh",
                        parse(include_str!("../../../src/i18n/locales/zh.json")),
                    ),
                    (
                        "zh-TW",
                        parse(include_str!("../../../src/i18n/locales/zh-TW.json")),
                    ),
                    (
                        "ja",
                        parse(include_str!("../../../src/i18n/locales/ja.json")),
                    ),
                ],
                icon_map_src: include_str!("../../../src/config/appConfig.tsx"),
                frontend_registry_src: include_str!("../../../src/entities/app/registry.ts"),
                presets_root: std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../src/features/providers/forms"),
            }
        }
    }

    /// 穷举检查：每个变体都有描述符，每个描述符在所有语言中有 i18n 名称、
    /// 有图标、有前端注册表条目和 presets 文件。返回所有缺项。
    fn exhaustiveness_gaps(apps: &[AppDescriptor], assets: &FrontendAssets) -> Vec<String> {
        let mut gaps = Vec::new();
        for variant in all_variants() {
            if !apps.iter().any(|d| d.app == variant) {
                gaps.push(format!("{variant:?}: no AppDescriptor"));
            }
        }
        for d in apps {
            let slug = d.slug;
            match d.label_key.split_once('.') {
                Some((ns, key)) => {
                    for (lang, locale) in &assets.locales {
                        if !locale[ns][key].is_string() {
                            gaps.push(format!("{slug}: label {} missing in {lang}", d.label_key));
                        }
                    }
                }
                None => gaps.push(format!("{slug}: malformed label key {}", d.label_key)),
            }
            let icon_key = if slug.contains('-') {
                format!("\n  \"{slug}\": {{")
            } else {
                format!("\n  {slug}: {{")
            };
            if !assets.icon_map_src.contains(&icon_key) {
                gaps.push(format!("{slug}: no APP_ICON_MAP entry in appConfig.tsx"));
            }
            if !assets
                .frontend_registry_src
                .contains(&format!("id: \"{slug}\""))
            {
                gaps.push(format!("{slug}: missing from src/entities/app/registry.ts"));
            }
            if !assets.presets_root.join(slug).join("presets.ts").is_file() {
                gaps.push(format!("{slug}: no {slug}/presets.ts"));
            }
        }
        gaps
    }

    #[test]
    fn every_app_has_descriptor_labels_icon_and_presets() {
        let gaps = exhaustiveness_gaps(APPS, &FrontendAssets::real());
        assert!(gaps.is_empty(), "app registry gaps:\n{}", gaps.join("\n"));
    }

    #[test]
    fn exhaustiveness_check_flags_dummy_app_without_label() {
        let dummy = AppDescriptor {
            app: AppType::Claude,
            slug: "dummy",
            aliases: &[],
            label_key: "apps.dummy",
            display_name: "Dummy",
            sync_mode: SyncMode::Switch,
            caps: AppCaps::NONE,
            proxy_route: None,
            input_token_semantics: InputTokenSemantics::Fresh,
            default_visible: false,
        };
        let gaps = exhaustiveness_gaps(std::slice::from_ref(&dummy), &FrontendAssets::real());
        for lang in ["en", "zh", "zh-TW", "ja"] {
            assert!(
                gaps.contains(&format!("dummy: label apps.dummy missing in {lang}")),
                "{gaps:?}"
            );
        }
        assert!(gaps.contains(&"dummy: no APP_ICON_MAP entry in appConfig.tsx".to_string()));
        assert!(gaps.contains(&"dummy: no dummy/presets.ts".to_string()));
        // 只登记了 dummy（借用 Claude 变体），其余变体都应被报告缺少描述符
        assert!(gaps.contains(&"Codex: no AppDescriptor".to_string()));

        // 只缺一个语言的名称也必须被发现
        let mut assets = FrontendAssets::real();
        assets.locales[1].1["apps"]
            .as_object_mut()
            .unwrap()
            .remove("hermes");
        let gaps = exhaustiveness_gaps(APPS, &assets);
        assert_eq!(
            gaps,
            vec!["hermes: label apps.hermes missing in zh".to_string()]
        );
    }

    #[test]
    fn registry_dto_mirrors_descriptors() {
        let dto = registry_dto();
        assert_eq!(dto.len(), APPS.len());
        for (entry, d) in dto.iter().zip(APPS) {
            assert_eq!(entry.id, d.slug);
            assert_eq!(entry.caps.mcp, d.has(AppCaps::MCP));
            assert_eq!(entry.caps.skills, d.has(AppCaps::SKILLS));
            assert_eq!(entry.caps.prompts, d.has(AppCaps::PROMPTS));
            assert_eq!(entry.caps.proxy_takeover, d.has(AppCaps::PROXY_TAKEOVER));
            assert_eq!(entry.caps.tray, d.has(AppCaps::TRAY));
            assert_eq!(entry.caps.usage, d.has(AppCaps::USAGE));
            assert_eq!(entry.caps.session_resume, d.has(AppCaps::SESSION_RESUME));
        }
        let json = serde_json::to_value(&dto[1]).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "id": "claude-desktop",
                "aliases": ["claude_desktop", "claudedesktop"],
                "labelKey": "apps.claude-desktop",
                "displayName": "Claude Desktop",
                "syncMode": "switch",
                "caps": {
                    "mcp": false,
                    "skills": false,
                    "prompts": false,
                    "proxyTakeover": false,
                    "tray": false,
                    "usage": true,
                    "sessionResume": false
                },
                "proxyRoute": "/claude-desktop",
                "inputTokenSemantics": "fresh",
                "defaultVisible": true
            })
        );
    }

    #[test]
    fn usage_cap_matches_proxy_routes_and_session_log_imports() {
        // services::session_usage::sync_all_session_usage 导入会话日志的应用
        const SESSION_LOG_APPS: [&str; 4] = ["claude", "codex", "gemini", "opencode"];
        for d in APPS {
            assert_eq!(
                d.has(AppCaps::USAGE),
                d.proxy_route.is_some() || SESSION_LOG_APPS.contains(&d.slug),
                "{}",
                d.slug
            );
        }
    }

    #[test]
    fn session_resume_cap_matches_resume_command_templates() {
        for d in APPS {
            assert_eq!(
                d.has(AppCaps::SESSION_RESUME),
                crate::session_manager::build_resume_command(d.slug, "abc-123").is_some(),
                "{}",
                d.slug
            );
        }
    }

    /// `tests/fixtures/app-registry.json` 是 `get_app_registry` 的完整快照，
    /// 前端能力矩阵测试读取同一份文件，保证前后端对注册表的理解一致。
    #[test]
    fn registry_dto_matches_shared_fixture() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../tests/fixtures/app-registry.json"))
                .unwrap();
        assert_eq!(
            serde_json::to_value(registry_dto()).unwrap(),
            fixture,
            "get_app_registry changed; regenerate tests/fixtures/app-registry.json"
        );
    }

    /// 防回归的"grep 检查"：曾经手写应用列表的调用点必须改为从 APPS 过滤。
    #[test]
    fn former_hard_coded_app_lists_stay_removed() {
        fn squash(src: &str) -> String {
            src.chars().filter(|c| !c.is_whitespace()).collect()
        }
        let sites: [(&str, &str, &[&str]); 4] = [
            (
                "lib.rs",
                include_str!("../lib.rs"),
                &[
                    "[\"claude\",\"codex\",\"gemini\",\"grokbuild\"]",
                    "crate::app_config::AppType::GrokBuild,crate::app_config::AppType::OpenCode,",
                    "PROXY_STARTUP_APP_TYPES",
                ],
            ),
            (
                "services/proxy/**",
                concat!(
                    include_str!("../services/proxy/mod.rs"),
                    include_str!("../services/proxy/lifecycle.rs"),
                    include_str!("../services/proxy/config.rs"),
                    include_str!("../services/proxy/backup.rs"),
                    include_str!("../services/proxy/hot_switch.rs"),
                    include_str!("../services/proxy/takeover/mod.rs"),
                    include_str!("../services/proxy/takeover/claude.rs"),
                    include_str!("../services/proxy/takeover/codex.rs"),
                    include_str!("../services/proxy/takeover/gemini.rs"),
                    include_str!("../services/proxy/takeover/grok.rs"),
                ),
                &[
                    "ifletOk(live_config)=self.read_claude_live()",
                    "forapp_typein[\"claude\",\"codex\",\"gemini\",\"grokbuild\"]",
                    "forapp_typein[AppType::Claude,AppType::Codex,AppType::Gemini,AppType::GrokBuild,]",
                ],
            ),
            (
                "tray.rs",
                include_str!("../tray.rs"),
                &["TrayAppSection{app_type:AppType::"],
            ),
            (
                "app_config.rs",
                include_str!("../app_config.rs"),
                &[
                    "apps.push(AppType::",
                    "AppType::GrokBuild,AppType::OpenCode,AppType::OpenClaw,AppType::Hermes,]",
                ],
            ),
        ];
        for (name, src, patterns) in sites {
            let squashed = squash(src);
            for pattern in patterns {
                assert!(
                    !squashed.contains(pattern),
                    "{name} reintroduced a hard-coded app list ({pattern}); filter apps::APPS by caps instead"
                );
            }
        }
    }
}
