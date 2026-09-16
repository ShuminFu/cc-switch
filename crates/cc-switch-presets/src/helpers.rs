//! Helper functions that lived next to the TypeScript catalogs.

use std::sync::OnceLock;

use regex::Regex;
use serde_json::{json, Map, Value};

use crate::{gemini, meta, universal, GeminiPreset, OpenClawSuggestedDefaults, UniversalPreset};

/// `generateThirdPartyAuth` from codexProviderPresets.ts: the `auth.json`
/// for a third-party Codex provider.
pub fn generate_codex_third_party_auth(api_key: &str) -> Value {
    json!({ "OPENAI_API_KEY": api_key })
}

/// `generateThirdPartyConfig` from codexProviderPresets.ts: the
/// `config.toml` for a third-party Codex provider. `model_name` defaults to
/// `gpt-5.5`. Strings are quoted the way `JSON.stringify` does, which is
/// valid TOML for the values involved.
pub fn generate_codex_third_party_config(
    provider_name: &str,
    base_url: &str,
    model_name: Option<&str>,
) -> String {
    let quote = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| format!("\"{s}\""));
    format!(
        "model_provider = \"custom\"\nmodel = {}\nmodel_reasoning_effort = \"high\"\ndisable_response_storage = true\n\n[model_providers.custom]\nname = {}\nbase_url = {}\nwire_api = \"responses\"\nrequires_openai_auth = true",
        quote(model_name.unwrap_or("gpt-5.5")),
        quote(provider_name),
        quote(base_url)
    )
}

fn rebase_openclaw_model_ref(model_ref: &str, provider_key: &str) -> String {
    match model_ref.find('/') {
        None => format!("{provider_key}/{model_ref}"),
        Some(idx) => format!("{provider_key}{}", &model_ref[idx..]),
    }
}

/// `rebaseOpenClawSuggestedDefaults`: rewrites `<provider-key>/<model>` refs
/// to the key the user chose in the form. An empty key returns the input
/// unchanged.
pub fn rebase_openclaw_suggested_defaults(
    defaults: &OpenClawSuggestedDefaults,
    provider_key: &str,
) -> OpenClawSuggestedDefaults {
    let key = provider_key.trim();
    if key.is_empty() {
        return defaults.clone();
    }
    OpenClawSuggestedDefaults {
        model: defaults
            .model
            .as_ref()
            .map(|m| crate::OpenClawDefaultModel {
                primary: rebase_openclaw_model_ref(&m.primary, key),
                fallbacks: m.fallbacks.as_ref().map(|f| {
                    f.iter()
                        .map(|r| rebase_openclaw_model_ref(r, key))
                        .collect()
                }),
                extra: m.extra.clone(),
            }),
        model_catalog: defaults.model_catalog.as_ref().map(|catalog| {
            catalog
                .iter()
                .map(|(model_ref, entry)| {
                    (rebase_openclaw_model_ref(model_ref, key), entry.clone())
                })
                .collect()
        }),
    }
}

/// `isHermesReadOnlyProvider`: providers sourced from Hermes' `providers:`
/// dict are rendered read-only.
pub fn is_hermes_read_only_provider(settings_config: &Value) -> bool {
    let source = &meta().hermes_provider_source;
    settings_config
        .as_object()
        .and_then(|o| o.get(&source.field))
        .and_then(Value::as_str)
        == Some(source.dict.as_str())
}

/// `getPresetModelDefaults`: enrichment metadata for an OpenCode model by
/// npm package and model id.
pub fn get_opencode_preset_model_defaults(npm: &str, model_id: &str) -> Option<&'static Value> {
    meta()
        .opencode_preset_model_variants
        .get(npm)?
        .iter()
        .find(|m| m["id"] == model_id)
}

fn coding_plan_patterns() -> &'static [(String, Regex)] {
    static CELL: OnceLock<Vec<(String, Regex)>> = OnceLock::new();
    CELL.get_or_init(|| {
        meta()
            .coding_plan_providers
            .iter()
            .map(|p| {
                let prefix = if p.flags.contains('i') { "(?i)" } else { "" };
                let regex = Regex::new(&format!("{prefix}{}", p.pattern.replace("\\/", "/")))
                    .unwrap_or_else(|e| panic!("bad coding plan pattern {}: {e}", p.pattern));
                (p.id.clone(), regex)
            })
            .collect()
    })
}

/// `detectCodingPlanProvider`: the coding-plan provider id whose pattern
/// matches `base_url`, if any.
pub fn detect_coding_plan_provider(base_url: Option<&str>) -> Option<&'static str> {
    let url = base_url.filter(|u| !u.is_empty())?;
    coding_plan_patterns()
        .iter()
        .find(|(_, re)| re.is_match(url))
        .map(|(id, _)| id.as_str())
}

/// `getGeminiPresetByName`.
pub fn gemini_preset_by_name(name: &str) -> Option<&'static GeminiPreset> {
    gemini().iter().find(|p| p.common.name == name)
}

/// `getGeminiPresetByUrl`: case-insensitive substring match on `baseURL`.
pub fn gemini_preset_by_url(url: &str) -> Option<&'static GeminiPreset> {
    if url.is_empty() {
        return None;
    }
    let lower = url.to_lowercase();
    gemini().iter().find(|p| {
        p.base_url
            .as_deref()
            .map(|b| lower.contains(&b.to_lowercase()))
            .unwrap_or(false)
    })
}

/// `findPresetByType` from universalProviderPresets.ts.
pub fn universal_preset_by_type(provider_type: &str) -> Option<&'static UniversalPreset> {
    universal()
        .iter()
        .find(|p| p.provider_type == provider_type)
}

/// MCP presets with the `npx` command wrapped for the target platform, as
/// `createNpxCommand` in mcpPresets.ts did: on Windows `npx` runs through
/// `cmd /c`.
pub fn mcp_presets_for_platform(windows: bool) -> Vec<Value> {
    meta()
        .mcp_presets
        .iter()
        .map(|preset| {
            let mut preset = preset.clone();
            if windows {
                if let Some(server) = preset.get_mut("server").and_then(Value::as_object_mut) {
                    if server.get("command").and_then(Value::as_str) == Some("npx") {
                        let mut args = vec![Value::from("/c"), Value::from("npx")];
                        if let Some(existing) = server.get("args").and_then(Value::as_array) {
                            args.extend(existing.iter().cloned());
                        }
                        server.insert("command".into(), Value::from("cmd"));
                        server.insert("args".into(), Value::Array(args));
                    }
                }
            }
            preset
        })
        .collect()
}

/// `getMcpPresetWithDescription`: attaches the translated description and
/// the `enabled: false` default.
pub fn mcp_preset_with_description(preset: &Value, description: &str) -> Value {
    let mut out = preset.as_object().cloned().unwrap_or_else(Map::new);
    out.insert("enabled".into(), Value::Bool(false));
    out.insert("description".into(), Value::from(description));
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_third_party_builders_match_typescript_output() {
        let example = &meta().codex_third_party_example;
        assert_eq!(generate_codex_third_party_auth("sk-test"), example["auth"]);
        assert_eq!(
            generate_codex_third_party_config(
                "Example Provider",
                "https://api.example.com/v1",
                None
            ),
            example["config"].as_str().unwrap()
        );
        assert_eq!(
            generate_codex_third_party_config(
                "Example Provider",
                "https://api.example.com/v1",
                Some("gpt-5.5-mini")
            ),
            example["configWithModel"].as_str().unwrap()
        );
        assert!(
            generate_codex_third_party_config("A \"quoted\" name", "u", None)
                .contains("name = \"A \\\"quoted\\\" name\"")
        );
    }

    #[test]
    fn rebases_openclaw_defaults() {
        let defaults: OpenClawSuggestedDefaults = serde_json::from_value(json!({
            "model": { "primary": "xiaomi/mimo", "fallbacks": ["other/x", "bare"] },
            "modelCatalog": { "xiaomi/mimo": { "alias": "MiMo" }, "bare": {} }
        }))
        .unwrap();
        let rebased = rebase_openclaw_suggested_defaults(&defaults, " my-key ");
        let model = rebased.model.unwrap();
        assert_eq!(model.primary, "my-key/mimo");
        assert_eq!(model.fallbacks.unwrap(), vec!["my-key/x", "my-key/bare"]);
        let catalog = rebased.model_catalog.unwrap();
        assert!(catalog.contains_key("my-key/mimo") && catalog.contains_key("my-key/bare"));
        assert_eq!(
            rebase_openclaw_suggested_defaults(&defaults, "  "),
            defaults
        );
    }

    #[test]
    fn hermes_read_only_marker() {
        assert!(is_hermes_read_only_provider(
            &json!({ "_cc_source": "providers_dict" })
        ));
        assert!(!is_hermes_read_only_provider(
            &json!({ "_cc_source": "custom_providers" })
        ));
        assert!(!is_hermes_read_only_provider(&json!("nope")));
    }

    #[test]
    fn coding_plan_detection_matches_patterns() {
        assert_eq!(
            detect_coding_plan_provider(Some("https://api.kimi.com/coding/v1")),
            Some("kimi")
        );
        assert_eq!(
            detect_coding_plan_provider(Some("https://API.MINIMAX.io/anthropic")),
            Some("minimax")
        );
        assert_eq!(
            detect_coding_plan_provider(Some("https://open.bigmodel.cn/api/paas")),
            Some("zhipu")
        );
        assert_eq!(
            detect_coding_plan_provider(Some("https://zenmux.ai/api")),
            Some("zenmux")
        );
        assert_eq!(
            detect_coding_plan_provider(Some("https://ark.cn-beijing.volces.com/api/coding")),
            Some("volcengine")
        );
        assert_eq!(
            detect_coding_plan_provider(Some("https://api.anthropic.com")),
            None
        );
        assert_eq!(detect_coding_plan_provider(Some("")), None);
        assert_eq!(detect_coding_plan_provider(None), None);
    }

    #[test]
    fn lookups_and_platform_wrapping() {
        assert!(gemini_preset_by_name("SubRouter").is_some());
        assert_eq!(
            gemini_preset_by_url("HTTPS://SUBROUTER.AI/v1beta/models")
                .map(|p| p.common.name.as_str()),
            Some("SubRouter")
        );
        assert!(gemini_preset_by_url("").is_none());
        assert!(universal_preset_by_type(&universal()[0].provider_type).is_some());
        assert!(
            get_opencode_preset_model_defaults("@ai-sdk/anthropic", "definitely-not-a-model")
                .is_none()
        );
        let (npm, variants) = meta().opencode_preset_model_variants.iter().next().unwrap();
        let id = variants[0]["id"].as_str().unwrap();
        assert!(get_opencode_preset_model_defaults(npm, id).is_some());

        let posix = mcp_presets_for_platform(false);
        let windows = mcp_presets_for_platform(true);
        let npx = posix
            .iter()
            .position(|p| p["server"]["command"] == "npx")
            .unwrap();
        assert_eq!(windows[npx]["server"]["command"], "cmd");
        let args = windows[npx]["server"]["args"].as_array().unwrap();
        assert_eq!(args[0], "/c");
        assert_eq!(args[1], "npx");
        assert_eq!(
            &args[2..],
            posix[npx]["server"]["args"].as_array().unwrap().as_slice()
        );
        let described = mcp_preset_with_description(&posix[0], "desc");
        assert_eq!(described["enabled"], false);
        assert_eq!(described["description"], "desc");
    }
}
