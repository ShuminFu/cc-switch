use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::app_config::AppType;
use crate::config::{self, get_claude_settings_path};
use crate::error::{AppError, IpcError};
use crate::services::common_config;

use std::path::PathBuf;
use std::str::FromStr;

#[tauri::command]
pub async fn get_claude_code_config_path() -> Result<String, IpcError> {
    Ok(get_claude_settings_path().to_string_lossy().to_string())
}

/// 应用配置目录（由各应用的 AppAdapter 解析）
fn app_config_dir(app: &AppType) -> Result<PathBuf, AppError> {
    crate::apps::adapter(app).config_dir()
}

#[tauri::command]
pub async fn get_config_dir(app: String) -> Result<String, IpcError> {
    let app = AppType::from_str(&app).map_err(IpcError::from)?;
    let dir = app_config_dir(&app).map_err(IpcError::from)?;

    Ok(dir.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn open_config_folder(handle: AppHandle, app: String) -> Result<bool, IpcError> {
    let app = AppType::from_str(&app).map_err(IpcError::from)?;
    let config_dir = app_config_dir(&app).map_err(IpcError::from)?;

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
pub async fn pick_directory(
    app: AppHandle,
    default_path: Option<String>,
) -> Result<Option<String>, IpcError> {
    let initial = default_path
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
            let resolved = resolved.to_string_lossy().to_string();
            crate::picked_paths::register(crate::picked_paths::PickPurpose::Directory, &resolved);
            Ok(Some(resolved))
        }
        None => Ok(None),
    }
}

#[tauri::command]
pub async fn get_app_config_path() -> Result<String, IpcError> {
    let config_path = config::get_app_config_path();
    Ok(config_path.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn open_app_config_folder(handle: AppHandle) -> Result<bool, IpcError> {
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
pub async fn get_claude_common_config_snippet(
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<Option<String>, IpcError> {
    common_config::get_snippet(&state, "claude").map_err(IpcError::from)
}

#[tauri::command]
pub async fn set_claude_common_config_snippet(
    snippet: String,
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<(), IpcError> {
    common_config::set_claude_snippet(&state, snippet).map_err(IpcError::from)
}

#[tauri::command]
pub async fn get_common_config_snippet(
    app_type: String,
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<Option<String>, IpcError> {
    common_config::get_snippet(&state, &app_type).map_err(IpcError::from)
}

/// 对前端编辑器里的 config.toml 文本做通用配置片段的合并/剥离。
/// 放后端是为了走 toml_edit（保注释、保键序）；前端 smol-toml 的
/// 整文档重序列化会破坏用户手写格式。
#[tauri::command]
pub async fn update_toml_common_config_snippet(
    config_toml: String,
    snippet_toml: String,
    enabled: bool,
) -> Result<String, IpcError> {
    crate::services::provider::update_toml_common_config_snippet(
        &config_toml,
        &snippet_toml,
        enabled,
    )
    .map_err(IpcError::from)
}

#[tauri::command]
pub async fn set_common_config_snippet(
    app_type: String,
    snippet: String,
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<(), IpcError> {
    common_config::set_snippet(&state, &app_type, snippet).map_err(IpcError::from)
}

#[tauri::command]
pub async fn extract_common_config_snippet(
    app_type: String,
    settings_config: Option<String>,
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<String, IpcError> {
    common_config::extract_snippet(&state, &app_type, settings_config).map_err(IpcError::from)
}
