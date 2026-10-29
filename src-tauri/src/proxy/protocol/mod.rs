//! 代理协议词汇表
//!
//! 用类型替代散落在各处的协议字符串字面量，让格式分发由编译器检查。

pub mod format;

#[allow(unused_imports)]
pub use format::{AppRef, BreakerKey, ClaudeApiFormat, UnknownClaudeApiFormat, WireFormat};
