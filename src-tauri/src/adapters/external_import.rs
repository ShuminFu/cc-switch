//! Import providers from adjacent tools' configs (PRODUCT-26, first increment).
//!
//! Detectors read another tool's config file and map it to cc-switch
//! providers plus PRODUCT-13 model routing rules, as a dry-run preview. The
//! first supported source is claude-code-router
//! (`~/.claude-code-router/config.json`):
//!
//! - every `Providers[]` entry becomes a Claude provider that talks to the
//!   relay's OpenAI Chat Completions endpoint through the proxy
//!   (`meta.apiFormat = "openai_chat"`, or `anthropic` when the entry uses
//!   ccr's `anthropic` transformer), with `ANTHROPIC_MODEL` set to the
//!   entry's first model;
//! - `Router.default` becomes a catch-all `*` rule and `Router.background`
//!   a `*haiku*` rule (Claude Code sends background work to the Haiku-class
//!   model). `think`, `longContext` and `webSearch` select by request
//!   features, not by model name, and are reported as warnings.
//!
//! API keys stay in the preview's `settings_config` because importing needs
//! them; the preview never logs them.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{json, Map, Value};

use super::credential_fingerprint;
use crate::error::AppError;
use crate::proxy::routing::ModelRoutingRule;

/// Source id of claude-code-router.
pub const CLAUDE_CODE_ROUTER: &str = "claude-code-router";

/// One provider a source would create.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExternalProviderCandidate {
    /// Provider id to create (stable for the same source entry).
    pub id_hint: String,
    pub name: String,
    /// Target app (`claude` for claude-code-router).
    pub app: String,
    pub settings_config: Value,
    /// `meta.apiFormat` to store.
    pub api_format: String,
    pub base_url: String,
    pub models: Vec<String>,
    /// Endpoint+key hash for de-duplication; never contains the key.
    pub fingerprint: String,
    /// Whether the entry carries an API key.
    pub has_key: bool,
}

/// What importing one source would do.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExternalImportPreview {
    pub source: String,
    pub path: String,
    pub providers: Vec<ExternalProviderCandidate>,
    /// Routing rules for the target app (provider ids refer to `id_hint`s).
    pub routing_rules: Vec<ModelRoutingRule>,
    /// English notes about entries that could not be mapped.
    pub warnings: Vec<String>,
}

/// `~/.claude-code-router/config.json`.
pub fn claude_code_router_path() -> PathBuf {
    crate::config::get_home_dir()
        .join(".claude-code-router")
        .join("config.json")
}

fn slug(value: &str) -> String {
    let mut out = String::new();
    for c in value.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// Base URL cc-switch should store for a ccr `api_base_url` (the proxy
/// appends `/v1/chat/completions` or `/v1/messages`).
fn base_url_for(api_base_url: &str) -> String {
    let trimmed = api_base_url.trim().trim_end_matches('/');
    for suffix in ["/chat/completions", "/messages"] {
        if let Some(stripped) = trimmed.strip_suffix(suffix) {
            return stripped.to_string();
        }
    }
    trimmed.to_string()
}

/// ccr `transformer.use` names an `anthropic` transformer for native relays.
fn uses_anthropic_transformer(entry: &Map<String, Value>) -> bool {
    let Some(transformer) = entry.get("transformer") else {
        return false;
    };
    let names: Vec<&str> = match transformer.get("use") {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|i| i.as_str().or_else(|| i.get(0).and_then(Value::as_str)))
            .collect(),
        _ => Vec::new(),
    };
    names.iter().any(|n| n.eq_ignore_ascii_case("anthropic"))
}

/// Map a claude-code-router config (already parsed) to a preview.
pub fn preview_claude_code_router(config: &Value, path: &Path) -> ExternalImportPreview {
    let mut preview = ExternalImportPreview {
        source: CLAUDE_CODE_ROUTER.to_string(),
        path: path.display().to_string(),
        providers: Vec::new(),
        routing_rules: Vec::new(),
        warnings: Vec::new(),
    };
    let entries = config
        .get("Providers")
        .or_else(|| config.get("providers"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for (index, entry) in entries.iter().enumerate() {
        let Some(entry) = entry.as_object() else {
            continue;
        };
        let name = entry
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        let api_base_url = entry
            .get("api_base_url")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if name.is_empty() || api_base_url.trim().is_empty() {
            preview.warnings.push(format!(
                "Provider #{} has no name or api_base_url and was skipped.",
                index + 1
            ));
            continue;
        }
        let key = entry
            .get("api_key")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        let models: Vec<String> = entry
            .get("models")
            .and_then(Value::as_array)
            .map(|m| {
                m.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let base_url = base_url_for(api_base_url);
        let api_format = if uses_anthropic_transformer(entry) {
            "anthropic"
        } else {
            "openai_chat"
        };
        let mut env = Map::new();
        env.insert("ANTHROPIC_BASE_URL".into(), json!(base_url));
        env.insert("ANTHROPIC_AUTH_TOKEN".into(), json!(key));
        if let Some(model) = models.first() {
            env.insert("ANTHROPIC_MODEL".into(), json!(model));
        }
        if key.starts_with('$') {
            preview.warnings.push(format!(
                "Provider \"{name}\" reads its key from the environment ({key}); fill it in after import."
            ));
        }
        preview.providers.push(ExternalProviderCandidate {
            id_hint: format!("ccr-{}", slug(&name)),
            name,
            app: "claude".to_string(),
            settings_config: json!({ "env": env }),
            api_format: api_format.to_string(),
            fingerprint: credential_fingerprint(&base_url, &key),
            has_key: !key.is_empty(),
            base_url,
            models,
        });
    }

    let router = config.get("Router").and_then(Value::as_object);
    let provider_id_for = |route: &str| -> Option<String> {
        let provider = route.split(',').next()?.trim();
        preview
            .providers
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(provider))
            .map(|p| p.id_hint.clone())
    };
    let mut rules = Vec::new();
    let mut warnings = Vec::new();
    if let Some(router) = router {
        // Most specific first: the catch-all goes last.
        for (scenario, pattern) in [("background", "*haiku*"), ("default", "*")] {
            let Some(route) = router.get(scenario).and_then(Value::as_str) else {
                continue;
            };
            match provider_id_for(route) {
                Some(id) => rules.push(ModelRoutingRule {
                    name: format!("ccr {scenario}"),
                    pattern: pattern.to_string(),
                    providers: vec![id],
                    strategy: None,
                    exclusive: false,
                }),
                None => warnings.push(format!(
                    "Router.{scenario} names a provider that is not in Providers: {route}"
                )),
            }
        }
        for scenario in ["think", "longContext", "webSearch", "image"] {
            if router
                .get(scenario)
                .and_then(Value::as_str)
                .is_some_and(|r| !r.trim().is_empty())
            {
                warnings.push(format!(
                    "Router.{scenario} selects by request type, which cc-switch routing rules cannot express yet; it was not imported."
                ));
            }
        }
    }
    preview.routing_rules = rules;
    preview.warnings.extend(warnings);
    preview
}

/// Previews for every detected source (missing files are skipped; a file
/// that does not parse is reported as a warning-only preview).
pub fn scan() -> Vec<ExternalImportPreview> {
    let mut out = Vec::new();
    let path = claude_code_router_path();
    if let Ok(text) = std::fs::read_to_string(&path) {
        match json_five::from_str::<Value>(&text) {
            Ok(config) => out.push(preview_claude_code_router(&config, &path)),
            Err(e) => out.push(ExternalImportPreview {
                source: CLAUDE_CODE_ROUTER.to_string(),
                path: path.display().to_string(),
                providers: Vec::new(),
                routing_rules: Vec::new(),
                warnings: vec![format!("The config could not be parsed: {e}")],
            }),
        }
    }
    out
}

/// The preview for one source, or an error when it is not present.
pub fn preview_for(source: &str) -> Result<ExternalImportPreview, AppError> {
    scan()
        .into_iter()
        .find(|p| p.source == source)
        .ok_or_else(|| {
            AppError::localized(
                "import.external.notFound",
                format!("未找到可导入的配置: {source}"),
                format!("No importable config found for {source}"),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ccr_config() -> Value {
        json!({
            "LOG": true,
            "Providers": [
                {"name": "openrouter", "api_base_url": "https://openrouter.ai/api/v1/chat/completions",
                 "api_key": "sk-or-1", "models": ["anthropic/claude-sonnet-4", "google/gemini-2.5-pro"]},
                {"name": "deepseek", "api_base_url": "https://api.deepseek.com/chat/completions",
                 "api_key": "sk-ds-2", "models": ["deepseek-chat", "deepseek-reasoner"],
                 "transformer": {"use": ["deepseek"]}},
                {"name": "ollama", "api_base_url": "http://localhost:11434/v1/chat/completions",
                 "api_key": "ollama", "models": ["qwen2.5-coder:latest"]},
                {"name": "Native Relay", "api_base_url": "https://relay.example/v1/messages",
                 "api_key": "sk-native-4", "models": ["claude-opus-4-8"],
                 "transformer": {"use": ["Anthropic"]}}
            ],
            "Router": {
                "default": "deepseek,deepseek-chat",
                "background": "ollama,qwen2.5-coder:latest",
                "think": "deepseek,deepseek-reasoner",
                "longContext": "openrouter,google/gemini-2.5-pro"
            }
        })
    }

    #[test]
    fn claude_code_router_providers_map_with_keys_intact() {
        let p = preview_claude_code_router(&ccr_config(), Path::new("/x/config.json"));
        assert_eq!(p.providers.len(), 4);
        let or = &p.providers[0];
        assert_eq!(or.id_hint, "ccr-openrouter");
        assert_eq!(or.base_url, "https://openrouter.ai/api/v1");
        assert_eq!(or.api_format, "openai_chat");
        assert_eq!(or.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-or-1");
        assert_eq!(
            or.settings_config["env"]["ANTHROPIC_MODEL"],
            "anthropic/claude-sonnet-4"
        );
        assert!(!or.fingerprint.contains("sk-or-1"));
        assert_eq!(p.providers[1].base_url, "https://api.deepseek.com");
        let native = &p.providers[3];
        assert_eq!(native.id_hint, "ccr-native-relay");
        assert_eq!(native.api_format, "anthropic");
        assert_eq!(native.base_url, "https://relay.example/v1");
    }

    #[test]
    fn claude_code_router_routes_become_rules_or_warnings() {
        let p = preview_claude_code_router(&ccr_config(), Path::new("/x/config.json"));
        let rules: Vec<(&str, &str)> = p
            .routing_rules
            .iter()
            .map(|r| (r.pattern.as_str(), r.providers[0].as_str()))
            .collect();
        assert_eq!(rules, [("*haiku*", "ccr-ollama"), ("*", "ccr-deepseek")]);
        assert_eq!(p.warnings.len(), 2, "{:?}", p.warnings);
        assert!(p.warnings.iter().any(|w| w.contains("Router.think")));
        assert!(p.warnings.iter().all(|w| !w.contains("sk-")));
    }

    #[test]
    fn unknown_router_provider_and_bad_entries_warn() {
        let config = json!({
            "Providers": [{"name": "a"}, {"name": "b", "api_base_url": "https://b/v1", "api_key": "$B_KEY"}],
            "Router": {"default": "nope,model"}
        });
        let p = preview_claude_code_router(&config, Path::new("c"));
        assert_eq!(p.providers.len(), 1);
        assert!(p.routing_rules.is_empty());
        assert_eq!(p.warnings.len(), 3, "{:?}", p.warnings);
    }

    #[test]
    fn scan_reads_the_ccr_file_from_home() {
        let home = crate::test_support::isolated_home();
        assert!(scan().is_empty());
        let dir = home.path().join(".claude-code-router");
        std::fs::create_dir_all(&dir).unwrap();
        // ccr configs may carry comments and trailing commas.
        std::fs::write(
            dir.join("config.json"),
            "{ // comment\n \"Providers\": [{\"name\": \"x\", \"api_base_url\": \"https://x/v1/chat/completions\", \"api_key\": \"k\", \"models\": [\"m\"],}],\n}",
        )
        .unwrap();
        let previews = scan();
        assert_eq!(previews.len(), 1);
        assert_eq!(previews[0].providers[0].id_hint, "ccr-x");
        assert!(preview_for("nope").is_err());
    }
}
