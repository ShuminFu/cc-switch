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
pub mod hermes;
mod json_snippet;
pub mod openclaw;
pub mod opencode;
pub mod registry;

pub(crate) use json_snippet::{json_deep_merge, json_deep_remove, json_is_subset};

pub use adapter::AppAdapter;
pub use registry::{AppCaps, AppDescriptor, InputTokenSemantics, SyncMode, APPS};

use crate::app_config::AppType;

/// 返回应用的配置适配器。
///
/// 每个应用都有适配器；match 是穷尽的，新增 [`AppType`] 必须在这里接入。
pub fn adapter(app: &AppType) -> &'static dyn AppAdapter {
    match app {
        AppType::Claude => &claude::ClaudeAdapter,
        AppType::ClaudeDesktop => &claude_desktop::ClaudeDesktopAdapter,
        AppType::Codex => &codex::CodexAdapter,
        AppType::Gemini => &gemini::GeminiAdapter,
        AppType::GrokBuild => &grok::GrokAdapter,
        AppType::OpenCode => &opencode::OpenCodeAdapter,
        AppType::OpenClaw => &openclaw::OpenClawAdapter,
        AppType::Hermes => &hermes::HermesAdapter,
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
        for (name, src) in sources {
            let production = production_source(src);
            for descriptor in APPS {
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

    /// `#[cfg(test)] mod foo;` 声明的子模块文件（相对 src），
    /// 与 tests/app_adapter_conformance.rs 的同名函数一致
    fn test_only_module_files(rel: &str, src: &str) -> Vec<String> {
        let dir = match rel.rsplit_once('/') {
            Some((parent, "mod.rs")) => format!("{parent}/"),
            Some((parent, file)) => format!("{parent}/{}/", file.trim_end_matches(".rs")),
            None if rel == "lib.rs" || rel == "main.rs" => String::new(),
            None => format!("{}/", rel.trim_end_matches(".rs")),
        };
        let lines: Vec<&str> = src.lines().map(str::trim).collect();
        lines
            .windows(2)
            .filter(|pair| pair[0] == "#[cfg(test)]")
            .filter_map(|pair| {
                let name = pair[1]
                    .trim_start_matches("pub ")
                    .strip_prefix("mod ")?
                    .strip_suffix(';')?;
                Some(name.to_string())
            })
            .flat_map(|name| [format!("{dir}{name}.rs"), format!("{dir}{name}/")])
            .collect()
    }

    /// apps/** 之外的生产代码里以 `AppType::` 变体做分支的 `match`。
    ///
    /// 返回 `(相对 src 的路径, match 所在行)`。测试模块（内联的 `#[cfg(test)]` 块
    /// 与 `#[cfg(test)] mod foo;` 声明的文件）、注释与 apps/** 不计入。
    fn match_app_sites() -> Vec<(String, String)> {
        fn collect(dir: &std::path::Path, root: &std::path::Path, out: &mut Vec<(String, String)>) {
            let mut entries: Vec<_> = std::fs::read_dir(dir)
                .expect("read src dir")
                .map(|entry| entry.expect("dir entry").path())
                .collect();
            entries.sort();
            for path in entries {
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                if path.is_dir() {
                    if rel != "apps" {
                        collect(&path, root, out);
                    }
                    continue;
                }
                if path.extension().is_none_or(|ext| ext != "rs") {
                    continue;
                }
                out.push((rel, std::fs::read_to_string(&path).expect("read source")));
            }
        }

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut sources = Vec::new();
        collect(&root, &root, &mut sources);
        let test_only: Vec<String> = sources
            .iter()
            .flat_map(|(rel, src)| test_only_module_files(rel, src))
            .collect();

        let mut sites = Vec::new();
        for (rel, src) in &sources {
            let is_test_only = test_only.iter().any(|excluded| {
                rel == excluded || (excluded.ends_with('/') && rel.starts_with(excluded.as_str()))
            });
            if is_test_only {
                continue;
            }
            let production = production_source(src);
            let lines: Vec<&str> = production.lines().map(str::trim).collect();
            for (i, line) in lines.iter().enumerate() {
                let is_match = !line.starts_with("//")
                    && line.ends_with('{')
                    && line
                        .split(|c: char| !c.is_alphanumeric() && c != '_')
                        .any(|word| word == "match");
                if !is_match {
                    continue;
                }
                let first_arm = lines[i + 1..]
                    .iter()
                    .find(|next| {
                        !next.is_empty() && !next.starts_with("//") && !next.starts_with('#')
                    })
                    .copied()
                    .unwrap_or("");
                if first_arm.starts_with("AppType::")
                    || first_arm.starts_with("crate::app_config::AppType::")
                {
                    sites.push((rel.clone(), line.to_string()));
                }
            }
        }
        sites
    }

    /// match-app 站点计数（CI 通过 `cargo test` 执行）。
    ///
    /// CCS-149 前为 56 处（同一统计口径），之后的上限见 [`MATCH_APP_SITE_BUDGET`]，只允许下降。
    /// P4 退出目标是 15 处以内。CCS-153/154/155 收敛接管引擎与按应用状态后实测 28 处
    ///（CCS-232 起不再把 `#[cfg(test)] mod foo;` 声明的测试文件计入）。
    const MATCH_APP_SITE_BUDGET: usize = 28;

    #[test]
    fn match_app_sites_outside_apps_stay_within_budget() {
        let sites = match_app_sites();
        let listing = sites
            .iter()
            .map(|(file, line)| format!("{file}: {line}"))
            .collect::<Vec<_>>()
            .join("\n");

        for file in [
            "proxy/forwarder/wire_golden.rs",
            "proxy/usage/cost_golden.rs",
        ] {
            assert!(
                sites.iter().all(|(site_file, _)| site_file != file),
                "{file} is a #[cfg(test)] module and must not be counted:\n{listing}"
            );
        }

        for file in [
            "services/provider/live.rs",
            "services/provider/mod.rs",
            "prompt_files.rs",
            "services/skill.rs",
            "commands/config.rs",
        ] {
            assert!(
                sites.iter().all(|(site_file, _)| site_file != file),
                "{file} must not match on AppType; dispatch through apps::adapter():\n{listing}"
            );
        }

        assert!(
            sites.len() <= MATCH_APP_SITE_BUDGET,
            "{} match-app sites outside apps/** (budget {MATCH_APP_SITE_BUDGET}); \
             dispatch through apps::adapter() instead:\n{listing}",
            sites.len()
        );
    }

    #[test]
    fn adapter_capabilities_match_registry() {
        for descriptor in APPS {
            let adapter = adapter(&descriptor.app);
            assert_eq!(
                adapter.prompt_file().is_some(),
                descriptor.has(AppCaps::PROMPTS),
                "{} prompt file vs PROMPTS cap",
                descriptor.slug
            );
            assert_eq!(
                adapter.mcp().is_some(),
                descriptor.has(AppCaps::MCP),
                "{} MCP projector vs MCP cap",
                descriptor.slug
            );
            assert_eq!(
                adapter.takeover().is_some(),
                descriptor.has(AppCaps::PROXY_TAKEOVER),
                "{} takeover vs PROXY_TAKEOVER cap",
                descriptor.slug
            );
            if descriptor.sync_mode == SyncMode::Additive {
                assert!(
                    adapter.import_live().is_err(),
                    "{} imports per provider, not as `default`",
                    descriptor.slug
                );
            }
        }
    }

    #[test]
    fn adapter_paths_match_legacy_layout() {
        let gemini = adapter(&AppType::Gemini);
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

        let grok = adapter(&AppType::GrokBuild);
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

        let claude = adapter(&AppType::Claude);
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

        let desktop = adapter(&AppType::ClaudeDesktop);
        assert_eq!(desktop.prompt_file(), None);
        assert_eq!(desktop.skills_dir(), None);
        assert!(desktop.mcp().is_none());
        assert!(desktop.takeover().is_none());
        assert!(desktop.session_source().is_none());
        assert!(desktop.common_snippet().is_none());
        assert!(!desktop.takeover_uses_live_backup());
        assert!(claude.takeover_uses_live_backup());

        let codex = adapter(&AppType::Codex);
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

        let opencode = adapter(&AppType::OpenCode);
        let opencode_dir = opencode.config_dir().unwrap();
        assert_eq!(opencode_dir, crate::opencode_config::get_opencode_dir());
        assert_eq!(opencode.prompt_file(), Some(opencode_dir.join("AGENTS.md")));
        assert_eq!(
            opencode.skills_dir(),
            Some(
                crate::config::get_home_dir()
                    .join(".config")
                    .join("opencode")
                    .join("skills")
            )
        );
        assert_eq!(
            opencode
                .session_source()
                .expect("opencode sessions")
                .session_roots(),
            vec![crate::session_manager::providers::opencode::get_opencode_data_dir()]
        );

        let openclaw = adapter(&AppType::OpenClaw);
        let openclaw_dir = openclaw.config_dir().unwrap();
        assert_eq!(openclaw_dir, crate::openclaw_config::get_openclaw_dir());
        assert_eq!(openclaw.prompt_file(), Some(openclaw_dir.join("AGENTS.md")));
        assert_eq!(
            openclaw.skills_dir(),
            Some(
                crate::config::get_home_dir()
                    .join(".openclaw")
                    .join("skills")
            )
        );
        assert_eq!(
            openclaw
                .session_source()
                .expect("openclaw sessions")
                .session_roots(),
            vec![openclaw_dir.join("agents")]
        );

        let hermes = adapter(&AppType::Hermes);
        let hermes_dir = hermes.config_dir().unwrap();
        assert_eq!(hermes_dir, crate::hermes_config::get_hermes_dir());
        assert_eq!(hermes.prompt_file(), Some(hermes_dir.join("AGENTS.md")));
        assert_eq!(hermes.skills_dir(), Some(hermes_dir.join("skills")));
        assert_eq!(
            hermes
                .session_source()
                .expect("hermes sessions")
                .session_roots(),
            vec![hermes_dir.join("sessions")]
        );
    }

    #[test]
    fn usage_credentials_follow_each_app_shape() {
        let cases = [
            (
                AppType::OpenCode,
                json!({ "options": { "baseURL": "https://a", "apiKey": "k1" } }),
            ),
            (
                AppType::OpenClaw,
                json!({ "baseUrl": "https://a", "apiKey": "k1" }),
            ),
            (
                AppType::Hermes,
                json!({ "base_url": "https://a", "api_key": "k1" }),
            ),
            (
                AppType::Claude,
                json!({ "env": { "ANTHROPIC_BASE_URL": "https://a", "ANTHROPIC_AUTH_TOKEN": "", "ANTHROPIC_API_KEY": "k1" } }),
            ),
        ];
        for (app, settings) in cases {
            assert_eq!(
                adapter(&app).usage_credentials(&settings),
                ("https://a".to_string(), "k1".to_string()),
                "{app:?}"
            );
            assert_eq!(
                adapter(&app).usage_credentials(&json!({})),
                (String::new(), String::new()),
                "{app:?}"
            );
        }
    }

    #[test]
    fn gemini_credentials_default_base_url() {
        let gemini = adapter(&AppType::Gemini);
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
        let grok = adapter(&AppType::GrokBuild);
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
        let gemini = adapter(&AppType::Gemini).takeover().unwrap();
        assert!(gemini.has_placeholder(&json!({ "env": { "GEMINI_API_KEY": token } })));
        assert!(!gemini.has_placeholder(&json!({ "env": { "GEMINI_API_KEY": "sk-real" } })));
        assert!(!gemini.has_placeholder(&json!({})));

        let claude = adapter(&AppType::Claude).takeover().unwrap();
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

        let grok = adapter(&AppType::GrokBuild).takeover().unwrap();
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
        let claude = adapter(&AppType::Claude);
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
            .common_snippet()
            .expect("claude common snippet");
        assert!(common.sync_from_live());
        assert!(!adapter(&AppType::Gemini)
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
        let claude = adapter(&AppType::Claude);
        let mut settings = json!({ "env": { "ANTHROPIC_SMALL_FAST_MODEL": "haiku" } });
        assert!(claude.normalize_settings(&mut settings));
        assert_eq!(settings["env"]["ANTHROPIC_DEFAULT_HAIKU_MODEL"], "haiku");
        assert!(settings["env"].get("ANTHROPIC_SMALL_FAST_MODEL").is_none());
        assert!(!claude.normalize_settings(&mut settings));

        let mut gemini_settings = json!({ "env": { "ANTHROPIC_SMALL_FAST_MODEL": "haiku" } });
        assert!(!adapter(&AppType::Gemini).normalize_settings(&mut gemini_settings));
    }

    #[test]
    fn claude_desktop_live_io_requires_switch_flow() {
        let desktop = adapter(&AppType::ClaudeDesktop);
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
