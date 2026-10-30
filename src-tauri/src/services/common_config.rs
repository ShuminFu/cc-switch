//! 通用配置片段（common config snippet）的读写（CCS-181 自命令层迁入）
//!
//! 写入片段前校验格式；替换 claude/codex/gemini 的片段时先迁移旧片段的用法，
//! 写入后把当前供应商重新同步到 live 配置；omo / omo-slim 片段在有当前
//! 供应商时重写其配置文件。

use std::str::FromStr;

use crate::app_config::AppType;
use crate::error::AppError;
use crate::services::omo::{OmoVariant, SLIM, STANDARD};
use crate::services::{OmoService, ProviderService};
use crate::settings;
use crate::store::AppState;

/// 按界面语言给出 JSON 格式错误
pub fn invalid_json_format_error(error: serde_json::Error) -> AppError {
    let lang = settings::get_settings()
        .language
        .unwrap_or_else(|| "zh".to_string());

    AppError::Message(match lang.as_str() {
        "en" => format!("Invalid JSON format: {error}"),
        "ja" => format!("JSON形式が無効です: {error}"),
        _ => format!("无效的 JSON 格式: {error}"),
    })
}

fn invalid_toml_format_error(error: toml_edit::TomlError) -> AppError {
    let lang = settings::get_settings()
        .language
        .unwrap_or_else(|| "zh".to_string());

    AppError::Message(match lang.as_str() {
        "en" => format!("Invalid TOML format: {error}"),
        "ja" => format!("TOML形式が無効です: {error}"),
        _ => format!("无效的 TOML 格式: {error}"),
    })
}

fn validate_common_config_snippet(app_type: &str, snippet: &str) -> Result<(), AppError> {
    if snippet.trim().is_empty() {
        return Ok(());
    }

    match app_type {
        "claude" | "gemini" | "omo" | "omo-slim" => {
            serde_json::from_str::<serde_json::Value>(snippet)
                .map_err(invalid_json_format_error)?;
        }
        "codex" => {
            snippet
                .parse::<toml_edit::DocumentMut>()
                .map_err(invalid_toml_format_error)?;
        }
        _ => {}
    }

    Ok(())
}

/// 保存的通用配置片段（未设置时为 None）
pub fn get_snippet(state: &AppState, app_type: &str) -> Result<Option<String>, AppError> {
    state.db.get_config_snippet(app_type)
}

/// 写入片段与"已清空"标记；空白片段视为清空
fn store_snippet(state: &AppState, app_type: &str, snippet: String) -> Result<(), AppError> {
    let is_cleared = snippet.trim().is_empty();
    let value = if is_cleared { None } else { Some(snippet) };
    state.db.set_config_snippet(app_type, value)?;
    state.db.set_config_snippet_cleared(app_type, is_cleared)
}

/// 旧版 Claude 专用入口：只校验 JSON 并写入，不做迁移与同步
pub fn set_claude_snippet(state: &AppState, snippet: String) -> Result<(), AppError> {
    if !snippet.trim().is_empty() {
        serde_json::from_str::<serde_json::Value>(&snippet).map_err(invalid_json_format_error)?;
    }
    store_snippet(state, "claude", snippet)
}

/// 保存通用配置片段并把变更传播到依赖它的 live 配置
pub fn set_snippet(state: &AppState, app_type: &str, snippet: String) -> Result<(), AppError> {
    let old_snippet = state.db.get_config_snippet(app_type)?;
    validate_common_config_snippet(app_type, &snippet)?;

    let syncs_live = matches!(app_type, "claude" | "codex" | "gemini");
    if syncs_live {
        if let Some(legacy_snippet) = old_snippet
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            let app = AppType::from_str(app_type)?;
            ProviderService::migrate_legacy_common_config_usage(state, app, legacy_snippet)?;
        }
    }

    store_snippet(state, app_type, snippet)?;

    if syncs_live {
        ProviderService::sync_current_provider_for_app(state, AppType::from_str(app_type)?)?;
    }
    match app_type {
        "omo" => rewrite_omo_config_if_current(state, &STANDARD),
        "omo-slim" => rewrite_omo_config_if_current(state, &SLIM),
        _ => Ok(()),
    }
}

fn rewrite_omo_config_if_current(state: &AppState, variant: &OmoVariant) -> Result<(), AppError> {
    if state
        .db
        .get_current_omo_provider("opencode", variant.category)?
        .is_some()
    {
        OmoService::write_config_to_file(state, variant)?;
    }
    Ok(())
}

/// 从供应商配置中抽取通用片段：传入编辑器里的配置时就地抽取，否则读当前 live
pub fn extract_snippet(
    state: &AppState,
    app_type: &str,
    settings_config: Option<String>,
) -> Result<String, AppError> {
    let app = AppType::from_str(app_type)?;
    if let Some(settings_config) = settings_config.filter(|s| !s.trim().is_empty()) {
        let settings: serde_json::Value =
            serde_json::from_str(&settings_config).map_err(invalid_json_format_error)?;
        return ProviderService::extract_common_config_snippet_from_settings(app, &settings);
    }
    ProviderService::extract_common_config_snippet(state, app)
}

#[cfg(test)]
mod tests {
    use super::validate_common_config_snippet;

    #[test]
    fn validate_common_config_snippet_accepts_comment_only_codex_snippet() {
        validate_common_config_snippet("codex", "# comment only\n")
            .expect("comment-only codex snippet should be valid");
    }

    #[test]
    fn validate_common_config_snippet_rejects_invalid_codex_snippet() {
        let err = validate_common_config_snippet("codex", "[broken")
            .expect_err("invalid codex snippet should be rejected")
            .to_string();
        assert!(
            err.contains("TOML") || err.contains("toml") || err.contains("格式"),
            "expected TOML validation error, got {err}"
        );
    }
}
