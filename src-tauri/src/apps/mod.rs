//! 应用元数据注册表与按应用划分的配置适配器
//!
//! 所有"按应用枚举"的场景（启动导入、代理接管恢复、托盘分区等）都应从
//! [`registry::APPS`] 按能力过滤，而不是各自维护一份手写列表；
//! 按应用分派的 live 配置读写走 [`adapter`] 返回的 [`AppAdapter`]。

pub mod adapter;
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
        AppType::Gemini => Some(&gemini::GeminiAdapter),
        AppType::GrokBuild => Some(&grok::GrokAdapter),
        AppType::Claude
        | AppType::ClaudeDesktop
        | AppType::Codex
        | AppType::OpenCode
        | AppType::OpenClaw
        | AppType::Hermes => None,
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
    fn only_gemini_and_grok_have_adapters() {
        let migrated: Vec<AppType> = APPS
            .iter()
            .map(|descriptor| descriptor.app.clone())
            .filter(|app| adapter(app).is_some())
            .collect();
        assert_eq!(migrated, vec![AppType::Gemini, AppType::GrokBuild]);
    }

    #[test]
    fn adapter_paths_match_legacy_layout() {
        let gemini = adapter(&AppType::Gemini).expect("gemini adapter");
        assert_eq!(gemini.config_dir(), crate::gemini_config::get_gemini_dir());
        assert_eq!(gemini.prompt_file(), gemini.config_dir().join("GEMINI.md"));
        assert_eq!(gemini.skills_dir().file_name().unwrap(), "skills");
        assert_eq!(
            gemini
                .session_source()
                .expect("gemini sessions")
                .session_roots(),
            vec![gemini.config_dir().join("tmp")]
        );

        let grok = adapter(&AppType::GrokBuild).expect("grok adapter");
        assert_eq!(grok.config_dir(), crate::grok_config::get_grok_config_dir());
        assert_eq!(grok.prompt_file(), grok.config_dir().join("AGENTS.md"));
        assert_eq!(grok.skills_dir().file_name().unwrap(), "skills");
        assert_eq!(
            grok.session_source()
                .expect("grok sessions")
                .session_roots(),
            vec![
                grok.config_dir().join("sessions"),
                grok.config_dir().join("archived_sessions"),
            ]
        );
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
}
