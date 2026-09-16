//! Port of `src/utils/grokBuildConfig.ts`.
//!
//! Whole-document parse / update / validate helpers for the Grok Build
//! `config.toml`. Like the TypeScript original, `update_grok_build_config`
//! re-serialises the entire document, so comments and hand-written key order
//! are lost — acceptable because this file is tool-owned.

use serde::{Deserialize, Serialize};
use toml::Value;

use crate::toml_text::parse_toml_table;

/// `GROK_BUILD_DEFAULT_MODEL` (`grokBuildConfig.ts`).
pub const GROK_BUILD_DEFAULT_MODEL: &str = "grok-4.5";
/// `GROK_BUILD_DEFAULT_API_BACKEND` (`grokBuildConfig.ts`).
pub const GROK_BUILD_DEFAULT_API_BACKEND: &str = "responses";
/// `GROK_BUILD_DEFAULT_CONTEXT_WINDOW` (`grokBuildConfig.ts`).
pub const GROK_BUILD_DEFAULT_CONTEXT_WINDOW: i64 = 500_000;

/// Port of the `GrokBuildConfigValues` interface (`grokBuildConfig.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrokBuildConfigValues {
    /// Client-visible profile selected by `[models].default`.
    pub model: String,
    /// Real model sent to the upstream provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_model: Option<String>,
    pub base_url: String,
    pub name: String,
    pub api_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env_key: Option<String>,
    pub api_backend: String,
    pub context_window: i64,
}

/// `asRecord` (`grokBuildConfig.ts`): only tables count as records.
fn as_record(value: Option<&Value>) -> Option<&toml::Table> {
    match value {
        Some(Value::Table(table)) => Some(table),
        _ => None,
    }
}

/// `asString` (`grokBuildConfig.ts`).
fn as_string<'a>(value: Option<&'a Value>, fallback: &'a str) -> &'a str {
    match value {
        Some(Value::String(s)) => s.as_str(),
        _ => fallback,
    }
}

/// `typeof x === "number" && Number.isInteger(x) && x > 0` — TOML integers
/// and integral floats both qualify, as they do once smol-toml hands
/// JavaScript a `number`.
fn positive_integer(value: Option<&Value>) -> Option<i64> {
    match value {
        Some(Value::Integer(i)) if *i > 0 => Some(*i),
        Some(Value::Float(f)) if f.is_finite() && *f > 0.0 && f.fract() == 0.0 => {
            i64::try_from(*f as i128).ok()
        }
        _ => None,
    }
}

fn fallback_values(fallback_name: &str) -> GrokBuildConfigValues {
    GrokBuildConfigValues {
        model: GROK_BUILD_DEFAULT_MODEL.to_string(),
        upstream_model: Some(GROK_BUILD_DEFAULT_MODEL.to_string()),
        base_url: String::new(),
        name: fallback_name.to_string(),
        api_key: String::new(),
        env_key: None,
        api_backend: GROK_BUILD_DEFAULT_API_BACKEND.to_string(),
        context_window: GROK_BUILD_DEFAULT_CONTEXT_WINDOW,
    }
}

/// Port of `parseGrokBuildConfig` (`grokBuildConfig.ts`).
///
/// Blank input or a parse failure yields the fallback object (`model` /
/// `upstream_model` = `grok-4.5`, empty `base_url` / `api_key`, `name` =
/// `fallback_name`, `api_backend` = `responses`, `context_window` = 500000, no
/// `env_key`). Otherwise `[models].default` selects the profile (defaulting to
/// `grok-4.5`) and the fields are read from `[model."<profile>"]`; a missing
/// profile table yields empty strings and defaults, not an error. Note that
/// a parsed document always reports `env_key` as `Some` (possibly empty).
pub fn parse_grok_build_config(
    config_toml: Option<&str>,
    fallback_name: &str,
) -> GrokBuildConfigValues {
    let fallback = fallback_values(fallback_name);
    let Some(config_toml) = config_toml.filter(|text| !text.trim().is_empty()) else {
        return fallback;
    };

    let Ok(root) = parse_toml_table(config_toml) else {
        return fallback;
    };

    let models = as_record(root.get("models"));
    let default_model = as_string(
        models.and_then(|m| m.get("default")),
        GROK_BUILD_DEFAULT_MODEL,
    );
    let model_tables = as_record(root.get("model"));
    let selected = as_record(model_tables.and_then(|m| m.get(default_model)));
    let field = |key: &str| selected.and_then(|s| s.get(key));

    GrokBuildConfigValues {
        model: default_model.to_string(),
        upstream_model: Some(as_string(field("model"), default_model).to_string()),
        base_url: as_string(field("base_url"), "").to_string(),
        name: as_string(field("name"), fallback_name).to_string(),
        api_key: as_string(field("api_key"), "").to_string(),
        env_key: Some(as_string(field("env_key"), "").to_string()),
        api_backend: as_string(field("api_backend"), GROK_BUILD_DEFAULT_API_BACKEND).to_string(),
        context_window: positive_integer(field("context_window"))
            .unwrap_or(GROK_BUILD_DEFAULT_CONTEXT_WINDOW),
    }
}

/// Port of `buildGrokBuildConfig` (`grokBuildConfig.ts`): generate from scratch.
pub fn build_grok_build_config(values: &GrokBuildConfigValues) -> String {
    update_grok_build_config(None, values)
}

/// Port of `updateGrokBuildConfig` (`grokBuildConfig.ts`).
///
/// Profile = `values.model.trim()` or `grok-4.5`; upstream = trimmed
/// `upstream_model` or the profile. The existing TOML is parsed (blank or
/// invalid → `{}` silently). `[models].default` is set to the profile while
/// other `[models]` keys are kept. The edited table is `model[profile]`,
/// falling back to the table of the *previous* profile so a rename carries the
/// old fields over (the old table is then deleted). `api_key` is written when
/// non-empty after trimming and deleted otherwise; `env_key` is the trimmed
/// `values.env_key`, else the existing `env_key`, else deleted. Output is the
/// full re-serialisation, trimmed, plus a trailing newline.
pub fn update_grok_build_config(
    config_toml: Option<&str>,
    values: &GrokBuildConfigValues,
) -> String {
    let profile = match values.model.trim() {
        "" => GROK_BUILD_DEFAULT_MODEL,
        trimmed => trimmed,
    }
    .to_string();
    let upstream_model = values
        .upstream_model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(profile.as_str())
        .to_string();

    let mut config = config_toml
        .filter(|text| !text.trim().is_empty())
        .and_then(|text| parse_toml_table(text).ok())
        .unwrap_or_default();

    let mut models = as_record(config.get("models")).cloned().unwrap_or_default();
    let previous_profile = as_string(models.get("default"), &profile).to_string();
    models.insert("default".to_string(), Value::String(profile.clone()));
    config.insert("models".to_string(), Value::Table(models));

    let model_tables = as_record(config.get("model")).cloned().unwrap_or_default();
    let existing_selected = as_record(model_tables.get(&profile))
        .or_else(|| as_record(model_tables.get(&previous_profile)))
        .cloned()
        .unwrap_or_default();

    let api_key = values.api_key.trim();
    let env_key = values
        .env_key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| as_string(existing_selected.get("env_key"), "").trim())
        .to_string();

    let mut updated_selected = existing_selected;
    updated_selected.insert("model".to_string(), Value::String(upstream_model));
    updated_selected.insert(
        "base_url".to_string(),
        Value::String(values.base_url.trim().to_string()),
    );
    updated_selected.insert(
        "name".to_string(),
        Value::String(values.name.trim().to_string()),
    );
    updated_selected.insert(
        "api_backend".to_string(),
        Value::String(
            match values.api_backend.trim() {
                "" => GROK_BUILD_DEFAULT_API_BACKEND,
                trimmed => trimmed,
            }
            .to_string(),
        ),
    );
    updated_selected.insert(
        "context_window".to_string(),
        Value::Integer(if values.context_window > 0 {
            values.context_window
        } else {
            GROK_BUILD_DEFAULT_CONTEXT_WINDOW
        }),
    );
    if api_key.is_empty() {
        updated_selected.remove("api_key");
    } else {
        updated_selected.insert("api_key".to_string(), Value::String(api_key.to_string()));
    }
    if env_key.is_empty() {
        updated_selected.remove("env_key");
    } else {
        updated_selected.insert("env_key".to_string(), Value::String(env_key));
    }

    let mut model = model_tables;
    let had_previous = model.contains_key(&previous_profile);
    model.insert(profile.clone(), Value::Table(updated_selected));
    if previous_profile != profile && had_previous {
        model.remove(&previous_profile);
    }
    config.insert("model".to_string(), Value::Table(model));

    format!("{}\n", toml::to_string(&config).unwrap_or_default().trim())
}

/// Port of `validateGrokBuildConfig` (`grokBuildConfig.ts`).
///
/// Returns `None` when valid, else an English message, checked in this order:
/// blank → `config.toml must not be empty`; parse error → the parser message;
/// missing profile or `[model."<profile>"]` → `Missing [models] default model
/// table`; then `model`, `base_url`, `name`, `api_backend` → `Missing <field>`;
/// then neither `api_key` nor `env_key` → `Missing api_key or env_key`; then a
/// non-positive-integer `context_window`.
pub fn validate_grok_build_config(config_toml: &str) -> Option<String> {
    if config_toml.trim().is_empty() {
        return Some("config.toml must not be empty".to_string());
    }
    let root = match parse_toml_table(config_toml) {
        Ok(root) => root,
        Err(error) => {
            return Some(if error.report.is_empty() {
                "Invalid TOML".to_string()
            } else {
                error.report
            });
        }
    };

    let models = as_record(root.get("models"));
    let profile = as_string(models.and_then(|m| m.get("default")), "").trim();
    let selected = as_record(as_record(root.get("model")).and_then(|m| m.get(profile)));
    let (Some(selected), false) = (selected, profile.is_empty()) else {
        return Some("Missing [models] default model table".to_string());
    };

    for field in ["model", "base_url", "name", "api_backend"] {
        if as_string(selected.get(field), "").trim().is_empty() {
            return Some(format!("Missing {field}"));
        }
    }

    if as_string(selected.get("api_key"), "").trim().is_empty()
        && as_string(selected.get("env_key"), "").trim().is_empty()
    {
        return Some("Missing api_key or env_key".to_string());
    }

    if positive_integer(selected.get("context_window")).is_none() {
        return Some("context_window must be a positive integer".to_string());
    }

    None
}

/// Port of `extractGrokBuildBaseUrl` (`grokBuildConfig.ts`): the parsed
/// `base_url`, `""` (never `None`) when absent or invalid.
pub fn extract_grok_build_base_url(config_toml: &str) -> String {
    parse_grok_build_config(Some(config_toml), "").base_url
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> toml::Table {
        toml::from_str(text).expect("generated TOML parses")
    }

    fn values(model: &str, api_key: &str) -> GrokBuildConfigValues {
        GrokBuildConfigValues {
            model: model.to_string(),
            upstream_model: None,
            base_url: "https://relay.example.com/v1".to_string(),
            name: "Relay \"A\"".to_string(),
            api_key: api_key.to_string(),
            env_key: None,
            api_backend: "responses".to_string(),
            context_window: 500_000,
        }
    }

    #[test]
    fn builds_the_expected_provider_toml() {
        let config = build_grok_build_config(&values("grok-4.5", "secret"));
        let parsed = parse(&config);

        assert_eq!(parsed["models"]["default"].as_str(), Some("grok-4.5"));
        let mut expected = toml::Table::new();
        expected.insert("model".into(), Value::String("grok-4.5".into()));
        expected.insert(
            "base_url".into(),
            Value::String("https://relay.example.com/v1".into()),
        );
        expected.insert("name".into(), Value::String("Relay \"A\"".into()));
        expected.insert("api_key".into(), Value::String("secret".into()));
        expected.insert("api_backend".into(), Value::String("responses".into()));
        expected.insert("context_window".into(), Value::Integer(500_000));
        assert_eq!(parsed["model"]["grok-4.5"], Value::Table(expected));
        assert!(config.contains("[model.\"grok-4.5\"]"), "{config}");
        assert!(config.ends_with('\n'));
        assert!(!config.ends_with("\n\n"));
    }

    #[test]
    fn reads_values_back_from_a_generated_config() {
        let input = GrokBuildConfigValues {
            model: "custom-model".to_string(),
            upstream_model: Some("upstream-model".to_string()),
            base_url: "https://api.example.com".to_string(),
            name: "Custom".to_string(),
            api_key: "key".to_string(),
            env_key: Some(String::new()),
            api_backend: "responses".to_string(),
            context_window: 320_000,
        };
        let config = build_grok_build_config(&input);

        assert_eq!(parse_grok_build_config(Some(&config), ""), input);
        assert_eq!(
            extract_grok_build_base_url(&config),
            "https://api.example.com"
        );
    }

    const ENV_KEY_CONFIG: &str = "[models]\ndefault = \"env-profile\"\n\n[model.\"env-profile\"]\nmodel = \"grok-4.5\"\nbase_url = \"https://api.example.com/v1\"\nname = \"Env Relay\"\nenv_key = \"XAI_API_KEY\"\napi_backend = \"responses\"\ncontext_window = 500000\n";

    #[test]
    fn accepts_env_key_credentials_without_adding_an_empty_api_key() {
        assert_eq!(validate_grok_build_config(ENV_KEY_CONFIG), None);
        assert_eq!(
            parse_grok_build_config(Some(ENV_KEY_CONFIG), "")
                .env_key
                .as_deref(),
            Some("XAI_API_KEY")
        );

        let mut updated_values = parse_grok_build_config(Some(ENV_KEY_CONFIG), "");
        updated_values.base_url = "https://updated.example.com/v1".to_string();
        let updated = update_grok_build_config(Some(ENV_KEY_CONFIG), &updated_values);
        let parsed = parse(&updated);
        let table = parsed["model"]["env-profile"].as_table().unwrap();
        assert_eq!(table["env_key"].as_str(), Some("XAI_API_KEY"));
        assert!(!table.contains_key("api_key"));
        assert_eq!(
            table["base_url"].as_str(),
            Some("https://updated.example.com/v1")
        );
    }

    #[test]
    fn reports_malformed_incomplete_and_invalid_window_configs() {
        assert_eq!(
            validate_grok_build_config("").as_deref(),
            Some("config.toml must not be empty")
        );
        assert!(validate_grok_build_config("[models").is_some());
        assert_eq!(
            validate_grok_build_config("[models]\ndefault = \"missing\"\n").as_deref(),
            Some("Missing [models] default model table")
        );

        let mut missing = values("grok-4.5", "");
        missing.base_url = "https://api.example.com/v1".to_string();
        missing.name = "Relay".to_string();
        let missing_credentials = build_grok_build_config(&missing);
        assert_eq!(
            validate_grok_build_config(&missing_credentials).as_deref(),
            Some("Missing api_key or env_key")
        );

        let invalid_window =
            missing_credentials.replace("context_window = 500000", "context_window = 0");
        assert_eq!(
            validate_grok_build_config(&invalid_window).as_deref(),
            Some("Missing api_key or env_key")
        );
        assert_eq!(
            validate_grok_build_config(
                &invalid_window
                    .replace("name = \"Relay\"", "name = \"Relay\"\napi_key = \"secret\"")
            )
            .as_deref(),
            Some("context_window must be a positive integer")
        );
    }

    #[test]
    fn reports_each_missing_field_in_order() {
        let base = "[models]\ndefault = \"p\"\n\n[model.p]\n";
        assert_eq!(
            validate_grok_build_config(base).as_deref(),
            Some("Missing model")
        );
        let with_model = format!("{base}model = \"m\"\n");
        assert_eq!(
            validate_grok_build_config(&with_model).as_deref(),
            Some("Missing base_url")
        );
        let with_url = format!("{with_model}base_url = \"u\"\n");
        assert_eq!(
            validate_grok_build_config(&with_url).as_deref(),
            Some("Missing name")
        );
        let with_name = format!("{with_url}name = \"  \"\n");
        assert_eq!(
            validate_grok_build_config(&with_name).as_deref(),
            Some("Missing name")
        );
        let with_real_name = format!("{with_url}name = \"n\"\n");
        assert_eq!(
            validate_grok_build_config(&with_real_name).as_deref(),
            Some("Missing api_backend")
        );
        assert_eq!(
            validate_grok_build_config("[models]\ndefault = \"\"\n\n[model.\"\"]\nmodel = \"m\"\n")
                .as_deref(),
            Some("Missing [models] default model table")
        );
    }

    #[test]
    fn renames_the_selected_profile_without_leaving_the_old_table_behind() {
        let mut original_values = values("old-profile", "secret");
        original_values.upstream_model = Some("grok-upstream".to_string());
        original_values.base_url = "https://api.example.com/v1".to_string();
        original_values.name = "Relay".to_string();
        let original = build_grok_build_config(&original_values);

        let mut renamed_values = parse_grok_build_config(Some(&original), "");
        renamed_values.model = "new-profile".to_string();
        let renamed = update_grok_build_config(Some(&original), &renamed_values);
        let parsed = parse(&renamed);

        assert_eq!(parsed["models"]["default"].as_str(), Some("new-profile"));
        assert_eq!(
            parsed["model"]["new-profile"]["model"].as_str(),
            Some("grok-upstream")
        );
        assert!(!parsed["model"]
            .as_table()
            .unwrap()
            .contains_key("old-profile"));
    }

    #[test]
    fn parse_falls_back_on_blank_and_invalid_input() {
        let fallback = parse_grok_build_config(None, "Fallback");
        assert_eq!(fallback.model, "grok-4.5");
        assert_eq!(fallback.upstream_model.as_deref(), Some("grok-4.5"));
        assert_eq!(fallback.name, "Fallback");
        assert_eq!(fallback.env_key, None);
        assert_eq!(fallback.context_window, 500_000);
        assert_eq!(parse_grok_build_config(Some("   "), "Fallback"), fallback);
        assert_eq!(
            parse_grok_build_config(Some("[broken"), "Fallback"),
            fallback
        );
        assert_eq!(extract_grok_build_base_url("[broken"), "");
    }

    #[test]
    fn parse_tolerates_a_missing_profile_table_and_bad_context_window() {
        let parsed = parse_grok_build_config(Some("[models]\ndefault = \"x\"\n"), "F");
        assert_eq!(parsed.model, "x");
        assert_eq!(parsed.upstream_model.as_deref(), Some("x"));
        assert_eq!(parsed.base_url, "");
        assert_eq!(parsed.name, "F");
        assert_eq!(parsed.env_key.as_deref(), Some(""));

        let bad_window = parse_grok_build_config(
            Some("[models]\ndefault = \"x\"\n[model.x]\ncontext_window = -5\n"),
            "",
        );
        assert_eq!(bad_window.context_window, 500_000);
        let float_window = parse_grok_build_config(
            Some("[models]\ndefault = \"x\"\n[model.x]\ncontext_window = 1000.0\n"),
            "",
        );
        assert_eq!(float_window.context_window, 1000);
    }

    #[test]
    fn update_keeps_other_models_keys_and_ignores_invalid_existing_toml() {
        let existing =
            "[models]\nother = 1\ndefault = \"p\"\n\n[model.p]\nextra = true\napi_key = \"k\"\n";
        let mut new_values = values("p", "");
        new_values.env_key = Some(" ENV ".to_string());
        new_values.api_backend = " ".to_string();
        new_values.context_window = 0;
        let updated = update_grok_build_config(Some(existing), &new_values);
        let parsed = parse(&updated);
        assert_eq!(parsed["models"]["other"].as_integer(), Some(1));
        let table = parsed["model"]["p"].as_table().unwrap();
        assert_eq!(table["extra"].as_bool(), Some(true));
        assert!(!table.contains_key("api_key"));
        assert_eq!(table["env_key"].as_str(), Some("ENV"));
        assert_eq!(table["api_backend"].as_str(), Some("responses"));
        assert_eq!(table["context_window"].as_integer(), Some(500_000));

        let from_invalid = update_grok_build_config(Some("[broken"), &values("", "k"));
        assert_eq!(
            parse(&from_invalid)["models"]["default"].as_str(),
            Some("grok-4.5")
        );
    }
}
