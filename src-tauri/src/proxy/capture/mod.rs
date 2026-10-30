//! 缺陷报告抓包（CaptureTap）：按需、限时地记录协议桥请求的完整往返。
//!
//! 用户在请求日志里开启抓包后（默认 10 分钟，到期自动关闭），每个走协议桥
//! （Anthropic ↔ Chat/Gemini/Responses、Responses ↔ Chat/Anthropic、Gemini ↔ Anthropic）的请求记录：
//!
//! - 客户端请求（方法、路径、请求头、JSON 体）；
//! - 实际发往上游的请求（最后一次尝试的 URL、请求头、JSON 体）；
//! - 上游响应体：流式按到达顺序保留每个 chunk 及其相对时间；
//! - 转换后发给客户端的响应体（同样带时间）。
//!
//! 请求头与 JSON 体在记录时即脱敏，流式字节在导出时拼接后整体脱敏（密钥可能跨
//! chunk 边界）。抓包只保存在内存里（最多 [`MAX_CAPTURES`] 条，单向流最多
//! [`MAX_STREAM_BYTES`] 字节），只有用户点击"导出缺陷报告"才写盘，导出格式即
//! `tests/fixtures/proxy/<bridge>/<case>/` 的 golden fixture 目录（见 [`export`]）。
//!
//! 透传请求不经过协议桥，不在抓包范围内：路由计划确定为透传后记录即被丢弃。

pub(crate) mod export;
pub(crate) mod redact;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use bytes::Bytes;
use futures::{Stream, StreamExt};
use serde::Serialize;
use serde_json::Value;

use crate::error::AppError;
use crate::proxy::protocol::ClaudeApiFormat;
use crate::proxy::routing::ResponseDecode;

/// 未指定时长时的默认抓包时长（分钟）
pub const DEFAULT_CAPTURE_MINUTES: u32 = 10;
/// 单次开启抓包的最长时长（分钟）
pub const MAX_CAPTURE_MINUTES: u32 = 60;
/// 内存中最多保留的抓包条数（超出时丢弃最旧的）
pub const MAX_CAPTURES: usize = 32;
/// 单向流（上游 / 客户端）最多记录的字节数，超出部分截断并标记
pub const MAX_STREAM_BYTES: usize = 8 * 1024 * 1024;

/// 被抓包的协议桥；目录名与 golden testkit 的 `Bridge::dir_name` 一致
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureBridge {
    AnthropicOpenAiChat,
    AnthropicGemini,
    AnthropicResponses,
    ResponsesChat,
    ResponsesAnthropic,
    GeminiAnthropic,
}

impl CaptureBridge {
    #[cfg(test)]
    pub const ALL: [CaptureBridge; 6] = [
        CaptureBridge::AnthropicOpenAiChat,
        CaptureBridge::AnthropicGemini,
        CaptureBridge::AnthropicResponses,
        CaptureBridge::ResponsesChat,
        CaptureBridge::ResponsesAnthropic,
        CaptureBridge::GeminiAnthropic,
    ];

    pub fn dir_name(self) -> &'static str {
        match self {
            CaptureBridge::AnthropicOpenAiChat => "anthropic__openai_chat",
            CaptureBridge::AnthropicGemini => "anthropic__gemini",
            CaptureBridge::AnthropicResponses => "anthropic__responses",
            CaptureBridge::ResponsesChat => "responses__chat",
            CaptureBridge::ResponsesAnthropic => "responses__anthropic",
            CaptureBridge::GeminiAnthropic => "gemini__anthropic",
        }
    }

    /// 由响应解码方式得出协议桥；透传（含 Anthropic → Anthropic）返回 None
    pub(crate) fn from_decode(decode: ResponseDecode) -> Option<Self> {
        match decode {
            ResponseDecode::AnthropicFrom(ClaudeApiFormat::OpenAiChat) => {
                Some(CaptureBridge::AnthropicOpenAiChat)
            }
            ResponseDecode::AnthropicFrom(ClaudeApiFormat::GeminiNative) => {
                Some(CaptureBridge::AnthropicGemini)
            }
            ResponseDecode::AnthropicFrom(ClaudeApiFormat::OpenAiResponses) => {
                Some(CaptureBridge::AnthropicResponses)
            }
            ResponseDecode::ResponsesFromChat => Some(CaptureBridge::ResponsesChat),
            ResponseDecode::ResponsesFromAnthropic => Some(CaptureBridge::ResponsesAnthropic),
            ResponseDecode::GeminiFromAnthropic => Some(CaptureBridge::GeminiAnthropic),
            ResponseDecode::AnthropicFrom(ClaudeApiFormat::Anthropic)
            | ResponseDecode::Passthrough => None,
        }
    }
}

/// 抓包开关状态（返回给前端）
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CaptureStatus {
    /// 当前是否正在抓包
    pub active: bool,
    /// 自动关闭的时间点（Unix 毫秒），未开启时为 None
    pub expires_at: Option<i64>,
    /// 距自动关闭的剩余秒数
    pub remaining_secs: u64,
    /// 已关联到请求日志、可导出的请求 ID（最新的在前）
    pub captured_request_ids: Vec<String>,
}

/// 一次请求（客户端或上游）的记录，记录时已脱敏
#[derive(Debug, Clone, Serialize)]
pub(crate) struct CapturedRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Value,
}

/// 一个响应体：流式为多个带时间的 chunk，非流式为单个 chunk
#[derive(Debug, Default)]
pub(crate) struct CapturedBody {
    /// `(相对抓包开始的毫秒数, 字节)`
    pub chunks: Vec<(u64, Bytes)>,
    pub is_stream: bool,
    pub bytes: usize,
    pub truncated: bool,
    /// 流中途出现的传输错误
    pub error: Option<String>,
}

impl CapturedBody {
    fn push(&mut self, offset_ms: u64, chunk: &Bytes) {
        if self.truncated {
            return;
        }
        let room = MAX_STREAM_BYTES.saturating_sub(self.bytes);
        if chunk.len() > room {
            self.truncated = true;
            if room == 0 {
                return;
            }
            self.chunks.push((offset_ms, chunk.slice(..room)));
            self.bytes += room;
            return;
        }
        self.bytes += chunk.len();
        self.chunks.push((offset_ms, chunk.clone()));
    }

    pub fn concat(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.bytes);
        for (_, chunk) in &self.chunks {
            out.extend_from_slice(chunk);
        }
        out
    }
}

/// 一条抓包记录
#[derive(Debug)]
pub(crate) struct CaptureRecord {
    pub id: String,
    /// 抓包开始时间（Unix 毫秒）
    pub started_at_ms: i64,
    pub app_type: String,
    pub provider_id: String,
    pub bridge: Option<CaptureBridge>,
    pub codex_oauth: bool,
    /// 请求日志中的 request_id（usage 落账时回填）
    pub request_id: Option<String>,
    pub client_request: Option<CapturedRequest>,
    pub upstream_request: Option<CapturedRequest>,
    pub upstream_status: Option<u16>,
    pub upstream: CapturedBody,
    pub client: CapturedBody,
}

/// 流向：上游响应或客户端响应
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CaptureSide {
    Upstream,
    Client,
}

#[derive(Debug)]
struct CaptureEntry {
    started: Instant,
    record: Mutex<CaptureRecord>,
}

/// 单个请求的抓包句柄；克隆开销为一次 `Arc` 计数
#[derive(Debug, Clone)]
pub struct CaptureHandle(Arc<CaptureEntry>);

impl CaptureHandle {
    fn new(app_type: &str, provider_id: &str) -> Self {
        Self(Arc::new(CaptureEntry {
            started: Instant::now(),
            record: Mutex::new(CaptureRecord {
                id: uuid::Uuid::new_v4().to_string(),
                started_at_ms: chrono::Utc::now().timestamp_millis(),
                app_type: app_type.to_string(),
                provider_id: provider_id.to_string(),
                bridge: None,
                codex_oauth: false,
                request_id: None,
                client_request: None,
                upstream_request: None,
                upstream_status: None,
                upstream: CapturedBody::default(),
                client: CapturedBody::default(),
            }),
        }))
    }

    fn lock(&self) -> MutexGuard<'_, CaptureRecord> {
        self.0
            .record
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn elapsed_ms(&self) -> u64 {
        self.0.started.elapsed().as_millis() as u64
    }

    /// 记录客户端请求（请求头与 JSON 体在此脱敏）
    pub(crate) fn record_client_request(
        &self,
        method: &http::Method,
        uri: &http::Uri,
        headers: &http::HeaderMap,
        body: &Value,
    ) {
        let request = CapturedRequest {
            method: method.to_string(),
            url: redact::redact_text(&uri.to_string()),
            headers: redact::redact_headers(headers),
            body: redact::redact_json(body),
        };
        self.lock().client_request = Some(request);
    }

    /// 记录发往上游的请求；故障转移时每次尝试覆盖一次，最终保留成功的那次
    pub(crate) fn record_upstream_request(
        &self,
        method: &http::Method,
        url: &str,
        headers: &http::HeaderMap,
        body: &Value,
    ) {
        let request = CapturedRequest {
            method: method.to_string(),
            url: redact::redact_text(url),
            headers: redact::redact_headers(headers),
            body: redact::redact_json(body),
        };
        self.lock().upstream_request = Some(request);
    }

    /// 记录路由结果：成功的 provider、协议桥与 Codex OAuth 标记
    pub(crate) fn set_route(&self, provider_id: &str, bridge: CaptureBridge, codex_oauth: bool) {
        let mut record = self.lock();
        record.provider_id = provider_id.to_string();
        record.bridge = Some(bridge);
        record.codex_oauth = codex_oauth;
    }

    pub(crate) fn set_upstream_status(&self, status: u16) {
        self.lock().upstream_status = Some(status);
    }

    /// 把这次抓包关联到请求日志行
    pub(crate) fn set_request_id(&self, request_id: &str) {
        self.lock().request_id = Some(request_id.to_string());
    }

    /// 记录整包（非流式）响应体
    pub(crate) fn record_body(&self, side: CaptureSide, body: &[u8]) {
        let offset = self.elapsed_ms();
        let mut record = self.lock();
        let target = record.side_mut(side);
        target.is_stream = false;
        target.push(offset, &Bytes::copy_from_slice(body));
    }

    /// 记录转换后的整包 JSON 响应
    pub(crate) fn record_json(&self, side: CaptureSide, body: &Value) {
        if let Ok(bytes) = serde_json::to_vec(body) {
            self.record_body(side, &bytes);
        }
    }

    fn record_stream_item(&self, side: CaptureSide, item: &Result<Bytes, std::io::Error>) {
        let offset = self.elapsed_ms();
        let mut record = self.lock();
        let target = record.side_mut(side);
        target.is_stream = true;
        match item {
            Ok(chunk) => target.push(offset, chunk),
            Err(error) => {
                if target.error.is_none() {
                    target.error = Some(redact::redact_text(&error.to_string()));
                }
            }
        }
    }

    pub(crate) fn request_id(&self) -> Option<String> {
        self.lock().request_id.clone()
    }

    fn ptr_eq(&self, other: &CaptureHandle) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// 在加锁状态下读取记录（导出用）
    pub(crate) fn with_record<R>(&self, f: impl FnOnce(&CaptureRecord) -> R) -> R {
        f(&self.lock())
    }
}

impl CaptureRecord {
    fn side_mut(&mut self, side: CaptureSide) -> &mut CapturedBody {
        match side {
            CaptureSide::Upstream => &mut self.upstream,
            CaptureSide::Client => &mut self.client,
        }
    }
}

/// 在字节流上旁路记录每个 chunk；`capture` 为 None 时原样透传
pub(crate) fn tap_stream<S>(
    capture: Option<CaptureHandle>,
    side: CaptureSide,
    stream: S,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
{
    stream.map(move |item| {
        if let Some(capture) = &capture {
            capture.record_stream_item(side, &item);
        }
        item
    })
}

#[derive(Debug, Default)]
struct StoreInner {
    armed_until: Option<Instant>,
    expires_at_ms: Option<i64>,
    entries: VecDeque<CaptureHandle>,
}

/// 抓包存储：开关（带到期时间）与最近的抓包记录
#[derive(Debug, Default)]
pub struct CaptureStore {
    /// 快速路径：未开启时每个请求只读一次原子变量
    armed: AtomicBool,
    inner: Mutex<StoreInner>,
}

static GLOBAL_STORE: LazyLock<Arc<CaptureStore>> =
    LazyLock::new(|| Arc::new(CaptureStore::default()));

impl CaptureStore {
    /// 应用级共享的存储：代理服务重启后抓包记录仍保留，供请求日志导出
    pub fn global() -> Arc<CaptureStore> {
        GLOBAL_STORE.clone()
    }

    fn lock(&self) -> MutexGuard<'_, StoreInner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 开启抓包 `duration`（上限 [`MAX_CAPTURE_MINUTES`]），到期自动关闭
    pub fn arm(&self, duration: Duration) -> CaptureStatus {
        let duration = duration.min(Duration::from_secs(u64::from(MAX_CAPTURE_MINUTES) * 60));
        {
            let mut inner = self.lock();
            inner.armed_until = Some(Instant::now() + duration);
            inner.expires_at_ms =
                Some(chrono::Utc::now().timestamp_millis() + duration.as_millis() as i64);
        }
        self.armed.store(true, Ordering::Release);
        log::info!(
            "[Capture] 已开启缺陷报告抓包，{} 秒后自动关闭",
            duration.as_secs()
        );
        self.status()
    }

    /// 立即关闭抓包（已有记录保留，仍可导出）
    pub fn disarm(&self) -> CaptureStatus {
        self.armed.store(false, Ordering::Release);
        {
            let mut inner = self.lock();
            inner.armed_until = None;
            inner.expires_at_ms = None;
        }
        self.status()
    }

    /// 是否处于抓包窗口内；到期时顺带关闭开关
    fn is_armed(&self, inner: &mut StoreInner) -> bool {
        match inner.armed_until {
            Some(until) if Instant::now() < until => true,
            Some(_) => {
                inner.armed_until = None;
                inner.expires_at_ms = None;
                self.armed.store(false, Ordering::Release);
                log::info!("[Capture] 缺陷报告抓包已到期自动关闭");
                false
            }
            None => false,
        }
    }

    pub fn status(&self) -> CaptureStatus {
        let mut inner = self.lock();
        let active = self.is_armed(&mut inner);
        let remaining_secs = inner
            .armed_until
            .map(|until| until.saturating_duration_since(Instant::now()).as_secs())
            .unwrap_or(0);
        let expires_at = inner.expires_at_ms;
        let entries: Vec<CaptureHandle> = inner.entries.iter().cloned().collect();
        drop(inner);
        let captured_request_ids = entries
            .iter()
            .rev()
            .filter_map(CaptureHandle::request_id)
            .collect();
        CaptureStatus {
            active,
            expires_at,
            remaining_secs,
            captured_request_ids,
        }
    }

    /// 抓包开启时为新请求创建记录；未开启时返回 None（热路径只读一次原子变量）
    pub(crate) fn begin(&self, app_type: &str, provider_id: &str) -> Option<CaptureHandle> {
        if !self.armed.load(Ordering::Acquire) {
            return None;
        }
        let mut inner = self.lock();
        if !self.is_armed(&mut inner) {
            return None;
        }
        let handle = CaptureHandle::new(app_type, provider_id);
        if inner.entries.len() >= MAX_CAPTURES {
            inner.entries.pop_front();
        }
        inner.entries.push_back(handle.clone());
        Some(handle)
    }

    /// 丢弃一条记录（透传请求、转发失败等不在抓包范围内的请求）
    pub(crate) fn discard(&self, handle: &CaptureHandle) {
        self.lock().entries.retain(|entry| !entry.ptr_eq(handle));
    }

    /// 按请求日志的 request_id 查找抓包（同一 ID 取最新的一条）
    pub(crate) fn find(&self, request_id: &str) -> Option<CaptureHandle> {
        let entries: Vec<CaptureHandle> = self.lock().entries.iter().cloned().collect();
        entries
            .into_iter()
            .rev()
            .find(|entry| entry.request_id().as_deref() == Some(request_id))
    }

    /// 导出 `request_id` 对应的缺陷报告 zip（golden fixture 目录结构）
    pub fn export_zip(&self, request_id: &str) -> Result<Vec<u8>, AppError> {
        let handle = self.find(request_id).ok_or_else(|| {
            AppError::localized(
                "proxy.capture.not_found",
                format!("没有找到请求 {request_id} 的抓包记录（需先开启抓包再复现问题）"),
                format!(
                    "No capture found for request {request_id} (turn on capture, then reproduce the issue)"
                ),
            )
        })?;
        handle.with_record(export::build_zip)
    }
}

#[cfg(test)]
mod tests;
