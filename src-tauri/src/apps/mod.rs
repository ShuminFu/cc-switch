//! 应用元数据注册表
//!
//! 所有"按应用枚举"的场景（启动导入、代理接管恢复、托盘分区等）都应从
//! [`registry::APPS`] 按能力过滤，而不是各自维护一份手写列表。

pub mod registry;

pub use registry::{AppCaps, AppDescriptor, InputTokenSemantics, SyncMode, APPS};
