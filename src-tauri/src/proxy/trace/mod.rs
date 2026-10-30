//! 请求追踪（Trace Inspector）：保留最近 [`MAX_TRACES`] 个代理请求的路由过程。
//!
//! 每个经过代理转发的请求记录：
//!
//! - 客户端请求概要（方法、路径、脱敏后的请求头）；
//! - 每次尝试：provider、该次尝试的 [`RoutePlan`] 摘要、应用的恢复策略、
//!   结果（成功 / 失败 / 被熔断器跳过）与原因、上游状态码、脱敏后的出站请求头；
//! - 成功尝试的首字节时间（TTFB）、首 token 时间与 usage 来源；
//! - 最终结果，以及写入请求日志后的 request_id（请求日志按它打开详情面板）。
//!
//! 与缺陷报告抓包（[`capture`](super::capture)）不同，追踪始终开启、不记录请求体与
//! 响应体，因此只占用很小的固定内存：环形缓冲区最多 [`MAX_TRACES`] 条，单条最多
//! [`MAX_TRACE_ATTEMPTS`] 次尝试，请求头值与原因文本按长度截断。

use std::collections::VecDeque;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use std::time::Instant;

use serde::Serialize;

use super::capture::redact;
use super::routing::RoutePlan;

/// 环形缓冲区保留的请求数（超出时丢弃最旧的）
pub const MAX_TRACES: usize = 100;
/// 单个请求最多记录的尝试次数（含被熔断器跳过的 provider）
pub const MAX_TRACE_ATTEMPTS: usize = 16;
/// 请求头值的最大长度（字符），超出部分截断
const MAX_HEADER_VALUE_CHARS: usize = 256;
/// 失败原因的最大长度（字符），超出部分截断
const MAX_REASON_CHARS: usize = 1024;

/// 一个脱敏后的请求头
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TraceHeader {
    pub name: String,
    pub value: String,
}

/// 路由计划摘要（URL 已去掉 query 并脱敏）
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TraceRoutePlan {
    /// 客户端线协议
    pub client: String,
    /// 上游线协议
    pub upstream: String,
    /// Claude 客户端解析出的上游 API 格式
    pub api_format: Option<String>,
    /// 响应解码方式
    pub decode: String,
    pub url: String,
    pub auth: Option<String>,
    /// 生效的供应商特性开关
    pub quirks: Vec<String>,
    /// 本次尝试读写的旁路存储
    pub side_stores: Vec<String>,
}

impl TraceRoutePlan {
    fn from_plan(plan: &RoutePlan) -> Self {
        let url = plan
            .url
            .split_once('?')
            .map_or(plan.url.as_str(), |(path, _)| path);
        let quirks = [
            ("copilot", plan.quirks.copilot),
            ("codex_oauth", plan.quirks.codex_oauth),
            ("full_url", plan.quirks.full_url),
            (
                "codex_official_auth_passthrough",
                plan.quirks.codex_official_auth_passthrough,
            ),
            (
                "impersonate_claude_code",
                plan.quirks.impersonate_claude_code,
            ),
        ];
        let side_stores = [
            ("codex_chat_history", plan.side_stores.codex_chat_history),
            ("gemini_shadow", plan.side_stores.gemini_shadow),
        ];
        let enabled = |flags: &[(&str, bool)]| {
            flags
                .iter()
                .filter(|(_, on)| *on)
                .map(|(name, _)| name.to_string())
                .collect()
        };
        Self {
            client: format!("{:?}", plan.client),
            upstream: format!("{:?}", plan.upstream),
            api_format: plan
                .claude_api_format
                .map(|format| format.as_str().to_string()),
            decode: format!("{:?}", plan.response_decode()),
            url: redact::redact_text(url),
            auth: plan.auth.map(|auth| format!("{auth:?}")),
            quirks: enabled(&quirks),
            side_stores: enabled(&side_stores),
        }
    }
}

/// 一次尝试的结果
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AttemptStatus {
    /// 已发出、尚未得出结果（请求被取消时停留在此状态）
    InFlight,
    Succeeded,
    Failed,
    /// 被熔断器拒绝，没有发送
    Skipped,
}

/// 对单个 provider 的一次尝试（含同一 provider 上的恢复重试）
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TraceAttempt {
    pub provider_id: String,
    pub provider_name: String,
    pub status: AttemptStatus,
    /// 失败或跳过的原因（已脱敏、截断）
    pub reason: Option<String>,
    /// 上游 HTTP 状态码（未收到响应时为 None）
    pub upstream_status: Option<u16>,
    /// 该次尝试最后一次发送所用的路由计划（发送前失败时为 None）
    pub plan: Option<TraceRoutePlan>,
    /// 按顺序应用的恢复策略
    pub recovery: Vec<String>,
    /// 最后一次发送的出站请求头（已脱敏）
    pub upstream_headers: Vec<TraceHeader>,
    /// 相对请求开始的毫秒数
    pub started_ms: u64,
    pub duration_ms: Option<u64>,
}

/// usage 数据来源
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UsageSource {
    /// 流式响应中的 usage 事件
    StreamEvents,
    /// 非流式响应体中的 usage 字段
    ResponseBody,
    /// 响应中没有可用的 usage
    Missing,
}

/// 一个请求的追踪记录（序列化后返回给前端）
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RequestTrace {
    pub trace_id: String,
    /// 请求日志中的 request_id（写入请求日志后回填）
    pub request_id: Option<String>,
    /// 请求开始时间（Unix 毫秒）
    pub started_at: i64,
    pub app_type: String,
    pub method: String,
    /// 客户端请求路径（已脱敏）
    pub path: String,
    pub is_stream: bool,
    /// 客户端请求头（已脱敏）
    pub client_headers: Vec<TraceHeader>,
    pub attempts: Vec<TraceAttempt>,
    /// 超出 [`MAX_TRACE_ATTEMPTS`] 而未记录的尝试数
    pub dropped_attempts: usize,
    /// 返回给客户端的状态码（转发结束后回填）
    pub status_code: Option<u16>,
    /// 最终错误（所有尝试失败或客户端错误时）
    pub error: Option<String>,
    /// 成功尝试收到上游响应头的时间（相对请求开始）
    pub ttfb_ms: Option<u64>,
    /// 流式响应首个 token 事件的时间（相对请求开始）
    pub first_token_ms: Option<u64>,
    /// 请求总耗时（写入请求日志时回填）
    pub latency_ms: Option<u64>,
    pub usage_source: Option<UsageSource>,
}

#[derive(Debug)]
struct TraceEntry {
    started: Instant,
    trace: Mutex<RequestTrace>,
}

/// 单个请求的追踪句柄；克隆开销为一次 `Arc` 计数
#[derive(Debug, Clone)]
pub struct TraceHandle(Arc<TraceEntry>);

fn truncate(text: String, max_chars: usize) -> String {
    match text.char_indices().nth(max_chars) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text,
    }
}

fn trace_headers(headers: &http::HeaderMap) -> Vec<TraceHeader> {
    redact::redact_headers(headers)
        .into_iter()
        .map(|(name, value)| TraceHeader {
            name,
            value: truncate(value, MAX_HEADER_VALUE_CHARS),
        })
        .collect()
}

impl TraceHandle {
    fn new(app_type: &str, method: &http::Method, uri: &http::Uri, is_stream: bool) -> Self {
        let path = uri
            .path_and_query()
            .map_or(uri.path(), |path_and_query| path_and_query.as_str());
        Self(Arc::new(TraceEntry {
            started: Instant::now(),
            trace: Mutex::new(RequestTrace {
                trace_id: uuid::Uuid::new_v4().to_string(),
                request_id: None,
                started_at: chrono::Utc::now().timestamp_millis(),
                app_type: app_type.to_string(),
                method: method.to_string(),
                path: redact::redact_text(path),
                is_stream,
                client_headers: Vec::new(),
                attempts: Vec::new(),
                dropped_attempts: 0,
                status_code: None,
                error: None,
                ttfb_ms: None,
                first_token_ms: None,
                latency_ms: None,
                usage_source: None,
            }),
        }))
    }

    fn lock(&self) -> MutexGuard<'_, RequestTrace> {
        self.0
            .trace
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn elapsed_ms(&self) -> u64 {
        self.0.started.elapsed().as_millis() as u64
    }

    /// 当前（最后一次）尝试；超出上限未记录时为 None
    fn with_current_attempt(&self, f: impl FnOnce(&mut TraceAttempt)) {
        let mut trace = self.lock();
        if trace.dropped_attempts > 0 {
            return;
        }
        if let Some(attempt) = trace.attempts.last_mut() {
            f(attempt);
        }
    }

    /// 记录客户端请求头（在此脱敏）
    pub(crate) fn record_client_headers(&self, headers: &http::HeaderMap) {
        let headers = trace_headers(headers);
        self.lock().client_headers = headers;
    }

    fn push_attempt(&self, attempt: TraceAttempt) {
        let mut trace = self.lock();
        if trace.attempts.len() >= MAX_TRACE_ATTEMPTS {
            trace.dropped_attempts += 1;
            return;
        }
        trace.attempts.push(attempt);
    }

    fn attempt(
        &self,
        provider_id: &str,
        provider_name: &str,
        status: AttemptStatus,
    ) -> TraceAttempt {
        TraceAttempt {
            provider_id: provider_id.to_string(),
            provider_name: provider_name.to_string(),
            status,
            reason: None,
            upstream_status: None,
            plan: None,
            recovery: Vec::new(),
            upstream_headers: Vec::new(),
            started_ms: self.elapsed_ms(),
            duration_ms: None,
        }
    }

    /// 开始对 `provider` 的一次尝试
    pub(crate) fn begin_attempt(&self, provider_id: &str, provider_name: &str) {
        self.push_attempt(self.attempt(provider_id, provider_name, AttemptStatus::InFlight));
    }

    /// 记录被熔断器跳过的 provider
    pub(crate) fn skip_attempt(&self, provider_id: &str, provider_name: &str, reason: &str) {
        let mut attempt = self.attempt(provider_id, provider_name, AttemptStatus::Skipped);
        attempt.reason = Some(reason.to_string());
        attempt.duration_ms = Some(0);
        self.push_attempt(attempt);
    }

    /// 记录当前尝试即将发送的路由计划与出站请求头（恢复重试时覆盖上一次）
    pub(crate) fn record_send(&self, plan: &RoutePlan, headers: &http::HeaderMap) {
        let plan = TraceRoutePlan::from_plan(plan);
        let headers = trace_headers(headers);
        self.with_current_attempt(|attempt| {
            attempt.plan = Some(plan);
            attempt.upstream_headers = headers;
        });
    }

    /// 当前尝试成功：收到上游响应头的时间即 TTFB
    pub(crate) fn succeed_attempt(&self, upstream_status: u16, recovery: &[&str]) {
        let now = self.elapsed_ms();
        self.with_current_attempt(|attempt| {
            attempt.status = AttemptStatus::Succeeded;
            attempt.upstream_status = Some(upstream_status);
            attempt.recovery = recovery.iter().map(|name| name.to_string()).collect();
            attempt.duration_ms = Some(now.saturating_sub(attempt.started_ms));
        });
        self.lock().ttfb_ms = Some(now);
    }

    /// 当前尝试失败（原因在此脱敏、截断）
    pub(crate) fn fail_attempt(
        &self,
        reason: &str,
        upstream_status: Option<u16>,
        recovery: &[&str],
    ) {
        let now = self.elapsed_ms();
        let reason = truncate(redact::redact_text(reason), MAX_REASON_CHARS);
        self.with_current_attempt(|attempt| {
            attempt.status = AttemptStatus::Failed;
            attempt.reason = Some(reason);
            attempt.upstream_status = upstream_status;
            attempt.recovery = recovery.iter().map(|name| name.to_string()).collect();
            attempt.duration_ms = Some(now.saturating_sub(attempt.started_ms));
        });
    }

    /// 转发结束：记录返回给客户端的状态码与最终错误
    pub(crate) fn finish(&self, status_code: u16, error: Option<&str>) {
        let error = error.map(|error| truncate(redact::redact_text(error), MAX_REASON_CHARS));
        let mut trace = self.lock();
        trace.status_code = Some(status_code);
        trace.error = error;
    }

    pub(crate) fn set_usage_source(&self, source: UsageSource) {
        self.lock().usage_source = Some(source);
    }

    /// 把追踪关联到请求日志行，并记录总耗时与首 token 时间
    pub(crate) fn link_request(
        &self,
        request_id: &str,
        latency_ms: u64,
        first_token_ms: Option<u64>,
    ) {
        let mut trace = self.lock();
        trace.request_id = Some(request_id.to_string());
        trace.latency_ms = Some(latency_ms);
        if first_token_ms.is_some() {
            trace.first_token_ms = first_token_ms;
        }
    }

    pub(crate) fn snapshot(&self) -> RequestTrace {
        self.lock().clone()
    }
}

/// 追踪存储：最近 [`MAX_TRACES`] 个请求的环形缓冲区
#[derive(Debug)]
pub struct TraceStore {
    capacity: usize,
    entries: Mutex<VecDeque<TraceHandle>>,
}

impl Default for TraceStore {
    fn default() -> Self {
        Self::with_capacity(MAX_TRACES)
    }
}

static GLOBAL_STORE: LazyLock<Arc<TraceStore>> = LazyLock::new(|| Arc::new(TraceStore::default()));

impl TraceStore {
    /// 应用级共享的存储：代理服务重启后追踪仍保留，供请求日志查看
    pub fn global() -> Arc<TraceStore> {
        GLOBAL_STORE.clone()
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            capacity,
            entries: Mutex::new(VecDeque::with_capacity(capacity)),
        }
    }

    fn lock(&self) -> MutexGuard<'_, VecDeque<TraceHandle>> {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 为新请求创建追踪；缓冲区已满时丢弃最旧的一条
    pub(crate) fn begin(
        &self,
        app_type: &str,
        method: &http::Method,
        uri: &http::Uri,
        is_stream: bool,
    ) -> TraceHandle {
        let handle = TraceHandle::new(app_type, method, uri, is_stream);
        let mut entries = self.lock();
        while entries.len() >= self.capacity {
            entries.pop_front();
        }
        entries.push_back(handle.clone());
        handle
    }

    /// 缓冲区中的追踪条数
    #[cfg_attr(not(any(test, feature = "testkit")), allow(dead_code))]
    pub fn count(&self) -> usize {
        self.lock().len()
    }

    /// 按请求日志的 request_id 查找追踪（同一 ID 取最新的一条）
    pub fn find(&self, request_id: &str) -> Option<RequestTrace> {
        let entries: Vec<TraceHandle> = self.lock().iter().cloned().collect();
        entries
            .iter()
            .rev()
            .find(|entry| entry.lock().request_id.as_deref() == Some(request_id))
            .map(TraceHandle::snapshot)
    }
}

#[cfg(test)]
mod tests;
