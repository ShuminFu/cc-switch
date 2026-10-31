#![allow(non_snake_case)]

use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

/// 应用更新下载进度（通过 `update-download-progress` 事件发给前端）。
#[derive(Clone, serde::Serialize)]
struct UpdateDownloadProgress {
    downloaded: u64,
    total: Option<u64>,
}

fn merge_settings_for_save(
    mut incoming: crate::settings::AppSettings,
    existing: &crate::settings::AppSettings,
) -> crate::settings::AppSettings {
    match (&mut incoming.webdav_sync, &existing.webdav_sync) {
        // incoming 没有 webdav → 保留现有
        (None, _) => {
            incoming.webdav_sync = existing.webdav_sync.clone();
        }
        // incoming 有 webdav 但密码为空，且现有有密码 → 填回现有密码
        // （get_settings_for_frontend 总是清空密码，所以通过 save_settings
        //   传入的空密码意味着"保持现有"而非"用户主动清空"）
        (Some(incoming_sync), Some(existing_sync))
            if incoming_sync.password.is_empty() && !existing_sync.password.is_empty() =>
        {
            incoming_sync.password = existing_sync.password.clone();
        }
        _ => {}
    }
    match (&mut incoming.s3_sync, &existing.s3_sync) {
        // incoming 没有 s3 → 保留现有
        (None, _) => {
            incoming.s3_sync = existing.s3_sync.clone();
        }
        // incoming 有 s3 但密钥为空，且现有有密钥 → 填回现有密钥
        (Some(incoming_sync), Some(existing_sync))
            if incoming_sync.secret_access_key.is_empty()
                && !existing_sync.secret_access_key.is_empty() =>
        {
            incoming_sync.secret_access_key = existing_sync.secret_access_key.clone();
        }
        _ => {}
    }
    // local_migrations 是纯后端状态（迁移完成标记），前端没有合法的修改场景，
    // 无条件取现有值。若按 incoming 透传：后端清掉 marker（如关闭统一会话
    // 开关）后、前端 query 缓存刷新前的一次全量保存会把旧 marker 重放回来，
    // 重新开启时被"复活"的标记挡住而漏迁。
    incoming.local_migrations = existing.local_migrations.clone();
    // 同步加密设置只由 sync_encryption_* 命令管理（前端拿不到密码）。
    incoming.sync_encryption = existing.sync_encryption.clone();
    // 当前供应商指针同样是后端状态：只由 set_current_provider（切换、托盘、
    // 故障转移）写入。前端保存设置时会把缓存里的整份结构（含旧指针）发回来，
    // 若按 incoming 透传，设置页打开期间发生的切换会在点击保存时被回滚。
    incoming.current_provider_claude = existing.current_provider_claude.clone();
    incoming.current_provider_claude_desktop = existing.current_provider_claude_desktop.clone();
    incoming.current_provider_codex = existing.current_provider_codex.clone();
    incoming.current_provider_gemini = existing.current_provider_gemini.clone();
    incoming.current_provider_grokbuild = existing.current_provider_grokbuild.clone();
    incoming.current_provider_opencode = existing.current_provider_opencode.clone();
    incoming.current_provider_openclaw = existing.current_provider_openclaw.clone();
    incoming.current_provider_hermes = existing.current_provider_hermes.clone();
    // 旧版 webdav_backup 不会下发给前端（get_settings_for_frontend 置空），
    // 前端回传的 None 不代表用户清除，保留现有值。
    if incoming.webdav_backup.is_none() {
        incoming.webdav_backup = existing.webdav_backup.clone();
    }
    incoming
}

/// 获取设置
#[tauri::command]
pub async fn get_settings() -> Result<crate::settings::AppSettings, String> {
    Ok(crate::settings::get_settings_for_frontend())
}

/// 保存设置
#[tauri::command]
pub async fn save_settings(
    app: AppHandle,
    state: tauri::State<'_, crate::store::AppState>,
    settings: crate::settings::AppSettings,
) -> Result<bool, String> {
    apply_settings_update(&app, state.inner(), move |existing| {
        Ok(merge_settings_for_save(settings, existing))
    })
}

/// Overlay a partial settings object (camelCase keys, as `get_settings`
/// returns them) onto `existing`. Unknown keys are rejected so a typo does
/// not silently do nothing.
fn apply_settings_patch(
    existing: &crate::settings::AppSettings,
    patch: &serde_json::Value,
) -> Result<crate::settings::AppSettings, String> {
    let serde_json::Value::Object(patch) = patch else {
        return Err("settings patch must be a JSON object".to_string());
    };
    let mut current =
        serde_json::to_value(existing).map_err(|e| format!("serialize settings: {e}"))?;
    let Some(fields) = current.as_object_mut() else {
        return Err("settings did not serialize to an object".to_string());
    };
    for (key, value) in patch {
        fields.insert(key.clone(), value.clone());
    }
    let parsed: crate::settings::AppSettings =
        serde_json::from_value(current).map_err(|e| format!("invalid settings patch: {e}"))?;
    // serde ignores unknown keys; a non-null key that does not survive the
    // round trip is not a settings field (or not a writable one).
    let round_trip = serde_json::to_value(&parsed).map_err(|e| e.to_string())?;
    for (key, value) in patch {
        if !value.is_null() && round_trip.get(key).is_none() {
            return Err(format!("unknown settings field: {key}"));
        }
    }
    Ok(parsed)
}

/// Update only the given settings fields (PATCH semantics).
///
/// The frontend's `save_settings` sends the whole settings object, so a
/// settings page holding a stale copy can roll back fields changed meanwhile.
/// This command reads-merges-writes under the settings lock and touches only
/// the keys in `patch`; backend-owned fields (current-provider pointers,
/// migrations, sync secrets) are still protected by `merge_settings_for_save`.
#[tauri::command]
#[specta::specta]
pub async fn patch_settings(
    app: AppHandle,
    state: tauri::State<'_, crate::store::AppState>,
    patch: serde_json::Value,
) -> Result<bool, String> {
    apply_settings_update(&app, state.inner(), move |existing| {
        let incoming = apply_settings_patch(existing, &patch)?;
        Ok(merge_settings_for_save(incoming, existing))
    })
}

/// Shared tail of `save_settings` / `patch_settings`: persist under the
/// settings write lock, then apply the side effects of changed fields.
fn apply_settings_update<F>(
    app: &AppHandle,
    state: &crate::store::AppState,
    build: F,
) -> Result<bool, String>
where
    F: FnOnce(&crate::settings::AppSettings) -> Result<crate::settings::AppSettings, String>,
{
    // 读取-合并-写入在同一把写锁内完成，避免与并发的 set_current_provider /
    // 同步状态更新交错导致丢失更新。
    let mut unify_codex_enabled = false;
    let mut build_error: Option<String> = None;
    let existing = crate::settings::update_settings_with(|existing| match build(existing) {
        Ok(merged) => {
            unify_codex_enabled = merged.unify_codex_session_history;
            merged
        }
        Err(e) => {
            // Keep the current settings untouched.
            unify_codex_enabled = existing.unify_codex_session_history;
            build_error = Some(e);
            existing.clone()
        }
    })
    .map_err(|e| e.to_string())?;
    if let Some(e) = build_error {
        return Err(e);
    }
    let unify_codex_changed = unify_codex_enabled != existing.unify_codex_session_history;

    // 托盘图标显隐随设置即时生效（CMD-15）
    let saved = crate::settings::get_settings();
    if saved.show_in_tray != existing.show_in_tray {
        crate::main_window::apply_tray_visibility(app, &saved);
    }

    // 统一会话开关变更时立即重写当前官方 Codex 供应商的 live 配置，
    // 不必等下一次切换才生效。
    if unify_codex_changed {
        // live 重写失败时回滚设置并把保存整体报失败：若设置保持已切换状态，
        // live 仍跑旧桶，后续的历史迁移/还原会让会话再次分裂（开启=历史
        // 迁走而新会话仍写 openai 桶；关闭=会话还原而 live 仍写 custom）。
        // 报错让前端 saved=false 短路还原；回滚是整次保存的事务语义
        // （本开关的保存只携带开关相关字段）。
        if let Err(err) = crate::services::provider::reapply_current_codex_official_live(state) {
            log::warn!("统一 Codex 会话历史开关变更后重写 live 配置失败，回滚设置: {err}");
            // 回滚同样走合并：恢复用户可编辑字段，但保留期间由后端写入的
            // 当前供应商指针与迁移标记。
            if let Err(rollback_err) = crate::settings::update_settings_with(|current| {
                merge_settings_for_save(existing.clone(), current)
            }) {
                log::error!("回滚统一会话开关设置失败: {rollback_err}");
            }
            return Err(format!(
                "统一 Codex 会话历史开关未生效（live 配置重写失败）: {err}"
            ));
        }

        if unify_codex_enabled {
            // 后台执行存量迁移（openai 桶 → custom 桶；仅当用户勾选了迁入既有
            // 会话，函数内部自门控）。大会话目录可能要读数秒，不能阻塞设置保存；
            // 失败时不写完成标记，下次启动自动重试。
            tauri::async_runtime::spawn_blocking(|| {
                match crate::codex_history_migration::maybe_migrate_codex_official_history_to_unified_bucket() {
                    Ok(outcome) => {
                        if let Some(reason) = outcome.skipped_reason {
                            log::debug!("○ Codex official history unify migration skipped: {reason}");
                        } else {
                            log::info!(
                                "✓ Codex official history unify migration completed: jsonl_files={}, state_rows={}",
                                outcome.migrated_jsonl_files,
                                outcome.migrated_state_rows
                            );
                        }
                    }
                    Err(e) => {
                        log::warn!("✗ Codex official history unify migration failed: {e}");
                    }
                }
            });
        } else {
            // 清除标记与迁移意愿，让重新开启并再次勾选时能补迁
            // 关闭期间落入 openai 桶的官方会话。
            if let Err(err) = crate::settings::clear_codex_official_history_unify_migration() {
                log::warn!("清除统一会话迁移标记失败: {err}");
            }
            if let Err(err) = crate::settings::clear_codex_unify_migrate_existing() {
                log::warn!("清除统一会话迁移意愿失败: {err}");
            }
        }
    }
    Ok(true)
}

#[derive(serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CodexUnifyHistoryRestoreResult {
    pub restored_jsonl_files: usize,
    pub restored_state_rows: usize,
    /// 还原被跳过的原因（如当前目录没有账本），前端据此提示而非报"成功 0 项"。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped_reason: Option<String>,
}

/// 是否存在统一会话开关的迁移备份（决定关闭弹窗里是否显示"恢复备份"勾选）。
#[tauri::command]
#[specta::specta]
pub async fn has_codex_unify_history_backup() -> Result<bool, String> {
    Ok(crate::codex_history_migration::has_codex_official_history_unify_backup())
}

/// 按迁移备份账本把当时迁入共享桶的官方会话还原回 "openai" 桶。
/// 由关闭统一会话开关的确认弹窗触发；幂等，可安全重试。
#[tauri::command]
#[specta::specta]
pub async fn restore_codex_unified_history() -> Result<CodexUnifyHistoryRestoreResult, String> {
    let outcome = tauri::async_runtime::spawn_blocking(|| {
        crate::codex_history_migration::restore_codex_official_history_from_backups()
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    if let Some(reason) = &outcome.skipped_reason {
        log::debug!("○ Codex official history restore skipped: {reason}");
    } else {
        log::info!(
            "✓ Codex official history restored from backups: jsonl_files={}, state_rows={}",
            outcome.restored_jsonl_files,
            outcome.restored_state_rows
        );
    }

    Ok(CodexUnifyHistoryRestoreResult {
        restored_jsonl_files: outcome.restored_jsonl_files,
        restored_state_rows: outcome.restored_state_rows,
        skipped_reason: outcome.skipped_reason,
    })
}

/// 重启应用程序（当 app_config_dir 变更后使用）
#[tauri::command]
#[specta::specta]
pub async fn restart_app(app: AppHandle) -> Result<bool, String> {
    crate::save_window_state_before_exit(&app);

    // 在后台延迟重启，让函数有时间返回响应
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        // app.restart() 走 RESTART_EXIT_CODE 路径，ExitRequested 处理器会直接
        // 放行给 Tauri 默认 re-exec，不执行代理/Live 清理。但本命令用于
        // app_config_dir 变更后的重启：新实例会切到新数据库，拿不到旧库里的
        // Live 备份，无法恢复被接管的 Live 配置。因此必须趁旧实例的事件循环
        // 仍存活，在这里同步完成恢复（保留代理状态，新实例启动时自动重新接管）。
        crate::cleanup_before_exit(&app).await;
        app.restart();
    });
    Ok(true)
}

/// 下载并安装应用更新，然后由后端直接重启应用。
///
/// macOS 更新会原地替换 `.app` bundle。如果先返回前端、再让旧 WebView 调
/// `process.relaunch()`，旧进程可能已经处在 bundle 被替换后的不稳定窗口期。
/// 这里把退出清理、安装和重启串在同一个后端流程中，避免依赖旧前端继续执行。
#[tauri::command]
#[specta::specta]
pub async fn install_update_and_restart(app: AppHandle) -> Result<bool, String> {
    let updater = app
        .updater_builder()
        .build()
        .map_err(|e| format!("初始化更新器失败: {e}"))?;

    let Some(update) = updater
        .check()
        .await
        .map_err(|e| format!("检查更新失败: {e}"))?
    else {
        return Ok(false);
    };

    log::info!("开始下载应用更新: {}", update.version);
    let progress_handle = app.clone();
    let mut downloaded: u64 = 0;
    let bytes = update
        .download(
            move |chunk_len, content_len| {
                downloaded = downloaded.saturating_add(chunk_len as u64);
                let _ = progress_handle.emit(
                    crate::events::UPDATE_DOWNLOAD_PROGRESS,
                    UpdateDownloadProgress {
                        downloaded,
                        total: content_len,
                    },
                );
            },
            || {},
        )
        .await
        .map_err(|e| format!("下载更新失败: {e}"))?;

    log::info!("开始安装应用更新: {}", update.version);

    #[cfg(target_os = "windows")]
    {
        // Windows updater 会在 install() 内启动安装器并直接退出当前进程
        // （插件内部 std::process::exit(0)，绕过 TrayIcon::drop、不发
        // NIM_DELETE，会残留死图标——与托盘"退出"路径相同的问题）。
        // 因此清理只能放在 install 前执行，且必须显式移除托盘图标。
        crate::save_window_state_before_exit(&app);
        crate::cleanup_before_exit(&app).await;
        crate::remove_tray_icon_before_exit(&app);
        crate::destroy_single_instance_lock(&app);
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        update.install(bytes).map_err(|e| {
            format!(
                "Windows 更新安装失败: {e}。已执行退出前清理，代理或 Live 接管可能已暂停；请重启应用或重新开启代理后再试。"
            )
        })?;
        Ok(true)
    }

    #[cfg(not(target_os = "windows"))]
    {
        // macOS/Linux install() 会返回；先安装，避免安装失败时误停代理/撤回接管。
        update
            .install(bytes)
            .map_err(|e| format!("安装更新失败: {e}"))?;

        crate::save_window_state_before_exit(&app);
        crate::cleanup_before_exit(&app).await;

        log::info!("应用更新安装完成，正在重启应用");
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        crate::restart_process(&app);
    }
}

/// 检查是否有可用的应用更新，返回可用的新版本号（无更新时返回 None）。
///
/// 数据库版本过新的恢复界面用它判断：升级应用能否解决问题。若返回 None，说明
/// 已是最新版本，但数据库仍不兼容（通常由第三方客户端或更高版本创建），应提示用户
/// 升级无法解决，而不是让其反复尝试。
#[tauri::command]
#[specta::specta]
pub async fn check_app_update_available(app: AppHandle) -> Result<Option<String>, String> {
    let updater = app
        .updater_builder()
        .build()
        .map_err(|e| format!("初始化更新器失败: {e}"))?;
    let update = updater
        .check()
        .await
        .map_err(|e| format!("检查更新失败: {e}"))?;
    Ok(update.map(|u| u.version))
}

/// 获取 app_config_dir 覆盖配置 (从 Store)
///
/// Read-only: it no longer refreshes the process-wide override cache. The data
/// directory is resolved once at startup; re-resolving it here flipped backups
/// and skills to another directory mid-session while the open database stayed
/// in the old one, and an unmounted override silently reverted to the default
/// (CMD-43).
#[tauri::command]
#[specta::specta]
pub async fn get_app_config_dir_override(app: AppHandle) -> Result<Option<String>, String> {
    Ok(crate::app_store::read_stored_app_config_dir(&app))
}

/// Validate a data-directory override before it is stored (CMD-38): after `~`
/// expansion it must be absolute, be (or become) a directory, and be writable.
/// Returns the resolved path.
fn validate_app_config_dir(raw: &str) -> Result<std::path::PathBuf, String> {
    let path = crate::app_store::resolve_path(raw.trim());
    if !path.is_absolute() {
        return Err(format!(
            "数据目录必须是绝对路径: {raw} / The data directory must be an absolute path"
        ));
    }
    if path.exists() && !path.is_dir() {
        return Err(format!(
            "数据目录不是文件夹: {} / Not a directory",
            path.display()
        ));
    }
    std::fs::create_dir_all(&path).map_err(|e| {
        format!(
            "无法创建数据目录 {}: {e} / Cannot create the data directory",
            path.display()
        )
    })?;
    let probe = path.join(format!(".cc-switch-write-test-{}", std::process::id()));
    std::fs::write(&probe, b"ok").map_err(|e| {
        format!(
            "数据目录不可写 {}: {e} / The data directory is not writable",
            path.display()
        )
    })?;
    let _ = std::fs::remove_file(&probe);
    Ok(path)
}

/// 设置 app_config_dir 覆盖配置 (到 Store)
///
/// The new directory takes effect after a restart (the frontend offers it);
/// the running process keeps using the directory its database was opened in.
#[tauri::command]
#[specta::specta]
pub async fn set_app_config_dir_override(
    app: AppHandle,
    path: Option<String>,
) -> Result<bool, String> {
    let path = path.filter(|p| !p.trim().is_empty());
    if let Some(raw) = path.as_deref() {
        validate_app_config_dir(raw)?;
    }
    crate::app_store::set_app_config_dir_to_store(&app, path.as_deref())?;
    Ok(true)
}

/// 设置开机自启
#[tauri::command]
#[specta::specta]
pub async fn set_auto_launch(enabled: bool) -> Result<bool, String> {
    if enabled {
        crate::auto_launch::enable_auto_launch().map_err(|e| format!("启用开机自启失败: {e}"))?;
    } else {
        crate::auto_launch::disable_auto_launch().map_err(|e| format!("禁用开机自启失败: {e}"))?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::{apply_settings_patch, merge_settings_for_save, validate_app_config_dir};

    #[test]
    fn app_config_dir_override_rejects_relative_and_file_paths() {
        assert!(validate_app_config_dir("relative/dir").is_err());

        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("not-a-dir");
        std::fs::write(&file, "x").unwrap();
        assert!(validate_app_config_dir(file.to_str().unwrap()).is_err());
    }

    #[test]
    fn app_config_dir_override_creates_missing_writable_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("nested").join("data");
        let resolved = validate_app_config_dir(target.to_str().unwrap()).expect("valid");
        assert_eq!(resolved, target);
        assert!(target.is_dir());
        // The write probe is cleaned up.
        assert_eq!(std::fs::read_dir(&target).unwrap().count(), 0);
    }

    #[test]
    fn settings_patch_changes_only_the_given_fields() {
        let existing = AppSettings {
            language: Some("ja".to_string()),
            show_in_tray: true,
            ..AppSettings::default()
        };
        let patched =
            apply_settings_patch(&existing, &serde_json::json!({ "showInTray": false })).unwrap();
        assert!(!patched.show_in_tray);
        assert_eq!(patched.language.as_deref(), Some("ja"));

        // Optional fields that are unset can still be patched.
        let patched = apply_settings_patch(
            &AppSettings::default(),
            &serde_json::json!({ "language": "en" }),
        )
        .unwrap();
        assert_eq!(patched.language.as_deref(), Some("en"));
    }

    #[test]
    fn settings_patch_rejects_unknown_fields_and_non_objects() {
        let existing = AppSettings::default();
        assert!(apply_settings_patch(&existing, &serde_json::json!({ "noSuchField": 1 })).is_err());
        assert!(apply_settings_patch(&existing, &serde_json::json!([1])).is_err());
        assert!(
            apply_settings_patch(&existing, &serde_json::json!({ "showInTray": "yes" })).is_err()
        );
    }

    #[test]
    fn settings_patch_cannot_move_current_provider_pointers() {
        let existing = AppSettings {
            current_provider_claude: Some("kept".to_string()),
            ..AppSettings::default()
        };
        let incoming = apply_settings_patch(
            &existing,
            &serde_json::json!({ "currentProviderClaude": "evil" }),
        )
        .unwrap();
        let merged = merge_settings_for_save(incoming, &existing);
        assert_eq!(merged.current_provider_claude.as_deref(), Some("kept"));
    }
    use crate::settings::{
        AppSettings, CodexOfficialHistoryUnifyMigration, CodexProviderTemplateMigration,
        CodexThirdPartyHistoryProviderBucketMigration, LocalMigrations, S3SyncSettings,
        WebDavSyncSettings,
    };

    #[test]
    fn save_settings_should_preserve_existing_webdav_when_payload_omits_it() {
        let existing = AppSettings {
            webdav_sync: Some(WebDavSyncSettings {
                base_url: "https://dav.example.com".to_string(),
                username: "alice".to_string(),
                password: "secret".to_string(),
                ..WebDavSyncSettings::default()
            }),
            ..AppSettings::default()
        };

        let incoming = AppSettings::default();
        let merged = merge_settings_for_save(incoming, &existing);

        assert!(merged.webdav_sync.is_some());
        assert_eq!(
            merged.webdav_sync.as_ref().map(|v| v.base_url.as_str()),
            Some("https://dav.example.com")
        );
    }

    #[test]
    fn save_settings_should_keep_incoming_webdav_when_present() {
        let existing = AppSettings {
            webdav_sync: Some(WebDavSyncSettings {
                base_url: "https://dav.old.example.com".to_string(),
                username: "old".to_string(),
                password: "old-pass".to_string(),
                ..WebDavSyncSettings::default()
            }),
            ..AppSettings::default()
        };

        let incoming = AppSettings {
            webdav_sync: Some(WebDavSyncSettings {
                base_url: "https://dav.new.example.com".to_string(),
                username: "new".to_string(),
                password: "new-pass".to_string(),
                ..WebDavSyncSettings::default()
            }),
            ..AppSettings::default()
        };

        let merged = merge_settings_for_save(incoming, &existing);

        assert_eq!(
            merged.webdav_sync.as_ref().map(|v| v.base_url.as_str()),
            Some("https://dav.new.example.com")
        );
    }

    /// Regression test: frontend always receives empty password from
    /// get_settings_for_frontend(). If a component accidentally spreads
    /// the full settings object into save_settings, the empty password
    /// must NOT overwrite the existing one.
    #[test]
    fn save_settings_should_preserve_password_when_incoming_has_empty_password() {
        let existing = AppSettings {
            webdav_sync: Some(WebDavSyncSettings {
                base_url: "https://dav.example.com".to_string(),
                username: "alice".to_string(),
                password: "secret".to_string(),
                ..WebDavSyncSettings::default()
            }),
            ..AppSettings::default()
        };

        // Simulate frontend sending settings with cleared password
        let incoming = AppSettings {
            webdav_sync: Some(WebDavSyncSettings {
                base_url: "https://dav.example.com".to_string(),
                username: "alice".to_string(),
                password: "".to_string(),
                ..WebDavSyncSettings::default()
            }),
            ..AppSettings::default()
        };

        let merged = merge_settings_for_save(incoming, &existing);

        assert_eq!(
            merged.webdav_sync.as_ref().map(|v| v.password.as_str()),
            Some("secret"),
            "empty password from frontend must not overwrite existing password"
        );
    }

    /// When both incoming and existing have no password, merge should
    /// work without panicking and keep the empty state.
    #[test]
    fn save_settings_should_handle_both_empty_passwords() {
        let existing = AppSettings {
            webdav_sync: Some(WebDavSyncSettings {
                base_url: "https://dav.example.com".to_string(),
                username: "alice".to_string(),
                password: "".to_string(),
                ..WebDavSyncSettings::default()
            }),
            ..AppSettings::default()
        };

        let incoming = AppSettings {
            webdav_sync: Some(WebDavSyncSettings {
                base_url: "https://dav.example.com".to_string(),
                username: "alice".to_string(),
                password: "".to_string(),
                ..WebDavSyncSettings::default()
            }),
            ..AppSettings::default()
        };

        let merged = merge_settings_for_save(incoming, &existing);

        assert_eq!(
            merged.webdav_sync.as_ref().map(|v| v.password.as_str()),
            Some("")
        );
    }

    #[test]
    fn save_settings_should_preserve_existing_s3_when_payload_omits_it() {
        let existing = AppSettings {
            s3_sync: Some(S3SyncSettings {
                bucket: "bucket".to_string(),
                access_key_id: "ak".to_string(),
                secret_access_key: "secret".to_string(),
                ..S3SyncSettings::default()
            }),
            ..AppSettings::default()
        };

        let incoming = AppSettings::default();
        let merged = merge_settings_for_save(incoming, &existing);

        assert!(merged.s3_sync.is_some());
        assert_eq!(
            merged
                .s3_sync
                .as_ref()
                .map(|v| v.secret_access_key.as_str()),
            Some("secret")
        );
    }

    #[test]
    fn save_settings_should_preserve_s3_secret_when_incoming_has_empty_secret() {
        let existing = AppSettings {
            s3_sync: Some(S3SyncSettings {
                bucket: "bucket".to_string(),
                access_key_id: "ak".to_string(),
                secret_access_key: "secret".to_string(),
                ..S3SyncSettings::default()
            }),
            ..AppSettings::default()
        };

        let incoming = AppSettings {
            s3_sync: Some(S3SyncSettings {
                bucket: "bucket".to_string(),
                access_key_id: "ak".to_string(),
                secret_access_key: "".to_string(),
                ..S3SyncSettings::default()
            }),
            ..AppSettings::default()
        };

        let merged = merge_settings_for_save(incoming, &existing);

        assert_eq!(
            merged
                .s3_sync
                .as_ref()
                .map(|v| v.secret_access_key.as_str()),
            Some("secret")
        );
    }

    #[test]
    fn save_settings_should_preserve_local_migrations_when_payload_omits_it() {
        let existing = AppSettings {
            local_migrations: Some(LocalMigrations {
                codex_third_party_history_provider_bucket_v1: Some(
                    CodexThirdPartyHistoryProviderBucketMigration {
                        completed_at: "2026-05-20T00:00:00Z".to_string(),
                        target_provider_id: "custom".to_string(),
                        source_provider_ids: vec!["rightcode".to_string()],
                        migrated_jsonl_files: 2,
                        migrated_state_rows: 3,
                        scanned_history_files: true,
                    },
                ),
                codex_provider_template_v1: Some(CodexProviderTemplateMigration {
                    completed_at: "2026-05-20T00:01:00Z".to_string(),
                    migrated_provider_ids: vec!["legacy".to_string()],
                }),
                codex_official_history_unify_v1: Some(CodexOfficialHistoryUnifyMigration {
                    completed_at: "2026-06-12T00:00:00Z".to_string(),
                    target_provider_id: "custom".to_string(),
                    migrated_jsonl_files: 5,
                    migrated_state_rows: 7,
                    codex_config_dir: None,
                }),
            }),
            ..AppSettings::default()
        };

        let incoming = AppSettings::default();
        let merged = merge_settings_for_save(incoming, &existing);

        let migration = merged
            .local_migrations
            .as_ref()
            .and_then(|migrations| {
                migrations
                    .codex_third_party_history_provider_bucket_v1
                    .as_ref()
            })
            .expect("local migration marker should be preserved");
        assert_eq!(migration.target_provider_id, "custom");
        assert_eq!(migration.migrated_jsonl_files, 2);
        assert_eq!(migration.migrated_state_rows, 3);

        let template_migration = merged
            .local_migrations
            .as_ref()
            .and_then(|migrations| migrations.codex_provider_template_v1.as_ref())
            .expect("template migration marker should be preserved");
        assert_eq!(
            template_migration.migrated_provider_ids,
            vec!["legacy".to_string()]
        );

        let unify_migration = merged
            .local_migrations
            .as_ref()
            .and_then(|migrations| migrations.codex_official_history_unify_v1.as_ref())
            .expect("official unify migration marker should be preserved");
        assert_eq!(unify_migration.migrated_jsonl_files, 5);
        assert_eq!(unify_migration.migrated_state_rows, 7);
    }

    /// incoming 带有 local_migrations（哪怕是空的）也不能覆盖后端维护的标记。
    #[test]
    fn save_settings_should_keep_backend_migration_markers_over_incoming() {
        let existing = AppSettings {
            local_migrations: Some(LocalMigrations {
                codex_third_party_history_provider_bucket_v1: None,
                codex_provider_template_v1: None,
                codex_official_history_unify_v1: Some(CodexOfficialHistoryUnifyMigration {
                    completed_at: "2026-06-12T00:00:00Z".to_string(),
                    target_provider_id: "custom".to_string(),
                    migrated_jsonl_files: 1,
                    migrated_state_rows: 2,
                    codex_config_dir: None,
                }),
            }),
            ..AppSettings::default()
        };

        let incoming = AppSettings {
            local_migrations: Some(LocalMigrations::default()),
            ..AppSettings::default()
        };
        let merged = merge_settings_for_save(incoming, &existing);

        assert!(merged
            .local_migrations
            .as_ref()
            .and_then(|migrations| migrations.codex_official_history_unify_v1.as_ref())
            .is_some());
    }

    /// 后端清掉 marker 后（如关闭统一会话开关）、前端缓存刷新前的全量保存
    /// 会携带旧 marker；merge 必须忽略它，否则被"复活"的标记会让重新开启
    /// 时误判已迁移而漏迁。
    #[test]
    fn save_settings_should_ignore_stale_incoming_migration_markers() {
        let existing = AppSettings::default();

        let incoming = AppSettings {
            local_migrations: Some(LocalMigrations {
                codex_official_history_unify_v1: Some(CodexOfficialHistoryUnifyMigration {
                    completed_at: "2026-06-12T00:00:00Z".to_string(),
                    target_provider_id: "custom".to_string(),
                    migrated_jsonl_files: 1,
                    migrated_state_rows: 2,
                    codex_config_dir: None,
                }),
                ..LocalMigrations::default()
            }),
            ..AppSettings::default()
        };
        let merged = merge_settings_for_save(incoming, &existing);

        assert!(merged.local_migrations.is_none());
    }

    /// 前端回传的整份设置里的当前供应商指针是缓存旧值，不能回滚后端切换。
    #[test]
    fn save_settings_should_keep_backend_current_provider_pointers() {
        let existing = AppSettings {
            current_provider_claude: Some("new-claude".to_string()),
            current_provider_codex: Some("new-codex".to_string()),
            ..AppSettings::default()
        };
        let incoming = AppSettings {
            current_provider_claude: Some("stale-claude".to_string()),
            current_provider_gemini: Some("stale-gemini".to_string()),
            language: Some("ja".to_string()),
            ..AppSettings::default()
        };
        let merged = merge_settings_for_save(incoming, &existing);

        assert_eq!(
            merged.current_provider_claude.as_deref(),
            Some("new-claude")
        );
        assert_eq!(merged.current_provider_codex.as_deref(), Some("new-codex"));
        assert_eq!(merged.current_provider_gemini, None);
        assert_eq!(merged.language.as_deref(), Some("ja"));
    }

    #[test]
    fn save_settings_should_persist_auto_sync_confirmed() {
        let incoming: AppSettings = serde_json::from_value(serde_json::json!({
            "autoSyncConfirmed": true
        }))
        .expect("parse frontend payload");
        let merged = merge_settings_for_save(incoming, &AppSettings::default());
        assert_eq!(merged.auto_sync_confirmed, Some(true));
    }

    #[test]
    fn save_settings_should_preserve_legacy_webdav_backup() {
        let existing = AppSettings {
            webdav_backup: Some(serde_json::json!({ "url": "https://dav.example.com" })),
            ..AppSettings::default()
        };
        let merged = merge_settings_for_save(AppSettings::default(), &existing);
        assert_eq!(merged.webdav_backup, existing.webdav_backup);
    }

    /// 并发的 save_settings（读取-合并-写入）与 set_current_provider 结束后两者的
    /// 修改都必须存在（CMD-06）。
    #[test]
    #[serial_test::serial]
    fn concurrent_save_and_provider_switch_keep_both_changes() {
        use crate::app_config::AppType;
        use std::sync::{Arc, Barrier};

        let dir = tempfile::tempdir().expect("tempdir");
        let original_test_home = std::env::var("CC_SWITCH_TEST_HOME").ok();
        std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
        crate::settings::reload_settings().expect("reload settings");

        // 前端缓存的旧快照：没有任何当前供应商指针
        let stale_snapshot = crate::settings::get_settings();
        let barrier = Arc::new(Barrier::new(2));

        let save_barrier = barrier.clone();
        let saver = std::thread::spawn(move || {
            save_barrier.wait();
            for i in 0..50 {
                let incoming = AppSettings {
                    backup_retain_count: Some(i + 1),
                    ..stale_snapshot.clone()
                };
                crate::settings::update_settings_with(|existing| {
                    merge_settings_for_save(incoming, existing)
                })
                .expect("save settings");
            }
        });
        let switch_barrier = barrier.clone();
        let switcher = std::thread::spawn(move || {
            switch_barrier.wait();
            for i in 0..50 {
                crate::settings::set_current_provider(&AppType::Claude, Some(&format!("p{i}")))
                    .expect("switch provider");
            }
        });
        saver.join().unwrap();
        switcher.join().unwrap();

        let in_memory = crate::settings::get_settings();
        assert_eq!(in_memory.backup_retain_count, Some(50));
        assert_eq!(in_memory.current_provider_claude.as_deref(), Some("p49"));

        // 磁盘上的文件与内存一致
        crate::settings::reload_settings().expect("reload settings");
        let on_disk = crate::settings::get_settings();
        assert_eq!(on_disk.backup_retain_count, Some(50));
        assert_eq!(on_disk.current_provider_claude.as_deref(), Some("p49"));

        match original_test_home {
            Some(v) => std::env::set_var("CC_SWITCH_TEST_HOME", v),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        crate::settings::reload_settings().expect("reload settings");
    }
}

/// 获取开机自启状态
#[tauri::command]
#[specta::specta]
pub async fn get_auto_launch_status() -> Result<bool, String> {
    crate::auto_launch::is_auto_launch_enabled().map_err(|e| format!("获取开机自启状态失败: {e}"))
}

/// 获取整流器配置
#[tauri::command]
pub async fn get_rectifier_config(
    state: tauri::State<'_, crate::AppState>,
) -> Result<crate::proxy::types::RectifierConfig, String> {
    state.db.get_rectifier_config().map_err(|e| e.to_string())
}

/// 设置整流器配置
#[tauri::command]
pub async fn set_rectifier_config(
    state: tauri::State<'_, crate::AppState>,
    config: crate::proxy::types::RectifierConfig,
) -> Result<bool, String> {
    state
        .db
        .set_rectifier_config(&config)
        .map_err(|e| e.to_string())?;
    Ok(true)
}

/// 获取优化器配置
#[tauri::command]
pub async fn get_optimizer_config(
    state: tauri::State<'_, crate::AppState>,
) -> Result<crate::proxy::types::OptimizerConfig, String> {
    state.db.get_optimizer_config().map_err(|e| e.to_string())
}

/// 设置优化器配置
#[tauri::command]
pub async fn set_optimizer_config(
    state: tauri::State<'_, crate::AppState>,
    config: crate::proxy::types::OptimizerConfig,
) -> Result<bool, String> {
    state
        .db
        .set_optimizer_config(&config)
        .map_err(|e| e.to_string())?;
    Ok(true)
}

/// 获取 Copilot 优化器配置
#[tauri::command]
pub async fn get_copilot_optimizer_config(
    state: tauri::State<'_, crate::AppState>,
) -> Result<crate::proxy::types::CopilotOptimizerConfig, String> {
    state
        .db
        .get_copilot_optimizer_config()
        .map_err(|e| e.to_string())
}

/// 设置 Copilot 优化器配置
#[tauri::command]
pub async fn set_copilot_optimizer_config(
    state: tauri::State<'_, crate::AppState>,
    config: crate::proxy::types::CopilotOptimizerConfig,
) -> Result<bool, String> {
    state
        .db
        .set_copilot_optimizer_config(&config)
        .map_err(|e| e.to_string())?;
    Ok(true)
}

/// 获取日志配置
#[tauri::command]
pub async fn get_log_config(
    state: tauri::State<'_, crate::AppState>,
) -> Result<crate::proxy::types::LogConfig, String> {
    state.db.get_log_config().map_err(|e| e.to_string())
}

/// 设置日志配置
#[tauri::command]
pub async fn set_log_config(
    state: tauri::State<'_, crate::AppState>,
    config: crate::proxy::types::LogConfig,
) -> Result<bool, String> {
    state
        .db
        .set_log_config(&config)
        .map_err(|e| e.to_string())?;
    log::set_max_level(config.to_level_filter());
    log::info!(
        "日志配置已更新: enabled={}, level={}",
        config.enabled,
        config.level
    );
    Ok(true)
}
