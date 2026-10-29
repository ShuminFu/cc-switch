//! 请求管线
//!
//! 把 forwarder 故障转移循环与单次尝试中的各个阶段拆成独立组件：
//!
//! - [`request_builder`]：请求体改写阶段（ModelMapping → VendorNormalize → Encode →
//!   Canonicalize → LocalOverrides）与路由计划；
//! - [`endpoint`]：上游端点改写与 URL 拼接；
//! - [`copilot`]：GitHub Copilot 专属的查询与优化器；
//! - [`auth`]：上游认证头（含托管账号 token）；
//! - [`headers`]：声明式出站头策略 [`headers::HeaderPolicy`]；
//! - [`transport`]：reqwest / raw hyper 发送；
//! - [`validate`]：2xx 语义校验、流式首包预读与错误体读取；
//! - [`classify`]：失败分类与日志；
//! - [`recovery`]：同一 provider 上的恢复策略；
//! - [`failover`]：故障转移记账。

pub(crate) mod auth;
pub(crate) mod classify;
pub(crate) mod copilot;
pub(crate) mod endpoint;
pub(crate) mod failover;
pub(crate) mod headers;
pub(crate) mod recovery;
pub(crate) mod request_builder;
pub(crate) mod transport;
pub(crate) mod validate;
