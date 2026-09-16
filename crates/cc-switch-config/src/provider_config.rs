//! Port of `src/utils/providerConfigUtils.ts`.
//!
//! JSON `env` API-key helpers, template substitution, the common-config
//! snippet merge / strip / subset logic, and the Codex `config.toml` text
//! editors. The TOML editors are deliberately **line based** (strict
//! whole-line regexes, section ranges by header text), exactly like the
//! TypeScript implementation, so comments and unrelated lines are preserved
//! byte for byte and the code keeps working while the user is mid-edit on a
//! document that does not parse yet.
//!
//! Every mutator that edits lines returns through [`finalize_toml_text`].

use std::sync::LazyLock;

use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use toml::Value as TomlValue;

use crate::text::normalize_toml_text;

// ========== JSON helpers ==========

/// JavaScript truthiness for a JSON value.
fn js_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// `JSON.stringify(value, null, 2)`.
pub fn pretty_json(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

/// Port of `deepMerge` (`providerConfigUtils.ts`, internal).
///
/// Recurses into plain objects (creating / replacing a non-object target
/// with `{}` first) and **overwrites** arrays and scalars wholesale. Key
/// positions of existing keys are kept; new keys are appended.
pub fn deep_merge(target: &mut Map<String, Value>, source: &Map<String, Value>) {
    for (key, value) in source {
        if let Value::Object(source_object) = value {
            let entry = target
                .entry(key.clone())
                .or_insert_with(|| Value::Object(Map::new()));
            if !entry.is_object() {
                *entry = Value::Object(Map::new());
            }
            if let Value::Object(target_object) = entry {
                deep_merge(target_object, source_object);
            }
        } else {
            target.insert(key.clone(), value.clone());
        }
    }
}

/// Port of `deepRemove` (`providerConfigUtils.ts`, internal).
///
/// Removes a key only when the target value is a subset of the snippet value
/// (see [`is_subset`]); nested objects are recursed into and pruned when they
/// become empty. Remaining keys keep their order (JavaScript `delete`).
pub fn deep_remove(target: &mut Map<String, Value>, source: &Map<String, Value>) {
    for (key, value) in source {
        let Some(existing) = target.get_mut(key) else {
            continue;
        };
        let remove = match (value, existing) {
            (Value::Object(source_object), Value::Object(existing_object)) => {
                deep_remove(existing_object, source_object);
                existing_object.is_empty()
            }
            (value, existing) => is_subset(existing, value),
        };
        if remove {
            target.shift_remove(key);
        }
    }
}

/// `target === source` for JSON scalars (numbers compare by value, so `1` and
/// `1.0` are equal as they are in JavaScript).
fn scalar_equals(target: &Value, source: &Value) -> bool {
    match (target, source) {
        (Value::Number(a), Value::Number(b)) => {
            a == b || matches!((a.as_f64(), b.as_f64()), (Some(x), Some(y)) if x == y)
        }
        _ => target == source,
    }
}

/// Port of `isSubset` (`providerConfigUtils.ts`, internal).
///
/// Objects: every snippet key must exist in the target and match recursively.
/// Arrays: **same length and index-wise** equality (unlike the backend's
/// order-insensitive `json_is_subset`). Scalars: strict equality.
pub fn is_subset(target: &Value, source: &Value) -> bool {
    match source {
        Value::Object(source_object) => match target {
            Value::Object(target_object) => source_object
                .iter()
                .all(|(key, value)| target_object.get(key).is_some_and(|t| is_subset(t, value))),
            _ => false,
        },
        Value::Array(source_items) => match target {
            Value::Array(target_items) if target_items.len() == source_items.len() => source_items
                .iter()
                .zip(target_items)
                .all(|(s, t)| is_subset(t, s)),
            _ => false,
        },
        _ => scalar_equals(target, source),
    }
}

/// Port of the `UpdateCommonConfigResult` interface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCommonConfigResult {
    pub updated_config: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Default `fieldName` of [`validate_json_config`].
pub const DEFAULT_JSON_FIELD_NAME: &str = "配置";
/// `fieldName` used for the snippet inside [`update_common_config_snippet`].
pub const COMMON_CONFIG_SNIPPET_FIELD_NAME: &str = "通用配置片段";
/// Error returned by [`update_common_config_snippet`] when the config itself does not parse.
pub const COMMON_CONFIG_PARSE_ERROR: &str = "配置 JSON 解析失败，无法应用通用配置";

/// Port of `validateJsonConfig` (`providerConfigUtils.ts`).
///
/// Returns `""` on success. Blank input is valid. A parse result that is not
/// an object (arrays included) → `"<fieldName>必须是 JSON 对象"`; a parse
/// failure → `"<fieldName>JSON格式错误，请检查语法"`. `field_name` defaults to
/// [`DEFAULT_JSON_FIELD_NAME`].
pub fn validate_json_config(value: &str, field_name: Option<&str>) -> String {
    let field_name = field_name.unwrap_or(DEFAULT_JSON_FIELD_NAME);
    if value.trim().is_empty() {
        return String::new();
    }
    match serde_json::from_str::<Value>(value) {
        Ok(Value::Object(_)) => String::new(),
        Ok(_) => format!("{field_name}必须是 JSON 对象"),
        Err(_) => format!("{field_name}JSON格式错误，请检查语法"),
    }
}

fn parse_config_json(json_string: &str) -> Result<Value, serde_json::Error> {
    if json_string.is_empty() {
        Ok(Value::Object(Map::new()))
    } else {
        serde_json::from_str(json_string)
    }
}

/// Port of `updateCommonConfigSnippet` (`providerConfigUtils.ts`).
///
/// Parses `json_string` (empty → `{}`); on failure returns the **original
/// string untouched** plus [`COMMON_CONFIG_PARSE_ERROR`]. A blank snippet
/// yields the re-serialised config with no error; an invalid snippet yields
/// the re-serialised config plus the [`validate_json_config`] message.
/// `enabled` merges the snippet in ([`deep_merge`]); otherwise it is stripped
/// ([`deep_remove`]). Output is always 2-space pretty JSON in key insertion
/// order. A config whose root is not an object is re-serialised unchanged
/// (the TypeScript version silently no-ops on arrays and throws on primitives).
pub fn update_common_config_snippet(
    json_string: &str,
    snippet_string: &str,
    enabled: bool,
) -> UpdateCommonConfigResult {
    let config = match parse_config_json(json_string) {
        Ok(config) => config,
        Err(_) => {
            return UpdateCommonConfigResult {
                updated_config: json_string.to_string(),
                error: Some(COMMON_CONFIG_PARSE_ERROR.to_string()),
            };
        }
    };

    if snippet_string.trim().is_empty() {
        return UpdateCommonConfigResult {
            updated_config: pretty_json(&config),
            error: None,
        };
    }

    let snippet_error =
        validate_json_config(snippet_string, Some(COMMON_CONFIG_SNIPPET_FIELD_NAME));
    if !snippet_error.is_empty() {
        return UpdateCommonConfigResult {
            updated_config: pretty_json(&config),
            error: Some(snippet_error),
        };
    }

    let snippet = match serde_json::from_str::<Value>(snippet_string) {
        Ok(Value::Object(snippet)) => snippet,
        _ => {
            return UpdateCommonConfigResult {
                updated_config: pretty_json(&config),
                error: None,
            };
        }
    };

    let mut updated = config;
    if let Value::Object(object) = &mut updated {
        if enabled {
            deep_merge(object, &snippet);
        } else {
            deep_remove(object, &snippet);
        }
    }

    UpdateCommonConfigResult {
        updated_config: pretty_json(&updated),
        error: None,
    }
}

/// Port of `hasCommonConfigSnippet` (`providerConfigUtils.ts`).
///
/// `false` for a blank snippet, a snippet that is not a JSON object, or any
/// parse failure; otherwise [`is_subset`] of the config (empty → `{}`).
pub fn has_common_config_snippet(json_string: &str, snippet_string: &str) -> bool {
    if snippet_string.trim().is_empty() {
        return false;
    }
    let Ok(config) = parse_config_json(json_string) else {
        return false;
    };
    let Ok(snippet) = serde_json::from_str::<Value>(snippet_string) else {
        return false;
    };
    if !snippet.is_object() {
        return false;
    }
    is_subset(&config, &snippet)
}

fn env_key_for(app_type: Option<&str>) -> Option<&'static str> {
    match app_type {
        Some("gemini") => Some("GEMINI_API_KEY"),
        Some("codex") => Some("CODEX_API_KEY"),
        _ => None,
    }
}

/// Port of `getApiKeyFromConfig` (`providerConfigUtils.ts`).
///
/// A top-level `apiKey` string wins when non-empty and not containing `${`
/// (template placeholder guard for Bedrock presets). Otherwise reads `env`:
/// `GEMINI_API_KEY` for `gemini`, `CODEX_API_KEY` for `codex`, else
/// `ANTHROPIC_AUTH_TOKEN` then `ANTHROPIC_API_KEY`. Non-strings, a missing
/// `env` and unparsable JSON all yield `""`.
pub fn get_api_key_from_config(json_string: &str, app_type: Option<&str>) -> String {
    let Ok(config) = serde_json::from_str::<Value>(json_string) else {
        return String::new();
    };

    if let Some(Value::String(api_key)) = config.get("apiKey") {
        if !api_key.is_empty() && !api_key.contains("${") {
            return api_key.clone();
        }
    }

    let Some(env) = config.get("env").filter(|env| js_truthy(env)) else {
        return String::new();
    };
    let string_at = |key: &str| match env.get(key) {
        Some(Value::String(s)) => Some(s.clone()),
        _ => None,
    };

    if let Some(key) = env_key_for(app_type) {
        return string_at(key).unwrap_or_default();
    }

    string_at("ANTHROPIC_AUTH_TOKEN")
        .or_else(|| string_at("ANTHROPIC_API_KEY"))
        .unwrap_or_default()
}

/// Port of the `TemplateValueConfig` interface (`src/config/claudeProviderPresets.ts`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateValueConfig {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub placeholder: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
    /// `undefined` in TypeScript is `None`; an empty string is a valid override.
    #[serde(default)]
    pub editor_value: Option<String>,
}

/// `editorValue ?? defaultValue ?? ""` — the resolved value of one template variable.
pub fn resolve_template_value(config: &TemplateValueConfig) -> String {
    config
        .editor_value
        .clone()
        .or_else(|| config.default_value.clone())
        .unwrap_or_default()
}

/// Port of `applyTemplateValues` (`providerConfigUtils.ts`).
///
/// Resolves every variable with [`resolve_template_value`], then deep-traverses
/// strings / arrays / objects producing a **new** value (scalars pass through)
/// and replaces every literal occurrence of `${key}` (no regex, no escaping
/// issues). Variables are applied in iteration order, one after another.
/// Pass `std::iter::empty()` for the TypeScript `undefined` case.
pub fn apply_template_values<'a, I>(config: &Value, template_values: I) -> Value
where
    I: IntoIterator<Item = (&'a str, &'a TemplateValueConfig)>,
{
    let mut resolved: Vec<(String, String)> = Vec::new();
    for (key, value) in template_values {
        let resolved_value = resolve_template_value(value);
        match resolved.iter_mut().find(|(existing, _)| existing == key) {
            Some(entry) => entry.1 = resolved_value,
            None => resolved.push((key.to_string(), resolved_value)),
        }
    }

    fn replace_in_string(text: &str, resolved: &[(String, String)]) -> String {
        resolved.iter().fold(text.to_string(), |acc, (key, value)| {
            let placeholder = format!("${{{key}}}");
            if acc.contains(&placeholder) {
                acc.replace(&placeholder, value)
            } else {
                acc
            }
        })
    }

    fn traverse(value: &Value, resolved: &[(String, String)]) -> Value {
        match value {
            Value::String(s) => Value::String(replace_in_string(s, resolved)),
            Value::Array(items) => {
                Value::Array(items.iter().map(|v| traverse(v, resolved)).collect())
            }
            Value::Object(object) => Value::Object(
                object
                    .iter()
                    .map(|(k, v)| (k.clone(), traverse(v, resolved)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    traverse(config, &resolved)
}

/// Port of `hasApiKeyField` (`providerConfigUtils.ts`).
///
/// `true` when the root owns an `apiKey` key (even if empty / null), else
/// when `env` owns `GEMINI_API_KEY` / `CODEX_API_KEY` / (`ANTHROPIC_AUTH_TOKEN`
/// or `ANTHROPIC_API_KEY`) for the respective app type. Unparsable → `false`.
pub fn has_api_key_field(json_string: &str, app_type: Option<&str>) -> bool {
    let Ok(config) = serde_json::from_str::<Value>(json_string) else {
        return false;
    };
    if let Value::Object(object) = &config {
        if object.contains_key("apiKey") {
            return true;
        }
    }
    // `Object.prototype.hasOwnProperty.call(null, ...)` throws → caught → false.
    let Some(env) = config.get("env").and_then(Value::as_object) else {
        return false;
    };
    match env_key_for(app_type) {
        Some(key) => env.contains_key(key),
        None => env.contains_key("ANTHROPIC_AUTH_TOKEN") || env.contains_key("ANTHROPIC_API_KEY"),
    }
}

/// Options of [`set_api_key_in_config`] (`options` parameter of `setApiKeyInConfig`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SetApiKeyOptions<'a> {
    /// Create missing `env` / key fields. Defaults to `false`: never create.
    pub create_if_missing: bool,
    /// `"gemini"`, `"codex"`, anything else (or `None`) is treated as Claude.
    pub app_type: Option<&'a str>,
    /// Claude only: the key to create when neither Anthropic key exists
    /// (defaults to `ANTHROPIC_AUTH_TOKEN`).
    pub api_key_field: Option<&'a str>,
}

/// Port of `setApiKeyInConfig` (`providerConfigUtils.ts`).
///
/// By default never creates missing fields and returns the **input string
/// verbatim** (not reformatted) when nothing can be written. A root `apiKey`
/// is always overwritten. Claude: overwrite `ANTHROPIC_AUTH_TOKEN` if present,
/// else `ANTHROPIC_API_KEY` if present, else (when creating)
/// `api_key_field` or `ANTHROPIC_AUTH_TOKEN`. Gemini / Codex write their
/// single key. Any failure returns the original string. Successful writes are
/// returned as 2-space pretty JSON.
pub fn set_api_key_in_config(
    json_string: &str,
    api_key: &str,
    options: SetApiKeyOptions<'_>,
) -> String {
    let original = || json_string.to_string();
    let Ok(config) = serde_json::from_str::<Value>(json_string) else {
        return original();
    };

    let mut object = match config {
        Value::Object(object) => object,
        other => {
            // Arrays accept (invisible) expando properties in JavaScript and
            // are re-serialised when creation is allowed; `null` and other
            // primitives throw and fall back to the original text.
            return if matches!(other, Value::Array(_)) && options.create_if_missing {
                pretty_json(&other)
            } else {
                original()
            };
        }
    };

    if object.contains_key("apiKey") {
        object.insert("apiKey".to_string(), Value::String(api_key.to_string()));
        return pretty_json(&Value::Object(object));
    }

    if !object.get("env").is_some_and(js_truthy) {
        if !options.create_if_missing {
            return original();
        }
        object.insert("env".to_string(), Value::Object(Map::new()));
    }

    let env = match object.get_mut("env") {
        Some(Value::Object(env)) => env,
        Some(Value::Array(_)) => {
            // `"KEY" in []` is false; creation sets an invisible expando property.
            return if options.create_if_missing {
                pretty_json(&Value::Object(object))
            } else {
                original()
            };
        }
        // `in` on a primitive throws a TypeError → caught → original string.
        _ => return original(),
    };

    let write = |env: &mut Map<String, Value>, key: &str| {
        env.insert(key.to_string(), Value::String(api_key.to_string()));
    };

    if let Some(key) = env_key_for(options.app_type) {
        if env.contains_key(key) || options.create_if_missing {
            write(env, key);
        } else {
            return original();
        }
    } else if env.contains_key("ANTHROPIC_AUTH_TOKEN") {
        write(env, "ANTHROPIC_AUTH_TOKEN");
    } else if env.contains_key("ANTHROPIC_API_KEY") {
        write(env, "ANTHROPIC_API_KEY");
    } else if options.create_if_missing {
        write(env, options.api_key_field.unwrap_or("ANTHROPIC_AUTH_TOKEN"));
    } else {
        return original();
    }

    pretty_json(&Value::Object(object))
}

// ========== TOML common config snippet ==========

fn parse_toml(text: &str) -> Option<toml::Table> {
    toml::from_str::<toml::Table>(text).ok()
}

/// `target === source` for TOML scalars as smol-toml hands them to JavaScript:
/// integers and floats are both `number`; datetimes are `Date` objects whose
/// `===` compares identity and therefore never matches.
fn toml_scalar_equals(target: &TomlValue, source: &TomlValue) -> bool {
    match (target, source) {
        (TomlValue::String(a), TomlValue::String(b)) => a == b,
        (TomlValue::Boolean(a), TomlValue::Boolean(b)) => a == b,
        (TomlValue::Integer(a), TomlValue::Integer(b)) => a == b,
        (TomlValue::Float(a), TomlValue::Float(b)) => a == b,
        (TomlValue::Integer(i), TomlValue::Float(f))
        | (TomlValue::Float(f), TomlValue::Integer(i)) => *i as f64 == *f,
        _ => false,
    }
}

/// [`is_subset`] over TOML values (same semantics: recursive tables, arrays
/// by equal length and index, scalars by strict equality).
pub fn toml_is_subset(target: &TomlValue, source: &TomlValue) -> bool {
    match source {
        TomlValue::Table(source_table) => match target {
            TomlValue::Table(target_table) => source_table.iter().all(|(key, value)| {
                target_table
                    .get(key)
                    .is_some_and(|t| toml_is_subset(t, value))
            }),
            _ => false,
        },
        TomlValue::Array(source_items) => match target {
            TomlValue::Array(target_items) if target_items.len() == source_items.len() => {
                source_items
                    .iter()
                    .zip(target_items)
                    .all(|(s, t)| toml_is_subset(t, s))
            }
            _ => false,
        },
        _ => toml_scalar_equals(target, source),
    }
}

/// Port of `hasTomlCommonConfigSnippet` (`providerConfigUtils.ts`).
///
/// Blank snippet → `false`. Both texts are quote-normalised, parsed and
/// compared with [`toml_is_subset`]. **If either fails to parse**, falls back
/// to fuzzy text containment: whitespace runs collapsed to one space, trimmed,
/// then `contains`. (Merging / stripping TOML snippets must go through the
/// comment-preserving backend implementation, not a parse → stringify here.)
pub fn has_toml_common_config_snippet(toml_string: &str, snippet_string: &str) -> bool {
    if snippet_string.trim().is_empty() {
        return false;
    }
    let config = parse_toml(&normalize_toml_text(toml_string));
    let snippet = parse_toml(&normalize_toml_text(snippet_string));
    match (config, snippet) {
        (Some(config), Some(snippet)) => {
            toml_is_subset(&TomlValue::Table(config), &TomlValue::Table(snippet))
        }
        _ => {
            let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
            norm(toml_string).contains(&norm(snippet_string))
        }
    }
}

// ========== Codex wire_api helpers ==========

/// Port of the `CodexApiFormat` union (`src/types.ts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexApiFormat {
    /// Native OpenAI Responses API (Codex default).
    OpenaiResponses,
    /// OpenAI Chat Completions; needs local routing.
    OpenaiChat,
    /// Native Anthropic Messages; needs local routing.
    Anthropic,
}

impl CodexApiFormat {
    /// The wire string used by the TypeScript union.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenaiResponses => "openai_responses",
            Self::OpenaiChat => "openai_chat",
            Self::Anthropic => "anthropic",
        }
    }
}

/// `CODEX_CHAT_WIRE_API_VALUES` (`providerConfigUtils.ts`).
pub const CODEX_CHAT_WIRE_API_VALUES: &[&str] = &[
    "chat",
    "chat_completions",
    "chat-completions",
    "openai_chat",
    "openai-chat",
    "openai_chat_completions",
];

/// Anthropic Messages aliases accepted by `isCodexAnthropicWireApi`.
pub const CODEX_ANTHROPIC_WIRE_API_VALUES: &[&str] = &[
    "anthropic",
    "anthropic_messages",
    "anthropic-messages",
    "messages",
    "claude",
];

fn normalized_wire_api(wire_api: Option<&str>) -> String {
    wire_api.unwrap_or_default().trim().to_lowercase()
}

/// Port of `isCodexChatWireApi` (`providerConfigUtils.ts`): trimmed,
/// lower-cased membership in [`CODEX_CHAT_WIRE_API_VALUES`].
pub fn is_codex_chat_wire_api(wire_api: Option<&str>) -> bool {
    CODEX_CHAT_WIRE_API_VALUES.contains(&normalized_wire_api(wire_api).as_str())
}

/// Port of `isCodexAnthropicWireApi` (`providerConfigUtils.ts`).
pub fn is_codex_anthropic_wire_api(wire_api: Option<&str>) -> bool {
    CODEX_ANTHROPIC_WIRE_API_VALUES.contains(&normalized_wire_api(wire_api).as_str())
}

/// Port of `codexApiFormatFromWireApi` (`providerConfigUtils.ts`).
///
/// Chat aliases → `OpenaiChat`; Anthropic aliases → `Anthropic`;
/// `responses` / `openai_responses` / `openai-responses` → `OpenaiResponses`;
/// anything else → `None`.
pub fn codex_api_format_from_wire_api(wire_api: Option<&str>) -> Option<CodexApiFormat> {
    if is_codex_chat_wire_api(wire_api) {
        return Some(CodexApiFormat::OpenaiChat);
    }
    if is_codex_anthropic_wire_api(wire_api) {
        return Some(CodexApiFormat::Anthropic);
    }
    match normalized_wire_api(wire_api).as_str() {
        "responses" | "openai_responses" | "openai-responses" => {
            Some(CodexApiFormat::OpenaiResponses)
        }
        _ => None,
    }
}

// ========== Line-based TOML helpers ==========

/// JavaScript `.` — any character except the four line terminators.
const ANY: &str = r"[^\n\r\x{2028}\x{2029}]";

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("provider_config regex is valid")
}

/// `(["'])([^"'\r\n]+)\1` without back-references: group 1 is the
/// double-quoted content, group 2 the single-quoted content.
fn quoted_assignment_pattern(key: &str) -> String {
    format!(r#"^\s*{key}\s*=\s*(?:"([^"'\r\n]+)"|'([^"'\r\n]+)')\s*(?:#{ANY}*)?$"#)
}

/// `^(\s*<key>\s*=\s*)(?:"basic"|'literal')(\s*(?:#.*)?)$` — captures the
/// prefix and trailing comment around any quoted string.
fn quoted_replace_pattern(key: &str) -> String {
    format!(r#"^(\s*{key}\s*=\s*)(?:"(?:\\{ANY}|[^"\\\r\n])*"|'[^'\r\n]*')(\s*(?:#{ANY}*)?)$"#)
}

static TOML_SECTION_HEADER_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| re(r"^\s*\[([^\]\r\n]+)\]\s*$"));
static TOML_BASE_URL_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    re(&format!(
        r#"^\s*base_url\s*=\s*(?:"((?:\\{ANY}|[^"\\\r\n])*)"|'([^'\r\n]*)')\s*(?:#{ANY}*)?$"#
    ))
});
static TOML_EXPERIMENTAL_BEARER_TOKEN_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| re(&quoted_assignment_pattern("experimental_bearer_token")));
static TOML_EXPERIMENTAL_BEARER_TOKEN_REPLACE_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| re(&quoted_replace_pattern("experimental_bearer_token")));
/// Double-quoted basic string (supports `\"` `\\` and other escapes so the
/// output of `set_codex_model_name` is recognised); the value must be
/// unescaped after extraction. Strict whole-line match on purpose.
static TOML_MODEL_DOUBLE_QUOTED_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    re(&format!(
        r#"^\s*model\s*=\s*"((?:[^"\\\r\n]|\\{ANY})*)"\s*(?:#{ANY}*)?$"#
    ))
});
/// Single-quoted literal string (no escapes, TOML semantics).
static TOML_MODEL_SINGLE_QUOTED_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| re(&format!(r"^\s*model\s*=\s*'([^'\r\n]*)'\s*(?:#{ANY}*)?$")));
static TOML_WIRE_API_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| re(&quoted_assignment_pattern("wire_api")));
static TOML_MODEL_PROVIDER_LINE_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| re(&quoted_assignment_pattern("model_provider")));
static TOML_PROVIDER_NAME_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| re(&quoted_assignment_pattern("name")));
static TOML_PROVIDER_NAME_REPLACE_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| re(&quoted_replace_pattern("name")));
static TOML_GOALS_FEATURE_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| re(&format!(r"^\s*goals\s*=\s*(true|false)\s*(?:#{ANY}*)?$")));
static TOML_GOALS_FEATURE_REPLACE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    re(&format!(
        r"^(\s*goals\s*=\s*)(true|false)(\s*(?:#{ANY}*)?)$"
    ))
});

/// `CODEX_RESERVED_MODEL_PROVIDER_IDS` (`providerConfigUtils.ts`): built-in
/// Codex provider ids that never get a custom `[model_providers.<id>]` table.
pub const CODEX_RESERVED_MODEL_PROVIDER_IDS: &[&str] = &[
    "amazon-bedrock",
    "openai",
    "ollama",
    "lmstudio",
    "oss",
    "ollama-chat",
];

/// Port of the `TomlSectionRange` interface: a `[section]` header line and
/// its body `[body_start_index, body_end_index)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TomlSectionRange {
    pub header_line_index: usize,
    pub body_start_index: usize,
    pub body_end_index: usize,
}

/// Port of the `TomlAssignmentMatch` interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TomlAssignmentMatch {
    pub index: usize,
    /// `None` for top-level assignments.
    pub section_name: Option<String>,
    pub value: String,
}

/// `text ? text.split("\n") : []`
fn split_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        Vec::new()
    } else {
        text.split('\n').map(str::to_string).collect()
    }
}

/// Port of `finalizeTomlText` (`providerConfigUtils.ts`, internal).
///
/// Joins with `\n`, collapses any run of three or more `\n` to exactly two,
/// then strips leading newlines. CRLF blank lines are **not** collapsed.
pub fn finalize_toml_text(lines: &[String]) -> String {
    let joined = lines.join("\n");
    let mut collapsed = String::with_capacity(joined.len());
    let mut newline_run = 0usize;
    for ch in joined.chars() {
        if ch == '\n' {
            newline_run += 1;
            if newline_run <= 2 {
                collapsed.push(ch);
            }
        } else {
            newline_run = 0;
            collapsed.push(ch);
        }
    }
    collapsed.trim_start_matches('\n').to_string()
}

fn section_header_name(line: &str) -> Option<&str> {
    TOML_SECTION_HEADER_PATTERN
        .captures(line)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str())
}

/// Port of `getTomlSectionRange`: finds the first `[section]` header whose
/// bracket content equals `section_name` exactly; the body runs to the next
/// header (any header) or the end of the document.
pub fn get_toml_section_range(lines: &[String], section_name: &str) -> Option<TomlSectionRange> {
    let mut header_line_index: Option<usize> = None;
    for (index, line) in lines.iter().enumerate() {
        let Some(name) = section_header_name(line) else {
            continue;
        };
        match header_line_index {
            None => {
                if name == section_name {
                    header_line_index = Some(index);
                }
            }
            Some(header) => {
                return Some(TomlSectionRange {
                    header_line_index: header,
                    body_start_index: header + 1,
                    body_end_index: index,
                });
            }
        }
    }
    header_line_index.map(|header| TomlSectionRange {
        header_line_index: header,
        body_start_index: header + 1,
        body_end_index: lines.len(),
    })
}

/// Port of `getTopLevelEndIndex`: index of the first `[...]` header, else `lines.len()`.
pub fn get_top_level_end_index(lines: &[String]) -> usize {
    lines
        .iter()
        .position(|line| TOML_SECTION_HEADER_PATTERN.is_match(line))
        .unwrap_or(lines.len())
}

/// Port of `getTomlSectionInsertIndex`: end of the section body, backing up
/// over trailing blank lines.
pub fn get_toml_section_insert_index(lines: &[String], range: &TomlSectionRange) -> usize {
    let mut insert_index = range.body_end_index;
    while insert_index > range.body_start_index && lines[insert_index - 1].trim().is_empty() {
        insert_index -= 1;
    }
    insert_index
}

/// `match?.[2] ?? match?.[1]`, truthy only.
fn assignment_value<'t>(caps: &Captures<'t>) -> Option<&'t str> {
    caps.get(2)
        .or_else(|| caps.get(1))
        .map(|m| m.as_str())
        .filter(|value| !value.is_empty())
}

fn find_toml_assignment_in_range(
    lines: &[String],
    pattern: &Regex,
    start_index: usize,
    end_index: usize,
    section_name: Option<&str>,
) -> Option<TomlAssignmentMatch> {
    (start_index..end_index.min(lines.len())).find_map(|index| {
        let caps = pattern.captures(&lines[index])?;
        let value = assignment_value(&caps)?;
        Some(TomlAssignmentMatch {
            index,
            section_name: section_name.map(str::to_string),
            value: value.to_string(),
        })
    })
}

fn find_toml_assignments_in_range(
    lines: &[String],
    pattern: &Regex,
    start_index: usize,
    end_index: usize,
    section_name: Option<&str>,
) -> Vec<TomlAssignmentMatch> {
    (start_index..end_index.min(lines.len()))
        .filter_map(|index| {
            let caps = pattern.captures(&lines[index])?;
            let value = assignment_value(&caps)?;
            Some(TomlAssignmentMatch {
                index,
                section_name: section_name.map(str::to_string),
                value: value.to_string(),
            })
        })
        .collect()
}

fn find_toml_line_in_range(
    lines: &[String],
    pattern: &Regex,
    start_index: usize,
    end_index: usize,
) -> Option<usize> {
    (start_index..end_index.min(lines.len())).find(|&index| pattern.is_match(&lines[index]))
}

/// Port of `findTomlAssignments`: every matching assignment in the document,
/// tagged with the section it sits in (`None` for top level).
fn find_toml_assignments(lines: &[String], pattern: &Regex) -> Vec<TomlAssignmentMatch> {
    let mut assignments = Vec::new();
    let mut current_section: Option<String> = None;
    for (index, line) in lines.iter().enumerate() {
        if let Some(name) = section_header_name(line) {
            current_section = Some(name.to_string());
            continue;
        }
        let Some(caps) = pattern.captures(line) else {
            continue;
        };
        let Some(value) = assignment_value(&caps) else {
            continue;
        };
        assignments.push(TomlAssignmentMatch {
            index,
            section_name: current_section.clone(),
            value: value.to_string(),
        });
    }
    assignments
}

fn is_mcp_server_section(section_name: Option<&str>) -> bool {
    section_name.is_some_and(|name| name == "mcp_servers" || name.starts_with("mcp_servers."))
}

fn is_other_provider_section(
    section_name: Option<&str>,
    target_section_name: Option<&str>,
) -> bool {
    section_name.is_some_and(|name| {
        Some(name) != target_section_name
            && (name == "model_providers" || name.starts_with("model_providers."))
    })
}

/// Port of `getRecoverableBaseUrlAssignments` (alias
/// `getRecoverableCodexProviderAssignments`): "misplaced" assignments eligible
/// for recovery — anything outside the target section that is neither an
/// `mcp_servers` table nor another `model_providers` table.
fn get_recoverable_assignments(
    assignments: Vec<TomlAssignmentMatch>,
    target_section_name: Option<&str>,
) -> Vec<TomlAssignmentMatch> {
    assignments
        .into_iter()
        .filter(|assignment| {
            let section = assignment.section_name.as_deref();
            section != target_section_name
                && !is_mcp_server_section(section)
                && !is_other_provider_section(section, target_section_name)
        })
        .collect()
}

/// Port of `getTopLevelModelProviderLineIndex`.
fn get_top_level_model_provider_line_index(lines: &[String]) -> Option<usize> {
    let top_level_end_index = get_top_level_end_index(lines);
    (0..top_level_end_index).find(|&index| TOML_MODEL_PROVIDER_LINE_PATTERN.is_match(&lines[index]))
}

fn has_toml_section_body_content(lines: &[String], range: &TomlSectionRange) -> bool {
    lines[range.body_start_index..range.body_end_index.min(lines.len())]
        .iter()
        .any(|line| !line.trim().is_empty())
}

/// Port of `escapeTomlBasicString`: escapes `"` `\` `\b \t \n \f \r`, every
/// other C0 control character as `\uXXXX` (lowercase hex). `U+007F` is left
/// alone, as in the TypeScript.
pub fn escape_toml_basic_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\u{8}' => escaped.push_str("\\b"),
            '\t' => escaped.push_str("\\t"),
            '\n' => escaped.push_str("\\n"),
            '\u{c}' => escaped.push_str("\\f"),
            '\r' => escaped.push_str("\\r"),
            c if (c as u32) < 0x20 => escaped.push_str(&format!("\\u{:04x}", c as u32)),
            c => escaped.push(c),
        }
    }
    escaped
}

/// Port of `tomlBasicString`: `"` + [`escape_toml_basic_string`] + `"`.
pub fn toml_basic_string(value: &str) -> String {
    format!("\"{}\"", escape_toml_basic_string(value))
}

fn is_js_line_terminator(ch: char) -> bool {
    matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

fn hex_prefix(text: &str, len: usize) -> Option<&str> {
    let hex = text.get(..len)?;
    (hex.len() == len && hex.bytes().all(|b| b.is_ascii_hexdigit())).then_some(hex)
}

/// Port of `unescapeTomlBasicString`: inverse of [`escape_toml_basic_string`].
/// `\uXXXX` / `\UXXXXXXXX` are decoded; **unknown escapes are preserved
/// verbatim**. Code points Rust cannot represent (lone surrogates, values
/// above `U+10FFFF`) become `U+FFFD`.
pub fn unescape_toml_basic_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(pos) = rest.find('\\') {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 1..];

        let unicode = after
            .strip_prefix('u')
            .and_then(|tail| hex_prefix(tail, 4))
            .or_else(|| after.strip_prefix('U').and_then(|tail| hex_prefix(tail, 8)));
        if let Some(hex) = unicode {
            let code_point = u32::from_str_radix(hex, 16).unwrap_or(0);
            out.push(char::from_u32(code_point).unwrap_or('\u{FFFD}'));
            rest = &after[1 + hex.len()..];
            continue;
        }

        match after.chars().next() {
            Some(ch) if !is_js_line_terminator(ch) => {
                match ch {
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    'b' => out.push('\u{8}'),
                    't' => out.push('\t'),
                    'n' => out.push('\n'),
                    'f' => out.push('\u{c}'),
                    'r' => out.push('\r'),
                    other => {
                        out.push('\\');
                        out.push(other);
                    }
                }
                rest = &after[ch.len_utf8()..];
            }
            _ => {
                // `\` at end of input or before a line terminator: no match, keep it.
                out.push('\\');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

// ========== Codex section resolution ==========

/// Port of `getCodexModelProviderName`: the top-level `model_provider`,
/// parsed with the TOML parser first and falling back to a **top-level-only**
/// line scan while the document is mid-edit / invalid.
pub fn get_codex_model_provider_name(config_text: &str) -> Option<String> {
    let normalized = normalize_toml_text(config_text);
    if let Some(parsed) = parse_toml(&normalized) {
        if let Some(TomlValue::String(provider)) = parsed.get("model_provider") {
            let provider = provider.trim();
            if !provider.is_empty() {
                return Some(provider.to_string());
            }
        }
    }

    let lines: Vec<String> = normalized.split('\n').map(str::to_string).collect();
    let index = get_top_level_model_provider_line_index(&lines)?;
    let caps = TOML_MODEL_PROVIDER_LINE_PATTERN.captures(&lines[index])?;
    let provider = assignment_value(&caps)?.trim();
    (!provider.is_empty()).then(|| provider.to_string())
}

/// Port of `getCodexProviderSectionName`: `model_providers.<id>` for the active provider.
pub fn get_codex_provider_section_name(config_text: &str) -> Option<String> {
    get_codex_model_provider_name(config_text).map(|provider| format!("model_providers.{provider}"))
}

/// Port of `isCustomCodexModelProviderId`: non-empty and not one of
/// [`CODEX_RESERVED_MODEL_PROVIDER_IDS`] (case-insensitive, trimmed).
pub fn is_custom_codex_model_provider_id(provider_name: &str) -> bool {
    let id = provider_name.trim().to_lowercase();
    !id.is_empty() && !CODEX_RESERVED_MODEL_PROVIDER_IDS.contains(&id.as_str())
}

/// Port of `getCodexCustomProviderSectionName`: like
/// [`get_codex_provider_section_name`] but only for non-reserved ids.
pub fn get_codex_custom_provider_section_name(config_text: &str) -> Option<String> {
    get_codex_model_provider_name(config_text)
        .filter(|provider| is_custom_codex_model_provider_id(provider))
        .map(|provider| format!("model_providers.{provider}"))
}

/// Shared three-tier lookup used by `extractCodexWireApi` / `extractCodexBaseUrl`:
/// active provider section → top level → exactly one recoverable misplaced assignment.
fn extract_codex_provider_assignment(config_text: &str, pattern: &Regex) -> Option<String> {
    let text = normalize_toml_text(config_text);
    if text.is_empty() {
        return None;
    }

    let lines = split_lines(&text);
    let target_section_name = get_codex_provider_section_name(&text);

    if let Some(target) = &target_section_name {
        if let Some(range) = get_toml_section_range(&lines, target) {
            if let Some(found) = find_toml_assignment_in_range(
                &lines,
                pattern,
                range.body_start_index,
                range.body_end_index,
                Some(target),
            ) {
                return Some(found.value);
            }
        }
    }

    if let Some(found) =
        find_toml_assignment_in_range(&lines, pattern, 0, get_top_level_end_index(&lines), None)
    {
        return Some(found.value);
    }

    let mut fallback = get_recoverable_assignments(
        find_toml_assignments(&lines, pattern),
        target_section_name.as_deref(),
    );
    if fallback.len() == 1 {
        Some(fallback.remove(0).value)
    } else {
        None
    }
}

/// Port of `extractCodexWireApi` (`providerConfigUtils.ts`).
///
/// Search order: `wire_api` inside the active `[model_providers.<id>]`, the
/// first top-level `wire_api`, then a single recoverable misplaced assignment
/// (ignoring `mcp_servers.*` and other `model_providers.*` tables). Accepts
/// single or double quotes; the value must be non-empty.
pub fn extract_codex_wire_api(config_text: &str) -> Option<String> {
    extract_codex_provider_assignment(config_text, &TOML_WIRE_API_PATTERN)
}

/// The closed `"responses" | "chat"` union accepted by `setCodexWireApi`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CodexWireApi {
    Responses,
    Chat,
}

impl CodexWireApi {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Responses => "responses",
            Self::Chat => "chat",
        }
    }
}

/// Port of `setCodexWireApi` (`providerConfigUtils.ts`).
///
/// Writes `wire_api = "<value>"` (always double-quoted). With an active
/// provider section: replace in-section (rewriting the **whole line**, so a
/// trailing comment on it is lost); else delete a single recoverable misplaced
/// assignment first; then insert at the section end (before trailing blanks);
/// if the section is missing, append `[<section>]` + the line (blank-line
/// separated). Without a provider: replace the top-level line, else insert
/// after the top-level `model_provider`, else `wire_api = "x"\n` on an empty
/// document, else insert before the first section header.
pub fn set_codex_wire_api(config_text: &str, wire_api: CodexWireApi) -> String {
    let normalized_text = normalize_toml_text(config_text);
    let mut lines = split_lines(&normalized_text);
    let target_section_name = get_codex_provider_section_name(&normalized_text);
    let replacement_line = format!("wire_api = \"{}\"", wire_api.as_str());
    let recoverable = get_recoverable_assignments(
        find_toml_assignments(&lines, &TOML_WIRE_API_PATTERN),
        target_section_name.as_deref(),
    );

    if let Some(target) = &target_section_name {
        let mut target_range = get_toml_section_range(&lines, target);
        let target_match = target_range.and_then(|range| {
            find_toml_assignment_in_range(
                &lines,
                &TOML_WIRE_API_PATTERN,
                range.body_start_index,
                range.body_end_index,
                Some(target),
            )
        });

        if let Some(found) = target_match {
            lines[found.index] = replacement_line;
            return finalize_toml_text(&lines);
        }

        if recoverable.len() == 1 {
            lines.remove(recoverable[0].index);
            target_range = get_toml_section_range(&lines, target);
        }

        if let Some(range) = target_range {
            let insert_index = get_toml_section_insert_index(&lines, &range);
            lines.insert(insert_index, replacement_line);
            return finalize_toml_text(&lines);
        }

        if lines.last().is_some_and(|last| !last.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push(format!("[{target}]"));
        lines.push(replacement_line);
        return finalize_toml_text(&lines);
    }

    let top_level_end_index = get_top_level_end_index(&lines);
    if let Some(found) =
        find_toml_assignment_in_range(&lines, &TOML_WIRE_API_PATTERN, 0, top_level_end_index, None)
    {
        lines[found.index] = replacement_line;
        return finalize_toml_text(&lines);
    }

    if let Some(model_provider_index) = get_top_level_model_provider_line_index(&lines) {
        lines.insert(model_provider_index + 1, replacement_line);
        return finalize_toml_text(&lines);
    }

    if lines.is_empty() {
        return format!("{replacement_line}\n");
    }

    lines.insert(top_level_end_index, replacement_line);
    finalize_toml_text(&lines)
}

// ========== Codex Goal mode ==========

/// Port of `isCodexGoalModeEnabled` (`providerConfigUtils.ts`).
///
/// Empty → `false`. Prefers a real parse (`features.goals === true`); when the
/// document does not parse, scans the `[features]` section for
/// `goals = true|false`. Missing section / key → `false`.
pub fn is_codex_goal_mode_enabled(config_text: &str) -> bool {
    let text = normalize_toml_text(config_text);
    if text.is_empty() {
        return false;
    }

    if let Some(parsed) = parse_toml(&text) {
        return parsed
            .get("features")
            .and_then(|features| features.get("goals"))
            == Some(&TomlValue::Boolean(true));
    }

    let lines = split_lines(&text);
    let Some(range) = get_toml_section_range(&lines, "features") else {
        return false;
    };
    let Some(index) = find_toml_line_in_range(
        &lines,
        &TOML_GOALS_FEATURE_PATTERN,
        range.body_start_index,
        range.body_end_index,
    ) else {
        return false;
    };
    TOML_GOALS_FEATURE_PATTERN
        .captures(&lines[index])
        .and_then(|caps| caps.get(1))
        .is_some_and(|m| m.as_str() == "true")
}

/// Port of `setCodexGoalMode` (`providerConfigUtils.ts`).
///
/// With a `[features]` table: enabling rewrites an existing `goals = …` line
/// **preserving indentation and trailing comment**, or inserts `goals = true`
/// at the section end; disabling deletes the `goals` line and then the whole
/// table when no non-blank body line remains (comments count as content).
/// Without the table: disabling returns the normalised text unchanged;
/// enabling inserts `[features]\ngoals = true` before the first section
/// header, blank-line separated on both sides as needed.
pub fn set_codex_goal_mode(config_text: &str, enabled: bool) -> String {
    let normalized_text = normalize_toml_text(config_text);
    let mut lines = split_lines(&normalized_text);
    let feature_range = get_toml_section_range(&lines, "features");

    if let Some(range) = feature_range {
        let goal_line_index = find_toml_line_in_range(
            &lines,
            &TOML_GOALS_FEATURE_REPLACE_PATTERN,
            range.body_start_index,
            range.body_end_index,
        );

        if enabled {
            match goal_line_index {
                Some(index) => {
                    if let Some(caps) = TOML_GOALS_FEATURE_REPLACE_PATTERN.captures(&lines[index]) {
                        lines[index] = format!("{}true{}", &caps[1], &caps[3]);
                    }
                }
                None => {
                    let insert_index = get_toml_section_insert_index(&lines, &range);
                    lines.insert(insert_index, "goals = true".to_string());
                }
            }
            return finalize_toml_text(&lines);
        }

        if let Some(index) = goal_line_index {
            lines.remove(index);
            if let Some(range) = get_toml_section_range(&lines, "features") {
                if !has_toml_section_body_content(&lines, &range) {
                    lines.drain(range.header_line_index..range.body_end_index);
                }
            }
        }
        return finalize_toml_text(&lines);
    }

    if !enabled {
        return normalized_text;
    }

    let top_level_end_index = get_top_level_end_index(&lines);
    let mut section_lines: Vec<String> = Vec::new();
    if top_level_end_index > 0 && !lines[top_level_end_index - 1].trim().is_empty() {
        section_lines.push(String::new());
    }
    section_lines.push("[features]".to_string());
    section_lines.push("goals = true".to_string());
    if top_level_end_index < lines.len() && !lines[top_level_end_index].trim().is_empty() {
        section_lines.push(String::new());
    }

    lines.splice(top_level_end_index..top_level_end_index, section_lines);
    finalize_toml_text(&lines)
}

// ========== Codex remote compaction ==========

/// Port of `isCodexRemoteCompactionEnabled` (`providerConfigUtils.ts`).
///
/// Remote compaction is encoded as the **active, non-reserved** provider's
/// table having `name = "OpenAI"`. Empty → `false`; reserved ids → `false`
/// even if named OpenAI. Prefers a real parse, falls back to a section scan.
pub fn is_codex_remote_compaction_enabled(config_text: &str) -> bool {
    let text = normalize_toml_text(config_text);
    if text.is_empty() {
        return false;
    }

    if let Some(parsed) = parse_toml(&text) {
        let provider_id = match parsed.get("model_provider") {
            Some(TomlValue::String(id)) => id.trim(),
            _ => "",
        };
        if provider_id.is_empty() || !is_custom_codex_model_provider_id(provider_id) {
            return false;
        }
        return parsed
            .get("model_providers")
            .and_then(|providers| providers.get(provider_id))
            .and_then(|provider| provider.get("name"))
            == Some(&TomlValue::String("OpenAI".to_string()));
    }

    let lines = split_lines(&text);
    let Some(target) = get_codex_custom_provider_section_name(&text) else {
        return false;
    };
    let Some(range) = get_toml_section_range(&lines, &target) else {
        return false;
    };
    find_toml_assignment_in_range(
        &lines,
        &TOML_PROVIDER_NAME_PATTERN,
        range.body_start_index,
        range.body_end_index,
        Some(&target),
    )
    .is_some_and(|found| found.value == "OpenAI")
}

/// Port of `setCodexRemoteCompaction` (`providerConfigUtils.ts`).
///
/// No custom (non-reserved) active provider → normalised text unchanged.
/// Replacement name is `"OpenAI"` when enabling, else the trimmed
/// `fallback_provider_name`, else the active provider id, else `"custom"`.
/// An existing `name = …` line is rewritten preserving indentation and
/// trailing comment; otherwise `name = "<x>"` is inserted at the section end.
/// A missing section is created only when enabling.
pub fn set_codex_remote_compaction(
    config_text: &str,
    enabled: bool,
    fallback_provider_name: Option<&str>,
) -> String {
    let normalized_text = normalize_toml_text(config_text);
    let mut lines = split_lines(&normalized_text);
    let Some(target) = get_codex_custom_provider_section_name(&normalized_text) else {
        return normalized_text;
    };

    let target_range = get_toml_section_range(&lines, &target);
    let replacement_name = if enabled {
        "OpenAI".to_string()
    } else {
        fallback_provider_name
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .or_else(|| get_codex_model_provider_name(&normalized_text))
            .unwrap_or_else(|| "custom".to_string())
    };
    let quoted = toml_basic_string(&replacement_name);
    let replacement_line = format!("name = {quoted}");

    if let Some(range) = target_range {
        let name_line = find_toml_line_in_range(
            &lines,
            &TOML_PROVIDER_NAME_REPLACE_PATTERN,
            range.body_start_index,
            range.body_end_index,
        );

        if let Some(index) = name_line {
            if let Some(caps) = TOML_PROVIDER_NAME_REPLACE_PATTERN.captures(&lines[index]) {
                lines[index] = format!("{}{quoted}{}", &caps[1], &caps[2]);
            }
            return finalize_toml_text(&lines);
        }

        let insert_index = get_toml_section_insert_index(&lines, &range);
        lines.insert(insert_index, replacement_line);
        return finalize_toml_text(&lines);
    }

    if !enabled {
        return normalized_text;
    }

    if lines.last().is_some_and(|last| !last.trim().is_empty()) {
        lines.push(String::new());
    }
    lines.push(format!("[{target}]"));
    lines.push(replacement_line);
    if get_toml_section_range(&lines, &target).is_some() {
        finalize_toml_text(&lines)
    } else {
        normalized_text
    }
}

// ========== Codex base_url ==========

/// Port of `extractCodexBaseUrl` (`providerConfigUtils.ts`).
///
/// Active `[model_providers.<id>].base_url` → top-level `base_url` → exactly
/// one recoverable misplaced assignment. Supports `"..."` (with escapes) and
/// `'...'`; a double-quoted value is returned **as written, not unescaped**.
pub fn extract_codex_base_url(config_text: &str) -> Option<String> {
    extract_codex_provider_assignment(config_text, &TOML_BASE_URL_PATTERN)
}

/// Port of `getCodexBaseUrl` (`providerConfigUtils.ts`): reads
/// `settingsConfig.config` when it is a string and delegates to
/// [`extract_codex_base_url`]. Pass the provider's `settingsConfig` value.
pub fn get_codex_base_url(settings_config: Option<&Value>) -> Option<String> {
    let text = match settings_config.and_then(|settings| settings.get("config")) {
        Some(Value::String(config)) => config.as_str(),
        _ => "",
    };
    extract_codex_base_url(text)
}

/// Remove the listed line indexes (which must be ascending) from `lines`.
fn remove_indexes_descending(lines: &mut Vec<String>, indexes: impl Iterator<Item = usize>) {
    let mut indexes: Vec<usize> = indexes.collect();
    indexes.sort_unstable_by(|a, b| b.cmp(a));
    for index in indexes {
        lines.remove(index);
    }
}

/// Port of `setCodexBaseUrl` (`providerConfigUtils.ts`).
///
/// An empty / whitespace `base_url` **deletes**: all `base_url` lines inside
/// the active provider section if any, else a single recoverable misplaced
/// assignment, else no-op. Otherwise the value is trimmed, **all internal
/// whitespace is stripped**, and written as `base_url = "<escaped>"` (always
/// double-quoted). In the provider section the first match is replaced and
/// duplicates removed; a single misplaced assignment is moved into the
/// section; a missing section is appended. Without a provider section: the
/// first top-level match is replaced (duplicates removed), else the line is
/// inserted after the top-level `model_provider`, else `base_url = "…"\n` on
/// an empty document, else before the first section header.
pub fn set_codex_base_url(config_text: &str, base_url: &str) -> String {
    let trimmed = base_url.trim();
    let normalized_text = normalize_toml_text(config_text);
    let mut lines = split_lines(&normalized_text);
    let target_section_name = get_codex_provider_section_name(&normalized_text);
    let recoverable = get_recoverable_assignments(
        find_toml_assignments(&lines, &TOML_BASE_URL_PATTERN),
        target_section_name.as_deref(),
    );

    if trimmed.is_empty() {
        if normalized_text.is_empty() {
            return normalized_text;
        }

        if let Some(target) = &target_section_name {
            let target_matches = get_toml_section_range(&lines, target)
                .map(|range| {
                    find_toml_assignments_in_range(
                        &lines,
                        &TOML_BASE_URL_PATTERN,
                        range.body_start_index,
                        range.body_end_index,
                        Some(target),
                    )
                })
                .unwrap_or_default();
            if !target_matches.is_empty() {
                remove_indexes_descending(&mut lines, target_matches.iter().map(|m| m.index));
                return finalize_toml_text(&lines);
            }
        }

        if recoverable.len() == 1 {
            lines.remove(recoverable[0].index);
        }
        return finalize_toml_text(&lines);
    }

    let normalized_url: String = trimmed.split_whitespace().collect();
    let replacement_line = format!("base_url = {}", toml_basic_string(&normalized_url));

    if let Some(target) = &target_section_name {
        let mut target_range = get_toml_section_range(&lines, target);
        let target_matches = target_range
            .map(|range| {
                find_toml_assignments_in_range(
                    &lines,
                    &TOML_BASE_URL_PATTERN,
                    range.body_start_index,
                    range.body_end_index,
                    Some(target),
                )
            })
            .unwrap_or_default();

        if let Some((first, duplicates)) = target_matches.split_first() {
            lines[first.index] = replacement_line;
            remove_indexes_descending(&mut lines, duplicates.iter().map(|m| m.index));
            return finalize_toml_text(&lines);
        }

        if recoverable.len() == 1 {
            lines.remove(recoverable[0].index);
            target_range = get_toml_section_range(&lines, target);
        }

        if let Some(range) = target_range {
            let insert_index = get_toml_section_insert_index(&lines, &range);
            lines.insert(insert_index, replacement_line);
            return finalize_toml_text(&lines);
        }

        if lines.last().is_some_and(|last| !last.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push(format!("[{target}]"));
        lines.push(replacement_line);
        return finalize_toml_text(&lines);
    }

    let top_level_end_index = get_top_level_end_index(&lines);
    let top_level_matches = find_toml_assignments_in_range(
        &lines,
        &TOML_BASE_URL_PATTERN,
        0,
        top_level_end_index,
        None,
    );
    if let Some((first, duplicates)) = top_level_matches.split_first() {
        lines[first.index] = replacement_line;
        remove_indexes_descending(&mut lines, duplicates.iter().map(|m| m.index));
        return finalize_toml_text(&lines);
    }

    if let Some(model_provider_index) = get_top_level_model_provider_line_index(&lines) {
        lines.insert(model_provider_index + 1, replacement_line);
        return finalize_toml_text(&lines);
    }

    if lines.is_empty() {
        return format!("{replacement_line}\n");
    }

    lines.insert(top_level_end_index, replacement_line);
    finalize_toml_text(&lines)
}

// ========== Codex experimental_bearer_token ==========

/// Port of `extractCodexExperimentalBearerToken` (`providerConfigUtils.ts`).
///
/// Prefers a real parse: the active **custom** provider table's
/// `experimental_bearer_token` (trimmed, non-empty), then the top-level token.
/// Falls back to line scanning the custom provider section, then the top
/// level. Reserved provider tables (`[model_providers.openai]`) are ignored and
/// only the top-level `model_provider` counts.
pub fn extract_codex_experimental_bearer_token(config_text: &str) -> Option<String> {
    let text = normalize_toml_text(config_text);
    if text.is_empty() {
        return None;
    }

    if let Some(parsed) = parse_toml(&text) {
        let provider_name = match parsed.get("model_provider") {
            Some(TomlValue::String(name)) => Some(name.trim()).filter(|name| !name.is_empty()),
            _ => None,
        };
        let provider_token = provider_name
            .filter(|name| is_custom_codex_model_provider_id(name))
            .and_then(|name| parsed.get("model_providers")?.as_table()?.get(name))
            .and_then(|provider| provider.get("experimental_bearer_token"))
            .and_then(TomlValue::as_str)
            .map(str::trim)
            .filter(|token| !token.is_empty());
        if let Some(token) = provider_token {
            return Some(token.to_string());
        }
        let top_level_token = parsed
            .get("experimental_bearer_token")
            .and_then(TomlValue::as_str)
            .map(str::trim)
            .filter(|token| !token.is_empty());
        if let Some(token) = top_level_token {
            return Some(token.to_string());
        }
    }

    let lines = split_lines(&text);
    if let Some(target) = get_codex_custom_provider_section_name(&text) {
        if let Some(range) = get_toml_section_range(&lines, &target) {
            if let Some(found) = find_toml_assignment_in_range(
                &lines,
                &TOML_EXPERIMENTAL_BEARER_TOKEN_PATTERN,
                range.body_start_index,
                range.body_end_index,
                Some(&target),
            ) {
                return Some(found.value);
            }
        }
    }

    find_toml_assignment_in_range(
        &lines,
        &TOML_EXPERIMENTAL_BEARER_TOKEN_PATTERN,
        0,
        get_top_level_end_index(&lines),
        None,
    )
    .map(|found| found.value)
}

/// Port of `updateCodexExperimentalBearerToken` (`providerConfigUtils.ts`).
///
/// **Never creates the field.** If the text is empty or does not contain
/// `experimental_bearer_token`, or no strict-match line is found (active
/// custom provider section first, then top level), the **original**
/// `config_text` is returned untouched. An empty / whitespace `token` deletes
/// the line. Otherwise the quoted value is replaced in place, preserving
/// indentation and any trailing comment, using
/// [`escape_toml_basic_string`].
pub fn update_codex_experimental_bearer_token(config_text: &str, token: &str) -> String {
    let normalized_text = normalize_toml_text(config_text);
    if normalized_text.is_empty() || !normalized_text.contains("experimental_bearer_token") {
        return config_text.to_string();
    }

    let mut lines = split_lines(&normalized_text);
    let mut token_line_index: Option<usize> = None;
    if let Some(target) = get_codex_custom_provider_section_name(&normalized_text) {
        if let Some(range) = get_toml_section_range(&lines, &target) {
            token_line_index = find_toml_line_in_range(
                &lines,
                &TOML_EXPERIMENTAL_BEARER_TOKEN_REPLACE_PATTERN,
                range.body_start_index,
                range.body_end_index,
            );
        }
    }
    if token_line_index.is_none() {
        token_line_index = find_toml_line_in_range(
            &lines,
            &TOML_EXPERIMENTAL_BEARER_TOKEN_REPLACE_PATTERN,
            0,
            get_top_level_end_index(&lines),
        );
    }

    let Some(index) = token_line_index else {
        return config_text.to_string();
    };

    let trimmed = token.trim();
    if trimmed.is_empty() {
        lines.remove(index);
    } else {
        let escaped = escape_toml_basic_string(trimmed);
        lines[index] = match TOML_EXPERIMENTAL_BEARER_TOKEN_REPLACE_PATTERN.captures(&lines[index])
        {
            Some(caps) => format!("{}\"{escaped}\"{}", &caps[1], &caps[2]),
            None => format!("experimental_bearer_token = \"{escaped}\""),
        };
    }
    finalize_toml_text(&lines)
}

// ========== Codex model name ==========

/// Port of `findTopLevelModelLineIndex`: first strictly recognised top-level `model` line.
fn find_top_level_model_line_index(lines: &[String], top_level_end_index: usize) -> Option<usize> {
    (0..top_level_end_index.min(lines.len())).find(|&index| {
        TOML_MODEL_DOUBLE_QUOTED_PATTERN.is_match(&lines[index])
            || TOML_MODEL_SINGLE_QUOTED_PATTERN.is_match(&lines[index])
    })
}

/// Port of `extractCodexModelName` (`providerConfigUtils.ts`).
///
/// **Top-level only**: `model` keys inside any section are ignored. A
/// double-quoted value is unescaped with [`unescape_toml_basic_string`] (so it
/// round-trips [`set_codex_model_name`]); a single-quoted literal is returned
/// raw. `model = ""` yields `Some("")`. Empty input → `None`.
pub fn extract_codex_model_name(config_text: &str) -> Option<String> {
    let text = normalize_toml_text(config_text);
    if text.is_empty() {
        return None;
    }
    let lines = split_lines(&text);
    let top_level_end_index = get_top_level_end_index(&lines);
    for line in &lines[..top_level_end_index] {
        if let Some(caps) = TOML_MODEL_DOUBLE_QUOTED_PATTERN.captures(line) {
            return Some(unescape_toml_basic_string(&caps[1]));
        }
        if let Some(caps) = TOML_MODEL_SINGLE_QUOTED_PATTERN.captures(line) {
            return Some(caps[1].to_string());
        }
    }
    None
}

/// Port of `setCodexModelName` (`providerConfigUtils.ts`).
///
/// An empty / whitespace `model_name` deletes the first top-level `model` line
/// (an empty document is returned unchanged). Otherwise writes
/// `model = "<escaped>"` — escaping is a **security requirement**: model ids
/// come from remote `/models` responses and a raw interpolation could inject
/// `[mcp_servers.*]` lines. Replaces the first top-level `model` line
/// (double- or single-quoted, including `model = ""`), else inserts after the
/// top-level `model_provider`, else `model = "x"\n` on an empty document, else
/// before the first section header.
pub fn set_codex_model_name(config_text: &str, model_name: &str) -> String {
    let trimmed = model_name.trim();
    let normalized_text = normalize_toml_text(config_text);
    let mut lines = split_lines(&normalized_text);
    let top_level_end_index = get_top_level_end_index(&lines);
    let model_line_index = find_top_level_model_line_index(&lines, top_level_end_index);

    if trimmed.is_empty() {
        if normalized_text.is_empty() {
            return normalized_text;
        }
        if let Some(index) = model_line_index {
            lines.remove(index);
        }
        return finalize_toml_text(&lines);
    }

    let replacement_line = format!("model = {}", toml_basic_string(trimmed));
    if let Some(index) = model_line_index {
        lines[index] = replacement_line;
        return finalize_toml_text(&lines);
    }

    if let Some(model_provider_index) = get_top_level_model_provider_line_index(&lines) {
        lines.insert(model_provider_index + 1, replacement_line);
        return finalize_toml_text(&lines);
    }

    if lines.is_empty() {
        return format!("{replacement_line}\n");
    }

    lines.insert(top_level_end_index, replacement_line);
    finalize_toml_text(&lines)
}

// ========== Codex top-level integer fields ==========

/// `^\s*<field>\s*=\s*(\d+)\s*(?:#.*)?$` — `field` is treated as a plain key
/// (the TypeScript interpolates it into a RegExp unescaped).
fn toml_top_level_int_pattern(field: &str) -> Regex {
    re(&format!(
        r"^\s*{}\s*=\s*([0-9]+)\s*(?:#{ANY}*)?$",
        regex::escape(field)
    ))
}

/// Index and raw digits of the first matching top-level integer line.
fn find_top_level_int_match<'a>(
    lines: &'a [String],
    field_name: &str,
    top_level_end_index: usize,
) -> Option<(usize, &'a str)> {
    let pattern = toml_top_level_int_pattern(field_name);
    (0..top_level_end_index.min(lines.len())).find_map(|index| {
        let caps = pattern.captures(&lines[index])?;
        let digits = caps.get(1)?;
        Some((index, &lines[index][digits.start()..digits.end()]))
    })
}

/// Port of `extractCodexTopLevelInt` (`providerConfigUtils.ts`).
///
/// Scans the top-level region only and returns the first
/// `<field> = <digits>` value. Only non-negative decimal integers match (no
/// sign, no `_` separators, no hex). A value that does not fit `u64` yields `None`.
pub fn extract_codex_top_level_int(config_text: &str, field_name: &str) -> Option<u64> {
    let text = normalize_toml_text(config_text);
    if text.is_empty() {
        return None;
    }
    let lines = split_lines(&text);
    let top_level_end_index = get_top_level_end_index(&lines);
    let (_, digits) = find_top_level_int_match(&lines, field_name, top_level_end_index)?;
    digits.parse::<u64>().ok()
}

/// Port of `setCodexTopLevelInt` (`providerConfigUtils.ts`).
///
/// Replaces an existing top-level line with `<field> = <value>` (whole-line
/// rewrite, so a trailing comment on it is lost), else appends before the
/// first section header; `<field> = <value>\n` on an empty document.
pub fn set_codex_top_level_int(config_text: &str, field_name: &str, value: u64) -> String {
    let normalized_text = normalize_toml_text(config_text);
    let mut lines = split_lines(&normalized_text);
    let top_level_end_index = get_top_level_end_index(&lines);
    let existing =
        find_top_level_int_match(&lines, field_name, top_level_end_index).map(|(index, _)| index);
    let replacement_line = format!("{field_name} = {value}");

    if let Some(index) = existing {
        lines[index] = replacement_line;
        return finalize_toml_text(&lines);
    }

    if lines.is_empty() {
        return format!("{replacement_line}\n");
    }

    lines.insert(top_level_end_index, replacement_line);
    finalize_toml_text(&lines)
}

/// Port of `removeCodexTopLevelField` (`providerConfigUtils.ts`).
///
/// Empty document → unchanged. Deletes the first matching top-level
/// **integer** line for the field (a non-integer value is not removed).
pub fn remove_codex_top_level_field(config_text: &str, field_name: &str) -> String {
    let normalized_text = normalize_toml_text(config_text);
    if normalized_text.is_empty() {
        return normalized_text;
    }
    let mut lines = split_lines(&normalized_text);
    let top_level_end_index = get_top_level_end_index(&lines);
    if let Some((index, _)) = find_top_level_int_match(&lines, field_name, top_level_end_index) {
        lines.remove(index);
    }
    finalize_toml_text(&lines)
}

#[cfg(test)]
mod tests;
