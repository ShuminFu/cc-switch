//! 响应处理器模块
//!
//! 统一处理流式和非流式 API 响应

use super::{
    capture::{tap_stream, CaptureHandle, CaptureSide},
    content_encoding::{decompress_body, get_content_encoding},
    forwarder::ActiveConnectionGuard,
    handler_config::{
        parser_config_for, ResponseUsageParser, StreamUsageEventFilter, UsageParserConfig,
    },
    handler_context::{RequestContext, StreamingTimeoutConfig},
    hyper_client::ProxyResponse,
    protocol::WireFormat,
    server::ProxyState,
    stream::{SseDecoder, SseEvent, StreamUsageFormat, TimingTap, UsageTap},
    usage::parser::TokenUsage,
    ProxyError,
};
use crate::database::PRICING_SOURCE_REQUEST;
use axum::http::{header::HeaderMap, HeaderName};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::Value;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

// ============================================================================
// 响应头处理
// ============================================================================

/// RFC 2616 / RFC 7230 中定义的不应被代理继续转发的响应头。
const HOP_BY_HOP_RESPONSE_HEADERS: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "proxy-connection",
    "te",
    "trailer",
    "trailers",
    "transfer-encoding",
    "upgrade",
];

/// 移除响应侧 hop-by-hop 头，以及 `Connection` 中点名的扩展头。
pub(crate) fn strip_hop_by_hop_response_headers(headers: &mut HeaderMap) {
    let connection_listed_headers: Vec<HeaderName> = headers
        .get_all(axum::http::header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .filter_map(|name| HeaderName::from_bytes(name.as_bytes()).ok())
        .collect();

    for name in HOP_BY_HOP_RESPONSE_HEADERS {
        headers.remove(*name);
    }

    for name in connection_listed_headers {
        headers.remove(name);
    }
}

/// 移除在重建响应体后会失真的实体头。
pub(crate) fn strip_entity_headers_for_rebuilt_body(headers: &mut HeaderMap) {
    headers.remove(axum::http::header::CONTENT_ENCODING);
    headers.remove(axum::http::header::CONTENT_LENGTH);
    headers.remove(axum::http::header::TRANSFER_ENCODING);
}

/// 读取响应体并在需要时解压，确保 headers 与返回 body 一致。
///
/// `body_timeout`: 整包超时。当非零时用 `tokio::time::timeout` 包住 `.bytes()` 调用，
/// 防止上游发完响应头后卡住 body 导致请求永远挂住。
/// 传入 `Duration::ZERO` 表示不启用超时；handler 调用方通过
/// `RequestContext::non_streaming_body_timeout()` 获取，始终非零（故障转移关闭时兜底 600s）。
pub(crate) async fn read_decoded_body(
    response: ProxyResponse,
    tag: &str,
    body_timeout: Duration,
) -> Result<(HeaderMap, http::StatusCode, Bytes), ProxyError> {
    let mut headers = response.headers().clone();
    let status = response.status();
    let raw_bytes = if body_timeout.is_zero() {
        response.bytes().await?
    } else {
        tokio::time::timeout(body_timeout, response.bytes())
            .await
            .map_err(|_| {
                ProxyError::Timeout(format!(
                    "响应体读取超时: {}s（上游发完响应头后 body 未到达）",
                    body_timeout.as_secs()
                ))
            })??
    };

    log::debug!(
        "[{tag}] 已接收上游响应体: status={}, bytes={}, headers={}",
        status.as_u16(),
        raw_bytes.len(),
        format_headers(&headers)
    );

    let mut body_bytes = raw_bytes.clone();
    let mut decoded = false;

    if let Some(encoding) = get_content_encoding(&headers) {
        log::debug!("[{tag}] 解压非流式响应: content-encoding={encoding}");
        match decompress_body(&encoding, &raw_bytes) {
            Ok(Some(decompressed)) => {
                body_bytes = Bytes::from(decompressed);
                decoded = true;
            }
            // 不支持的编码：原样透传且保留 content-encoding 头，
            // 让下游诊断/客户端知道这仍是压缩字节
            Ok(None) => {}
            Err(e) => {
                log::warn!("[{tag}] 解压失败 ({encoding}): {e}，使用原始数据");
            }
        }
    }

    if decoded {
        strip_entity_headers_for_rebuilt_body(&mut headers);
    }

    Ok((headers, status, body_bytes))
}

// ============================================================================
// 公共接口
// ============================================================================

/// 检测响应是否为 SSE 流式响应
#[inline]
pub fn is_sse_response(response: &ProxyResponse) -> bool {
    response.is_sse()
}

/// 处理流式响应
pub async fn handle_streaming(
    response: ProxyResponse,
    ctx: &RequestContext,
    state: &ProxyState,
    parser_config: &UsageParserConfig,
    connection_guard: Option<ActiveConnectionGuard>,
) -> Response {
    let status = response.status();
    log::debug!(
        "[{}] 已接收上游流式响应: status={}, headers={}",
        ctx.tag,
        status.as_u16(),
        format_headers(response.headers())
    );
    // 检查流式响应是否被压缩（SSE 通常不压缩，如果压缩则 SSE 解析会失败）
    if let Some(encoding) = get_content_encoding(response.headers()) {
        log::warn!(
            "[{}] 流式响应含 content-encoding={encoding}，SSE 解析可能失败。\
             上游在 accept-encoding 透传后压缩了 SSE 流。",
            ctx.tag
        );
    }

    let mut response_headers = response.headers().clone();
    strip_hop_by_hop_response_headers(&mut response_headers);

    let mut builder = axum::response::Response::builder().status(status);

    // 复制响应头
    for (key, value) in &response_headers {
        builder = builder.header(key, value);
    }

    // 创建字节流
    let stream = response.bytes_stream();

    // 创建使用量收集器；关闭 usage logging 时不要在流式热路径上解析每个 SSE event。
    let usage_collector = create_usage_collector(
        ctx,
        state,
        status.as_u16(),
        parser_config,
        MissingStreamUsage::Record,
    );

    // 获取流式超时配置
    let timeout_config = ctx.streaming_timeout_config();

    // 创建带日志和超时的透传流
    let logged_stream = create_logged_passthrough_stream(
        stream,
        ctx.tag,
        usage_collector,
        timeout_config,
        connection_guard,
    );

    let body = axum::body::Body::from_stream(logged_stream);
    match builder.body(body) {
        Ok(resp) => resp,
        Err(e) => {
            log::error!("[{}] 构建流式响应失败: {e}", ctx.tag);
            ProxyError::Internal(format!("Failed to build streaming response: {e}")).into_response()
        }
    }
}

/// 处理非流式响应
pub async fn handle_non_streaming(
    response: ProxyResponse,
    ctx: &RequestContext,
    state: &ProxyState,
    parser_config: &UsageParserConfig,
    // guard 在函数 scope 内持有，整包响应读取完成后随函数返回一并 drop
    _connection_guard: Option<ActiveConnectionGuard>,
) -> Result<Response, ProxyError> {
    let body_timeout = ctx.non_streaming_body_timeout();
    let (mut response_headers, status, body_bytes) =
        read_decoded_body(response, ctx.tag, body_timeout).await?;
    strip_hop_by_hop_response_headers(&mut response_headers);

    log::debug!(
        "[{}] 上游响应体内容: {}",
        ctx.tag,
        String::from_utf8_lossy(&body_bytes)
    );

    // 解析并记录使用量。关闭 usage logging 时直接跳过，避免非流式响应整包 JSON parse。
    if usage_logging_enabled(state) {
        if let Ok(json_value) = serde_json::from_slice::<Value>(&body_bytes) {
            // 解析使用量
            if let Some(usage) = (parser_config.response_parser)(&json_value) {
                // 归因优先级：usage 解析出的模型 → 响应 model 字段 → 映射后的出站
                // 模型（路由接管真值）→ 客户端请求模型。空字符串视为缺失。
                let model = usage
                    .model
                    .clone()
                    .filter(|m| !m.is_empty())
                    .or_else(|| {
                        json_value
                            .get("model")
                            .and_then(|m| m.as_str())
                            .filter(|m| !m.is_empty())
                            .map(str::to_string)
                    })
                    .or_else(|| ctx.outbound_model.clone())
                    .unwrap_or_else(|| ctx.request_model.clone());

                spawn_log_usage(
                    state,
                    ctx,
                    usage,
                    &model,
                    &ctx.request_model,
                    status.as_u16(),
                    false,
                );
            } else {
                let model = json_value
                    .get("model")
                    .and_then(|m| m.as_str())
                    .filter(|m| !m.is_empty())
                    .map(str::to_string)
                    .or_else(|| ctx.outbound_model.clone())
                    .unwrap_or_else(|| ctx.request_model.clone());
                spawn_log_usage(
                    state,
                    ctx,
                    TokenUsage::default(),
                    &model,
                    &ctx.request_model,
                    status.as_u16(),
                    false,
                );
                log::debug!(
                    "[{}] 未能解析 usage 信息，跳过记录",
                    parser_config.app_type_str
                );
            }
        } else {
            log::debug!(
                "[{}] <<< 响应 (非 JSON): {} bytes",
                ctx.tag,
                body_bytes.len()
            );
            spawn_log_usage(
                state,
                ctx,
                TokenUsage::default(),
                ctx.outbound_model.as_deref().unwrap_or(&ctx.request_model),
                &ctx.request_model,
                status.as_u16(),
                false,
            );
        }
    } else {
        log::debug!("[{}] usage logging 已关闭，跳过非流式 usage 解析", ctx.tag);
    }

    // 构建响应
    let mut builder = axum::response::Response::builder().status(status);
    for (key, value) in response_headers.iter() {
        builder = builder.header(key, value);
    }

    let body = axum::body::Body::from(body_bytes);
    builder.body(body).map_err(|e| {
        log::error!("[{}] 构建响应失败: {e}", ctx.tag);
        ProxyError::Internal(format!("Failed to build response: {e}"))
    })
}

/// 通用响应处理入口
///
/// 根据响应类型自动选择流式或非流式处理
pub async fn process_response(
    response: ProxyResponse,
    ctx: &RequestContext,
    state: &ProxyState,
    parser_config: &UsageParserConfig,
    connection_guard: Option<ActiveConnectionGuard>,
) -> Result<Response, ProxyError> {
    if is_sse_response(&response) {
        Ok(handle_streaming(response, ctx, state, parser_config, connection_guard).await)
    } else {
        handle_non_streaming(response, ctx, state, parser_config, connection_guard).await
    }
}

/// 把转换后的 SSE 流包装成客户端响应：接上 usage/timing tap（全 0 usage 不落账）、
/// 流式超时与连接计数，并设置 SSE 响应头。
///
/// usage 按 `client_format`（转换后输出给客户端的线协议）解析。
pub(crate) fn transformed_sse_response(
    sse_stream: impl Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
    client_format: WireFormat,
    ctx: &RequestContext,
    state: &ProxyState,
    status_code: u16,
    connection_guard: Option<ActiveConnectionGuard>,
) -> Response {
    let usage_collector = create_usage_collector(
        ctx,
        state,
        status_code,
        parser_config_for(client_format),
        MissingStreamUsage::SkipUnbillable,
    );
    let sse_stream = tap_stream(ctx.capture.clone(), CaptureSide::Client, sse_stream);
    let logged_stream = create_logged_passthrough_stream(
        sse_stream,
        ctx.tag,
        usage_collector,
        ctx.streaming_timeout_config(),
        connection_guard,
    );

    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("text/event-stream"),
    );
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-cache"),
    );
    (headers, axum::body::Body::from_stream(logged_stream)).into_response()
}

/// 用转换后的 JSON 重建非流式响应：沿用上游状态码与响应头（剥掉实体头与逐跳头），
/// Content-Type 固定为 application/json。
pub(crate) fn rebuilt_json_response(
    status: axum::http::StatusCode,
    mut headers: HeaderMap,
    body: &Value,
    tag: &str,
) -> Result<Response, ProxyError> {
    strip_entity_headers_for_rebuilt_body(&mut headers);
    strip_hop_by_hop_response_headers(&mut headers);
    // Builder::header 是 append 语义；不先 remove 会和上游 Content-Type 双发。
    headers.remove(axum::http::header::CONTENT_TYPE);

    let mut builder = axum::response::Response::builder().status(status);
    for (key, value) in headers.iter() {
        builder = builder.header(key, value);
    }
    builder = builder.header(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/json"),
    );

    let response_body = serde_json::to_vec(body).map_err(|e| {
        log::error!("[{tag}] 序列化响应失败: {e}");
        ProxyError::TransformError(format!("Failed to serialize response: {e}"))
    })?;
    builder
        .body(axum::body::Body::from(response_body))
        .map_err(|e| {
            log::error!("[{tag}] 构建响应失败: {e}");
            ProxyError::Internal(format!("Failed to build response: {e}"))
        })
}

// ============================================================================
// SSE 使用量收集器
// ============================================================================

type UsageCallback = Arc<dyn Fn(UsageTap, TimingTap) + Send + Sync + 'static>;

/// SSE 使用量收集器
///
/// 在透传流上旁路观测：[`UsageTap`] 逐事件累计 usage（内存与事件数无关），
/// [`TimingTap`] 记录首字节与首个 token 事件的时间点；流结束时把两者交给回调。
#[derive(Clone)]
pub struct SseUsageCollector {
    inner: Arc<SseUsageCollectorInner>,
}

struct SseUsageCollectorInner {
    taps: std::sync::Mutex<(UsageTap, TimingTap)>,
    on_complete: UsageCallback,
    should_collect: Option<StreamUsageEventFilter>,
    finished: AtomicBool,
}

impl SseUsageCollector {
    /// 创建使用量收集器；`should_collect` 用来在 hot path 跳过与 usage 无关的事件。
    pub fn new(
        start_time: std::time::Instant,
        format: StreamUsageFormat,
        should_collect: Option<StreamUsageEventFilter>,
        callback: impl Fn(UsageTap, TimingTap) + Send + Sync + 'static,
    ) -> Self {
        Self {
            inner: Arc::new(SseUsageCollectorInner {
                taps: std::sync::Mutex::new((UsageTap::new(format), TimingTap::new(start_time))),
                on_complete: Arc::new(callback),
                should_collect,
                finished: AtomicBool::new(false),
            }),
        }
    }

    pub fn should_collect(&self, data: &str) -> bool {
        self.inner
            .should_collect
            .map(|filter| filter(data))
            .unwrap_or(true)
    }

    fn with_taps<R>(&self, f: impl FnOnce(&mut (UsageTap, TimingTap)) -> R) -> R {
        let mut taps = self
            .inner
            .taps
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        f(&mut taps)
    }

    /// 标记收到上游首个字节块
    pub fn mark_first_byte(&self) {
        self.with_taps(|(_, timing)| timing.on_first_byte());
    }

    /// 折叠一个 SSE 事件；首个被收集的事件同时记为 first token
    pub fn push(&self, event: &Value) {
        self.with_taps(|(usage, timing)| {
            timing.on_token();
            usage.push(event);
        });
    }

    /// 完成收集并触发回调（只触发一次）
    pub fn finish(&self) {
        if self.inner.finished.swap(true, Ordering::SeqCst) {
            return;
        }
        let (usage, timing) = self.with_taps(|(usage, timing)| {
            let format = usage.format();
            (std::mem::replace(usage, UsageTap::new(format)), *timing)
        });
        (self.inner.on_complete)(usage, timing);
    }
}

struct SseUsageFinishGuard {
    collector: Option<SseUsageCollector>,
}

impl SseUsageFinishGuard {
    fn new(collector: SseUsageCollector) -> Self {
        Self {
            collector: Some(collector),
        }
    }

    fn disarm(&mut self) {
        self.collector = None;
    }
}

impl Drop for SseUsageFinishGuard {
    fn drop(&mut self) {
        if let Some(collector) = self.collector.take() {
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    collector.finish();
                });
            } else {
                log::warn!("SSE 用量收尾保护触发时 Tokio runtime 不可用，跳过异步 finish");
            }
        }
    }
}

// ============================================================================
// 内部辅助函数
// ============================================================================

/// 流结束时 usage 缺失（或全 0）的处理方式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MissingStreamUsage {
    /// 仍写入一条全 0 记录（透传路径：保留请求计数与延迟）
    Record,
    /// 跳过不计费的 usage（转换路径：转换器会为缺 usage 的上游合成全 0 终止事件，
    /// 无 message_id 时 dedup_request_id 退化为随机 UUID，写入只会虚增空行）
    SkipUnbillable,
}

/// 创建使用量收集器；关闭 usage logging 时返回 None，热路径上不再解析 SSE 事件。
pub(crate) fn create_usage_collector(
    ctx: &RequestContext,
    state: &ProxyState,
    status_code: u16,
    parser_config: &UsageParserConfig,
    missing: MissingStreamUsage,
) -> Option<SseUsageCollector> {
    if !usage_logging_enabled(state) {
        return None;
    }

    let state = state.clone();
    let provider_id = ctx.provider.id.clone();
    let request_model = ctx.request_model.clone();
    // 流式事件缺失模型名时的归因兜底：映射后的出站模型（路由接管真值）优先，
    // 其次才是客户端请求别名
    let fallback_model = ctx
        .outbound_model
        .clone()
        .unwrap_or_else(|| ctx.request_model.clone());
    // 用 ctx 的 app_type 而不是 parser_config 的：Claude Desktop 流式透传复用
    // CLAUDE_PARSER_CONFIG（app_type_str="claude"），按 parser_config 记账会把
    // claude-desktop 的行错记到 claude 名下，导致供应商计价覆盖解析不到。
    let app_type_str = ctx.app_type_str;
    let tag = ctx.tag;
    let start_time = ctx.start_time;
    let session_id = ctx.session_id.clone();
    let capture = ctx.capture.clone();

    Some(SseUsageCollector::new(
        start_time,
        parser_config.stream_usage,
        parser_config.stream_event_filter,
        move |usage_tap, timing| {
            let usage = match (usage_tap.usage(), missing) {
                (Some(usage), MissingStreamUsage::Record) => usage,
                (Some(usage), MissingStreamUsage::SkipUnbillable)
                    if usage.has_billable_tokens() =>
                {
                    usage
                }
                (_, MissingStreamUsage::SkipUnbillable) => {
                    log::debug!("[{tag}] 流式响应 usage 全 0 或缺失，跳过消费记录");
                    return;
                }
                (None, MissingStreamUsage::Record) => {
                    log::debug!("[{tag}] 流式响应缺少 usage 统计，按 0 记录");
                    TokenUsage::default()
                }
            };
            let model = usage_tap.model(&fallback_model);
            let latency_ms = start_time.elapsed().as_millis() as u64;
            log::debug!(
                "[{tag}] 流式 usage 汇总: events={}, ttfb_ms={:?}, first_token_ms={:?}",
                usage_tap.events(),
                timing.ttfb_ms(),
                timing.first_token_ms()
            );

            let state = state.clone();
            let provider_id = provider_id.clone();
            let session_id = session_id.clone();
            let request_model = request_model.clone();
            let outbound_model = fallback_model.clone();
            let capture = capture.clone();

            tokio::spawn(async move {
                let request_id = log_usage_internal(
                    &state,
                    &provider_id,
                    app_type_str,
                    &model,
                    &request_model,
                    &outbound_model,
                    usage,
                    latency_ms,
                    timing.first_token_ms(),
                    true, // is_streaming
                    status_code,
                    Some(session_id),
                )
                .await;
                link_capture(capture.as_ref(), request_id);
            });
        },
    ))
}

/// 非流式响应（转换路径）的 usage 记录任务：logging 关闭、解析失败或全 0 时为 None。
///
/// 归因优先级：响应 model 字段 → 映射后的出站模型 → 客户端请求模型，空字符串视为缺失。
pub(crate) fn response_usage_log_task(
    state: &ProxyState,
    ctx: &RequestContext,
    parser: ResponseUsageParser,
    response: &Value,
    status_code: u16,
) -> Option<impl std::future::Future<Output = ()> + Send + 'static> {
    if !usage_logging_enabled(state) {
        return None;
    }
    let usage = parser(response).filter(TokenUsage::has_billable_tokens)?;
    let model = response_model(ctx, response);
    Some(usage_log_task(
        state,
        ctx,
        usage,
        &model,
        &ctx.request_model,
        status_code,
        false,
    ))
}

/// 异步记录非流式响应（转换路径）的 usage，见 [`response_usage_log_task`]
pub(crate) fn spawn_response_usage_log(
    state: &ProxyState,
    ctx: &RequestContext,
    parser: ResponseUsageParser,
    response: &Value,
    status_code: u16,
) {
    if let Some(task) = response_usage_log_task(state, ctx, parser, response, status_code) {
        tokio::spawn(task);
    }
}

/// 非流式响应的归因模型：响应 model 字段 → 映射后的出站模型 → 客户端请求模型
fn response_model(ctx: &RequestContext, response: &Value) -> String {
    response
        .get("model")
        .and_then(Value::as_str)
        .filter(|model| !model.is_empty())
        .map(str::to_string)
        .or_else(|| ctx.outbound_model.clone())
        .unwrap_or_else(|| ctx.request_model.clone())
}

/// 构造一条 usage 记录任务（不检查 usage logging 开关）
fn usage_log_task(
    state: &ProxyState,
    ctx: &RequestContext,
    usage: TokenUsage,
    model: &str,
    request_model: &str,
    status_code: u16,
    is_streaming: bool,
) -> impl std::future::Future<Output = ()> + Send + 'static {
    let state = state.clone();
    let provider_id = ctx.provider.id.clone();
    let app_type_str = ctx.app_type_str.to_string();
    let model = model.to_string();
    let request_model = request_model.to_string();
    // 「按请求计价」模式的锚点：映射后的出站模型，无映射时等于 request_model
    let outbound_model = ctx
        .outbound_model
        .clone()
        .unwrap_or_else(|| ctx.request_model.clone());
    let latency_ms = ctx.latency_ms();
    let session_id = ctx.session_id.clone();
    let capture = ctx.capture.clone();

    async move {
        let request_id = log_usage_internal(
            &state,
            &provider_id,
            &app_type_str,
            &model,
            &request_model,
            &outbound_model,
            usage,
            latency_ms,
            None,
            is_streaming,
            status_code,
            Some(session_id),
        )
        .await;
        link_capture(capture.as_ref(), request_id);
    }
}

/// 把抓包关联到刚写入的请求日志行，供请求日志按 request_id 导出缺陷报告
fn link_capture(capture: Option<&CaptureHandle>, request_id: Option<String>) {
    if let (Some(capture), Some(request_id)) = (capture, request_id) {
        capture.set_request_id(&request_id);
    }
}

/// 异步记录使用量
fn spawn_log_usage(
    state: &ProxyState,
    ctx: &RequestContext,
    usage: TokenUsage,
    model: &str,
    request_model: &str,
    status_code: u16,
    is_streaming: bool,
) {
    // Check enable_logging before spawning the log task
    if let Ok(config) = state.config.try_read() {
        if !config.enable_logging {
            return;
        }
    }

    tokio::spawn(usage_log_task(
        state,
        ctx,
        usage,
        model,
        request_model,
        status_code,
        is_streaming,
    ));
}

pub(crate) fn usage_logging_enabled(state: &ProxyState) -> bool {
    state
        .config
        .try_read()
        .map(|config| config.enable_logging)
        .unwrap_or(true)
}

/// 内部使用量记录函数
///
/// `outbound_model` 是「按请求计价」模式的锚点：实际发往上游的模型
/// （路由接管映射后的真值，无映射时等于 request_model）。该模式的语义是
/// 「按代理发出的请求计价、不信任上游回显」，接管场景下发出的请求模型是
/// 映射后的 Y 而非客户端别名 X，按 X 计价会用错定价表行。
///
/// 返回写入的 request_id；写入失败时为 None。
#[allow(clippy::too_many_arguments)]
async fn log_usage_internal(
    state: &ProxyState,
    provider_id: &str,
    app_type: &str,
    model: &str,
    request_model: &str,
    outbound_model: &str,
    usage: TokenUsage,
    latency_ms: u64,
    first_token_ms: Option<u64>,
    is_streaming: bool,
    status_code: u16,
    session_id: Option<String>,
) -> Option<String> {
    use super::usage::logger::UsageLogger;

    let logger = UsageLogger::new(&state.db).with_runtime_config(&state.runtime_config);
    let (multiplier, pricing_model_source) =
        logger.resolve_pricing_config(provider_id, app_type).await;
    let pricing_model = if pricing_model_source == PRICING_SOURCE_REQUEST {
        outbound_model
    } else {
        model
    };

    let request_id = usage.dedup_request_id();

    log::debug!(
        "[{app_type}] 记录请求日志: id={request_id}, provider={provider_id}, model={model}, streaming={is_streaming}, status={status_code}, latency_ms={latency_ms}, first_token_ms={first_token_ms:?}, session={}, input={}, output={}, cache_read={}, cache_creation={}",
        session_id.as_deref().unwrap_or("none"),
        usage.input_tokens,
        usage.output_tokens,
        usage.cache_read_tokens,
        usage.cache_creation_tokens
    );

    match logger.log_with_calculation(
        request_id.clone(),
        provider_id.to_string(),
        app_type.to_string(),
        model.to_string(),
        request_model.to_string(),
        pricing_model.to_string(),
        usage,
        multiplier,
        latency_ms,
        first_token_ms,
        status_code,
        session_id,
        None, // provider_type
        is_streaming,
    ) {
        Ok(()) => Some(request_id),
        Err(e) => {
            log::warn!("[USG-001] 记录使用量失败: {e}");
            None
        }
    }
}

/// 创建带日志记录和超时控制的透传流
pub fn create_logged_passthrough_stream(
    stream: impl Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
    tag: &'static str,
    usage_collector: Option<SseUsageCollector>,
    timeout_config: StreamingTimeoutConfig,
    connection_guard: Option<ActiveConnectionGuard>,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let _conn_guard = connection_guard;
        let mut decoder = SseDecoder::new();
        let mut collector = usage_collector;
        let mut finish_guard = collector.clone().map(SseUsageFinishGuard::new);
        let inspect_sse_events =
            collector.is_some() || log::log_enabled!(log::Level::Debug);
        let mut is_first_chunk = true;

        // 超时配置
        let first_byte_timeout = if timeout_config.first_byte_timeout > 0 {
            Some(Duration::from_secs(timeout_config.first_byte_timeout))
        } else {
            None
        };
        let idle_timeout = if timeout_config.idle_timeout > 0 {
            Some(Duration::from_secs(timeout_config.idle_timeout))
        } else {
            None
        };

        tokio::pin!(stream);

        loop {
            // 选择超时时间：首字节超时或静默期超时
            let timeout_duration = if is_first_chunk {
                first_byte_timeout
            } else {
                idle_timeout
            };

            let chunk_result = match timeout_duration {
                Some(duration) => {
                    match tokio::time::timeout(duration, stream.next()).await {
                        Ok(Some(chunk)) => Some(chunk),
                        Ok(None) => None, // 流结束
                        Err(_) => {
                            // 超时
                            let timeout_type = if is_first_chunk { "首字节" } else { "静默期" };
                            log::error!("[{tag}] 流式响应{}超时 ({}秒)", timeout_type, duration.as_secs());
                            yield Err(std::io::Error::other(format!("流式响应{timeout_type}超时")));
                            break;
                        }
                    }
                }
                None => stream.next().await, // 无超时限制
            };

            match chunk_result {
                Some(Ok(bytes)) => {
                    if is_first_chunk {
                        log::debug!(
                            "[{tag}] 已接收上游流式首包: bytes={}",
                            bytes.len()
                        );
                        if let Some(c) = &collector {
                            c.mark_first_byte();
                        }
                    }
                    is_first_chunk = false;
                    if inspect_sse_events {
                        for event in decoder.feed(&bytes) {
                            inspect_sse_event(tag, collector.as_ref(), &event);
                        }
                    }

                    yield Ok(bytes);
                }
                Some(Err(e)) => {
                    log::error!("[{tag}] 流错误: {e}");
                    yield Err(std::io::Error::other(e.to_string()));
                    break;
                }
                None => {
                    // 流正常结束
                    break;
                }
            }
        }

        // 最后一个事件后可能没有空行分隔：残余尾块也交给 tap
        if inspect_sse_events {
            if let Some(event) = decoder.finish() {
                inspect_sse_event(tag, collector.as_ref(), &event);
            }
        }
        if let Some(c) = collector.take() {
            c.finish();
        }
        if let Some(guard) = &mut finish_guard {
            guard.disarm();
        }
    }
}

/// 记录一个透传的 SSE 事件；只有 usage collector 存在且预过滤命中时才解析 JSON。
fn inspect_sse_event(tag: &str, collector: Option<&SseUsageCollector>, event: &SseEvent) {
    let data = event.data.as_str();
    if data.trim() == "[DONE]" {
        log::debug!("[{tag}] <<< SSE: [DONE]");
        return;
    }
    let collected = match collector {
        Some(c) if c.should_collect(data) => match serde_json::from_str::<Value>(data) {
            Ok(json_value) => {
                c.push(&json_value);
                true
            }
            Err(_) => false,
        },
        _ => false,
    };
    if collected {
        log::debug!("[{tag}] <<< SSE 事件: {data}");
    } else {
        log::debug!("[{tag}] <<< SSE 数据: {data}");
    }
}

fn format_headers(headers: &HeaderMap) -> String {
    headers
        .iter()
        .map(|(key, value)| {
            let value_str = value.to_str().unwrap_or("<non-utf8>");
            format!("{key}={value_str}")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::error::AppError;
    use crate::provider::ProviderMeta;
    use crate::proxy::handler_context::{
        non_streaming_body_timeout, DEFAULT_NON_STREAMING_BODY_TIMEOUT,
    };
    use crate::proxy::types::{AppProxyConfig, ProxyConfig};
    use rust_decimal::Decimal;
    use std::str::FromStr;
    use std::sync::Arc;

    /// 透传流旁路折叠 usage：字节原样转发，usage/timing 在流结束时交给回调，
    /// 未被预过滤命中的事件不解析；无空行收尾的残余尾块也计入。
    #[tokio::test]
    async fn logged_passthrough_folds_usage_through_taps() {
        use crate::proxy::handler_config::CLAUDE_PARSER_CONFIG;

        let (tx, rx) = std::sync::mpsc::channel();
        let collector = SseUsageCollector::new(
            std::time::Instant::now(),
            CLAUDE_PARSER_CONFIG.stream_usage,
            CLAUDE_PARSER_CONFIG.stream_event_filter,
            move |usage, timing| {
                tx.send((
                    usage.usage(),
                    usage.events(),
                    timing.ttfb_ms(),
                    timing.first_token_ms(),
                ))
                .unwrap();
            },
        );

        let mut chunks = vec![
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude-x\",\"usage\":{\"input_tokens\":7}}}\n\n".to_string(),
        ];
        for _ in 0..1000 {
            chunks.push("event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"x\"}}\n\n".to_string());
        }
        // message_delta 跨块切分，且最后没有空行分隔
        chunks.push("event: message_delta\ndata: {\"type\":\"message_de".to_string());
        chunks.push("lta\",\"usage\":{\"output_tokens\":42}}".to_string());
        let expected: String = chunks.concat();

        let upstream = futures::stream::iter(
            chunks
                .into_iter()
                .map(|c| Ok::<_, std::io::Error>(Bytes::from(c))),
        );
        let stream = create_logged_passthrough_stream(
            upstream,
            "test",
            Some(collector),
            StreamingTimeoutConfig {
                first_byte_timeout: 0,
                idle_timeout: 0,
            },
            None,
        );
        let forwarded: Vec<_> = stream.collect().await;
        let forwarded: String = forwarded
            .into_iter()
            .map(|c| String::from_utf8(c.unwrap().to_vec()).unwrap())
            .collect();
        assert_eq!(forwarded, expected);

        let (usage, events, ttfb_ms, first_token_ms) = rx.try_recv().expect("finish callback");
        let usage = usage.expect("usage");
        assert_eq!(usage.input_tokens, 7);
        assert_eq!(usage.output_tokens, 42);
        assert_eq!(usage.model.as_deref(), Some("claude-x"));
        assert_eq!(usage.message_id.as_deref(), Some("msg_1"));
        // 只有 message_start 与 message_delta 命中预过滤
        assert_eq!(events, 2);
        assert!(ttfb_ms.is_some());
        assert!(first_token_ms.is_some());
        assert!(rx.try_recv().is_err(), "callback must fire once");
    }

    fn app_proxy_config(auto_failover_enabled: bool, non_streaming_timeout: u32) -> AppProxyConfig {
        AppProxyConfig {
            app_type: "claude".to_string(),
            enabled: true,
            auto_failover_enabled,
            max_retries: 0,
            streaming_first_byte_timeout: 60,
            streaming_idle_timeout: 120,
            non_streaming_timeout,
            circuit_failure_threshold: 4,
            circuit_success_threshold: 2,
            circuit_timeout_seconds: 60,
            circuit_error_rate_threshold: 0.5,
            circuit_min_requests: 10,
        }
    }

    #[test]
    fn test_non_streaming_body_timeout_is_never_zero() {
        assert_eq!(
            non_streaming_body_timeout(&app_proxy_config(true, 30)),
            Duration::from_secs(30)
        );
        assert_eq!(
            non_streaming_body_timeout(&app_proxy_config(true, 0)),
            DEFAULT_NON_STREAMING_BODY_TIMEOUT
        );
        assert_eq!(
            non_streaming_body_timeout(&app_proxy_config(false, 30)),
            DEFAULT_NON_STREAMING_BODY_TIMEOUT
        );
        assert_eq!(DEFAULT_NON_STREAMING_BODY_TIMEOUT, Duration::from_secs(600));
    }

    /// 上游发完响应头和 1 字节 body 后停住：故障转移关闭时 raw hyper 路径的
    /// `read_decoded_body` 也必须在兜底超时后返回 `ProxyError::Timeout`，而不是永久挂起。
    #[tokio::test]
    async fn test_read_decoded_body_times_out_on_stalled_hyper_body_with_failover_off() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = socket.read(&mut buf).await.unwrap();
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 16\r\n\r\n{",
                )
                .await
                .unwrap();
            socket.flush().await.unwrap();
            // 保持连接打开但不再发送任何数据
            std::future::pending::<()>().await;
        });

        let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let (mut sender, conn) =
            hyper::client::conn::http1::handshake(hyper_util::rt::TokioIo::new(stream))
                .await
                .unwrap();
        let conn_task = tokio::spawn(conn);
        let request = http::Request::builder()
            .uri("/v1/messages")
            .header("host", addr.to_string())
            .body(http_body_util::Empty::<Bytes>::new())
            .unwrap();
        let response = sender.send_request(request).await.unwrap();
        assert_eq!(response.status(), http::StatusCode::OK);

        let body_timeout = non_streaming_body_timeout(&app_proxy_config(false, 30));

        // 响应头已到达，之后冻结时钟，由 tokio 自动推进到下一个定时器
        tokio::time::pause();
        let started = tokio::time::Instant::now();
        let result = tokio::time::timeout(
            Duration::from_secs(3600),
            read_decoded_body(ProxyResponse::Hyper(response), "Test", body_timeout),
        )
        .await
        .expect("read_decoded_body must not hang when failover is off");

        match result {
            Err(ProxyError::Timeout(_)) => {}
            other => panic!("expected ProxyError::Timeout, got {other:?}"),
        }
        assert!(started.elapsed() >= DEFAULT_NON_STREAMING_BODY_TIMEOUT);
        assert!(started.elapsed() < Duration::from_secs(3600));

        server.abort();
        conn_task.abort();
    }

    #[test]
    fn test_strip_sse_field_accepts_optional_space() {
        assert_eq!(
            crate::proxy::sse::strip_sse_field("data: {\"ok\":true}", "data"),
            Some("{\"ok\":true}")
        );
        assert_eq!(
            crate::proxy::sse::strip_sse_field("data:{\"ok\":true}", "data"),
            Some("{\"ok\":true}")
        );
        assert_eq!(
            crate::proxy::sse::strip_sse_field("event: message_start", "event"),
            Some("message_start")
        );
        assert_eq!(
            crate::proxy::sse::strip_sse_field("event:message_start", "event"),
            Some("message_start")
        );
        assert_eq!(crate::proxy::sse::strip_sse_field("id:1", "data"), None);
    }

    #[test]
    fn test_strip_hop_by_hop_response_headers_removes_standard_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::CONNECTION,
            axum::http::HeaderValue::from_static("keep-alive"),
        );
        headers.insert(
            axum::http::header::HeaderName::from_static("keep-alive"),
            axum::http::HeaderValue::from_static("timeout=5"),
        );
        headers.insert(
            axum::http::header::TRANSFER_ENCODING,
            axum::http::HeaderValue::from_static("chunked"),
        );
        headers.insert(
            axum::http::header::HeaderName::from_static("proxy-connection"),
            axum::http::HeaderValue::from_static("keep-alive"),
        );
        headers.insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/json"),
        );
        headers.insert(
            axum::http::header::CONTENT_LENGTH,
            axum::http::HeaderValue::from_static("12"),
        );

        strip_hop_by_hop_response_headers(&mut headers);

        assert!(!headers.contains_key(axum::http::header::CONNECTION));
        assert!(!headers.contains_key("keep-alive"));
        assert!(!headers.contains_key(axum::http::header::TRANSFER_ENCODING));
        assert!(!headers.contains_key("proxy-connection"));
        assert_eq!(
            headers.get(axum::http::header::CONTENT_TYPE),
            Some(&axum::http::HeaderValue::from_static("application/json"))
        );
        assert_eq!(
            headers.get(axum::http::header::CONTENT_LENGTH),
            Some(&axum::http::HeaderValue::from_static("12"))
        );
    }

    #[test]
    fn test_strip_hop_by_hop_response_headers_removes_connection_listed_extensions() {
        let mut headers = HeaderMap::new();
        headers.append(
            axum::http::header::CONNECTION,
            axum::http::HeaderValue::from_static("x-trace-hop, x-debug-hop"),
        );
        headers.append(
            axum::http::header::CONNECTION,
            axum::http::HeaderValue::from_static("upgrade"),
        );
        headers.insert(
            axum::http::header::HeaderName::from_static("x-trace-hop"),
            axum::http::HeaderValue::from_static("trace"),
        );
        headers.insert(
            axum::http::header::HeaderName::from_static("x-debug-hop"),
            axum::http::HeaderValue::from_static("debug"),
        );
        headers.insert(
            axum::http::header::UPGRADE,
            axum::http::HeaderValue::from_static("websocket"),
        );
        headers.insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("text/event-stream"),
        );

        strip_hop_by_hop_response_headers(&mut headers);

        assert!(!headers.contains_key(axum::http::header::CONNECTION));
        assert!(!headers.contains_key("x-trace-hop"));
        assert!(!headers.contains_key("x-debug-hop"));
        assert!(!headers.contains_key(axum::http::header::UPGRADE));
        assert_eq!(
            headers.get(axum::http::header::CONTENT_TYPE),
            Some(&axum::http::HeaderValue::from_static("text/event-stream"))
        );
    }

    fn build_state(db: Arc<Database>) -> ProxyState {
        ProxyState::for_db(db, ProxyConfig::default())
    }

    fn seed_pricing(db: &Database) -> Result<(), AppError> {
        let conn = crate::database::lock_conn!(db.conn);
        conn.execute(
            "INSERT OR REPLACE INTO model_pricing (model_id, display_name, input_cost_per_million, output_cost_per_million)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["resp-model", "Resp Model", "1.0", "0"],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "INSERT OR REPLACE INTO model_pricing (model_id, display_name, input_cost_per_million, output_cost_per_million)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["req-model", "Req Model", "2.0", "0"],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    fn insert_provider(
        db: &Database,
        id: &str,
        app_type: &str,
        meta: ProviderMeta,
    ) -> Result<(), AppError> {
        let meta_json =
            serde_json::to_string(&meta).map_err(|e| AppError::Database(e.to_string()))?;
        let conn = crate::database::lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![id, app_type, "Test Provider", "{}", meta_json],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    #[tokio::test]
    async fn test_log_usage_uses_provider_override_config() -> Result<(), AppError> {
        let db = Arc::new(Database::memory()?);
        let app_type = "claude";

        db.set_default_cost_multiplier(app_type, "1.5").await?;
        db.set_pricing_model_source(app_type, "response").await?;
        seed_pricing(&db)?;

        let meta = ProviderMeta {
            cost_multiplier: Some("2".to_string()),
            pricing_model_source: Some("request".to_string()),
            ..ProviderMeta::default()
        };
        insert_provider(&db, "provider-1", app_type, meta)?;

        let state = build_state(db.clone());
        let usage = TokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            model: None,
            message_id: None,
        };

        log_usage_internal(
            &state,
            "provider-1",
            app_type,
            "resp-model",
            "req-model",
            "req-model",
            usage,
            10,
            None,
            false,
            200,
            None,
        )
        .await;

        let conn = crate::database::lock_conn!(db.conn);
        let (model, request_model, total_cost, cost_multiplier): (String, String, String, String) =
            conn.query_row(
                "SELECT model, request_model, total_cost_usd, cost_multiplier
                 FROM proxy_request_logs WHERE provider_id = ?1",
                ["provider-1"],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        assert_eq!(model, "resp-model");
        assert_eq!(request_model, "req-model");
        assert_eq!(
            Decimal::from_str(&cost_multiplier).unwrap(),
            Decimal::from_str("2").unwrap()
        );
        assert_eq!(
            Decimal::from_str(&total_cost).unwrap(),
            Decimal::from_str("4").unwrap()
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_request_pricing_mode_anchors_to_outbound_model() -> Result<(), AppError> {
        let db = Arc::new(Database::memory()?);
        let app_type = "claude";

        db.set_pricing_model_source(app_type, "request").await?;
        seed_pricing(&db)?;
        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute(
                "INSERT OR REPLACE INTO model_pricing (model_id, display_name, input_cost_per_million, output_cost_per_million)
                 VALUES ('outbound-model', 'Outbound Model', '4.0', '0')",
                [],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        }

        insert_provider(&db, "provider-3", app_type, ProviderMeta::default())?;

        let state = build_state(db.clone());
        let usage = TokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            model: None,
            message_id: None,
        };

        // 路由接管场景：客户端请求 req-model（$2/M），代理实际发出 outbound-model
        // （$4/M），上游回显 resp-model。「按请求计价」必须锚定实际发出的模型。
        log_usage_internal(
            &state,
            "provider-3",
            app_type,
            "resp-model",
            "req-model",
            "outbound-model",
            usage,
            10,
            None,
            false,
            200,
            None,
        )
        .await;

        let conn = crate::database::lock_conn!(db.conn);
        let (model, request_model, total_cost): (String, String, String) = conn
            .query_row(
                "SELECT model, request_model, total_cost_usd
                 FROM proxy_request_logs WHERE provider_id = ?1",
                ["provider-3"],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        // model / request_model 列不受计价锚点影响
        assert_eq!(model, "resp-model");
        assert_eq!(request_model, "req-model");
        // 按 outbound-model（$4/M）计价，而不是 req-model（$2/M）或 resp-model（$1/M）
        assert_eq!(
            Decimal::from_str(&total_cost).unwrap(),
            Decimal::from_str("4").unwrap()
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_claude_desktop_inherits_claude_global_defaults() -> Result<(), AppError> {
        use crate::proxy::usage::logger::UsageLogger;

        let db = Arc::new(Database::memory()?);

        // 全局计费配置只有 claude/codex/gemini 三行；claude-desktop 的
        // 全局默认必须继承 claude，而不是静默落回工厂默认（1 / response）
        db.set_default_cost_multiplier("claude", "1.5").await?;
        db.set_pricing_model_source("claude", "request").await?;

        let logger = UsageLogger::new(&db);
        let (multiplier, source) = logger
            .resolve_pricing_config("nonexistent-provider", "claude-desktop")
            .await;

        assert_eq!(multiplier, Decimal::from_str("1.5").unwrap());
        assert_eq!(source, "request");
        Ok(())
    }

    #[tokio::test]
    async fn test_log_usage_falls_back_to_global_defaults() -> Result<(), AppError> {
        let db = Arc::new(Database::memory()?);
        let app_type = "claude";

        db.set_default_cost_multiplier(app_type, "1.5").await?;
        db.set_pricing_model_source(app_type, "response").await?;
        seed_pricing(&db)?;

        let meta = ProviderMeta::default();
        insert_provider(&db, "provider-2", app_type, meta)?;

        let state = build_state(db.clone());
        let usage = TokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            model: None,
            message_id: None,
        };

        log_usage_internal(
            &state,
            "provider-2",
            app_type,
            "resp-model",
            "req-model",
            "req-model",
            usage,
            10,
            None,
            false,
            200,
            None,
        )
        .await;

        let conn = crate::database::lock_conn!(db.conn);
        let (total_cost, cost_multiplier): (String, String) = conn
            .query_row(
                "SELECT total_cost_usd, cost_multiplier
                 FROM proxy_request_logs WHERE provider_id = ?1",
                ["provider-2"],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        assert_eq!(
            Decimal::from_str(&cost_multiplier).unwrap(),
            Decimal::from_str("1.5").unwrap()
        );
        assert_eq!(
            Decimal::from_str(&total_cost).unwrap(),
            Decimal::from_str("1.5").unwrap()
        );
        Ok(())
    }
}
