//! 请求路由决策
//!
//! 每次转发尝试解析出的 [`RoutePlan`] 是请求构造与响应解码共同的唯一真值。

pub mod plan;

pub use plan::{ResponseDecode, RoutePlan, SideStores, VendorQuirks};
