use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::config::{delete_file, get_home_dir, read_json_file, write_json_file};
use crate::error::AppError;
use crate::model_capabilities::{image_input_capability_from_modalities, ImageInputCapability};
use serde_json::{json, Value};
use std::fs;
use std::process::Command;
use toml_edit::DocumentMut;

pub const CC_SWITCH_CODEX_MODEL_PROVIDER_ID: &str = "custom";
/// Temporary model-provider id used while the built-in `codex-official`
/// provider is routed through CC Switch.  A dedicated id is an ownership
/// marker: unlike a generic localhost `base_url`, it can be detected and
/// cleaned up without mistaking a user's own local provider for takeover.
pub const CC_SWITCH_CODEX_OFFICIAL_PROXY_PROVIDER_ID: &str = "cc-switch-official";
pub const CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME: &str = "cc-switch-model-catalog.json";
const CODEX_PROXY_AUTH_PLACEHOLDER: &str = "PROXY_MANAGED";

/// Top-level `config.toml` key that controls Codex's built-in web-search tool.
pub(crate) const CODEX_WEB_SEARCH_FIELD: &str = "web_search";
/// Value that disables the web-search tool. Some native `/responses` gateways
/// reject a `web_search` tool with `responses_feature_not_supported` ("tool type
/// 'web_search' is not supported by this gateway phase"), so for those we write
/// this per the vendors' official Codex docs. Also doubles as cc-switch's
/// ownership sentinel: we only ever remove a `web_search` key whose value equals
/// this string, never a user's own setting.
pub(crate) const CODEX_WEB_SEARCH_DISABLED: &str = "disabled";

/// Native `/responses` gateways whose first-party models do NOT support the Codex
/// `web_search` hosted tool. A BLACKLIST (default-on): everything not listed keeps
/// Codex's default, so relays/aggregators fronting real GPT — and any unknown
/// provider — are never touched. This avoids a whitelist's dangerous failure mode
/// (a fragile "is this GPT?" heuristic wrongly keeping web_search ON → hard 400);
/// the blacklist's failure mode is the safe, recoverable one (a not-yet-listed
/// broken gateway errors once → add it here).
///
/// Matched two ways so an aggregator (e.g. SiliconFlow) fronting these vendors'
/// models is also caught:
/// - `base_url` host substring, and
/// - the model id's brand prefix (after stripping any `vendor/` path segment).
///
/// Verified 2026-06-28 doc audit — reject: MiMo (hard 400), LongCat (official
/// config ships `web_search = "disabled"`), MiniMax (tool-type enum `['function']`
/// only), and Qwen3-Coder models (百炼 marks built-in tools unsupported for
/// the coder series). Deliberately NOT listed by host: 火山方舟豆包, general
/// 阿里百炼 Qwen models that support built-in web_search, and GPT-native relays.
const CODEX_WEB_SEARCH_REJECT_HOSTS: &[&str] = &[
    "xiaomimimo.com", // Xiaomi MiMo (api.xiaomimimo.com, token-plan-cn.xiaomimimo.com)
    "longcat.chat",   // Meituan LongCat (api.longcat.chat)
    "minimax.io",     // MiniMax global (api.minimax.io)
    "minimaxi.com",   // MiniMax CN (api.minimaxi.com)
];

/// Brand prefixes of models whose native gateways reject `web_search`, matched
/// against the model id's last `/`-segment so aggregator ids like
/// `MiniMaxAI/MiniMax-M3` are caught. Exact brand names (not a fuzzy heuristic),
/// so a supporting gateway is never wrongly matched.
const CODEX_WEB_SEARCH_REJECT_MODEL_PREFIXES: &[&str] =
    &["mimo", "longcat", "minimax", "qwen3-coder"];

/// Top-level `model` id from a Codex `config.toml`.
fn codex_top_level_model(config_text: &str) -> Option<String> {
    let doc = config_text.parse::<toml::Value>().ok()?;
    doc.get("model")
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Whether a native `/responses` provider's gateway is known to reject the Codex
/// `web_search` hosted tool — by `base_url` host OR by the active model's brand
/// (so an aggregator fronting a reject vendor's model is caught too). Driven by
/// the live `config.toml`, so it applies to existing providers without a re-save.
fn codex_native_gateway_rejects_web_search(config_text: &str) -> bool {
    if let Some(base_url) = extract_codex_base_url(config_text) {
        let base_url = base_url.to_ascii_lowercase();
        if CODEX_WEB_SEARCH_REJECT_HOSTS
            .iter()
            .any(|host| base_url.contains(host))
        {
            return true;
        }
    }
    if let Some(model) = codex_top_level_model(config_text) {
        let model = model.to_ascii_lowercase();
        // Strip any aggregator "vendor/" prefix, e.g. "MiniMaxAI/MiniMax-M3"
        // or "qwen/qwen3-coder-plus".
        let model = model.rsplit('/').next().unwrap_or(model.as_str());
        if CODEX_WEB_SEARCH_REJECT_MODEL_PREFIXES
            .iter()
            .any(|prefix| model.starts_with(prefix))
        {
            return true;
        }
    }
    false
}
const CODEX_MODEL_CATALOG_TEMPLATE_SLUG: &str = "gpt-5.5";

/// Which Codex tool surface the generated model catalog should target.
///
/// - `ProxyChat`: cc-switch's proxy takes over and converts Responses<->Chat,
///   so the catalog keeps Codex's default tool set (incl. the freeform
///   `apply_patch` custom tool, which the proxy rewrites to a function tool).
/// - `NativeResponses`: Codex talks directly to a provider's native
///   `/responses` endpoint (no proxy). Such gateways (e.g. Xiaomi MiMo,
///   MiniMax) reject `type=="custom"` tools, so the catalog must suppress the
///   freeform `apply_patch` and rely on `shell_type="shell_command"` for edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexCatalogToolProfile {
    ProxyChat,
    NativeResponses,
    /// Codex talks (through cc-switch's proxy) to a native Anthropic Messages
    /// gateway. Like `NativeResponses` it must suppress Codex's freeform custom
    /// tools — the Responses→Anthropic transform keeps only `function` tools.
    /// Additionally the Codex `web_search` hosted tool is unusable on this path
    /// (the transform drops it), so it is always disabled — see
    /// `prepare_codex_config_text_with_model_catalog`.
    Anthropic,
}

impl CodexCatalogToolProfile {
    /// Pick the catalog tool profile from a provider's `apiFormat` meta value.
    ///
    /// Prefer [`crate::proxy::providers::codex::resolve_codex_catalog_tool_profile`],
    /// which also honors settings-level `apiFormat` and the TOML `wire_api` (matching
    /// the proxy router). This string-only mapping is the fallback for non-Anthropic
    /// cases.
    pub fn from_api_format(api_format: Option<&str>) -> Self {
        match api_format {
            Some("anthropic") => CodexCatalogToolProfile::Anthropic,
            // Native (direct) Responses gateways reject Codex's freeform custom
            // tools (apply_patch, etc.); strip them via the NativeResponses profile.
            Some("openai_responses") => CodexCatalogToolProfile::NativeResponses,
            _ => CodexCatalogToolProfile::ProxyChat,
        }
    }
}

/// Reserved built-in provider IDs from OpenAI Codex's config/model-provider
/// catalog. Keep in sync with Codex `RESERVED_MODEL_PROVIDER_IDS` and legacy
/// removed provider aliases.
const CODEX_RESERVED_MODEL_PROVIDER_IDS: &[&str] = &[
    "amazon-bedrock",
    "openai",
    "ollama",
    "lmstudio",
    "oss",
    "ollama-chat",
];

/// Codex config directory and where it came from (CFG-12).
pub fn resolve_codex_config_dir() -> (PathBuf, crate::config::ConfigDirSource) {
    crate::config::resolve_tool_dir(
        crate::settings::get_codex_override_dir(),
        crate::config::tool_env_dir("CODEX_HOME").map(|dir| ("CODEX_HOME", dir)),
        || get_home_dir().join(".codex"),
    )
}

/// 获取 Codex 配置目录路径
///
/// cc-switch 设置覆盖 → `CODEX_HOME` → `~/.codex`（与 Codex CLI 自身一致）。
pub fn get_codex_config_dir() -> PathBuf {
    resolve_codex_config_dir().0
}

/// 获取 Codex auth.json 路径
pub fn get_codex_auth_path() -> PathBuf {
    get_codex_config_dir().join("auth.json")
}

/// 获取 Codex config.toml 路径
pub fn get_codex_config_path() -> PathBuf {
    get_codex_config_dir().join("config.toml")
}

pub fn get_codex_model_catalog_path() -> PathBuf {
    get_codex_config_dir().join(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME)
}

/// Journal of the previous `auth.json` / `config.toml` bytes, kept while a
/// two-file Codex write is in progress (CFG-27). If cc-switch dies between
/// the two renames, the next Codex read/write finds the journal and restores
/// both files, so auth and config never stay from different providers.
#[derive(serde::Serialize, serde::Deserialize)]
struct CodexLiveJournal {
    auth_path: PathBuf,
    config_path: PathBuf,
    auth: Option<String>,
    config: Option<String>,
}

fn codex_live_journal_path() -> PathBuf {
    crate::config::get_app_config_dir()
        .join("journal")
        .join("codex-live.json")
}

fn restore_journaled_file(path: &Path, content: &Option<String>) -> Result<(), AppError> {
    match content {
        Some(text) => crate::config::atomic_write_secret(path, text.as_bytes()),
        None => {
            if path.exists() {
                delete_file(path)?;
            }
            Ok(())
        }
    }
}

/// Restore Codex live files from an interrupted two-file write. Returns
/// whether a journal was found.
pub fn recover_codex_live_journal() -> Result<bool, AppError> {
    let journal_path = codex_live_journal_path();
    let Ok(raw) = fs::read_to_string(&journal_path) else {
        return Ok(false);
    };
    match serde_json::from_str::<CodexLiveJournal>(&raw) {
        Ok(journal) => {
            log::warn!("Restoring Codex auth.json/config.toml from an interrupted switch");
            restore_journaled_file(&journal.auth_path, &journal.auth)?;
            restore_journaled_file(&journal.config_path, &journal.config)?;
        }
        Err(e) => log::warn!("Discarding unreadable Codex live journal: {e}"),
    }
    let _ = fs::remove_file(&journal_path);
    Ok(true)
}

/// Write Codex `auth.json` and `config.toml` as one unit: the previous bytes
/// of both are journaled first, a failure of the second write restores both,
/// and a crash in between is repaired by [`recover_codex_live_journal`].
pub fn write_codex_live_atomic(
    auth: &Value,
    config_text_opt: Option<&str>,
) -> Result<(), AppError> {
    let auth_path = get_codex_auth_path();
    let config_path = get_codex_config_path();

    if let Some(parent) = auth_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    recover_codex_live_journal()?;

    // 准备写入内容
    let cfg_text = match config_text_opt {
        Some(s) => s.to_string(),
        None => String::new(),
    };
    if !cfg_text.trim().is_empty() {
        toml::from_str::<toml::Table>(&cfg_text).map_err(|e| AppError::toml(&config_path, e))?;
    }

    let read_old = |path: &Path| -> Result<Option<String>, AppError> {
        match fs::read(path) {
            Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(AppError::io(path, e)),
        }
    };
    let journal = CodexLiveJournal {
        auth_path: auth_path.clone(),
        config_path: config_path.clone(),
        auth: read_old(&auth_path)?,
        config: read_old(&config_path)?,
    };
    let journal_path = codex_live_journal_path();
    // the journal holds credentials: owner-only
    crate::config::atomic_write_secret(
        &journal_path,
        serde_json::to_string(&journal)
            .map_err(|e| AppError::JsonSerialize { source: e })?
            .as_bytes(),
    )?;

    let result = crate::config::write_json_file_secret(&auth_path, auth)
        .and_then(|()| crate::config::write_text_file_secret(&config_path, &cfg_text));
    if let Err(e) = result {
        if let Err(restore_err) = restore_journaled_file(&auth_path, &journal.auth)
            .and_then(|()| restore_journaled_file(&config_path, &journal.config))
        {
            log::error!("Failed to roll back Codex live files: {restore_err}");
            // keep the journal so the next start retries the restore
            return Err(e);
        }
        let _ = fs::remove_file(&journal_path);
        return Err(e);
    }
    let _ = fs::remove_file(&journal_path);
    Ok(())
}

/// 读取 `~/.codex/config.toml`，若不存在返回空字符串
pub fn read_codex_config_text() -> Result<String, AppError> {
    let path = get_codex_config_path();
    if path.exists() {
        std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))
    } else {
        Ok(String::new())
    }
}

/// 对非空的 TOML 文本进行语法校验
pub fn validate_config_toml(text: &str) -> Result<(), AppError> {
    if text.trim().is_empty() {
        return Ok(());
    }
    toml::from_str::<toml::Table>(text)
        .map(|_| ())
        .map_err(|e| AppError::toml(Path::new("config.toml"), e))
}

/// 读取并校验 `~/.codex/config.toml`，返回文本（可能为空）
pub fn read_and_validate_codex_config_text() -> Result<String, AppError> {
    let s = read_codex_config_text()?;
    validate_config_toml(&s)?;
    Ok(s)
}

fn active_codex_model_provider_id(doc: &DocumentMut) -> Option<String> {
    doc.get("model_provider")
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

pub(crate) fn is_custom_codex_model_provider_id(id: &str) -> bool {
    let id = id.trim();
    !id.is_empty()
        && !CODEX_RESERVED_MODEL_PROVIDER_IDS
            .iter()
            .any(|reserved| reserved.eq_ignore_ascii_case(id))
}

/// Write only Codex `config.toml` for provider switching.
///
/// Codex login state lives in `auth.json`; provider routing, endpoint, model,
/// and provider-scoped bearer tokens live in `config.toml`. Provider switches
/// should not overwrite the user's ChatGPT login cache.
pub fn write_codex_live_config_atomic(config_text_opt: Option<&str>) -> Result<(), AppError> {
    let config_path = get_codex_config_path();
    let cfg_text = match config_text_opt {
        Some(config_text) => config_text.to_string(),
        None => String::new(),
    };

    if !cfg_text.trim().is_empty() {
        toml::from_str::<toml::Table>(&cfg_text).map_err(|e| AppError::toml(&config_path, e))?;
    }

    crate::config::write_text_file_secret(&config_path, &cfg_text)
}

pub fn extract_codex_auth_api_key(auth: &Value) -> Option<String> {
    auth.get("OPENAI_API_KEY")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(str::to_string)
}

pub fn extract_codex_api_key(auth: Option<&Value>, config_text: Option<&str>) -> Option<String> {
    auth.and_then(extract_codex_auth_api_key)
        .or_else(|| config_text.and_then(extract_codex_experimental_bearer_token))
}

/// Extract the upstream base URL from a Codex `config.toml` string.
///
/// Prefers the active `[model_providers.<model_provider>].base_url`, falling
/// back to a top-level `base_url`. Deliberately never reads a non-active
/// `[model_providers.*]` section — the frontend `extractCodexBaseUrl`
/// (`getRecoverableBaseUrlAssignments`) excludes those too, and a leftover
/// section unrelated to the active provider must not leak into `{{baseUrl}}`.
pub fn extract_codex_base_url(config_text: &str) -> Option<String> {
    let doc = config_text.parse::<toml::Value>().ok()?;

    if let Some(active_provider) = doc.get("model_provider").and_then(|v| v.as_str()) {
        if let Some(base_url) = doc
            .get("model_providers")
            .and_then(|providers| providers.get(active_provider))
            .and_then(|provider| provider.get("base_url"))
            .and_then(|v| v.as_str())
        {
            return Some(base_url.to_string());
        }
    }

    doc.get("base_url")
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
}

pub fn codex_auth_has_login_material(auth: &Value) -> bool {
    let Some(obj) = auth.as_object() else {
        return false;
    };

    obj.iter().any(|(key, value)| {
        if key == "auth_mode" {
            return false;
        }

        if key == "OPENAI_API_KEY" {
            return value
                .as_str()
                .map(str::trim)
                .is_some_and(|token| !token.is_empty());
        }

        match value {
            Value::Null => false,
            Value::String(text) => !text.trim().is_empty(),
            Value::Array(items) => !items.is_empty(),
            Value::Object(map) => !map.is_empty(),
            _ => true,
        }
    })
}

pub fn codex_auth_has_oauth_login_material(auth: &Value) -> bool {
    let Some(obj) = auth.as_object() else {
        return false;
    };

    obj.iter().any(|(key, value)| {
        if key == "auth_mode" || key == "OPENAI_API_KEY" {
            return false;
        }

        match value {
            Value::Null => false,
            Value::String(text) => !text.trim().is_empty(),
            Value::Array(items) => !items.is_empty(),
            Value::Object(map) => !map.is_empty(),
            _ => true,
        }
    })
}

pub fn should_restore_codex_provider_token_for_backfill(
    category: Option<&str>,
    template_settings: &Value,
) -> bool {
    if category == Some("official") {
        return false;
    }

    let Some(auth) = template_settings.get("auth") else {
        return true;
    };

    let has_provider_api_key = extract_codex_auth_api_key(auth).is_some();
    let has_oauth_login = codex_auth_has_oauth_login_material(auth);
    !has_oauth_login || has_provider_api_key
}

fn parse_codex_positive_u64(value: Option<&Value>) -> Option<u64> {
    match value {
        Some(Value::Number(n)) => n.as_u64().filter(|v| *v > 0),
        Some(Value::String(s)) => s.trim().parse::<u64>().ok().filter(|v| *v > 0),
        _ => None,
    }
}

fn extract_codex_top_level_u64(config_text: &str, field: &str) -> Option<u64> {
    let doc = config_text.parse::<toml::Value>().ok()?;
    doc.get(field)
        .and_then(|value| value.as_integer())
        .and_then(|value| u64::try_from(value).ok())
        .filter(|value| *value > 0)
}

fn codex_catalog_input_modalities(
    model: &str,
    declared_modalities: Option<&[String]>,
) -> Vec<String> {
    let modalities = match image_input_capability_from_modalities(model, declared_modalities) {
        ImageInputCapability::Unsupported => &["text"][..],
        ImageInputCapability::Supported | ImageInputCapability::Unknown => &["text", "image"][..],
    };
    modalities.iter().map(|item| (*item).to_string()).collect()
}

fn codex_catalog_model_entry(
    template: &Value,
    spec: &CodexCatalogModelSpec,
    priority: usize,
    profile: CodexCatalogToolProfile,
) -> Value {
    let mut entry = template.clone();
    let Some(entry_obj) = entry.as_object_mut() else {
        return json!({});
    };

    entry_obj.insert("slug".to_string(), json!(spec.model));
    entry_obj.insert("display_name".to_string(), json!(spec.display_name));
    entry_obj.insert("description".to_string(), json!(spec.display_name));
    entry_obj.insert("context_window".to_string(), json!(spec.context_window));
    entry_obj.insert("max_context_window".to_string(), json!(spec.context_window));
    entry_obj.insert("priority".to_string(), json!(1000 + priority));
    entry_obj.insert("additional_speed_tiers".to_string(), json!([]));
    entry_obj.insert("service_tiers".to_string(), json!([]));
    entry_obj.insert("availability_nux".to_string(), Value::Null);
    entry_obj.insert("upgrade".to_string(), Value::Null);

    // Image support is a model capability, not a tool-profile capability.
    // Trust hidden preset metadata first, then the confirmed text-only registry;
    // every unknown model fails open so GPT/relay aliases are never declared
    // text-only merely because a template had a conservative default.
    entry_obj.insert(
        "input_modalities".to_string(),
        json!(codex_catalog_input_modalities(
            &spec.model,
            spec.input_modalities.as_deref(),
        )),
    );

    if profile != CodexCatalogToolProfile::ProxyChat {
        // Native `/responses` and Anthropic gateways reject / drop Codex's freeform
        // `apply_patch` (type=="custom") tool. Strip any key that would make Codex
        // emit a custom/freeform tool, and rely on shell_type="shell_command" for
        // edits. Defensive even though the native template is already clean
        // (guards against template drift / an accidental gpt-5.5 clone).
        //
        // NOTE: `base_instructions` is NOT stripped — Codex's catalog parser
        // treats it as a REQUIRED field and refuses to load the file without
        // it ("missing field `base_instructions`"). The template carries a
        // neutral identity default; per-vendor official text overrides below.
        for key in [
            "apply_patch_tool_type",
            "web_search_tool_type",
            "tools",
            "model_messages",
        ] {
            entry_obj.remove(key);
        }
        entry_obj.insert("shell_type".to_string(), json!("shell_command"));

        if let Some(base_instructions) = spec
            .base_instructions
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            entry_obj.insert("base_instructions".to_string(), json!(base_instructions));
        }
        if let Some(parallel) = spec.supports_parallel_tool_calls {
            entry_obj.insert("supports_parallel_tool_calls".to_string(), json!(parallel));
        }
    }

    entry
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexCatalogModelSpec {
    model: String,
    display_name: String,
    context_window: u64,
    /// Per-row override for the native template's `supports_parallel_tool_calls`
    /// (e.g. MiniMax=true, MiMo=false). Only consulted for `NativeResponses`.
    supports_parallel_tool_calls: Option<bool>,
    /// Hidden per-row capability declaration from built-in provider metadata.
    /// When omitted, all catalog profiles consult the shared text-only model
    /// registry and otherwise default to `["text", "image"]`.
    input_modalities: Option<Vec<String>>,
    /// Per-row override for the native template's `base_instructions` (the
    /// model identity / system preamble). Carries each vendor's OFFICIAL value
    /// (e.g. MiMo "developed by Xiaomi", MiniMax "based on MiniMax-M3"); falls
    /// back to the template default when absent. Only consulted for
    /// `NativeResponses`.
    base_instructions: Option<String>,
}

fn codex_catalog_model_specs(settings: &Value, config_text: &str) -> Vec<CodexCatalogModelSpec> {
    let Some(models) = settings
        .get("modelCatalog")
        .and_then(|catalog| catalog.get("models"))
        .and_then(|models| models.as_array())
    else {
        return Vec::new();
    };

    let default_context_window =
        extract_codex_top_level_u64(config_text, "model_context_window").unwrap_or(128_000);
    let mut seen = std::collections::HashSet::new();
    let mut specs = Vec::new();

    for model_config in models {
        let Some(model) = model_config
            .get("model")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|model| !model.is_empty())
        else {
            continue;
        };

        if !seen.insert(model.to_string()) {
            continue;
        }

        let display_name = model_config
            .get("displayName")
            .or_else(|| model_config.get("display_name"))
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(model);
        let context_window = parse_codex_positive_u64(
            model_config
                .get("contextWindow")
                .or_else(|| model_config.get("context_window")),
        )
        .unwrap_or(default_context_window);

        let supports_parallel_tool_calls = model_config
            .get("supportsParallelToolCalls")
            .or_else(|| model_config.get("supports_parallel_tool_calls"))
            .and_then(|value| value.as_bool());
        let input_modalities = model_config
            .get("inputModalities")
            .or_else(|| model_config.get("input_modalities"))
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str())
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .filter(|items| !items.is_empty());

        let base_instructions = model_config
            .get("baseInstructions")
            .or_else(|| model_config.get("base_instructions"))
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string);

        specs.push(CodexCatalogModelSpec {
            model: model.to_string(),
            display_name: display_name.to_string(),
            context_window,
            supports_parallel_tool_calls,
            input_modalities,
            base_instructions,
        });
    }

    specs
}

fn find_codex_model_template(catalog: &Value) -> Option<Value> {
    catalog
        .get("models")
        .and_then(|models| models.as_array())
        .and_then(|models| {
            models.iter().find(|model| {
                model.get("slug").and_then(|slug| slug.as_str())
                    == Some(CODEX_MODEL_CATALOG_TEMPLATE_SLUG)
            })
        })
        .cloned()
}

fn load_codex_model_template_from_cache() -> Result<Option<Value>, AppError> {
    let path = get_codex_config_dir().join("models_cache.json");
    if !path.exists() {
        return Ok(None);
    }

    let text = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    let catalog: Value = serde_json::from_str(&text).map_err(|e| AppError::json(&path, e))?;
    Ok(find_codex_model_template(&catalog))
}

/// Fixed candidates for locating the `codex` CLI when it is not on the process
/// PATH (common in GUI apps launched outside a terminal).
const CODEX_CLI_FIXED_CANDIDATES: &[&str] = &[
    "codex", // PATH (all platforms)
    #[cfg(windows)]
    "codex.cmd", // npm shim on PATH: `Command` only appends `.exe` itself
    "/opt/homebrew/bin/codex", // macOS Apple Silicon Homebrew
    "/usr/local/bin/codex", // macOS Intel Homebrew / Linux
    "/home/linuxbrew/.linuxbrew/bin/codex", // Linux Homebrew
];

fn push_codex_cli_candidate(
    candidates: &mut Vec<PathBuf>,
    seen: &mut HashSet<String>,
    candidate: PathBuf,
) {
    let key = candidate.to_string_lossy().into_owned();
    if seen.insert(key) {
        candidates.push(candidate);
    }
}

fn push_existing_codex_cli_candidate(
    candidates: &mut Vec<PathBuf>,
    seen: &mut HashSet<String>,
    candidate: PathBuf,
) {
    if candidate.exists() {
        push_codex_cli_candidate(candidates, seen, candidate);
    }
}

fn push_codex_cli_candidates_from_version_dirs(
    candidates: &mut Vec<PathBuf>,
    seen: &mut HashSet<String>,
    versions_dir: PathBuf,
    suffix: &[&str],
) {
    let Ok(entries) = fs::read_dir(versions_dir) else {
        return;
    };

    let mut discovered = entries
        .filter_map(Result::ok)
        .map(|entry| {
            let mut candidate = entry.path();
            for component in suffix {
                candidate.push(component);
            }
            candidate
        })
        .filter(|candidate| candidate.exists())
        .collect::<Vec<_>>();

    // Prefer newer-looking version directories before older global installs.
    discovered.sort_by(|a, b| b.cmp(a));
    for candidate in discovered {
        push_codex_cli_candidate(candidates, seen, candidate);
    }
}

fn push_home_codex_cli_candidates(
    candidates: &mut Vec<PathBuf>,
    seen: &mut HashSet<String>,
    home: &Path,
) {
    for relative in [
        ".nvm/current/bin/codex",
        ".volta/bin/codex",
        ".asdf/shims/codex",
        ".local/share/mise/shims/codex",
        ".config/mise/shims/codex",
        ".local/bin/codex",
        ".npm-global/bin/codex",
        ".npm-packages/bin/codex",
        ".local/share/pnpm/codex",
        "Library/pnpm/codex",
    ] {
        push_existing_codex_cli_candidate(candidates, seen, home.join(relative));
    }

    push_codex_cli_candidates_from_version_dirs(
        candidates,
        seen,
        home.join(".nvm/versions/node"),
        &["bin", "codex"],
    );
    push_codex_cli_candidates_from_version_dirs(
        candidates,
        seen,
        home.join(".local/share/fnm/node-versions"),
        &["installation", "bin", "codex"],
    );
    push_codex_cli_candidates_from_version_dirs(
        candidates,
        seen,
        home.join("Library/Application Support/fnm/node-versions"),
        &["installation", "bin", "codex"],
    );
}

fn push_env_codex_cli_candidates(candidates: &mut Vec<PathBuf>, seen: &mut HashSet<String>) {
    for (env_key, suffix) in [
        ("NPM_CONFIG_PREFIX", &["bin", "codex"][..]),
        ("VOLTA_HOME", &["bin", "codex"][..]),
        ("ASDF_DATA_DIR", &["shims", "codex"][..]),
        ("MISE_DATA_DIR", &["shims", "codex"][..]),
        ("PNPM_HOME", &["codex"][..]),
    ] {
        let Some(prefix) = std::env::var_os(env_key) else {
            continue;
        };
        let mut candidate = PathBuf::from(prefix);
        for component in suffix {
            candidate.push(component);
        }
        push_existing_codex_cli_candidate(candidates, seen, candidate);
    }

    if let Some(nvm_dir) = std::env::var_os("NVM_DIR") {
        push_codex_cli_candidates_from_version_dirs(
            candidates,
            seen,
            PathBuf::from(nvm_dir).join("versions/node"),
            &["bin", "codex"],
        );
    }

    if let Some(fnm_dir) = std::env::var_os("FNM_DIR") {
        push_codex_cli_candidates_from_version_dirs(
            candidates,
            seen,
            PathBuf::from(fnm_dir).join("node-versions"),
            &["installation", "bin", "codex"],
        );
    }

    #[cfg(windows)]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            let npm_dir = PathBuf::from(appdata).join("npm");
            for name in ["codex.cmd", "codex.exe", "codex"] {
                push_existing_codex_cli_candidate(candidates, seen, npm_dir.join(name));
            }
        }
    }
}

fn codex_cli_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();

    for candidate in CODEX_CLI_FIXED_CANDIDATES {
        push_codex_cli_candidate(&mut candidates, &mut seen, PathBuf::from(candidate));
    }

    push_env_codex_cli_candidates(&mut candidates, &mut seen);
    push_home_codex_cli_candidates(&mut candidates, &mut seen, &get_home_dir());

    candidates
}

/// How long one `codex debug models --bundled` probe may run (CFG-15).
const CODEX_CLI_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Number of CLI probes started; tests assert the switch path never probes.
static CODEX_CLI_PROBES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Cached template extracted from the `codex` CLI, so a provider switch never
/// has to spawn Node (CFG-15).
fn codex_cli_template_cache_path() -> PathBuf {
    crate::config::get_app_config_dir()
        .join("cache")
        .join("codex-model-template.json")
}

fn load_codex_model_template_from_cli_cache() -> Option<Value> {
    let path = codex_cli_template_cache_path();
    let text = fs::read_to_string(&path).ok()?;
    match serde_json::from_str::<Value>(&text) {
        Ok(template) if template.get("slug").is_some() => Some(template),
        Ok(_) => None,
        Err(e) => {
            log::debug!(
                "Ignoring unreadable Codex template cache {}: {e}",
                path.display()
            );
            None
        }
    }
}

/// Run `command` capturing its output, killing it after `timeout`.
/// Returns `Ok(None)` on timeout.
fn run_with_timeout(
    mut command: Command,
    timeout: std::time::Duration,
) -> std::io::Result<Option<std::process::Output>> {
    use std::io::Read;
    use std::process::Stdio;

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    // drain the pipes on helper threads so a large catalog cannot block the child
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let out_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(pipe) = stdout.as_mut() {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(pipe) = stderr.as_mut() {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });

    let deadline = std::time::Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            // a grandchild (e.g. a Node shim's child) may still hold the
            // pipes open: leave the reader threads to finish on their own
            return Ok(None);
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    };
    let stdout = out_reader.join().unwrap_or_default();
    let stderr = err_reader.join().unwrap_or_default();
    Ok(Some(std::process::Output {
        status,
        stdout,
        stderr,
    }))
}

fn probe_codex_cli_template(timeout: std::time::Duration) -> Option<Value> {
    CODEX_CLI_PROBES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    for candidate in codex_cli_candidates() {
        let candidate_label = candidate.to_string_lossy().into_owned();
        let mut command = Command::new(&candidate);
        command.args(["debug", "models", "--bundled"]);
        let output = match run_with_timeout(command, timeout) {
            Ok(Some(output)) => output,
            Ok(None) => {
                log::warn!("`{candidate_label} debug models --bundled` timed out");
                continue;
            }
            Err(err) => {
                log::debug!("failed to run `{candidate_label} debug models --bundled`: {err}");
                continue;
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            log::debug!("`{candidate_label} debug models --bundled` failed: {stderr}");
            continue;
        }

        let catalog: Value = match serde_json::from_slice(&output.stdout) {
            Ok(catalog) => catalog,
            Err(e) => {
                log::debug!(
                    "Failed to parse `{candidate_label} debug models --bundled` output: {e}"
                );
                continue;
            }
        };
        if let Some(template) = find_codex_model_template(&catalog) {
            return Some(template);
        }
    }
    None
}

/// Probe the `codex` CLI (5 s timeout, hidden window) and refresh the cached
/// template. Runs off the switch path; safe to call from a background task.
pub fn refresh_codex_cli_template_cache() -> Result<bool, AppError> {
    let Some(template) = probe_codex_cli_template(CODEX_CLI_PROBE_TIMEOUT) else {
        return Ok(false);
    };
    let path = codex_cli_template_cache_path();
    write_json_file(&path, &template)?;
    Ok(true)
}

/// Start one background CLI probe per process when no cached template
/// exists, so later switches can use the CLI's template.
fn schedule_codex_cli_template_refresh() {
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if cfg!(test) || STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("codex-template-probe".into())
        .spawn(|| {
            if let Err(e) = refresh_codex_cli_template_cache() {
                log::debug!("Codex CLI template refresh failed: {e}");
            }
        });
    if let Err(e) = spawned {
        log::debug!("Could not start Codex CLI template probe: {e}");
    }
}

fn load_codex_model_template_static() -> Option<Value> {
    let text = include_str!("resources/gpt5_5_template.json");
    match serde_json::from_str(text) {
        Ok(template) => Some(template),
        Err(e) => {
            log::warn!("Failed to parse bundled gpt-5.5 template: {e}");
            None
        }
    }
}

/// Bundled clean template for native `/responses` providers. Unlike the
/// gpt-5.5 template it carries NO freeform `apply_patch` / `web_search` tool
/// declarations and no GPT-5 base_instructions, so Codex never emits a
/// `type=="custom"` tool that native gateways (MiMo/MiniMax/…) reject. Edits
/// flow through `shell_type="shell_command"` instead. We deliberately do NOT
/// fall back to `models_cache.json` here (that would reintroduce gpt-5.5's
/// freeform apply_patch).
fn load_codex_native_responses_template() -> Value {
    let text = include_str!("resources/codex_native_responses_template.json");
    serde_json::from_str(text).expect("bundled codex native responses template must be valid JSON")
}

fn load_codex_model_catalog_template() -> Result<Value, AppError> {
    // ① models_cache.json (created by Codex when it connects to OpenAI)
    if let Some(template) = load_codex_model_template_from_cache()? {
        return Ok(template);
    }
    // ② template previously extracted from the codex CLI. The CLI itself is
    //    never run on the switch path (CFG-15); a missing cache triggers one
    //    background probe for later switches.
    if let Some(template) = load_codex_model_template_from_cli_cache() {
        return Ok(template);
    }
    schedule_codex_cli_template_refresh();
    // ③ Static fallback bundled at compile time
    if let Some(template) = load_codex_model_template_static() {
        return Ok(template);
    }

    Err(AppError::Message(format!(
        "Codex model catalog template `{CODEX_MODEL_CATALOG_TEMPLATE_SLUG}` not found. Please start Codex once so models_cache.json is available, or ensure the `codex` CLI is on PATH."
    )))
}

fn codex_model_catalog_from_specs(
    specs: &[CodexCatalogModelSpec],
    template: &Value,
    profile: CodexCatalogToolProfile,
) -> Value {
    let entries: Vec<Value> = specs
        .iter()
        .enumerate()
        .map(|(index, spec)| codex_catalog_model_entry(template, spec, index, profile))
        .collect();

    json!({ "models": entries })
}

fn codex_model_catalog_from_settings(
    settings: &Value,
    config_text: &str,
    profile: CodexCatalogToolProfile,
) -> Result<Option<Value>, AppError> {
    let specs = codex_catalog_model_specs(settings, config_text);
    if specs.is_empty() {
        return Ok(None);
    }

    // Native providers use the bundled clean template (no freeform apply_patch,
    // no cache dependency); proxy-chat providers keep cloning Codex's gpt-5.5
    // entry so the proxy can rewrite custom<->function tools as before.
    let template = match profile {
        CodexCatalogToolProfile::NativeResponses | CodexCatalogToolProfile::Anthropic => {
            load_codex_native_responses_template()
        }
        CodexCatalogToolProfile::ProxyChat => load_codex_model_catalog_template()?,
    };
    Ok(Some(codex_model_catalog_from_specs(
        &specs, &template, profile,
    )))
}

fn set_codex_model_catalog_json_field(
    config_text: &str,
    catalog_path: Option<&Path>,
) -> Result<String, AppError> {
    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;

    match catalog_path {
        Some(_) => {
            doc["model_catalog_json"] = toml_edit::value(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME);
        }
        None => {
            let should_remove = doc
                .get("model_catalog_json")
                .and_then(|item| item.as_str())
                .map(|path| {
                    Path::new(path).file_name().and_then(|name| name.to_str())
                        == Some(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME)
                })
                .unwrap_or(false);
            if should_remove {
                doc.as_table_mut().remove("model_catalog_json");
            }
        }
    }

    Ok(doc.to_string())
}

/// Pure toggle for the top-level `web_search` field that turns Codex's built-in
/// web-search tool off. When `disable` is true we write `web_search = "disabled"`
/// (the catalog's `supports_search_tool` does NOT gate this — the request-time
/// tool comes from the config, defaulting on). When false we *remove* the field,
/// but only when it carries cc-switch's own `"disabled"` sentinel, so switching
/// back to a web-search-capable provider re-enables it without clobbering a
/// user's manual setting.
///
/// The caller decides `disable` (see `codex_native_gateway_rejects_web_search`);
/// lifecycle is bound to the cc-switch catalog pointer so the field is set/cleaned
/// up wherever the native catalog is written/removed.
fn set_codex_native_web_search_field(config_text: &str, disable: bool) -> Result<String, AppError> {
    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;

    if disable {
        doc[CODEX_WEB_SEARCH_FIELD] = toml_edit::value(CODEX_WEB_SEARCH_DISABLED);
    } else {
        let owned = doc
            .get(CODEX_WEB_SEARCH_FIELD)
            .and_then(|item| item.as_str())
            == Some(CODEX_WEB_SEARCH_DISABLED);
        if owned {
            doc.as_table_mut().remove(CODEX_WEB_SEARCH_FIELD);
        }
    }

    Ok(doc.to_string())
}

/// Generate Codex `model_catalog_json` from provider settings and inject/remove
/// the top-level TOML field that points Codex to the generated file.
pub fn prepare_codex_config_text_with_model_catalog(
    settings: &Value,
    config_text: &str,
    profile: CodexCatalogToolProfile,
) -> Result<String, AppError> {
    let catalog_path = get_codex_model_catalog_path();

    if let Some(catalog) = codex_model_catalog_from_settings(settings, config_text, profile)? {
        let config_text = set_codex_model_catalog_json_field(config_text, Some(&catalog_path))?;
        // Disable web_search only for native gateways on the reject blacklist
        // (MiMo/LongCat/MiniMax by host or model brand; Qwen3-Coder by model).
        // Everything else — relays, DouBao, web-search-capable Qwen models,
        // unknown providers — keeps Codex's default.
        let disable_web_search = match profile {
            // The Responses→Anthropic transform silently drops the Codex web_search
            // hosted tool, so always disable it here rather than present a dead tool.
            CodexCatalogToolProfile::Anthropic => true,
            CodexCatalogToolProfile::NativeResponses => {
                codex_native_gateway_rejects_web_search(&config_text)
            }
            CodexCatalogToolProfile::ProxyChat => false,
        };
        let config_text = set_codex_native_web_search_field(&config_text, disable_web_search)?;
        write_json_file(&catalog_path, &catalog)?;
        Ok(config_text)
    } else {
        let config_text = set_codex_model_catalog_json_field(config_text, None)?;
        // Even without a generated catalog, the Responses→Anthropic transform drops the
        // Codex web_search hosted tool, so keep the invariant that an Anthropic provider
        // never presents it as a dead tool.
        let disable_web_search = profile == CodexCatalogToolProfile::Anthropic;
        set_codex_native_web_search_field(&config_text, disable_web_search)
    }
}

/// Reverse of `prepare_codex_config_text_with_model_catalog`: read the
/// cc-switch–maintained catalog file referenced by `~/.codex/config.toml` and
/// convert it back into the simplified shape the frontend table uses:
/// `{ "models": [{ "model", "displayName"?, "contextWindow"?, hidden overrides... }, ...] }`.
///
/// We only reverse-parse catalogs whose `model_catalog_json` path is the
/// cc-switch–generated file (identified by filename
/// `cc-switch-model-catalog.json`). A user-managed external catalog file is
/// left alone — surfacing its richer structure as the simplified table would
/// be a downgrade we can't safely round-trip.
///
/// `displayName`, `contextWindow`, and `inputModalities` are omitted from the
/// returned entry when the on-disk value matches the fallback that
/// `codex_model_catalog_from_settings` injects for unset inputs (slug for
/// display_name, `model_context_window` or 128_000 for context_window, and the
/// shared confirmed-text-only inference for input modalities). This preserves
/// the "user left it blank" intent across round-trip; an unavoidable edge case
/// is that a user-typed value that happens to equal the fallback also collapses
/// to blank, but the next save writes the same fallback so behavior is stable.
///
/// All failure modes (missing file, parse error, no `model_catalog_json`,
/// entries without `slug`) collapse to `Ok(None)` so callers can treat this
/// as best-effort enrichment without making `read_live_settings` brittle.
pub fn read_codex_model_catalog_simplified_from_live() -> Result<Option<Value>, AppError> {
    let config_text = read_codex_config_text()?;
    let generated_path = get_codex_model_catalog_path();
    let Some(catalog_path) = resolve_cc_switch_catalog_path(&config_text, &generated_path) else {
        return Ok(None);
    };
    if !catalog_path.exists() {
        return Ok(None);
    }
    let Ok(catalog_text) = fs::read_to_string(&catalog_path) else {
        return Ok(None);
    };
    Ok(build_simplified_catalog_from_texts(
        &config_text,
        &catalog_text,
    ))
}

/// Given `config.toml` text, resolve the on-disk path of the cc-switch–owned
/// catalog file (returns `None` if `model_catalog_json` is absent or points at
/// a file we don't own). Relative paths fall back to `generated_path`.
pub(crate) fn resolve_cc_switch_catalog_path(
    config_text: &str,
    generated_path: &Path,
) -> Option<PathBuf> {
    if config_text.trim().is_empty() {
        return None;
    }
    let doc = config_text.parse::<DocumentMut>().ok()?;
    let catalog_path_str = doc
        .get("model_catalog_json")
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;

    let referenced_path = Path::new(catalog_path_str);
    let is_cc_switch_owned = referenced_path.file_name().and_then(|name| name.to_str())
        == Some(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME);
    if !is_cc_switch_owned {
        return None;
    }

    if referenced_path.is_absolute() {
        Some(referenced_path.to_path_buf())
    } else {
        Some(generated_path.to_path_buf())
    }
}

/// Pure reverse-parsing core: convert Codex catalog JSON text back into the
/// frontend's simplified model-mapping shape. Returns `None` when the catalog
/// is unparseable, has no `models` array, or yields zero valid entries.
fn build_simplified_catalog_from_texts(config_text: &str, catalog_text: &str) -> Option<Value> {
    let catalog: Value = serde_json::from_str(catalog_text).ok()?;
    let models = catalog.get("models").and_then(|m| m.as_array())?;

    let default_context_window =
        extract_codex_top_level_u64(config_text, "model_context_window").unwrap_or(128_000);

    let mut entries = Vec::with_capacity(models.len());
    for entry in models {
        let Some(model) = entry
            .get("slug")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };

        let mut obj = serde_json::Map::new();
        obj.insert("model".to_string(), json!(model));

        if let Some(display_name) = entry
            .get("display_name")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty() && *s != model)
        {
            obj.insert("displayName".to_string(), json!(display_name));
        }

        if let Some(context_window) = entry
            .get("context_window")
            .and_then(|v| v.as_u64())
            .filter(|v| *v > 0 && *v != default_context_window)
        {
            obj.insert("contextWindow".to_string(), json!(context_window));
        }

        // Preserve native-profile per-row overrides so a DB-SSOT-missing
        // fallback round-trip doesn't silently drop them.
        if let Some(parallel) = entry
            .get("supports_parallel_tool_calls")
            .and_then(|v| v.as_bool())
        {
            obj.insert("supportsParallelToolCalls".to_string(), json!(parallel));
        }
        if let Some(modalities) = entry.get("input_modalities").and_then(|v| v.as_array()) {
            let mods: Vec<String> = modalities
                .iter()
                .filter_map(|m| m.as_str())
                .map(str::to_string)
                .collect();
            let inferred = codex_catalog_input_modalities(model, None);
            if !mods.is_empty() && mods != inferred {
                obj.insert("inputModalities".to_string(), json!(mods));
            }
        }

        entries.push(Value::Object(obj));
    }

    if entries.is_empty() {
        return None;
    }

    Some(json!({ "models": entries }))
}

/// Decide the `config.toml` text to write during a takeover-off restore,
/// projecting the model catalog **only when `settings` carries an inline
/// `modelCatalog`**.
///
/// Restore feeds back a stored backup, and Codex backups come in two shapes that
/// need opposite handling:
///
/// - **Snapshot backup** (`read_codex_live_settings`): `{ auth, config }` with no
///   inline `modelCatalog`. Its `config.toml` text already carries whatever
///   `model_catalog_json` pointer existed at backup time, and the generated
///   catalog file on disk is untouched. Here we must keep the config **raw** —
///   running catalog projection would see "no specs" and strip the live pointer.
/// - **Provider-rebuilt backup** (`update_live_backup_from_provider`): the DB
///   provider's settings, i.e. `{ auth, config (no pointer), modelCatalog
///   (inline DB SSOT) }`. Here the pointer/catalog file must be (re)generated
///   from the inline `modelCatalog`, or the mapping is lost on restore.
///
/// Gating on the presence of the inline `modelCatalog` key routes each shape
/// correctly; an empty inline catalog still projects (and so correctly drops a
/// now-stale pointer), while an absent key leaves the text untouched. This is
/// **orthogonal to auth** — a provider-rebuilt backup can pair an inline
/// `modelCatalog` with empty `auth.json` (the API key living in the config's
/// `experimental_bearer_token`), so the caller must decide config projection
/// independently of whether it writes or deletes `auth.json`.
pub fn prepare_codex_live_config_text_with_optional_catalog(
    settings: &Value,
    config_text: &str,
    profile: CodexCatalogToolProfile,
) -> Result<String, AppError> {
    if settings.get("modelCatalog").is_some() {
        prepare_codex_config_text_with_model_catalog(settings, config_text, profile)
    } else {
        Ok(config_text.to_string())
    }
}

pub fn write_codex_provider_live_with_catalog(
    settings: &Value,
    category: Option<&str>,
    auth: &Value,
    config_text: Option<&str>,
    profile: CodexCatalogToolProfile,
) -> Result<(), AppError> {
    let prepared_config = config_text
        .map(|text| prepare_codex_config_text_with_model_catalog(settings, text, profile))
        .transpose()?;

    write_codex_live_for_provider(category, auth, prepared_config.as_deref())
}

/// Extract a provider-scoped `experimental_bearer_token` from Codex `config.toml`.
///
/// Mobile compat: third-party providers may store the API key inside
/// `[model_providers.<id>].experimental_bearer_token` while keeping the
/// user's ChatGPT login cache intact in `auth.json`. Falls back to the
/// top-level `experimental_bearer_token` when no active model provider is set.
pub fn extract_codex_experimental_bearer_token(config_text: &str) -> Option<String> {
    if !config_text.contains("experimental_bearer_token") {
        return None;
    }
    let doc = config_text.parse::<DocumentMut>().ok()?;
    let provider_id = active_codex_model_provider_id(&doc);

    let top_level_token = || {
        doc.get("experimental_bearer_token")
            .and_then(|item| item.as_str())
    };
    let token = match provider_id.as_deref() {
        Some(id) if is_custom_codex_model_provider_id(id) => doc
            .get("model_providers")
            .and_then(|item| item.as_table())
            .and_then(|table| table.get(id))
            .and_then(|item| item.as_table())
            .and_then(|table| table.get("experimental_bearer_token"))
            .and_then(|item| item.as_str())
            .or_else(top_level_token),
        Some(_) => top_level_token(),
        None => top_level_token(),
    };

    token
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
}

fn set_codex_experimental_bearer_token(config_text: &str, token: &str) -> Result<String, AppError> {
    if config_text.trim().is_empty() {
        return Err(AppError::localized(
            "provider.codex.config.missing",
            "Codex 第三方供应商缺少 config.toml 配置，无法写入 bearer token",
            "Codex third-party provider is missing config.toml, cannot write bearer token",
        ));
    }

    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;

    let Some(provider_id) = active_codex_model_provider_id(&doc) else {
        doc["experimental_bearer_token"] = toml_edit::value(token);
        return Ok(doc.to_string());
    };

    if !is_custom_codex_model_provider_id(&provider_id) {
        // Reserved Codex provider IDs are owned by the CLI. Keep third-party
        // bearer tokens at the top level so we do not shadow built-in tables.
        doc["experimental_bearer_token"] = toml_edit::value(token);
        return Ok(doc.to_string());
    }

    if let Some(model_providers) = doc
        .get_mut("model_providers")
        .and_then(|item| item.as_table_mut())
    {
        if let Some(provider_table) = model_providers
            .get_mut(provider_id.as_str())
            .and_then(|item| item.as_table_mut())
        {
            provider_table["experimental_bearer_token"] = toml_edit::value(token);
            return Ok(doc.to_string());
        }
    }

    doc["experimental_bearer_token"] = toml_edit::value(token);
    Ok(doc.to_string())
}

pub fn remove_codex_experimental_bearer_token_if(
    config_text: &str,
    predicate: impl Fn(&str) -> bool,
) -> Result<String, AppError> {
    if config_text.trim().is_empty() || !config_text.contains("experimental_bearer_token") {
        return Ok(config_text.to_string());
    }

    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;

    if let Some(provider_id) = active_codex_model_provider_id(&doc) {
        if let Some(provider_table) = doc
            .get_mut("model_providers")
            .and_then(|item| item.as_table_mut())
            .and_then(|table| table.get_mut(provider_id.as_str()))
            .and_then(|item| item.as_table_mut())
        {
            let should_remove = provider_table
                .get("experimental_bearer_token")
                .and_then(|item| item.as_str())
                .map(str::trim)
                .is_some_and(&predicate);
            if should_remove {
                provider_table.remove("experimental_bearer_token");
            }
        }
    }

    let should_remove_top_level = doc
        .get("experimental_bearer_token")
        .and_then(|item| item.as_str())
        .map(str::trim)
        .is_some_and(&predicate);
    if should_remove_top_level {
        doc.as_table_mut().remove("experimental_bearer_token");
    }
    Ok(doc.to_string())
}

fn remove_codex_experimental_bearer_token(config_text: &str) -> Result<String, AppError> {
    remove_codex_experimental_bearer_token_if(config_text, |_| true)
}

/// Read the current Codex live settings as a `{ auth, config }` object.
///
/// Missing `auth.json` collapses to `{}` so a config-only third-party install
/// is still importable; both files empty is treated as "no live install".
pub fn read_codex_live_settings() -> Result<Value, AppError> {
    let auth_path = get_codex_auth_path();
    let auth_present = auth_path.exists();
    let auth: Value = if auth_present {
        read_json_file(&auth_path)?
    } else {
        json!({})
    };
    let cfg_text = read_and_validate_codex_config_text()?;
    if !auth_present && cfg_text.trim().is_empty() {
        return Err(AppError::localized(
            "codex.live.missing",
            "Codex 配置文件不存在",
            "Codex configuration is missing",
        ));
    }
    Ok(json!({ "auth": auth, "config": cfg_text }))
}

/// `[model_providers.custom]` entry that makes an official (ChatGPT OAuth)
/// provider behave like Codex's built-in `openai` entry while running under
/// the shared custom id: `requires_openai_auth` routes auth to the ChatGPT
/// login in `auth.json` (base_url then defaults to the official Codex
/// backend), `name = "OpenAI"` keeps Codex's `is_openai()` feature gates
/// (web search, remote compaction), and `supports_websockets` restores the
/// built-in default that custom entries otherwise lose.
fn codex_official_provider_table(
    base_url: Option<&str>,
    supports_websockets: bool,
) -> toml_edit::Table {
    let mut table = toml_edit::Table::new();
    table["name"] = toml_edit::value("OpenAI");
    table["requires_openai_auth"] = toml_edit::value(true);
    table["supports_websockets"] = toml_edit::value(supports_websockets);
    table["wire_api"] = toml_edit::value("responses");
    if let Some(base_url) = base_url {
        table["base_url"] = toml_edit::value(base_url.trim_end_matches('/'));
    }
    table
}

fn codex_unified_official_provider_table() -> toml_edit::Table {
    codex_official_provider_table(None, true)
}

fn remove_codex_proxy_placeholders_from_providers(providers: &mut dyn toml_edit::TableLike) {
    for (_, item) in providers.iter_mut() {
        if let Some(table) = item.as_table_mut() {
            let should_remove = table
                .get("experimental_bearer_token")
                .and_then(|item| item.as_str())
                == Some(CODEX_PROXY_AUTH_PLACEHOLDER);
            if should_remove {
                table.remove("experimental_bearer_token");
            }
        } else if let Some(table) = item.as_inline_table_mut() {
            let should_remove = table
                .get("experimental_bearer_token")
                .and_then(|value| value.as_str())
                == Some(CODEX_PROXY_AUTH_PLACEHOLDER);
            if should_remove {
                table.remove("experimental_bearer_token");
            }
        }
    }
}

/// Project the built-in Codex official provider through the local proxy while
/// keeping authentication owned by Codex itself.
///
/// The resulting custom provider explicitly opts into OpenAI authentication,
/// so Codex forwards its existing ChatGPT login to the local `/responses`
/// endpoint.  No API key or bearer placeholder is written to `auth.json`.
pub fn apply_codex_official_proxy_route(
    config_text: &str,
    proxy_base_url: &str,
) -> Result<String, AppError> {
    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;

    // A third-party takeover may have left the proxy placeholder in config.toml.
    // The official route must use Codex's native OpenAI login instead.
    doc.as_table_mut().remove("experimental_bearer_token");
    doc["model_provider"] = toml_edit::value(CC_SWITCH_CODEX_OFFICIAL_PROXY_PROVIDER_ID);

    // The local proxy currently exposes HTTP/SSE, not Codex websocket routes.
    let table = codex_official_provider_table(Some(proxy_base_url), false);

    // Edit `model_providers` in place, keeping its position and its inline or
    // standard form, so removing the route restores the original text (CFG-33).
    if !doc.contains_key("model_providers") {
        let mut providers = toml_edit::Table::new();
        providers.set_implicit(true);
        doc.insert("model_providers", toml_edit::Item::Table(providers));
    }
    let item = doc.get_mut("model_providers").expect("inserted above");
    let is_inline = item.is_inline_table();
    let providers = item.as_table_like_mut().ok_or_else(|| {
        AppError::Message("Invalid Codex config.toml: model_providers must be a table".to_string())
    })?;

    // Clean only CC Switch's placeholder from every stale provider table. Real
    // user bearer tokens are preserved, as are all unrelated provider fields.
    remove_codex_proxy_placeholders_from_providers(providers);

    let route = if is_inline {
        toml_edit::value(table.into_inline_table())
    } else {
        toml_edit::Item::Table(table)
    };
    providers.insert(CC_SWITCH_CODEX_OFFICIAL_PROXY_PROVIDER_ID, route);
    Ok(doc.to_string())
}

/// Whether a live Codex config is the official route projected by CC Switch.
pub fn codex_config_has_official_proxy_route(config_text: &str) -> bool {
    if !config_text.contains(CC_SWITCH_CODEX_OFFICIAL_PROXY_PROVIDER_ID) {
        return false;
    }
    config_text
        .parse::<DocumentMut>()
        .ok()
        .and_then(|doc| {
            doc.get("model_provider")
                .and_then(|item| item.as_str())
                .map(str::to_string)
        })
        .as_deref()
        == Some(CC_SWITCH_CODEX_OFFICIAL_PROXY_PROVIDER_ID)
}

/// Remove only the official takeover route owned by CC Switch. This is a
/// last-resort crash cleanup when no live backup or provider SSOT is usable.
pub fn remove_codex_official_proxy_route(config_text: &str) -> Result<String, AppError> {
    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;
    if doc.get("model_provider").and_then(|item| item.as_str())
        != Some(CC_SWITCH_CODEX_OFFICIAL_PROXY_PROVIDER_ID)
    {
        return Ok(config_text.to_string());
    }

    doc.as_table_mut().remove("model_provider");
    let mut now_empty = false;
    if let Some(item) = doc.get_mut("model_providers") {
        let providers = item.as_table_like_mut().ok_or_else(|| {
            AppError::Message(
                "Invalid Codex config.toml: model_providers must be a table".to_string(),
            )
        })?;
        providers.remove(CC_SWITCH_CODEX_OFFICIAL_PROXY_PROVIDER_ID);
        remove_codex_proxy_placeholders_from_providers(providers);
        now_empty = providers.is_empty();
    }
    if now_empty {
        doc.as_table_mut().remove("model_providers");
    }
    Ok(doc.to_string())
}

fn table_matches_codex_unified_official_provider(table: &toml_edit::Table) -> bool {
    table.len() == 4
        && table.get("name").and_then(|item| item.as_str()) == Some("OpenAI")
        && table
            .get("requires_openai_auth")
            .and_then(|item| item.as_bool())
            == Some(true)
        && table
            .get("supports_websockets")
            .and_then(|item| item.as_bool())
            == Some(true)
        && table.get("wire_api").and_then(|item| item.as_str()) == Some("responses")
}

/// 统一 Codex 会话历史：把官方供应商的 live 配置改写为以共享的
/// `custom` model_provider 标识运行（认证仍走 `auth.json` 的 ChatGPT 登录），
/// 使开关开启后创建的官方会话与第三方会话共用同一个 resume 历史桶。
///
/// 两种情况拒绝注入、原样返回：
/// - 配置已有显式 `model_provider`：用户手工指定的路由不被覆盖；
/// - 配置已有形态不同的 `[model_providers.custom]` 表：设置 `model_provider`
///   会激活这张我们不认识的表（可能带第三方 base_url/token，会把 ChatGPT
///   OAuth 流量路由到错误后端），宁可让开关对该配置不生效。
pub fn inject_codex_unified_session_bucket(config_text: &str) -> Result<String, AppError> {
    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;

    if doc.get("model_provider").is_some() {
        return Ok(config_text.to_string());
    }

    let existing_custom_conflicts = doc
        .get("model_providers")
        .and_then(|item| item.as_table())
        .and_then(|providers| providers.get(CC_SWITCH_CODEX_MODEL_PROVIDER_ID))
        .and_then(|item| item.as_table())
        .is_some_and(|table| !table_matches_codex_unified_official_provider(table));
    if existing_custom_conflicts {
        log::warn!(
            "官方 Codex 配置已存在自定义 [model_providers.custom]，跳过统一会话路由注入以避免激活未知路由"
        );
        return Ok(config_text.to_string());
    }

    doc["model_provider"] = toml_edit::value(CC_SWITCH_CODEX_MODEL_PROVIDER_ID);

    if doc.get("model_providers").is_none() {
        let mut parent = toml_edit::Table::new();
        parent.set_implicit(true);
        doc["model_providers"] = toml_edit::Item::Table(parent);
    }
    if let Some(providers) = doc["model_providers"].as_table_mut() {
        if !providers.contains_key(CC_SWITCH_CODEX_MODEL_PROVIDER_ID) {
            providers.insert(
                CC_SWITCH_CODEX_MODEL_PROVIDER_ID,
                toml_edit::Item::Table(codex_unified_official_provider_table()),
            );
        }
    }
    Ok(doc.to_string())
}

/// `inject_codex_unified_session_bucket` 的反向操作：从配置文本里剥掉注入的
/// 统一会话路由，保证切换回填不会把它带进数据库的存储配置（关闭开关后
/// 切换即可完全还原）。仅当形态与注入产物完全一致时才剥离；第三方模板和
/// 用户自定义的 `custom` 条目（带 base_url 等差异字段）原样保留。
pub fn strip_codex_unified_session_bucket(config_text: &str) -> Result<String, AppError> {
    if !config_text.contains("model_provider") {
        return Ok(config_text.to_string());
    }
    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;

    if doc.get("model_provider").and_then(|item| item.as_str())
        != Some(CC_SWITCH_CODEX_MODEL_PROVIDER_ID)
    {
        return Ok(config_text.to_string());
    }
    let matches_injected = doc
        .get("model_providers")
        .and_then(|item| item.as_table())
        .and_then(|providers| providers.get(CC_SWITCH_CODEX_MODEL_PROVIDER_ID))
        .and_then(|item| item.as_table())
        .is_some_and(table_matches_codex_unified_official_provider);
    if !matches_injected {
        return Ok(config_text.to_string());
    }

    doc.as_table_mut().remove("model_provider");
    let providers_empty = doc["model_providers"]
        .as_table_mut()
        .map(|providers| {
            providers.remove(CC_SWITCH_CODEX_MODEL_PROVIDER_ID);
            providers.is_empty()
        })
        .unwrap_or(false);
    if providers_empty {
        doc.as_table_mut().remove("model_providers");
    }
    Ok(doc.to_string())
}

/// 统一会话开关开启时，把官方供应商 `{ auth, config }` 设置对象中的
/// config 文本注入共享 custom 路由；开关关闭或非官方供应商时不做改动。
///
/// 普通 live 写入（`write_codex_live_for_provider`）与代理接管备份
/// （`update_live_backup_from_provider`）两条落盘路径共用：接管期间
/// live 归代理所有，注入必须进备份，接管释放恢复的 live 才带统一路由。
pub fn apply_codex_unified_session_bucket_to_settings(
    category: Option<&str>,
    settings: &mut Value,
) -> Result<(), AppError> {
    if category != Some("official") || !crate::settings::unify_codex_session_history() {
        return Ok(());
    }
    let config_text = settings
        .get("config")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let injected = inject_codex_unified_session_bucket(&config_text)?;
    if injected != config_text {
        if let Some(obj) = settings.as_object_mut() {
            obj.insert("config".to_string(), Value::String(injected));
        }
    }
    Ok(())
}

/// Backfill helper: strip the unified-session injection from a live
/// `{ auth, config }` settings object before it is stored back to the DB.
pub fn strip_codex_unified_session_bucket_from_settings(
    settings: &mut Value,
) -> Result<(), AppError> {
    let Some(config_text) = settings
        .get("config")
        .and_then(|value| value.as_str())
        .map(str::to_string)
    else {
        return Ok(());
    };
    let stripped = strip_codex_unified_session_bucket(&config_text)?;
    if stripped != config_text {
        if let Some(obj) = settings.as_object_mut() {
            obj.insert("config".to_string(), Value::String(stripped));
        }
    }
    Ok(())
}

/// Backfill helper: strip `[mcp_servers]` from a live `{ auth, config }`
/// settings object before it is stored back to the DB.
///
/// MCP 服务器的 SSOT 是 DB 的 mcp_servers 表，live `config.toml` 里的
/// `[mcp_servers]` 只是每次写 live 之后由 MCP 同步重新投影的产物。若回填时
/// 烙进供应商存储配置，已在应用里删除的服务器会随下次激活该供应商被写回
/// live，而逐条 reconcile 只认识 DB 现存条目、永远清不掉这种孤儿。
pub fn strip_codex_mcp_servers_from_settings(settings: &mut Value) -> Result<(), AppError> {
    let Some(config_text) = settings
        .get("config")
        .and_then(|value| value.as_str())
        .map(str::to_string)
    else {
        return Ok(());
    };
    if !config_text.contains("mcp") {
        return Ok(());
    }
    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;
    let mut changed = doc.as_table_mut().remove("mcp_servers").is_some();
    // 历史错误格式 [mcp.servers] 一并清理（live 侧 MCP 同步也做同样迁移）
    if let Some(mcp_tbl) = doc.get_mut("mcp").and_then(|item| item.as_table_like_mut()) {
        if mcp_tbl.remove("servers").is_some() {
            changed = true;
        }
        if mcp_tbl.is_empty() {
            doc.as_table_mut().remove("mcp");
        }
    }
    if changed {
        if let Some(obj) = settings.as_object_mut() {
            obj.insert("config".to_string(), Value::String(doc.to_string()));
        }
    }
    Ok(())
}

/// Route a Codex live write between full auth+config or config-only.
///
/// Official providers with usable login material own `auth.json`. Third-party
/// providers only touch `config.toml` when the compatibility setting is enabled
/// so the user's ChatGPT login cache survives provider switches.
///
/// 统一会话开关开启时，官方配置在落盘前注入共享的 `custom` 路由
/// （见 `inject_codex_unified_session_bucket`）。
pub fn write_codex_live_for_provider(
    category: Option<&str>,
    auth: &Value,
    config_text: Option<&str>,
) -> Result<(), AppError> {
    let unified_official_config =
        if category == Some("official") && crate::settings::unify_codex_session_history() {
            Some(inject_codex_unified_session_bucket(
                config_text.unwrap_or(""),
            )?)
        } else {
            None
        };
    let config_text = unified_official_config.as_deref().or(config_text);

    let should_write_auth = (category == Some("official") && codex_auth_has_login_material(auth))
        || (category != Some("official")
            && !crate::settings::preserve_codex_official_auth_on_switch());

    if should_write_auth {
        if category != Some("official") {
            // Compat mode replaces auth.json wholesale. Keep a recoverable copy
            // of a ChatGPT login cache before it is overwritten (CFG-38).
            let auth_path = get_codex_auth_path();
            if read_json_file::<Value>(&auth_path)
                .is_ok_and(|live| codex_auth_has_oauth_login_material(&live))
            {
                if let Some(backup) = crate::config::backup_live_file("codex", &auth_path) {
                    log::warn!(
                        "Codex auth.json held a ChatGPT login; backed up to {} before the third-party switch",
                        backup.display()
                    );
                }
            }
        }
        write_codex_live_atomic(auth, config_text)
    } else {
        if category == Some("official") {
            strip_foreign_codex_api_key(auth)?;
        }
        let live_config = prepare_codex_provider_live_config(auth, config_text.unwrap_or(""))?;
        write_codex_live_config_atomic(Some(&live_config))
    }
}

/// Switching to an official provider whose stored `auth` carries no login
/// material keeps `auth.json` — but an `OPENAI_API_KEY` left there by an
/// earlier third-party write would make Codex send that key to OpenAI. Drop
/// it (the previous file is backed up first); OAuth tokens stay (CFG-38).
fn strip_foreign_codex_api_key(official_auth: &Value) -> Result<(), AppError> {
    let auth_path = get_codex_auth_path();
    let Ok(mut live) = read_json_file::<Value>(&auth_path) else {
        return Ok(());
    };
    let Some(live_key) = extract_codex_auth_api_key(&live) else {
        return Ok(());
    };
    if extract_codex_auth_api_key(official_auth).as_deref() == Some(live_key.as_str()) {
        return Ok(());
    }
    if let Some(obj) = live.as_object_mut() {
        obj.remove("OPENAI_API_KEY");
    }
    crate::config::backup_live_file("codex", &auth_path);
    crate::config::write_json_file_secret(&auth_path, &live)?;
    log::info!(
        "Removed a third-party OPENAI_API_KEY from Codex auth.json for the official provider"
    );
    Ok(())
}

/// Build the live Codex config for provider switching.
///
/// The stored provider keeps its API key in `auth.OPENAI_API_KEY`. Live Codex
/// requests can use a provider-scoped `experimental_bearer_token`, so switching
/// providers only needs to update `config.toml`; `auth.json` stays as the user's
/// long-lived ChatGPT login cache.
pub fn prepare_codex_provider_live_config(
    auth: &Value,
    config_text: &str,
) -> Result<String, AppError> {
    let token = extract_codex_auth_api_key(auth)
        .or_else(|| extract_codex_experimental_bearer_token(config_text));

    Ok(match token {
        Some(token) => set_codex_experimental_bearer_token(config_text, &token)?,
        None => config_text.to_string(),
    })
}

/// During DB backfill, lift a live `experimental_bearer_token` back into
/// `auth.OPENAI_API_KEY` so the stored provider keeps its canonical shape
/// and generated live tokens don't leak into stored provider TOML.
///
/// Only intervenes when the live config actually carries a bearer token —
/// otherwise the function is a no-op so the caller's normal backfill path
/// (which keeps live `auth` as the authoritative source) is unaffected.
pub fn restore_codex_provider_token_for_backfill(
    settings: &mut Value,
    template_settings: &Value,
) -> Result<(), AppError> {
    let Some(config_text) = settings
        .get("config")
        .and_then(|value| value.as_str())
        .map(str::to_string)
    else {
        return Ok(());
    };

    let Some(token) = extract_codex_experimental_bearer_token(&config_text) else {
        return Ok(());
    };

    let cleaned_config = remove_codex_experimental_bearer_token(&config_text)?;

    if let Some(obj) = settings.as_object_mut() {
        obj.insert("config".to_string(), Value::String(cleaned_config));

        let mut auth = template_settings
            .get("auth")
            .filter(|value| value.is_object())
            .cloned()
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
        if let Some(auth_obj) = auth.as_object_mut() {
            auth_obj.insert("OPENAI_API_KEY".to_string(), Value::String(token));
        }
        obj.insert("auth".to_string(), auth);
    }

    Ok(())
}

pub fn restore_codex_settings_for_backfill(
    settings: &mut Value,
    template_settings: &Value,
    restore_provider_token: bool,
) -> Result<(), AppError> {
    if restore_provider_token {
        restore_codex_provider_token_for_backfill(settings, template_settings)?;
    }
    Ok(())
}

/// Update a field in Codex config.toml using toml_edit (syntax-preserving).
///
/// Supported fields:
/// - `"base_url"`: writes to `[model_providers.<current>].base_url` if `model_provider` exists,
///   otherwise falls back to top-level `base_url`.
/// - `"wire_api"`: writes to `[model_providers.<current>].wire_api` if `model_provider` exists,
///   otherwise falls back to top-level `wire_api`.
/// - `"model"` / `"model_catalog_json"`: writes to top-level field.
///
/// Empty value removes the field.
pub fn update_codex_toml_field(toml_str: &str, field: &str, value: &str) -> Result<String, String> {
    let mut doc = toml_str
        .parse::<DocumentMut>()
        .map_err(|e| format!("TOML parse error: {e}"))?;

    let trimmed = value.trim();

    match field {
        "base_url" | "wire_api" => {
            let model_provider = doc
                .get("model_provider")
                .and_then(|item| item.as_str())
                .map(str::to_string);

            if let Some(provider_key) = model_provider {
                // Ensure [model_providers] table exists
                if doc.get("model_providers").is_none() {
                    doc["model_providers"] = toml_edit::table();
                }

                if let Some(model_providers) = doc["model_providers"].as_table_mut() {
                    // Ensure [model_providers.<provider_key>] table exists
                    if !model_providers.contains_key(&provider_key) {
                        model_providers[&provider_key] = toml_edit::table();
                    }

                    if let Some(provider_table) = model_providers[&provider_key].as_table_mut() {
                        if trimmed.is_empty() {
                            provider_table.remove(field);
                        } else {
                            provider_table[field] = toml_edit::value(trimmed);
                        }
                        return Ok(doc.to_string());
                    }
                }
            }

            // Fallback: no model_provider or structure mismatch → top-level field
            if trimmed.is_empty() {
                doc.as_table_mut().remove(field);
            } else {
                doc[field] = toml_edit::value(trimmed);
            }
        }
        "model" | "model_catalog_json" => {
            if trimmed.is_empty() {
                doc.as_table_mut().remove(field);
            } else {
                doc[field] = toml_edit::value(trimmed);
            }
        }
        _ => return Err(format!("unsupported field: {field}")),
    }

    Ok(doc.to_string())
}

/// Remove `base_url` from the active model_provider section only if it matches `predicate`.
/// Also removes top-level `base_url` if it matches.
/// Used by proxy cleanup to strip local proxy URLs without touching user-configured URLs.
pub fn remove_codex_toml_base_url_if(toml_str: &str, predicate: impl Fn(&str) -> bool) -> String {
    let mut doc = match toml_str.parse::<DocumentMut>() {
        Ok(doc) => doc,
        Err(_) => return toml_str.to_string(),
    };

    let model_provider = doc
        .get("model_provider")
        .and_then(|item| item.as_str())
        .map(str::to_string);

    if let Some(provider_key) = model_provider {
        if let Some(model_providers) = doc
            .get_mut("model_providers")
            .and_then(|v| v.as_table_mut())
        {
            if let Some(provider_table) = model_providers
                .get_mut(provider_key.as_str())
                .and_then(|v| v.as_table_mut())
            {
                let should_remove = provider_table
                    .get("base_url")
                    .and_then(|item| item.as_str())
                    .map(&predicate)
                    .unwrap_or(false);
                if should_remove {
                    provider_table.remove("base_url");
                }
            }
        }
    }

    // Fallback: also clean up top-level base_url if it matches
    let should_remove_root = doc
        .get("base_url")
        .and_then(|item| item.as_str())
        .map(&predicate)
        .unwrap_or(false);
    if should_remove_root {
        doc.as_table_mut().remove("base_url");
    }

    doc.to_string()
}

#[cfg(test)]
mod tests;
