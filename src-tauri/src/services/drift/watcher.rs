//! live 文件监听：应用外的改动落盘后防抖重新检测漂移，经 [`DRIFT_EVENT`] 通知前端。
//!
//! 监听各应用配置目录（非递归），只对漂移报告里出现过的 live 文件做出反应。
//! CC Switch 自己写 live 同样会触发事件，但写完后 live 与预期输出一致，检测不会
//! 报告漂移；同一份漂移（相同指纹）只推送一次，被忽略的漂移不推送。

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tauri::{Emitter, Manager};

use super::{DriftReport, DriftService, DRIFT_EVENT};
use crate::app_config::AppType;
use crate::error::AppError;
use crate::store::AppState;

/// 事件安静这么久后才重新检测（编辑器保存常是"写临时文件 + 改名"多次事件）
const DEBOUNCE: Duration = Duration::from_millis(800);

/// 持有底层监听器；drop 后监听线程随之退出
pub struct DriftWatcher {
    _watcher: RecommendedWatcher,
}

/// Tauri 托管状态：保活监听器
#[derive(Default)]
pub struct DriftWatcherState(pub Mutex<Option<DriftWatcher>>);

/// 监听 `dirs`（不存在的目录跳过），事件安静 `debounce` 后以累计的路径调用 `on_change`
pub fn watch_dirs(
    dirs: &[PathBuf],
    debounce: Duration,
    mut on_change: impl FnMut(Vec<PathBuf>) + Send + 'static,
) -> Result<DriftWatcher, AppError> {
    let (tx, rx) = mpsc::channel::<Vec<PathBuf>>();
    let mut watcher =
        notify::recommended_watcher(move |res: notify::Result<notify::Event>| match res {
            Ok(event) if !matches!(event.kind, EventKind::Access(_)) => {
                let _ = tx.send(event.paths);
            }
            Ok(_) => {}
            Err(e) => log::debug!("live 配置文件监听事件错误: {e}"),
        })
        .map_err(|e| AppError::Message(format!("创建 live 配置文件监听失败: {e}")))?;

    for dir in dirs.iter().filter(|dir| dir.is_dir()) {
        if let Err(e) = watcher.watch(dir, RecursiveMode::NonRecursive) {
            log::warn!("监听 {} 失败: {e}", dir.display());
        }
    }

    std::thread::Builder::new()
        .name("live-drift-watcher".to_string())
        .spawn(move || {
            while let Ok(first) = rx.recv() {
                let mut pending: BTreeSet<PathBuf> = first.into_iter().collect();
                loop {
                    match rx.recv_timeout(debounce) {
                        Ok(paths) => pending.extend(paths),
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => {
                            on_change(pending.into_iter().collect());
                            return;
                        }
                    }
                }
                on_change(pending.into_iter().collect());
            }
        })
        .map_err(|e| AppError::Message(format!("启动 live 配置文件监听线程失败: {e}")))?;

    Ok(DriftWatcher { _watcher: watcher })
}

/// 记录各应用的 live 文件与已推送的指纹，决定哪些报告需要推送
#[derive(Debug, Default)]
pub struct DriftTracker {
    files: HashMap<String, BTreeSet<PathBuf>>,
    notified: HashMap<String, String>,
}

impl DriftTracker {
    /// 改动路径涉及的应用（只认报告里出现过的 live 文件）
    pub fn apps_for_paths(&self, paths: &[PathBuf]) -> Vec<AppType> {
        DriftService::supported_apps()
            .filter(|app| {
                self.files
                    .get(app.as_str())
                    .is_some_and(|files| paths.iter().any(|path| files.contains(path)))
            })
            .cloned()
            .collect()
    }

    /// 吸收新报告，返回需要推送给前端的那些
    pub fn ingest(&mut self, reports: Vec<DriftReport>) -> Vec<DriftReport> {
        let mut to_notify = Vec::new();
        for report in reports {
            // 同时记录规范化路径：macOS FSEvents 报告的是解析过符号链接的路径
            let files = report
                .files
                .iter()
                .map(PathBuf::from)
                .flat_map(|path| {
                    let canonical = std::fs::canonicalize(&path).ok();
                    std::iter::once(path).chain(canonical)
                })
                .collect();
            self.files.insert(report.app.clone(), files);
            if !report.needs_attention() {
                self.notified.remove(&report.app);
                continue;
            }
            if self.notified.get(&report.app) != Some(&report.fingerprint) {
                self.notified
                    .insert(report.app.clone(), report.fingerprint.clone());
                to_notify.push(report);
            }
        }
        to_notify
    }
}

/// 启动时检测一次全部应用并开始监听。应在异常退出恢复与代理状态恢复之后调用，
/// 并放在阻塞线程中执行（检测会同步读写数据库与文件）。
pub fn start(handle: tauri::AppHandle) -> Result<DriftWatcher, AppError> {
    let tracker = Arc::new(Mutex::new(DriftTracker::default()));

    let mut dirs: Vec<PathBuf> = DriftService::supported_apps()
        .filter_map(|app| crate::apps::adapter(app).config_dir().ok())
        .collect();
    dirs.sort();
    dirs.dedup();

    let initial = DriftService::detect_all(&handle.state::<AppState>());
    notify_reports(&handle, &tracker, initial);

    let watch_handle = handle.clone();
    let watch_tracker = tracker.clone();
    watch_dirs(&dirs, DEBOUNCE, move |paths| {
        let apps = match watch_tracker.lock() {
            Ok(tracker) => tracker.apps_for_paths(&paths),
            Err(_) => return,
        };
        if apps.is_empty() {
            return;
        }
        let state = watch_handle.state::<AppState>();
        let reports = apps
            .iter()
            .filter_map(|app| match DriftService::detect(&state, app) {
                Ok(report) => Some(report),
                Err(e) => {
                    log::warn!("检测 {} live 配置漂移失败: {e}", app.as_str());
                    None
                }
            })
            .collect();
        notify_reports(&watch_handle, &watch_tracker, reports);
    })
}

fn notify_reports(
    handle: &tauri::AppHandle,
    tracker: &Mutex<DriftTracker>,
    reports: Vec<DriftReport>,
) {
    let to_notify = match tracker.lock() {
        Ok(mut tracker) => tracker.ingest(reports),
        Err(_) => return,
    };
    for report in to_notify {
        log::info!(
            "{} live 配置漂移：残留占位符={}，字段 {} 处",
            report.app,
            report.leftover_placeholder,
            report.fields.len()
        );
        if let Err(e) = handle.emit(DRIFT_EVENT, &report) {
            log::warn!("推送 live 配置漂移事件失败: {e}");
        }
    }
}
