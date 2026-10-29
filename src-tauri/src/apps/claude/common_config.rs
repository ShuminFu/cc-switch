//! Claude Code 通用配置片段提取与旧版模型字段迁移

use serde_json::Value;

use crate::error::AppError;

/// 判断一个 env / 顶层配置键名是否为凭据/机密：凡命中一律不得写入共享的
/// 通用配置片段。**故意从严**——多剥一个非机密键只是它不被共享（可恢复的小
/// 不便），漏剥一个凭据则会把密钥注入到每个供应商（不可恢复的泄漏）。因此用
/// 模式匹配覆盖整类，而非枚举具体名字（枚举永远会漏掉下一个 `*_API_KEY`）。
///
/// 覆盖：Anthropic / OpenRouter / Google / OpenAI / Gemini 等 `*_API_KEY`
/// （Claude provider 的凭据见 `Provider::resolve_usage_credentials`，确实支持
/// `OPENROUTER_API_KEY` / `GOOGLE_API_KEY` 等回退）、各类 `*_AUTH_TOKEN` /
/// 单数 `*_TOKEN`、AWS Bedrock / Vertex 凭据、以及通用 secret / password /
/// 私钥命名。
pub(crate) fn is_sensitive_config_key(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();

    // 单数 `_TOKEN` 命中 AWS_SESSION_TOKEN 等，但**不**误伤复数 `_TOKENS`
    // （CLAUDE_CODE_MAX_OUTPUT_TOKENS / MAX_THINKING_TOKENS 是正常可共享配置）。
    const SENSITIVE_SUFFIXES: &[&str] = &[
        "_API_KEY",
        "_APIKEY",
        "_AUTH_TOKEN",
        "_TOKEN",
        "_ACCESS_KEY",
        "_ACCESS_KEY_ID",
        "_KEY_ID",
        "_PRIVATE_KEY",
    ];
    const SENSITIVE_EXACT: &[&str] = &[
        "APIKEY",
        "API_KEY",
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "CREDENTIALS",
    ];
    // contains：覆盖 AWS_SECRET_ACCESS_KEY / *_CLIENT_SECRET /
    // GOOGLE_APPLICATION_CREDENTIALS / AWS_BEARER_TOKEN_BEDROCK 等变体。
    const SENSITIVE_CONTAINS: &[&str] = &[
        "SECRET",
        "PASSWORD",
        "PASSWD",
        "CREDENTIAL",
        "PRIVATE_KEY",
        "BEARER_TOKEN",
    ];

    SENSITIVE_EXACT.contains(&upper.as_str())
        || SENSITIVE_SUFFIXES.iter().any(|s| upper.ends_with(s))
        || SENSITIVE_CONTAINS.iter().any(|c| upper.contains(c))
}

/// Extract common config for Claude (JSON format)
pub(crate) fn extract_common_config(settings: &Value) -> Result<String, AppError> {
    let mut config = settings.clone();

    // 供应商专属的**非机密**字段（模型 + 端点），不应共享。凭据/机密不在此列举，
    // 改由 `is_sensitive_config_key`（模式匹配）统一剥离，新供应商的 `*_API_KEY`
    // 等无需再手工补名单即可被覆盖。
    const ENV_PROVIDER_SPECIFIC_EXCLUDES: &[&str] = &[
        "ANTHROPIC_MODEL",
        "ANTHROPIC_REASONING_MODEL", // legacy: 已废弃，但旧配置可能残留
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
        // Fable 是 v3.16.3 新增的第四档模型映射，与 haiku/sonnet/opus 同属供应商专属，
        // 不得进入通用配置片段，否则会污染其它供应商（issue #4272）。
        "ANTHROPIC_DEFAULT_FABLE_MODEL",
        "ANTHROPIC_DEFAULT_FABLE_MODEL_NAME",
        "CLAUDE_CODE_SUBAGENT_MODEL",
        // Context limits follow the actual upstream model. Sharing these
        // across providers can cap GPT/Kimi to the wrong window and make
        // Claude Code compact too early or miss the upstream limit.
        "CLAUDE_CODE_MAX_CONTEXT_TOKENS",
        "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
        "ANTHROPIC_BASE_URL",
    ];

    const TOP_LEVEL_EXCLUDES: &[&str] = &[
        "apiBaseUrl",
        // Legacy model fields
        "primaryModel",
        "smallFastModel",
    ];

    // Remove env fields: provider-specific (models/endpoint) + 任何凭据键。
    if let Some(env) = config.get_mut("env").and_then(|v| v.as_object_mut()) {
        let sensitive: Vec<String> = env
            .keys()
            .filter(|k| is_sensitive_config_key(k))
            .cloned()
            .collect();
        for key in ENV_PROVIDER_SPECIFIC_EXCLUDES {
            env.remove(*key);
        }
        for key in &sensitive {
            env.remove(key);
        }
        // If env is empty after removal, remove the env object itself
        if env.is_empty() {
            config.as_object_mut().map(|obj| obj.remove("env"));
        }
    }

    // Remove top-level fields: legacy model fields + 任何凭据键
    // （例如非标准的顶层 apiKey / api_key / *_TOKEN）。
    if let Some(obj) = config.as_object_mut() {
        let sensitive: Vec<String> = obj
            .keys()
            .filter(|k| is_sensitive_config_key(k))
            .cloned()
            .collect();
        for key in TOP_LEVEL_EXCLUDES {
            obj.remove(*key);
        }
        for key in &sensitive {
            obj.remove(key);
        }
    }

    // Check if result is empty
    if config.as_object().is_none_or(|obj| obj.is_empty()) {
        return Ok("{}".to_string());
    }

    serde_json::to_string_pretty(&config)
        .map_err(|e| AppError::Message(format!("Serialization failed: {e}")))
}

/// Normalize Claude model keys in a JSON value
///
/// Reads old key (ANTHROPIC_SMALL_FAST_MODEL), writes new keys (DEFAULT_*), and deletes old key.
pub(crate) fn normalize_models_in_value(settings: &mut Value) -> bool {
    let mut changed = false;
    let env = match settings.get_mut("env").and_then(|v| v.as_object_mut()) {
        Some(obj) => obj,
        None => return changed,
    };

    let model = env
        .get("ANTHROPIC_MODEL")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let small_fast = env
        .get("ANTHROPIC_SMALL_FAST_MODEL")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let current_haiku = env
        .get("ANTHROPIC_DEFAULT_HAIKU_MODEL")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let current_sonnet = env
        .get("ANTHROPIC_DEFAULT_SONNET_MODEL")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let current_opus = env
        .get("ANTHROPIC_DEFAULT_OPUS_MODEL")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let target_haiku = current_haiku
        .or_else(|| small_fast.clone())
        .or_else(|| model.clone());
    let target_sonnet = current_sonnet
        .or_else(|| model.clone())
        .or_else(|| small_fast.clone());
    let target_opus = current_opus
        .or_else(|| model.clone())
        .or_else(|| small_fast.clone());

    if env.get("ANTHROPIC_DEFAULT_HAIKU_MODEL").is_none() {
        if let Some(v) = target_haiku {
            env.insert(
                "ANTHROPIC_DEFAULT_HAIKU_MODEL".to_string(),
                Value::String(v),
            );
            changed = true;
        }
    }
    if env.get("ANTHROPIC_DEFAULT_SONNET_MODEL").is_none() {
        if let Some(v) = target_sonnet {
            env.insert(
                "ANTHROPIC_DEFAULT_SONNET_MODEL".to_string(),
                Value::String(v),
            );
            changed = true;
        }
    }
    if env.get("ANTHROPIC_DEFAULT_OPUS_MODEL").is_none() {
        if let Some(v) = target_opus {
            env.insert("ANTHROPIC_DEFAULT_OPUS_MODEL".to_string(), Value::String(v));
            changed = true;
        }
    }

    if env.remove("ANTHROPIC_SMALL_FAST_MODEL").is_some() {
        changed = true;
    }

    changed
}
