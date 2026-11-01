use serde_json::Value;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};
use tauri_plugin_store::StoreExt;

use crate::error::AppError;

/// Store 中的键名
pub(crate) const STORE_KEY_APP_CONFIG_DIR: &str = "app_config_dir_override";

/// 缓存当前的 app_config_dir 覆盖路径，避免存储 AppHandle
static APP_CONFIG_DIR_OVERRIDE: OnceLock<RwLock<Option<PathBuf>>> = OnceLock::new();

fn override_cache() -> &'static RwLock<Option<PathBuf>> {
    APP_CONFIG_DIR_OVERRIDE.get_or_init(|| RwLock::new(None))
}

fn update_cached_override(value: Option<PathBuf>) {
    if let Ok(mut guard) = override_cache().write() {
        *guard = value;
    }
}

/// 获取缓存中的 app_config_dir 覆盖路径
pub fn get_app_config_dir_override() -> Option<PathBuf> {
    override_cache().read().ok()?.clone()
}

/// Set the cached override for this process without a Tauri `AppHandle`.
///
/// Used by the `ccs` CLI and the headless proxy (PRODUCT-10/11), which resolve
/// the override from the same `app_paths.json` store file (or a flag/env var)
/// before any path lookup happens.
pub(crate) fn set_app_config_dir_override_for_process(value: Option<PathBuf>) {
    update_cached_override(value);
}

fn read_override_from_store(app: &tauri::AppHandle) -> Option<PathBuf> {
    let store = match app.store_builder("app_paths.json").build() {
        Ok(store) => store,
        Err(e) => {
            log::warn!("无法创建 Store: {e}");
            return None;
        }
    };

    match store.get(STORE_KEY_APP_CONFIG_DIR) {
        Some(Value::String(path_str)) => {
            let path_str = path_str.trim();
            if path_str.is_empty() {
                return None;
            }

            let path = resolve_path(path_str);

            if !path.exists() {
                log::warn!(
                    "Store 中配置的 app_config_dir 不存在: {path:?}\n\
                     将使用默认路径。"
                );
                // Explain the empty provider list instead of falling back silently (CMD-38).
                crate::init_status::push_startup_warning(crate::init_status::StartupWarning {
                    kind: "config_dir_unavailable".to_string(),
                    app_type: None,
                    path: Some(path.display().to_string()),
                    message:
                        "The configured data directory does not exist; using the default directory."
                            .to_string(),
                });
                return None;
            }

            log::info!("使用 Store 中的 app_config_dir: {path:?}");
            Some(path)
        }
        Some(_) => {
            log::warn!("Store 中的 {STORE_KEY_APP_CONFIG_DIR} 类型不正确，应为字符串");
            None
        }
        None => None,
    }
}

/// 从 Store 刷新 app_config_dir 覆盖值并更新缓存
pub fn refresh_app_config_dir_override(app: &tauri::AppHandle) -> Option<PathBuf> {
    let value = read_override_from_store(app);
    update_cached_override(value.clone());
    value
}

/// The override string as stored, without touching the process-wide cache
/// and without dropping it when the directory is momentarily missing.
pub fn read_stored_app_config_dir(app: &tauri::AppHandle) -> Option<String> {
    let store = app.store_builder("app_paths.json").build().ok()?;
    match store.get(STORE_KEY_APP_CONFIG_DIR) {
        Some(Value::String(path_str)) if !path_str.trim().is_empty() => {
            Some(resolve_path(path_str.trim()).to_string_lossy().to_string())
        }
        _ => None,
    }
}

/// 写入 app_config_dir 到 Tauri Store
pub fn set_app_config_dir_to_store(
    app: &tauri::AppHandle,
    path: Option<&str>,
) -> Result<(), AppError> {
    let store = app
        .store_builder("app_paths.json")
        .build()
        .map_err(|e| AppError::Message(format!("创建 Store 失败: {e}")))?;

    match path {
        Some(p) => {
            let trimmed = p.trim();
            if !trimmed.is_empty() {
                store.set(STORE_KEY_APP_CONFIG_DIR, Value::String(trimmed.to_string()));
                log::info!("已将 app_config_dir 写入 Store: {trimmed}");
            } else {
                store.delete(STORE_KEY_APP_CONFIG_DIR);
                log::info!("已从 Store 中删除 app_config_dir 配置");
            }
        }
        None => {
            store.delete(STORE_KEY_APP_CONFIG_DIR);
            log::info!("已从 Store 中删除 app_config_dir 配置");
        }
    }

    store
        .save()
        .map_err(|e| AppError::Message(format!("保存 Store 失败: {e}")))?;

    // Deliberately no `refresh_app_config_dir_override` here: the new path
    // applies from the next start. Switching it in-process split the open
    // database from backups/skills resolved per call (CMD-43).
    Ok(())
}

/// 解析路径，支持 ~ 开头的相对路径
pub(crate) fn resolve_path(raw: &str) -> PathBuf {
    crate::config::expand_tilde(raw)
}
