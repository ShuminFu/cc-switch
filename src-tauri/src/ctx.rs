//! 命令与 service 之间的上下文接缝（CCS-181）
//!
//! Tauri 命令只做参数解析与错误转换：把 `AppState` 与一个 [`EventSink`]
//! 装进 [`Ctx`] 交给 service。service 通过 sink 发前端事件、刷新托盘，
//! 不直接依赖 `AppHandle`，因此测试可以用 [`RecordingEventSink`] 直接调用
//! service 并断言发出的事件，不再需要 `*_test_hook` 之类的命令旁路。

use std::sync::Mutex;

use serde_json::Value;
use tauri::Emitter;

use crate::store::AppState;

/// service 向外界（前端 / 托盘）发信号的出口
pub trait EventSink: Send + Sync {
    /// 向前端发射事件
    fn emit(&self, event: &str, payload: Value);

    /// 立即重建托盘菜单
    fn refresh_tray(&self);

    /// 合并短时间内的多次请求后再重建托盘菜单
    fn schedule_tray_refresh(&self);

    /// 运行中应用的句柄；测试 sink 返回 None（需要句柄的步骤随之跳过）
    fn app_handle(&self) -> Option<&tauri::AppHandle> {
        None
    }
}

/// 生产环境的 sink：事件经 `AppHandle` 发往前端
pub struct TauriEventSink {
    app: tauri::AppHandle,
}

impl TauriEventSink {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }
}

impl EventSink for TauriEventSink {
    fn emit(&self, event: &str, payload: Value) {
        if let Err(e) = self.app.emit(event, payload) {
            log::error!("发射 {event} 事件失败: {e}");
        }
    }

    fn refresh_tray(&self) {
        crate::tray::refresh_tray_menu(&self.app);
    }

    fn schedule_tray_refresh(&self) {
        crate::tray::schedule_tray_refresh(&self.app);
    }

    fn app_handle(&self) -> Option<&tauri::AppHandle> {
        Some(&self.app)
    }
}

/// 记录全部信号的 sink（测试用）
#[derive(Default)]
pub struct RecordingEventSink {
    events: Mutex<Vec<(String, Value)>>,
    tray_refreshes: Mutex<usize>,
}

impl RecordingEventSink {
    pub fn new() -> Self {
        Self::default()
    }

    /// 已发射的 `(事件名, payload)`，按发射顺序
    pub fn events(&self) -> Vec<(String, Value)> {
        self.events.lock().map(|e| e.clone()).unwrap_or_default()
    }

    /// 托盘刷新请求次数（立即与合并刷新合计）
    pub fn tray_refreshes(&self) -> usize {
        self.tray_refreshes.lock().map(|n| *n).unwrap_or_default()
    }

    fn bump_tray(&self) {
        if let Ok(mut n) = self.tray_refreshes.lock() {
            *n += 1;
        }
    }
}

impl EventSink for RecordingEventSink {
    fn emit(&self, event: &str, payload: Value) {
        if let Ok(mut events) = self.events.lock() {
            events.push((event.to_string(), payload));
        }
    }

    fn refresh_tray(&self) {
        self.bump_tray();
    }

    fn schedule_tray_refresh(&self) {
        self.bump_tray();
    }
}

/// 一次命令调用交给 service 的上下文
#[derive(Clone, Copy)]
pub struct Ctx<'a> {
    pub state: &'a AppState,
    pub events: &'a dyn EventSink,
}

impl<'a> Ctx<'a> {
    pub fn new(state: &'a AppState, events: &'a dyn EventSink) -> Self {
        Self { state, events }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn recording_sink_keeps_events_in_order_and_counts_tray_refreshes() {
        let sink = RecordingEventSink::new();
        let state = AppState::new(Arc::new(
            crate::database::Database::memory().expect("memory db"),
        ));
        let ctx = Ctx::new(&state, &sink);

        ctx.events.emit("a", json!({ "n": 1 }));
        ctx.events.emit("b", json!({ "n": 2 }));
        ctx.events.refresh_tray();
        ctx.events.schedule_tray_refresh();

        assert_eq!(
            sink.events(),
            vec![
                ("a".to_string(), json!({ "n": 1 })),
                ("b".to_string(), json!({ "n": 2 })),
            ]
        );
        assert_eq!(sink.tray_refreshes(), 2);
        assert!(ctx.events.app_handle().is_none());
    }
}
