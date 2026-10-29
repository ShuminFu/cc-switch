//! 应用元数据注册表与按应用划分的配置适配器
//!
//! 所有"按应用枚举"的场景（启动导入、代理接管恢复、托盘分区等）都应从
//! [`registry::APPS`] 按能力过滤，而不是各自维护一份手写列表；
//! 按应用分派的 live 配置读写走 [`adapter`] 返回的 [`AppAdapter`]。

pub mod adapter;
pub mod claude;
pub mod claude_desktop;
pub mod codex;
pub mod gemini;
pub mod grok;
mod json_snippet;
pub mod registry;

pub(crate) use json_snippet::{json_deep_merge, json_deep_remove, json_is_subset};

pub use adapter::AppAdapter;
pub use registry::{AppCaps, AppDescriptor, InputTokenSemantics, SyncMode, APPS};

use crate::app_config::AppType;

/// 返回应用的配置适配器。
///
/// 尚未迁移到 [`AppAdapter`] 的应用返回 `None`，调用方继续走原有的按应用分派。
pub fn adapter(app: &AppType) -> Option<&'static dyn AppAdapter> {
    match app {
        AppType::Claude => Some(&claude::ClaudeAdapter),
        AppType::ClaudeDesktop => Some(&claude_desktop::ClaudeDesktopAdapter),
        AppType::Codex => Some(&codex::CodexAdapter),
        AppType::Gemini => Some(&gemini::GeminiAdapter),
        AppType::GrokBuild => Some(&grok::GrokAdapter),
        AppType::OpenCode | AppType::OpenClaw | AppType::Hermes => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::provider::Provider;

    /// 去掉顶层 `#[cfg(test)]` 模块，只保留生产代码
    fn production_source(src: &str) -> String {
        let mut out = String::new();
        let mut in_test_module = false;
        for line in src.lines() {
            if line == "#[cfg(test)]" {
                in_test_module = true;
                continue;
            }
            if in_test_module {
                if line == "}" {
                    in_test_module = false;
                }
                continue;
            }
            out.push_str(line);
            out.push('\n');
        }
        out
    }

    #[test]
    fn adapter_backed_apps_have_no_arms_in_provider_service() {
        let sources = [
            (
                "services/provider/live.rs",
                include_str!("../services/provider/live.rs"),
            ),
            (
                "services/provider/mod.rs",
                include_str!("../services/provider/mod.rs"),
            ),
            ("prompt_files.rs", include_str!("../prompt_files.rs")),
            ("commands/config.rs", include_str!("../commands/config.rs")),
        ];
        let migrated: Vec<&AppDescriptor> = APPS
            .iter()
            .filter(|descriptor| adapter(&descriptor.app).is_some())
            .collect();
        assert!(!migrated.is_empty());

        for (name, src) in sources {
            let production = production_source(src);
            for descriptor in &migrated {
                let pattern = format!("AppType::{:?}", descriptor.app);
                let hits: Vec<_> = production
                    .lines()
                    .filter(|line| !line.trim_start().starts_with("//"))
                    .filter(|line| line.contains(&pattern))
                    .collect();
                assert!(
                    hits.is_empty(),
                    "{name} still dispatches {pattern} by hand; route it through apps::adapter():\n{}",
                    hits.join("\n")
                );
            }
        }
    }

    #[test]
    fn migrated_apps_have_adapters() {
        let migrated: Vec<AppType> = APPS
            .iter()
            .map(|descriptor| descriptor.app.clone())
            .filter(|app| adapter(app).is_some())
            .collect();
        assert_eq!(
            migrated,
            vec![
                AppType::Claude,
                AppType::ClaudeDesktop,
                AppType::Codex,
                AppType::Gemini,
                AppType::GrokBuild
            ]
        );
    }

    #[test]
    fn adapter_paths_match_legacy_layout() {
        let gemini = adapter(&AppType::Gemini).expect("gemini adapter");
        let gemini_dir = gemini.config_dir().unwrap();
        assert_eq!(gemini_dir, crate::gemini_config::get_gemini_dir());
        assert_eq!(gemini.prompt_file(), Some(gemini_dir.join("GEMINI.md")));
        assert_eq!(gemini.skills_dir().unwrap().file_name().unwrap(), "skills");
        assert_eq!(
            gemini
                .session_source()
                .expect("gemini sessions")
                .session_roots(),
            vec![gemini_dir.join("tmp")]
        );

        let grok = adapter(&AppType::GrokBuild).expect("grok adapter");
        let grok_dir = grok.config_dir().unwrap();
        assert_eq!(grok_dir, crate::grok_config::get_grok_config_dir());
        assert_eq!(grok.prompt_file(), Some(grok_dir.join("AGENTS.md")));
        assert_eq!(grok.skills_dir().unwrap().file_name().unwrap(), "skills");
        assert_eq!(
            grok.session_source()
                .expect("grok sessions")
                .session_roots(),
            vec![
                grok_dir.join("sessions"),
                grok_dir.join("archived_sessions"),
            ]
        );

        let claude = adapter(&AppType::Claude).expect("claude adapter");
        let claude_dir = claude.config_dir().unwrap();
        assert_eq!(claude_dir, crate::config::get_claude_config_dir());
        assert_eq!(
            claude.prompt_file(),
            Some(
                crate::config::get_claude_settings_path()
                    .parent()
                    .unwrap()
                    .join("CLAUDE.md")
            )
        );
        assert_eq!(claude.skills_dir().unwrap().file_name().unwrap(), "skills");
        assert_eq!(
            claude
                .session_source()
                .expect("claude sessions")
                .session_roots(),
            vec![claude_dir.join("projects")]
        );

        let desktop = adapter(&AppType::ClaudeDesktop).expect("claude desktop adapter");
        assert_eq!(desktop.prompt_file(), None);
        assert_eq!(desktop.skills_dir(), None);
        assert!(desktop.mcp().is_none());
        assert!(desktop.takeover().is_none());
        assert!(desktop.session_source().is_none());
        assert!(desktop.common_snippet().is_none());
        assert!(!desktop.takeover_uses_live_backup());
        assert!(claude.takeover_uses_live_backup());

        let codex = adapter(&AppType::Codex).expect("codex adapter");
        let codex_dir = codex.config_dir().unwrap();
        assert_eq!(codex_dir, crate::codex_config::get_codex_config_dir());
        assert_eq!(
            codex.prompt_file(),
            Some(
                crate::codex_config::get_codex_auth_path()
                    .parent()
                    .unwrap()
                    .join("AGENTS.md")
            )
        );
        assert_eq!(codex.skills_dir().unwrap().file_name().unwrap(), "skills");
        assert_eq!(
            codex
                .session_source()
                .expect("codex sessions")
                .session_roots(),
            crate::session_manager::providers::codex::session_roots()
        );
        assert!(codex.takeover_uses_live_backup());
    }

    #[test]
    fn gemini_credentials_default_base_url() {
        let gemini = adapter(&AppType::Gemini).unwrap();
        let provider = Provider::with_id(
            "p".to_string(),
            "P".to_string(),
            json!({ "env": { "GEMINI_API_KEY": "sk-test" } }),
            None,
        );
        let credentials = gemini.credentials(&provider).unwrap();
        assert_eq!(credentials.api_key, "sk-test");
        assert_eq!(
            credentials.base_url,
            "https://generativelanguage.googleapis.com"
        );

        let missing = Provider::with_id("p".to_string(), "P".to_string(), json!({}), None);
        assert!(gemini.credentials(&missing).is_err());
    }

    #[test]
    fn grok_credentials_come_from_config_toml() {
        let grok = adapter(&AppType::GrokBuild).unwrap();
        let provider = Provider::with_id(
            "p".to_string(),
            "P".to_string(),
            json!({
                "config": "[models]\ndefault = \"grok-4.5\"\n\n[model.\"grok-4.5\"]\nmodel = \"grok-4.5\"\nbase_url = \"https://example.com/v1\"\nname = \"Example\"\napi_key = \"secret\"\napi_backend = \"responses\"\ncontext_window = 500000\n"
            }),
            None,
        );
        let credentials = grok.credentials(&provider).unwrap();
        assert_eq!(credentials.api_key, "secret");
        assert_eq!(credentials.base_url, "https://example.com/v1");
        assert!(grok.common_snippet().is_none());
    }

    #[test]
    fn gemini_common_snippet_round_trips_env() {
        let common = adapter(&AppType::Gemini)
            .unwrap()
            .common_snippet()
            .expect("gemini common snippet");
        let settings = json!({
            "env": {
                "GEMINI_API_KEY": "sk-test",
                "GOOGLE_GEMINI_BASE_URL": "https://example.com",
                "GEMINI_MODEL": "gemini-2.5-pro"
            }
        });

        let snippet = common.extract(&settings).unwrap();
        assert!(!snippet.contains("GEMINI_API_KEY"));
        assert!(!snippet.contains("GOOGLE_GEMINI_BASE_URL"));
        assert!(common.contains(&settings, &snippet));

        let stripped = common.remove(&settings, &snippet).unwrap();
        assert!(stripped["env"].get("GEMINI_MODEL").is_none());
        assert!(!common.contains(&stripped, &snippet));
        assert_eq!(common.apply(&stripped, &snippet).unwrap(), settings);
    }

    #[test]
    fn takeover_placeholder_detection() {
        let token = crate::proxy::inbound_auth::LEGACY_PROXY_TOKEN;
        let gemini = adapter(&AppType::Gemini).unwrap().takeover().unwrap();
        assert!(gemini.has_placeholder(&json!({ "env": { "GEMINI_API_KEY": token } })));
        assert!(!gemini.has_placeholder(&json!({ "env": { "GEMINI_API_KEY": "sk-real" } })));
        assert!(!gemini.has_placeholder(&json!({})));

        let claude = adapter(&AppType::Claude).unwrap().takeover().unwrap();
        for key in [
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_API_KEY",
            "OPENROUTER_API_KEY",
            "OPENAI_API_KEY",
        ] {
            assert!(
                claude.has_placeholder(&json!({ "env": { key: token } })),
                "{key}"
            );
        }
        assert!(!claude.has_placeholder(&json!({ "env": { "ANTHROPIC_AUTH_TOKEN": "sk-real" } })));
        assert!(!claude.has_placeholder(&json!({ "env": { "GOOGLE_API_KEY": token } })));

        let grok = adapter(&AppType::GrokBuild).unwrap().takeover().unwrap();
        let config = |key: &str| {
            json!({
                "config": format!(
                    "[models]\ndefault = \"grok-4.5\"\n\n[model.\"grok-4.5\"]\nmodel = \"grok-4.5\"\nbase_url = \"http://127.0.0.1:15721/grokbuild/v1\"\nname = \"Example\"\napi_key = \"{key}\"\napi_backend = \"responses\"\ncontext_window = 500000\n"
                )
            })
        };
        assert!(grok.has_placeholder(&config(token)));
        assert!(!grok.has_placeholder(&config("secret")));
    }

    #[test]
    fn claude_credentials_prefer_auth_token() {
        let claude = adapter(&AppType::Claude).unwrap();
        let provider = Provider::with_id(
            "p".to_string(),
            "P".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "tok",
                    "ANTHROPIC_API_KEY": "key",
                    "ANTHROPIC_BASE_URL": "https://example.com"
                }
            }),
            None,
        );
        let credentials = claude.credentials(&provider).unwrap();
        assert_eq!(credentials.api_key, "tok");
        assert_eq!(credentials.base_url, "https://example.com");

        let missing = Provider::with_id("p".to_string(), "P".to_string(), json!({}), None);
        assert!(claude.credentials(&missing).is_err());
        let not_object = Provider::with_id("p".to_string(), "P".to_string(), json!([]), None);
        assert!(claude.validate_provider(&not_object).is_err());
    }

    #[test]
    fn claude_common_snippet_round_trips_and_syncs_from_live() {
        let common = adapter(&AppType::Claude)
            .unwrap()
            .common_snippet()
            .expect("claude common snippet");
        assert!(common.sync_from_live());
        assert!(!adapter(&AppType::Gemini)
            .unwrap()
            .common_snippet()
            .unwrap()
            .sync_from_live());

        let settings = json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "sk-test",
                "ANTHROPIC_BASE_URL": "https://example.com",
                "ENABLE_TOOL_SEARCH": "true"
            },
            "theme": "dark"
        });
        let snippet = common.extract(&settings).unwrap();
        assert!(!snippet.contains("ANTHROPIC_AUTH_TOKEN"));
        assert!(!snippet.contains("ANTHROPIC_BASE_URL"));
        assert!(common.contains(&settings, &snippet));

        let stripped = common.remove(&settings, &snippet).unwrap();
        assert!(stripped.get("theme").is_none());
        assert!(!common.contains(&stripped, &snippet));
        assert_eq!(common.apply(&stripped, &snippet).unwrap(), settings);
        assert!(!common.contains(&settings, "[not json"));
    }

    #[test]
    fn claude_normalizes_legacy_small_fast_model() {
        let claude = adapter(&AppType::Claude).unwrap();
        let mut settings = json!({ "env": { "ANTHROPIC_SMALL_FAST_MODEL": "haiku" } });
        assert!(claude.normalize_settings(&mut settings));
        assert_eq!(settings["env"]["ANTHROPIC_DEFAULT_HAIKU_MODEL"], "haiku");
        assert!(settings["env"].get("ANTHROPIC_SMALL_FAST_MODEL").is_none());
        assert!(!claude.normalize_settings(&mut settings));

        let mut gemini_settings = json!({ "env": { "ANTHROPIC_SMALL_FAST_MODEL": "haiku" } });
        assert!(!adapter(&AppType::Gemini)
            .unwrap()
            .normalize_settings(&mut gemini_settings));
    }

    #[test]
    fn claude_desktop_live_io_requires_switch_flow() {
        let desktop = adapter(&AppType::ClaudeDesktop).unwrap();
        assert!(desktop.read_live().is_err());
        assert!(desktop.import_live().is_err());
        let provider = Provider::with_id("p".to_string(), "P".to_string(), json!({}), None);
        let err = desktop.write_provider(&provider).unwrap_err();
        assert!(
            err.to_string().contains("provider switch flow")
                || err.to_string().contains("供应商切换流程"),
            "{err}"
        );
    }
}
