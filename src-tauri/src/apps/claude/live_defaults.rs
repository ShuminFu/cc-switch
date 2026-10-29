//! 只注入到 Claude Code live 配置的上下文窗口默认值
//!
//! 写 live 时由 [`super::ClaudeAdapter`] 的 `apply_live_defaults` 注入，切走回填时
//! 由 `backfill_live_settings` 对称剥掉，不进入供应商的存储配置。

use serde_json::{json, Value};

use crate::provider::Provider;

/// ChatGPT Codex catalogs gpt-5.6 at a 372K context window with a ~353K
/// effective budget (openai/codex#31860), far below the 1.05M API spec.
/// Declare the catalog window for both knobs: Claude Code's built-in output
/// reserve and compact buffer already keep the actual compact trigger
/// (~278K-339K) below the effective budget, so anything lower only wastes
/// usable context.
const CODEX_OAUTH_CLAUDE_MAX_CONTEXT_TOKENS: &str = "372000";
const CODEX_OAUTH_CLAUDE_AUTO_COMPACT_WINDOW: &str = "372000";
const KIMI_FOR_CODING_CONTEXT_TOKENS: &str = "262144";

/// Model env keys Claude Code may route requests through. The defaults above
/// are calibrated against gpt-5.6's Codex catalog, so every configured model
/// must belong to that family before they are injected — gpt-5.5's upstream
/// catalog oscillates between 272K and 372K and must not inherit them.
const CODEX_OAUTH_MODEL_ENV_KEYS: [&str; 6] = [
    "ANTHROPIC_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_FABLE_MODEL",
    "CLAUDE_CODE_SUBAGENT_MODEL",
];

fn provider_env_targets_gpt56(provider_env: Option<&serde_json::Map<String, Value>>) -> bool {
    let Some(env) = provider_env else {
        return false;
    };
    let mut saw_model = false;
    for key in CODEX_OAUTH_MODEL_ENV_KEYS {
        let Some(value) = env.get(key) else {
            continue;
        };
        let Some(model) = value.as_str() else {
            return false;
        };
        let model = model.trim();
        if model.is_empty() {
            continue;
        }
        saw_model = true;
        if !model.to_ascii_lowercase().starts_with("gpt-5.6") {
            return false;
        }
    }
    saw_model
}

fn is_kimi_for_coding_provider(provider: &Provider) -> bool {
    provider
        .settings_config
        .pointer("/env/ANTHROPIC_BASE_URL")
        .and_then(Value::as_str)
        .map(str::trim)
        .map(|url| url.trim_end_matches('/'))
        == Some("https://api.kimi.com/coding")
}

/// Claude Code assigns unknown non-Claude model ids a 200K context window.
/// Codex OAuth deliberately exposes GPT ids through Claude Code, so enrich the
/// effective live settings for both newly-created and already-saved providers.
/// Explicit user values always win; the defaults are only injected when every
/// configured model targets gpt-5.6.
pub(crate) fn apply_codex_oauth_claude_context_defaults(settings: &mut Value, provider: &Provider) {
    if !provider.is_codex_oauth() {
        return;
    }

    // Read provider-owned values before mutably borrowing the effective
    // settings. This also deliberately prevents a legacy common-config
    // snippet from overriding model-specific context limits.
    let provider_env = provider
        .settings_config
        .get("env")
        .and_then(Value::as_object);
    let Some(root) = settings.as_object_mut() else {
        return;
    };
    let env = root.entry("env".to_string()).or_insert_with(|| json!({}));
    let Some(env) = env.as_object_mut() else {
        log::warn!(
            "Cannot apply Codex OAuth Claude context defaults for '{}': env is not an object",
            provider.id
        );
        return;
    };

    let inject_defaults = provider_env_targets_gpt56(provider_env);
    for (key, default_value) in [
        (
            "CLAUDE_CODE_MAX_CONTEXT_TOKENS",
            CODEX_OAUTH_CLAUDE_MAX_CONTEXT_TOKENS,
        ),
        (
            "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
            CODEX_OAUTH_CLAUDE_AUTO_COMPACT_WINDOW,
        ),
    ] {
        match provider_env.and_then(|provider_env| provider_env.get(key)) {
            Some(value) => {
                env.insert(key.to_string(), value.clone());
            }
            None if inject_defaults => {
                env.insert(key.to_string(), Value::String(default_value.to_string()));
            }
            // 老模型不注入默认值，同时剥掉遗留共享片段可能带进来的值
            None => {
                env.remove(key);
            }
        }
    }
}

/// Kimi For Coding serves a 256K window, but Claude Code caps unknown models at
/// 200K unless `CLAUDE_CODE_MAX_CONTEXT_TOKENS` is set — and that env is ignored
/// for `claude-`-prefixed ids, so these defaults only bite when the provider also
/// routes the endpoint's `kimi-for-coding` alias (the preset does). Keep the
/// defaults provider-owned so an old shared snippet cannot override them.
pub(crate) fn apply_kimi_for_coding_context_defaults(settings: &mut Value, provider: &Provider) {
    if !is_kimi_for_coding_provider(provider) {
        return;
    }

    let provider_env = provider
        .settings_config
        .get("env")
        .and_then(Value::as_object);
    let Some(env) = settings.get_mut("env").and_then(Value::as_object_mut) else {
        return;
    };

    for key in [
        "CLAUDE_CODE_MAX_CONTEXT_TOKENS",
        "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
    ] {
        let value = provider_env
            .and_then(|provider_env| provider_env.get(key))
            .cloned()
            .unwrap_or_else(|| Value::String(KIMI_FOR_CODING_CONTEXT_TOKENS.to_string()));
        env.insert(key.to_string(), value);
    }
}

/// 与 `apply_codex_oauth_claude_context_defaults` 严格对称：注入产物只活在
/// live，切走回填时必须剥掉，否则程序默认值会固化成供应商的"用户显式值"，
/// 之后调整默认值或更换模型时旧值永远压住新默认。仅当"注入会发生且注入的
/// 就是这个值、且存储配置本来没有显式值"时才剥；用户显式存储的值和手改
/// live 成其他数字的值都保留。
pub(crate) fn strip_injected_codex_oauth_context_defaults(
    settings: &mut Value,
    provider: &Provider,
) {
    if !provider.is_codex_oauth() {
        return;
    }
    let provider_env = provider
        .settings_config
        .get("env")
        .and_then(Value::as_object);
    if !provider_env_targets_gpt56(provider_env) {
        return;
    }
    let Some(env) = settings.get_mut("env").and_then(Value::as_object_mut) else {
        return;
    };
    for (key, default_value) in [
        (
            "CLAUDE_CODE_MAX_CONTEXT_TOKENS",
            CODEX_OAUTH_CLAUDE_MAX_CONTEXT_TOKENS,
        ),
        (
            "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
            CODEX_OAUTH_CLAUDE_AUTO_COMPACT_WINDOW,
        ),
    ] {
        let stored_explicit = provider_env.is_some_and(|e| e.contains_key(key));
        if stored_explicit {
            continue;
        }
        if env.get(key).and_then(Value::as_str) == Some(default_value) {
            env.remove(key);
        }
    }
}

pub(crate) fn strip_injected_kimi_for_coding_context_defaults(
    settings: &mut Value,
    provider: &Provider,
) {
    if !is_kimi_for_coding_provider(provider) {
        return;
    }
    let provider_env = provider
        .settings_config
        .get("env")
        .and_then(Value::as_object);
    let Some(env) = settings.get_mut("env").and_then(Value::as_object_mut) else {
        return;
    };
    for key in [
        "CLAUDE_CODE_MAX_CONTEXT_TOKENS",
        "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
    ] {
        if provider_env.is_some_and(|provider_env| provider_env.contains_key(key)) {
            continue;
        }
        if env.get(key).and_then(Value::as_str) == Some(KIMI_FOR_CODING_CONTEXT_TOKENS) {
            env.remove(key);
        }
    }
}
