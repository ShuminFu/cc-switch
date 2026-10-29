//! 请求管线
//!
//! 把 forwarder 故障转移循环中的各个阶段拆成独立组件

pub(crate) mod failover;
