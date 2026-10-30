//! Live 配置切换日志与撤销
//!
//! 每次普通切换（[`ProviderService::switch`] 写 live 的路径）在真实写盘的同时经
//! [`crate::config::record_writes`] 记下每个被改动文件的原始字节，切换成功后作为一条
//! [`LiveSnapshot`] 存入 `<app_config_dir>/journal/<app>/`（目录 0700、文件 0600，
//! 内容含 API Key）。每个应用最多保留 [`JOURNAL_LIMIT`] 条，旧条目在写入新条目时删除。
//!
//! [`ProviderService::revert_last_switch`] 取最近一条，把 live 文件逐字节还原为切换前
//! 的内容并恢复当前供应商，随后删除该条目。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::app_config::AppType;
use crate::config::{atomic_write, atomic_write_secret, delete_file, get_app_config_dir};
use crate::config::{record_writes, PriorContent};
use crate::error::AppError;
use crate::store::AppState;

use super::{takeover_uses_live_backup, ProviderService, SwitchResult};

/// 每个应用保留的切换日志条数上限
pub const JOURNAL_LIMIT: usize = 10;

const JOURNAL_DIR: &str = "journal";
const SNAPSHOT_EXT: &str = "json";

/// 同一毫秒内多次切换时保证条目名唯一且有序
static SNAPSHOT_SEQ: AtomicU64 = AtomicU64::new(0);

/// 一个 live 文件在切换前的内容；`before` 为 base64，`None` 表示切换前文件不存在
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotFile {
    path: String,
    before: Option<String>,
}

/// 一次切换前的 live 状态
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LiveSnapshot {
    app: String,
    created_at: i64,
    /// 切换前的当前供应商（累加模式应用没有"当前"概念，为 `None`）
    from_provider_id: Option<String>,
    to_provider_id: String,
    /// 累加模式应用：切换前目标供应商的 `live_config_managed` 标记
    #[serde(default)]
    to_live_config_managed: Option<bool>,
    files: Vec<SnapshotFile>,
}

/// 撤销最近一次切换的结果
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(specta::Type))]
pub struct RevertSwitchResult {
    /// 撤销后的当前供应商（累加模式应用为 `None`）
    pub provider_id: Option<String>,
    /// 被还原（重写或删除）的 live 文件
    pub restored_files: Vec<String>,
}

fn journal_dir(app_type: &AppType) -> PathBuf {
    get_app_config_dir()
        .join(JOURNAL_DIR)
        .join(app_type.as_str())
}

/// 按条目名（创建时间）升序列出某应用的日志文件
fn list_entries(dir: &Path) -> Result<Vec<PathBuf>, AppError> {
    let read_dir = match fs::read_dir(dir) {
        Ok(read_dir) => read_dir,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(AppError::io(dir, e)),
    };
    let mut entries: Vec<PathBuf> = read_dir
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some(SNAPSHOT_EXT)
        })
        .collect();
    entries.sort();
    Ok(entries)
}

fn ensure_private_dir(dir: &Path) -> Result<(), AppError> {
    fs::create_dir_all(dir).map_err(|e| AppError::io(dir, e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
            .map_err(|e| AppError::io(dir, e))?;
    }
    Ok(())
}

/// 只保留真正被切换改动过的 live 文件；CC Switch 自身目录（settings.json、日志本身）
/// 由撤销逻辑按语义恢复，不做逐字节还原
fn changed_live_files(priors: Vec<PriorContent>) -> Vec<SnapshotFile> {
    let own_dir = get_app_config_dir();
    let engine = base64::engine::general_purpose::STANDARD;
    priors
        .into_iter()
        .filter(|prior| !prior.path.starts_with(&own_dir))
        .filter(|prior| {
            let now = match fs::read(&prior.path) {
                Ok(bytes) => Some(bytes),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                // 读不到当前内容就无法判断，保守地保留
                Err(_) => return true,
            };
            now != prior.before
        })
        .map(|prior| SnapshotFile {
            path: prior.path.display().to_string(),
            before: prior.before.map(|bytes| engine.encode(bytes)),
        })
        .collect()
}

fn write_snapshot(app_type: &AppType, snapshot: &LiveSnapshot) -> Result<PathBuf, AppError> {
    let dir = journal_dir(app_type);
    ensure_private_dir(&dir)?;

    let seq = SNAPSHOT_SEQ.fetch_add(1, Ordering::Relaxed);
    let path = dir.join(format!(
        "{:016}-{:08}.{SNAPSHOT_EXT}",
        snapshot.created_at.max(0),
        seq % 100_000_000
    ));
    let json =
        serde_json::to_vec_pretty(snapshot).map_err(|e| AppError::JsonSerialize { source: e })?;
    atomic_write_secret(&path, &json)?;

    prune(&dir, JOURNAL_LIMIT)?;
    Ok(path)
}

/// 删除超出 `limit` 的最旧条目
fn prune(dir: &Path, limit: usize) -> Result<(), AppError> {
    let entries = list_entries(dir)?;
    let excess = entries.len().saturating_sub(limit);
    for path in entries.into_iter().take(excess) {
        fs::remove_file(&path).map_err(|e| AppError::io(&path, e))?;
    }
    Ok(())
}

fn read_snapshot(path: &Path) -> Result<LiveSnapshot, AppError> {
    let bytes = fs::read(path).map_err(|e| AppError::io(path, e))?;
    serde_json::from_slice(&bytes).map_err(|e| AppError::json(path, e))
}

fn nothing_to_revert() -> AppError {
    AppError::localized(
        "switch.nothingToRevert",
        "没有可撤销的切换",
        "There is no switch to undo.",
    )
}

impl ProviderService {
    /// 执行写 live 的切换并记入日志；记录失败只降级为不可撤销。
    pub(super) fn switch_journaled(
        state: &AppState,
        app_type: &AppType,
        id: &str,
        switch: impl FnOnce() -> Result<SwitchResult, AppError>,
    ) -> Result<SwitchResult, AppError> {
        let from_provider_id = if app_type.is_additive_mode() {
            None
        } else {
            crate::settings::get_effective_current_provider(&state.db, app_type)?
        };
        let to_live_config_managed = if app_type.is_additive_mode() {
            state
                .db
                .get_provider_by_id(id, app_type.as_str())?
                .and_then(|provider| Self::provider_live_config_managed(&provider))
        } else {
            None
        };

        let (result, priors) = record_writes(switch);
        let mut result = result?;

        let snapshot = LiveSnapshot {
            app: app_type.as_str().to_string(),
            created_at: chrono::Utc::now().timestamp_millis(),
            from_provider_id,
            to_provider_id: id.to_string(),
            to_live_config_managed,
            files: changed_live_files(priors),
        };
        match write_snapshot(app_type, &snapshot) {
            Ok(_) => result.undoable = true,
            Err(e) => log::warn!(
                "记录 {} 的切换日志失败，本次切换不可撤销: {e}",
                app_type.as_str()
            ),
        }
        Ok(result)
    }

    /// 撤销该应用最近一次切换：live 文件逐字节还原为切换前内容，并恢复当前供应商。
    ///
    /// 以下情况拒绝撤销（条目保留不动）：没有日志；代理接管期间（live 由接管备份托管）；
    /// 最近一条日志的目标供应商已不是当前供应商（之后又经别的路径改了当前供应商）。
    pub fn revert_last_switch(
        state: &AppState,
        app_type: AppType,
    ) -> Result<RevertSwitchResult, AppError> {
        // 与切换共用同一把锁，避免还原与并发切换 / 接管交错写 live
        let _switch_guard =
            futures::executor::block_on(state.proxy_service.lock_switch_for_app(app_type.as_str()));

        let dir = journal_dir(&app_type);
        let Some(entry_path) = list_entries(&dir)?.pop() else {
            return Err(nothing_to_revert());
        };
        let snapshot = read_snapshot(&entry_path)?;

        if takeover_uses_live_backup(&app_type) {
            let has_live_backup =
                futures::executor::block_on(state.db.get_live_backup(app_type.as_str()))
                    .ok()
                    .flatten()
                    .is_some();
            if has_live_backup
                || state
                    .proxy_service
                    .detect_takeover_in_live_config_for_app(&app_type)
            {
                return Err(AppError::localized(
                    "switch.revertBlockedByProxy",
                    "代理接管期间不能撤销切换，请先关闭代理接管。",
                    "Cannot undo a switch while proxy takeover is active. Turn off takeover first.",
                ));
            }
        }

        if !app_type.is_additive_mode() {
            let current = crate::settings::get_effective_current_provider(&state.db, &app_type)?;
            if current.as_deref() != Some(snapshot.to_provider_id.as_str()) {
                return Err(AppError::localized(
                    "switch.revertStale",
                    "当前供应商已在切换后变更，无法撤销。",
                    "The current provider changed after that switch, so it cannot be undone.",
                ));
            }
        }

        let engine = base64::engine::general_purpose::STANDARD;
        let mut restored_files = Vec::with_capacity(snapshot.files.len());
        for file in &snapshot.files {
            let path = PathBuf::from(&file.path);
            match &file.before {
                Some(encoded) => {
                    let bytes = engine.decode(encoded).map_err(|e| {
                        AppError::Message(format!("切换日志内容损坏 ({}): {e}", file.path))
                    })?;
                    atomic_write(&path, &bytes)?;
                }
                None => delete_file(&path)?,
            }
            restored_files.push(file.path.clone());
        }

        let provider_id = if app_type.is_additive_mode() {
            if snapshot.to_live_config_managed != Some(true) {
                if let Some(mut provider) = state
                    .db
                    .get_provider_by_id(&snapshot.to_provider_id, app_type.as_str())?
                {
                    provider
                        .meta
                        .get_or_insert_with(Default::default)
                        .live_config_managed = snapshot.to_live_config_managed;
                    state.db.save_provider(app_type.as_str(), &provider)?;
                }
            }
            None
        } else {
            let from = snapshot.from_provider_id.as_deref().filter(|id| {
                state
                    .db
                    .get_provider_by_id(id, app_type.as_str())
                    .ok()
                    .flatten()
                    .is_some()
            });
            crate::settings::set_current_provider(&app_type, from)?;
            if let Some(from) = from {
                state.db.set_current_provider(app_type.as_str(), from)?;
            }
            from.map(str::to_string)
        };

        fs::remove_file(&entry_path).map_err(|e| AppError::io(&entry_path, e))?;
        log::info!(
            "已撤销 {} 的切换 {} -> {}，还原 {} 个 live 文件",
            app_type.as_str(),
            snapshot.from_provider_id.as_deref().unwrap_or("-"),
            snapshot.to_provider_id,
            restored_files.len()
        );

        Ok(RevertSwitchResult {
            provider_id,
            restored_files,
        })
    }
}

#[cfg(test)]
mod tests;
