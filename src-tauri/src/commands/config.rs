#![allow(non_snake_case)]

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::app_config::AppType;
use crate::codex_config;
use crate::config::{self, get_claude_settings_path, ConfigStatus};
use crate::settings;
use crate::store::AppState;

#[tauri::command]
pub async fn get_claude_config_status() -> Result<ConfigStatus, String> {
    Ok(config::get_claude_config_status())
}

use std::str::FromStr;

/// 当前提示语言：优先用户设置，未设置（首次安装）时按系统区域推断，
/// 未知区域回退到英文（与前端 `getInitialLanguage` 和托盘一致）。
fn current_message_language() -> String {
    settings::get_settings()
        .language
        .unwrap_or_else(|| crate::tray::detect_system_tray_language().to_string())
}

fn format_error_message(lang: &str, format: &str, error: &dyn std::fmt::Display) -> String {
    match lang {
        "zh" => format!("无效的 {format} 格式: {error}"),
        "zh-TW" => format!("無效的 {format} 格式: {error}"),
        "ja" => format!("{format}形式が無効です: {error}"),
        _ => format!("Invalid {format} format: {error}"),
    }
}

fn invalid_json_format_error(error: serde_json::Error) -> String {
    format_error_message(&current_message_language(), "JSON", &error)
}

fn invalid_toml_format_error(error: toml_edit::TomlError) -> String {
    format_error_message(&current_message_language(), "TOML", &error)
}

fn validate_common_config_snippet(app_type: &str, snippet: &str) -> Result<(), String> {
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

#[tauri::command]
pub async fn get_config_status(
    state: State<'_, AppState>,
    app: String,
) -> Result<ConfigStatus, String> {
    match AppType::from_str(&app).map_err(|e| e.to_string())? {
        AppType::Claude => Ok(config::get_claude_config_status()),
        AppType::ClaudeDesktop => {
            let status = crate::claude_desktop_config::get_status(
                state.db.as_ref(),
                state.proxy_service.is_running().await,
            )
            .map_err(|e| e.to_string())?;
            Ok(ConfigStatus {
                exists: status.configured,
                path: status.config_library_path.unwrap_or_default(),
            })
        }
        AppType::Codex => {
            let auth_path = codex_config::get_codex_auth_path();
            let config_text = codex_config::read_codex_config_text().unwrap_or_default();
            let exists = auth_path.exists() || !config_text.trim().is_empty();
            let path = codex_config::get_codex_config_dir()
                .to_string_lossy()
                .to_string();

            Ok(ConfigStatus { exists, path })
        }
        AppType::Gemini => {
            let env_path = crate::gemini_config::get_gemini_env_path();
            let exists = env_path.exists();
            let path = crate::gemini_config::get_gemini_dir()
                .to_string_lossy()
                .to_string();

            Ok(ConfigStatus { exists, path })
        }
        AppType::GrokBuild => {
            let config_path = crate::grok_config::get_grok_config_path();
            let exists = config_path.exists();
            let path = crate::grok_config::get_grok_config_dir()
                .to_string_lossy()
                .to_string();

            Ok(ConfigStatus { exists, path })
        }
        AppType::OpenCode => {
            let config_path = crate::opencode_config::get_opencode_config_path();
            let exists = config_path.exists();
            let path = crate::opencode_config::get_opencode_dir()
                .to_string_lossy()
                .to_string();

            Ok(ConfigStatus { exists, path })
        }
        AppType::OpenClaw => {
            let config_path = crate::openclaw_config::get_openclaw_config_path();
            let exists = config_path.exists();
            let path = crate::openclaw_config::get_openclaw_dir()
                .to_string_lossy()
                .to_string();

            Ok(ConfigStatus { exists, path })
        }
        AppType::Hermes => {
            let config_path = crate::hermes_config::get_hermes_config_path();
            let exists = config_path.exists();
            let path = crate::hermes_config::get_hermes_dir()
                .to_string_lossy()
                .to_string();

            Ok(ConfigStatus { exists, path })
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_claude_code_config_path() -> Result<String, String> {
    Ok(get_claude_settings_path().to_string_lossy().to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn get_config_dir(app: String) -> Result<String, String> {
    let dir = match AppType::from_str(&app).map_err(|e| e.to_string())? {
        AppType::Claude => config::get_claude_config_dir(),
        AppType::ClaudeDesktop => {
            crate::claude_desktop_config::get_config_library_path().map_err(|e| e.to_string())?
        }
        AppType::Codex => codex_config::get_codex_config_dir(),
        AppType::Gemini => crate::gemini_config::get_gemini_dir(),
        AppType::GrokBuild => crate::grok_config::get_grok_config_dir(),
        AppType::OpenCode => crate::opencode_config::get_opencode_dir(),
        AppType::OpenClaw => crate::openclaw_config::get_openclaw_dir(),
        AppType::Hermes => crate::hermes_config::get_hermes_dir(),
    };

    Ok(dir.to_string_lossy().to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn open_config_folder(handle: AppHandle, app: String) -> Result<bool, String> {
    let config_dir = match AppType::from_str(&app).map_err(|e| e.to_string())? {
        AppType::Claude => config::get_claude_config_dir(),
        AppType::ClaudeDesktop => {
            crate::claude_desktop_config::get_config_library_path().map_err(|e| e.to_string())?
        }
        AppType::Codex => codex_config::get_codex_config_dir(),
        AppType::Gemini => crate::gemini_config::get_gemini_dir(),
        AppType::GrokBuild => crate::grok_config::get_grok_config_dir(),
        AppType::OpenCode => crate::opencode_config::get_opencode_dir(),
        AppType::OpenClaw => crate::openclaw_config::get_openclaw_dir(),
        AppType::Hermes => crate::hermes_config::get_hermes_dir(),
    };

    if !config_dir.exists() {
        std::fs::create_dir_all(&config_dir).map_err(|e| format!("创建目录失败: {e}"))?;
    }

    handle
        .opener()
        .open_path(config_dir.to_string_lossy().to_string(), None::<String>)
        .map_err(|e| format!("打开文件夹失败: {e}"))?;

    Ok(true)
}

#[tauri::command]
#[specta::specta]
pub async fn pick_directory(
    app: AppHandle,
    #[allow(non_snake_case)] defaultPath: Option<String>,
) -> Result<Option<String>, String> {
    let initial = defaultPath
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty());

    let result = tauri::async_runtime::spawn_blocking(move || {
        let mut builder = app.dialog().file();
        if let Some(path) = initial {
            builder = builder.set_directory(path);
        }
        builder.blocking_pick_folder()
    })
    .await
    .map_err(|e| format!("弹出目录选择器失败: {e}"))?;

    match result {
        Some(file_path) => {
            let resolved = file_path
                .simplified()
                .into_path()
                .map_err(|e| format!("解析选择的目录失败: {e}"))?;
            Ok(Some(resolved.to_string_lossy().to_string()))
        }
        None => Ok(None),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_app_config_path() -> Result<String, String> {
    let config_path = config::get_app_config_path();
    Ok(config_path.to_string_lossy().to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn open_app_config_folder(handle: AppHandle) -> Result<bool, String> {
    let config_dir = config::get_app_config_dir();

    if !config_dir.exists() {
        std::fs::create_dir_all(&config_dir).map_err(|e| format!("创建目录失败: {e}"))?;
    }

    handle
        .opener()
        .open_path(config_dir.to_string_lossy().to_string(), None::<String>)
        .map_err(|e| format!("打开文件夹失败: {e}"))?;

    Ok(true)
}

#[tauri::command]
#[specta::specta]
pub async fn get_claude_common_config_snippet(
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<Option<String>, String> {
    state
        .db
        .get_config_snippet("claude")
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn set_claude_common_config_snippet(
    snippet: String,
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<(), String> {
    let is_cleared = snippet.trim().is_empty();

    if !snippet.trim().is_empty() {
        serde_json::from_str::<serde_json::Value>(&snippet).map_err(invalid_json_format_error)?;
    }

    let value = if is_cleared { None } else { Some(snippet) };

    state
        .db
        .set_config_snippet("claude", value)
        .map_err(|e| e.to_string())?;
    state
        .db
        .set_config_snippet_cleared("claude", is_cleared)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn get_common_config_snippet(
    app_type: String,
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<Option<String>, String> {
    // Validate the webview-supplied app id before it reaches the DAO (CMD-23).
    let app_type = super::canonical_app_type(&app_type)?.as_str().to_string();
    state
        .db
        .get_config_snippet(&app_type)
        .map_err(|e| e.to_string())
}

/// 对前端编辑器里的 config.toml 文本做通用配置片段的合并/剥离。
/// 放后端是为了走 toml_edit（保注释、保键序）；前端 smol-toml 的
/// 整文档重序列化会破坏用户手写格式。
#[tauri::command]
#[specta::specta]
pub async fn update_toml_common_config_snippet(
    config_toml: String,
    snippet_toml: String,
    enabled: bool,
) -> Result<String, String> {
    crate::services::provider::update_toml_common_config_snippet(
        &config_toml,
        &snippet_toml,
        enabled,
    )
    .map_err(|e| e.to_string())
}

/// 保存通用配置片段。会迁移旧片段、重写当前供应商的 live 文件（接管期间
/// 改写备份，需要等待代理的切换锁），整体放到阻塞线程池执行。
#[tauri::command]
#[specta::specta]
pub async fn set_common_config_snippet(
    app_type: String,
    snippet: String,
    handle: tauri::AppHandle,
) -> Result<(), String> {
    // Validate the webview-supplied app id before it reaches the DAO (CMD-23).
    let app_type = super::canonical_app_type(&app_type)?.as_str().to_string();
    super::timed_blocking_with_state("set_common_config_snippet", handle, move |state| {
        apply_common_config_snippet(state, &app_type, snippet)
    })
    .await
}

/// Save `snippet` for `app_type` and re-project the live files that use it.
///
/// The DB steps run first; when a live write then fails, the stored snippet
/// and its cleared flag are put back so the DB and the live files do not
/// disagree (CMD-25).
fn apply_common_config_snippet(
    state: &crate::store::AppState,
    app_type: &str,
    snippet: String,
) -> Result<(), String> {
    let is_cleared = snippet.trim().is_empty();
    let old_snippet = state
        .db
        .get_config_snippet(app_type)
        .map_err(|e| e.to_string())?;
    let old_cleared = state
        .db
        .is_config_snippet_cleared(app_type)
        .map_err(|e| e.to_string())?;

    validate_common_config_snippet(app_type, &snippet)?;

    let value = if is_cleared { None } else { Some(snippet) };

    if matches!(app_type, "claude" | "codex" | "gemini") {
        if let Some(legacy_snippet) = old_snippet
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            let app = AppType::from_str(app_type).map_err(|e| e.to_string())?;
            crate::services::provider::ProviderService::migrate_legacy_common_config_usage(
                state,
                app,
                legacy_snippet,
            )
            .map_err(|e| e.to_string())?;
        }
    }

    state
        .db
        .set_config_snippet(app_type, value)
        .map_err(|e| e.to_string())?;
    state
        .db
        .set_config_snippet_cleared(app_type, is_cleared)
        .map_err(|e| e.to_string())?;

    if let Err(err) = project_common_config_snippet(state, app_type) {
        let reverted = state
            .db
            .set_config_snippet(app_type, old_snippet)
            .and_then(|_| state.db.set_config_snippet_cleared(app_type, old_cleared));
        if let Err(revert_err) = reverted {
            log::warn!("Reverting the {app_type} common config snippet failed: {revert_err}");
        }
        return Err(err);
    }
    Ok(())
}

/// The live writes that follow a snippet change.
fn project_common_config_snippet(
    state: &crate::store::AppState,
    app_type: &str,
) -> Result<(), String> {
    if matches!(app_type, "claude" | "codex" | "gemini") {
        let app = AppType::from_str(app_type).map_err(|e| e.to_string())?;
        crate::services::provider::ProviderService::sync_current_provider_for_app(state, app)
            .map_err(|e| e.to_string())?;
    }

    if app_type == "omo"
        && state
            .db
            .get_current_omo_provider("opencode", "omo")
            .map_err(|e| e.to_string())?
            .is_some()
    {
        crate::services::OmoService::write_config_to_file(state, &crate::services::omo::STANDARD)
            .map_err(|e| e.to_string())?;
    }
    if app_type == "omo-slim"
        && state
            .db
            .get_current_omo_provider("opencode", "omo-slim")
            .map_err(|e| e.to_string())?
            .is_some()
    {
        crate::services::OmoService::write_config_to_file(state, &crate::services::omo::SLIM)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{format_error_message, validate_common_config_snippet};

    /// A failed live write leaves the stored snippet unchanged (CMD-25).
    #[test]
    // Provider writes update the process-wide settings cache.
    #[serial_test::serial]
    fn snippet_is_reverted_when_the_live_write_fails() {
        use crate::database::Database;
        use crate::provider::Provider;
        use crate::store::AppState;
        use std::sync::Arc;

        let _home = crate::test_support::isolated_home();
        let db = Arc::new(Database::memory().unwrap());
        let provider = Provider::with_id(
            "p1".into(),
            "P1".into(),
            serde_json::json!({ "env": { "ANTHROPIC_BASE_URL": "https://example.test" } }),
            None,
        );
        db.save_provider("claude", &provider).unwrap();
        db.set_current_provider("claude", "p1").unwrap();
        db.set_config_snippet("claude", Some(r#"{"a":1}"#.to_string()))
            .unwrap();
        // settings.json is a directory: the live write fails
        std::fs::create_dir_all(crate::config::get_claude_settings_path()).unwrap();

        let state = AppState::new(db.clone());
        let result = super::apply_common_config_snippet(&state, "claude", r#"{"b":2}"#.to_string());
        assert!(result.is_err());
        assert_eq!(
            db.get_config_snippet("claude").unwrap().as_deref(),
            Some(r#"{"a":1}"#)
        );
        assert!(!db.is_config_snippet_cleared("claude").unwrap());

        // with a writable live file the snippet is saved
        std::fs::remove_dir_all(crate::config::get_claude_settings_path()).unwrap();
        super::apply_common_config_snippet(&state, "claude", r#"{"b":2}"#.to_string()).unwrap();
        assert_eq!(
            db.get_config_snippet("claude").unwrap().as_deref(),
            Some(r#"{"b":2}"#)
        );
    }

    #[test]
    fn format_error_message_localizes_and_falls_back_to_english() {
        assert_eq!(
            format_error_message("en", "JSON", &"boom"),
            "Invalid JSON format: boom"
        );
        assert_eq!(
            format_error_message("zh", "TOML", &"boom"),
            "无效的 TOML 格式: boom"
        );
        assert_eq!(
            format_error_message("zh-TW", "JSON", &"boom"),
            "無效的 JSON 格式: boom"
        );
        assert_eq!(
            format_error_message("ja", "JSON", &"boom"),
            "JSON形式が無効です: boom"
        );
        // 未知语言码回退到英文，而不是简体中文
        assert_eq!(
            format_error_message("de", "JSON", &"boom"),
            "Invalid JSON format: boom"
        );
    }

    #[test]
    fn validate_common_config_snippet_accepts_comment_only_codex_snippet() {
        validate_common_config_snippet("codex", "# comment only\n")
            .expect("comment-only codex snippet should be valid");
    }

    #[test]
    fn validate_common_config_snippet_rejects_invalid_codex_snippet() {
        let err = validate_common_config_snippet("codex", "[broken")
            .expect_err("invalid codex snippet should be rejected");
        assert!(
            err.contains("TOML") || err.contains("toml") || err.contains("格式"),
            "expected TOML validation error, got {err}"
        );
    }
}

#[tauri::command]
#[specta::specta]
pub async fn extract_common_config_snippet(
    appType: String,
    settingsConfig: Option<String>,
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<String, String> {
    let app = AppType::from_str(&appType).map_err(|e| e.to_string())?;

    if let Some(settings_config) = settingsConfig.filter(|s| !s.trim().is_empty()) {
        let settings: serde_json::Value =
            serde_json::from_str(&settings_config).map_err(invalid_json_format_error)?;

        return crate::services::provider::ProviderService::extract_common_config_snippet_from_settings(
            app,
            &settings,
        )
        .map_err(|e| e.to_string());
    }

    crate::services::provider::ProviderService::extract_common_config_snippet(&state, app)
        .map_err(|e| e.to_string())
}
