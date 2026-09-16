//! Provider preset catalogs.
//!
//! The data lives in `data/*.json`, generated from the TypeScript catalogs in
//! `src/config/` by `pnpm presets:dump` (the TS files remain the source of
//! truth until the React UI is removed; CI fails when the JSON is stale). The
//! JSON is embedded at compile time and parsed once on first use, so this
//! crate works identically on the backend and in the wasm32 frontend.
//!
//! Besides the data, this crate ports the small helper functions that lived
//! next to the catalogs (`generateThirdPartyConfig`,
//! `rebaseOpenClawSuggestedDefaults`, `detectCodingPlanProvider`, ...).

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub mod helpers;

pub use helpers::*;

/// Fields shared by every preset catalog entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PresetCommon {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_key: Option<String>,
    #[serde(default)]
    pub website_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_official: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_partner: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prime_partner: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partner_promotion_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<PresetTheme>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint_candidates: Option<Vec<String>>,
}

impl PresetCommon {
    pub fn is_official(&self) -> bool {
        self.is_official.unwrap_or(false) || self.category.as_deref() == Some("official")
    }

    pub fn is_partner(&self) -> bool {
        self.is_partner.unwrap_or(false)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PresetTheme {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_color: Option<String>,
}

/// A placeholder the add-provider form asks the user to fill in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TemplateValueConfig {
    pub label: String,
    #[serde(default)]
    pub placeholder: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
    #[serde(default)]
    pub editor_value: String,
}

pub type TemplateValues = BTreeMap<String, TemplateValueConfig>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClaudePreset {
    #[serde(flatten)]
    pub common: PresetCommon,
    #[serde(default)]
    pub settings_config: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_values: Option<TemplateValues>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires_o_auth: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models_url: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CodexPreset {
    #[serde(flatten)]
    pub common: PresetCommon,
    /// Contents of `~/.codex/auth.json`.
    #[serde(default)]
    pub auth: Value,
    /// Contents of `~/.codex/config.toml`.
    #[serde(default)]
    pub config: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_custom_template: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_catalog: Option<Vec<Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_chat_reasoning: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_cache_routing: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GeminiPreset {
    #[serde(flatten)]
    pub common: PresetCommon,
    #[serde(default)]
    pub settings_config: Value,
    #[serde(rename = "baseURL", default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeDesktopRoutePreset {
    pub route_id: String,
    pub upstream_model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_override: Option<String>,
    #[serde(default)]
    pub supports1m: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeDesktopPreset {
    #[serde(flatten)]
    pub common: PresetCommon,
    #[serde(default)]
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_field: Option<String>,
    /// `"direct"` or `"proxy"`.
    #[serde(default)]
    pub mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_routes: Option<Vec<ClaudeDesktopRoutePreset>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires_o_auth: Option<bool>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodePreset {
    #[serde(flatten)]
    pub common: PresetCommon,
    #[serde(default)]
    pub settings_config: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_values: Option<TemplateValues>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_custom_template: Option<bool>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OpenClawDefaultModel {
    pub primary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallbacks: Option<Vec<String>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OpenClawSuggestedDefaults {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<OpenClawDefaultModel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_catalog: Option<BTreeMap<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OpenClawPreset {
    #[serde(flatten)]
    pub common: PresetCommon,
    #[serde(default)]
    pub settings_config: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_values: Option<TemplateValues>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_custom_template: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_defaults: Option<OpenClawSuggestedDefaults>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HermesPreset {
    #[serde(flatten)]
    pub common: PresetCommon,
    #[serde(default)]
    pub settings_config: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_values: Option<TemplateValues>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_defaults: Option<Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UniversalPreset {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub website_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub provider_type: String,
    #[serde(default)]
    pub default_apps: Value,
    #[serde(default)]
    pub default_models: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_custom_template: Option<bool>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `{ value, label }` / `{ value, labelKey }` option lists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LabeledOption {
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_key: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HermesProviderSource {
    pub field: String,
    pub custom_list: String,
    pub dict: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CodexTemplate {
    #[serde(default)]
    pub auth: Value,
    #[serde(default)]
    pub config: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CodingPlanProvider {
    pub id: String,
    pub label: String,
    /// JavaScript regex source; see [`detect_coding_plan_provider`].
    pub pattern: String,
    #[serde(default)]
    pub flags: String,
}

/// Everything that is not a per-app catalog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    pub claude_desktop_role_route_ids: BTreeMap<String, String>,
    pub opencode_npm_packages: Vec<LabeledOption>,
    pub opencode_preset_model_variants: BTreeMap<String, Vec<Value>>,
    pub openclaw_api_protocols: Vec<LabeledOption>,
    pub hermes_api_modes: Vec<LabeledOption>,
    pub hermes_default_api_mode: String,
    pub hermes_provider_source: HermesProviderSource,
    pub user_agent_presets: Vec<String>,
    pub codex_custom_template: CodexTemplate,
    #[serde(default)]
    pub codex_third_party_example: Value,
    pub coding_plan_providers: Vec<CodingPlanProvider>,
    /// MCP presets as dumped on a POSIX host (`npx ...`); use
    /// [`mcp_presets_for_platform`] to get the Windows form.
    pub mcp_presets: Vec<Value>,
}

macro_rules! catalog {
    ($fn_name:ident, $ty:ty, $file:literal) => {
        #[doc = concat!("Presets from `data/", $file, "`.")]
        pub fn $fn_name() -> &'static [$ty] {
            static CELL: OnceLock<Vec<$ty>> = OnceLock::new();
            CELL.get_or_init(|| {
                serde_json::from_str(include_str!(concat!("../data/", $file)))
                    .unwrap_or_else(|e| panic!(concat!("invalid ", $file, ": {}"), e))
            })
        }
    };
}

catalog!(claude, ClaudePreset, "claude.json");
catalog!(codex, CodexPreset, "codex.json");
catalog!(gemini, GeminiPreset, "gemini.json");
catalog!(claude_desktop, ClaudeDesktopPreset, "claude_desktop.json");
catalog!(opencode, OpenCodePreset, "opencode.json");
catalog!(openclaw, OpenClawPreset, "openclaw.json");
catalog!(hermes, HermesPreset, "hermes.json");
catalog!(universal, UniversalPreset, "universal.json");

/// Shared constants and option lists from `data/meta.json`.
pub fn meta() -> &'static Meta {
    static CELL: OnceLock<Meta> = OnceLock::new();
    CELL.get_or_init(|| {
        serde_json::from_str(include_str!("../data/meta.json"))
            .unwrap_or_else(|e| panic!("invalid meta.json: {e}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogs_parse_with_expected_sizes() {
        assert_eq!(claude().len(), 70);
        assert_eq!(codex().len(), 63);
        assert_eq!(gemini().len(), 20);
        assert_eq!(claude_desktop().len(), 67);
        assert_eq!(opencode().len(), 59);
        assert_eq!(openclaw().len(), 59);
        assert_eq!(hermes().len(), 60);
        assert_eq!(universal().len(), 2);
        let m = meta();
        assert_eq!(m.opencode_npm_packages.len(), 5);
        assert_eq!(m.openclaw_api_protocols.len(), 5);
        assert_eq!(m.hermes_api_modes.len(), 4);
        assert_eq!(m.user_agent_presets.len(), 5);
        assert_eq!(m.coding_plan_providers.len(), 6);
        assert_eq!(m.mcp_presets.len(), 5);
        assert_eq!(m.claude_desktop_role_route_ids.len(), 4);
        assert_eq!(m.hermes_provider_source.field, "_cc_source");
    }

    #[test]
    fn round_trip_preserves_every_field() {
        // Serializing the typed catalogs must reproduce the JSON exactly, so
        // no field is silently dropped by the typed layer.
        fn check<T: Serialize + for<'de> Deserialize<'de>>(items: &[T], raw: &str) {
            let expected: Value = serde_json::from_str(raw).unwrap();
            let actual = serde_json::to_value(items).unwrap();
            assert_eq!(actual, expected);
        }
        check(claude(), include_str!("../data/claude.json"));
        check(codex(), include_str!("../data/codex.json"));
        check(gemini(), include_str!("../data/gemini.json"));
        check(
            claude_desktop(),
            include_str!("../data/claude_desktop.json"),
        );
        check(opencode(), include_str!("../data/opencode.json"));
        check(openclaw(), include_str!("../data/openclaw.json"));
        check(hermes(), include_str!("../data/hermes.json"));
        check(universal(), include_str!("../data/universal.json"));
        let expected: Value = serde_json::from_str(include_str!("../data/meta.json")).unwrap();
        assert_eq!(serde_json::to_value(meta()).unwrap(), expected);
    }

    fn by_name<'a, T>(items: &'a [T], name: &str, get: impl Fn(&T) -> &str) -> &'a T {
        items
            .iter()
            .find(|p| get(p) == name)
            .unwrap_or_else(|| panic!("preset {name} missing"))
    }

    /// Port of tests/config/longcatProviderPresets.test.ts.
    #[test]
    fn longcat_uses_the_official_model_everywhere() {
        const MODEL: &str = "LongCat-2.0";
        let claude = by_name(claude(), "Longcat", |p| &p.common.name);
        let env = &claude.settings_config["env"];
        for key in [
            "ANTHROPIC_MODEL",
            "ANTHROPIC_SMALL_FAST_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
        ] {
            assert_eq!(env[key], MODEL, "{key}");
        }
        assert_eq!(env["CLAUDE_CODE_MAX_OUTPUT_TOKENS"], "131072");

        let desktop = by_name(claude_desktop(), "Longcat", |p| &p.common.name);
        let routes = desktop.model_routes.as_ref().unwrap();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].upstream_model, MODEL);
        assert_eq!(routes[0].label_override.as_deref(), Some(MODEL));

        let hermes = by_name(hermes(), "Longcat", |p| &p.common.name);
        assert_eq!(
            hermes.settings_config["models"],
            serde_json::json!([{ "id": MODEL, "name": "LongCat 2.0" }])
        );
        assert_eq!(
            hermes.suggested_defaults.as_ref().unwrap()["model"],
            serde_json::json!({ "default": MODEL, "provider": "longcat" })
        );

        let opencode = by_name(opencode(), "Longcat", |p| &p.common.name);
        assert_eq!(
            opencode.settings_config["options"]["baseURL"],
            "https://api.longcat.chat/openai/v1"
        );
        let openclaw = by_name(openclaw(), "Longcat", |p| &p.common.name);
        assert!(openclaw.settings_config["models"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["id"] == MODEL));
        let codex = by_name(codex(), "Longcat", |p| &p.common.name);
        assert!(codex.config.contains(&format!("model = \"{MODEL}\"")));
    }

    /// Port of tests/config/subrouterProviderPresets.test.ts.
    #[test]
    fn subrouter_endpoints_per_app() {
        let claude = by_name(claude(), "SubRouter", |p| &p.common.name);
        assert_eq!(claude.common.website_url, "https://subrouter.ai");
        assert_eq!(
            claude.common.api_key_url.as_deref(),
            Some("https://subrouter.ai/register?aff=l3ri")
        );
        assert_eq!(claude.common.category.as_deref(), Some("aggregator"));
        assert!(claude.common.is_partner());
        assert_eq!(
            claude.common.partner_promotion_key.as_deref(),
            Some("subrouter")
        );
        assert_eq!(claude.common.icon.as_deref(), Some("subrouter"));
        assert_eq!(
            claude.settings_config["env"]["ANTHROPIC_BASE_URL"],
            "https://subrouter.ai"
        );
        assert_eq!(claude.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"], "");

        let codex = by_name(codex(), "SubRouter", |p| &p.common.name);
        assert_eq!(
            codex.common.endpoint_candidates.as_deref(),
            Some(&["https://subrouter.ai/v1".to_string()][..])
        );
        assert_eq!(codex.auth, serde_json::json!({ "OPENAI_API_KEY": "" }));
        for needle in [
            "name = \"subrouter\"",
            "model = \"gpt-5.5\"",
            "base_url = \"https://subrouter.ai/v1\"",
            "wire_api = \"responses\"",
        ] {
            assert!(codex.config.contains(needle), "{needle}");
        }

        let gemini = by_name(gemini(), "SubRouter", |p| &p.common.name);
        assert_eq!(
            gemini.base_url.as_deref(),
            Some("https://subrouter.ai/v1beta")
        );
        assert_eq!(gemini.model.as_deref(), Some("gemini-3.5-flash"));
        assert_eq!(
            gemini.settings_config["env"]["GOOGLE_GEMINI_BASE_URL"],
            "https://subrouter.ai/v1beta"
        );

        let opencode = by_name(opencode(), "SubRouter", |p| &p.common.name);
        assert_eq!(opencode.settings_config["npm"], "@ai-sdk/openai-compatible");
        assert_eq!(
            opencode.settings_config["options"]["baseURL"],
            "https://subrouter.ai/v1"
        );
        assert!(opencode.settings_config["models"].get("gpt-5.5").is_some());

        let openclaw = by_name(openclaw(), "SubRouter", |p| &p.common.name);
        assert_eq!(
            openclaw.settings_config["baseUrl"],
            "https://subrouter.ai/v1"
        );
        assert_eq!(openclaw.settings_config["api"], "openai-completions");
        let model = &openclaw.settings_config["models"][0];
        assert_eq!(model["id"], "gpt-5.5");
        assert_eq!(model["contextWindow"], 400000);
        assert!(model.get("cost").is_none());
        assert_eq!(
            openclaw
                .suggested_defaults
                .as_ref()
                .unwrap()
                .model
                .as_ref()
                .unwrap()
                .primary,
            "subrouter/gpt-5.5"
        );

        let hermes = by_name(hermes(), "SubRouter", |p| &p.common.name);
        assert_eq!(
            hermes.settings_config["base_url"],
            "https://subrouter.ai/v1"
        );
        assert_eq!(hermes.settings_config["api_mode"], "chat_completions");

        let desktop = by_name(claude_desktop(), "SubRouter", |p| &p.common.name);
        assert_eq!(desktop.base_url, "https://subrouter.ai");
        assert_eq!(desktop.mode, "direct");
        assert_eq!(desktop.api_format.as_deref(), Some("anthropic"));
        assert!(!desktop.model_routes.as_ref().unwrap().is_empty());
    }
}
