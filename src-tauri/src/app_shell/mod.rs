//! 应用外壳：启动流水线（[`bootstrap`]）与运行期事件路由（[`run_events`]）。
//!
//! `lib.rs` 的 `run()` 只负责组装 Builder，具体的启动步骤与事件处理都在这里。

pub mod bootstrap;
pub mod run_events;
