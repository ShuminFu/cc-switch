//! 响应处理器模块
//!
//! 统一处理流式和非流式 API 响应

use super::{
    content_encoding::{decompress_body, get_content_encoding},
    forwarder::ActiveConnectionGuard,
    handler_config::{
        ResponseUsageParser, StreamErrorFormat, StreamEventRetention, StreamUsageEventFilter,
        StreamUsageParser, UsageParserConfig,
    },
    handler_context::{RequestContext, StreamingTimeoutConfig},
    hyper_client::ProxyResponse,
    server::ProxyState,
    sse::{strip_sse_field, SseBlockBuffer},
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
        Arc, Mutex as StdMutex, OnceLock,
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
/// 传入 `Duration::ZERO` 表示不启用超时（故障转移关闭时）。
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
    let usage_collector = create_usage_collector(ctx, state, status.as_u16(), parser_config);

    // 获取流式超时配置
    let timeout_config = ctx.streaming_timeout_config();

    // 创建带日志和超时的透传流
    let logged_stream = create_logged_passthrough_stream(
        stream,
        ctx.tag,
        usage_collector,
        timeout_config,
        connection_guard,
        parser_config.stream_error_format,
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
    // 整包超时：仅在故障转移开启且配置值非零时生效
    let body_timeout =
        if ctx.app_config.auto_failover_enabled && ctx.app_config.non_streaming_timeout > 0 {
            Duration::from_secs(ctx.app_config.non_streaming_timeout as u64)
        } else {
            Duration::ZERO
        };
    let (mut response_headers, status, body_bytes) =
        read_decoded_body(response, ctx.tag, body_timeout).await?;
    strip_hop_by_hop_response_headers(&mut response_headers);

    log::debug!(
        "[{}] 上游响应体内容: {}",
        ctx.tag,
        super::http_client::log_body_excerpt(&String::from_utf8_lossy(&body_bytes))
    );

    // 解析并记录使用量。关闭 usage logging 时直接跳过，避免非流式响应整包 JSON parse。
    if usage_logging_enabled(state) {
        let target = UsageLogTarget::from_ctx(state, ctx, UsageSource::Passthrough);
        let record = match serde_json::from_slice::<Value>(&body_bytes) {
            Ok(json_value) => {
                target.usage_from_response(parser_config.response_parser, &json_value)
            }
            Err(_) => {
                log::debug!(
                    "[{}] <<< 响应 (非 JSON, parser={}): {} bytes",
                    ctx.tag,
                    parser_config.app_type_str,
                    body_bytes.len()
                );
                Some((TokenUsage::default(), target.attribute_model([])))
            }
        };
        if let Some((usage, model)) = record {
            target.spawn(usage, model, None, false, status.as_u16());
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

// ============================================================================
// SSE 使用量收集器
// ============================================================================

type UsageCallbackWithTiming = Arc<dyn Fn(Vec<Value>, Option<u64>) + Send + Sync + 'static>;

/// SSE 使用量收集器
#[derive(Clone)]
pub struct SseUsageCollector {
    inner: Arc<SseUsageCollectorInner>,
}

struct SseUsageCollectorInner {
    retained: StdMutex<RetainedEvents>,
    first_event_time: OnceLock<std::time::Instant>,
    start_time: std::time::Instant,
    on_complete: UsageCallbackWithTiming,
    should_collect: Option<StreamUsageEventFilter>,
    retention: StreamEventRetention,
    finished: AtomicBool,
}

/// 收集器保留的事件：`All` 模式下是完整列表；`LastUsage` 模式下至多两个
/// （首个带模型名的事件 + 最后一个 usage 事件），内存与流长度无关（PERF-07）。
#[derive(Default)]
struct RetainedEvents {
    all: Vec<Value>,
    model_event: Option<Value>,
    usage_event: Option<Value>,
}

impl RetainedEvents {
    fn push(&mut self, event: Value, retention: StreamEventRetention) {
        let StreamEventRetention::LastUsage {
            usage_key,
            ignore_null_usage,
            model_key,
        } = retention
        else {
            self.all.push(event);
            return;
        };

        let has_model = |value: &Value| {
            value
                .get(model_key)
                .and_then(Value::as_str)
                .is_some_and(|model| !model.is_empty())
        };
        let is_usage_event = event
            .get(usage_key)
            .is_some_and(|usage| !(ignore_null_usage && usage.is_null()));

        if is_usage_event {
            // 被替换的旧 usage 事件若是首个带模型名的事件，转存为 model_event，
            // 保持「首个模型名」的既有语义。
            if let Some(previous) = self.usage_event.replace(event) {
                if self.model_event.is_none() && has_model(&previous) {
                    self.model_event = Some(previous);
                }
            }
        } else if self.model_event.is_none()
            && !self.usage_event.as_ref().is_some_and(&has_model)
            && has_model(&event)
        {
            self.model_event = Some(event);
        }
    }

    fn into_events(self) -> Vec<Value> {
        let mut events = self.all;
        events.extend(self.model_event);
        events.extend(self.usage_event);
        events
    }
}

impl SseUsageCollector {
    /// 创建使用量收集器；`should_collect` 用来在 hot path 跳过与 usage 无关的事件。
    pub fn new(
        start_time: std::time::Instant,
        should_collect: Option<StreamUsageEventFilter>,
        callback: impl Fn(Vec<Value>, Option<u64>) + Send + Sync + 'static,
    ) -> Self {
        Self::with_retention(
            start_time,
            should_collect,
            StreamEventRetention::All,
            callback,
        )
    }

    /// 同 `new`，但按 `retention` 只保留解析器需要的事件。
    pub fn with_retention(
        start_time: std::time::Instant,
        should_collect: Option<StreamUsageEventFilter>,
        retention: StreamEventRetention,
        callback: impl Fn(Vec<Value>, Option<u64>) + Send + Sync + 'static,
    ) -> Self {
        let on_complete: UsageCallbackWithTiming = Arc::new(callback);
        Self {
            inner: Arc::new(SseUsageCollectorInner {
                retained: StdMutex::new(RetainedEvents::default()),
                first_event_time: OnceLock::new(),
                start_time,
                on_complete,
                should_collect,
                retention,
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

    /// 推送 SSE 事件。首个被收集事件的时间沿用 `first_token_ms` 的既有近似语义。
    ///
    /// 临界区极短且不跨 await，用 std Mutex + OnceLock 代替两把 tokio Mutex。
    pub async fn push(&self, event: Value) {
        self.inner
            .first_event_time
            .get_or_init(std::time::Instant::now);
        let mut retained = self
            .inner
            .retained
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        retained.push(event, self.inner.retention);
    }

    /// 完成收集并触发回调
    pub async fn finish(&self) {
        if self.inner.finished.swap(true, Ordering::SeqCst) {
            return;
        }

        let events = {
            let mut retained = self
                .inner
                .retained
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            std::mem::take(&mut *retained).into_events()
        };

        let first_token_ms = self
            .inner
            .first_event_time
            .get()
            .map(|t| (*t - self.inner.start_time).as_millis() as u64);

        (self.inner.on_complete)(events, first_token_ms);
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
                    collector.finish().await;
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

/// 创建使用量收集器（透传流）
fn create_usage_collector(
    ctx: &RequestContext,
    state: &ProxyState,
    status_code: u16,
    parser_config: &UsageParserConfig,
) -> Option<SseUsageCollector> {
    if !usage_logging_enabled(state) {
        return None;
    }

    // 用 ctx 的 app_type 而不是 parser_config 的：Claude Desktop 流式透传复用
    // CLAUDE_PARSER_CONFIG（app_type_str="claude"），按 parser_config 记账会把
    // claude-desktop 的行错记到 claude 名下，导致供应商计价覆盖解析不到。
    let target = UsageLogTarget::from_ctx(state, ctx, UsageSource::Passthrough);
    let tag = ctx.tag;
    let stream_parser = parser_config.stream_parser;
    let model_extractor = parser_config.model_extractor;

    Some(SseUsageCollector::with_retention(
        ctx.start_time,
        parser_config.stream_event_filter,
        parser_config.stream_event_retention,
        move |events, first_token_ms| {
            // 流式事件缺失模型名时的归因兜底：映射后的出站模型（路由接管真值）优先，
            // 其次才是客户端请求别名
            let model = model_extractor(&events, target.fallback_model());
            let usage = stream_parser(&events).unwrap_or_else(|| {
                log::debug!("[{tag}] 流式响应缺少 usage 统计，记录零用量行");
                TokenUsage::default()
            });
            target.spawn(usage, model, first_token_ms, true, status_code);
        },
    ))
}

/// 使用量来源：决定唯一的「零用量是否落账」规则（PROXY-18）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UsageSource {
    /// 上游响应原样透传：即使没有计费 token 也写一行，让请求与其状态码在日志中可见。
    Passthrough,
    /// 响应经过格式转换重建：上游省略 usage 时转换器会合成全 0 usage，且没有可去重的
    /// message id（dedup id 退化为随机 UUID），写入只会产生空行、虚增请求数，故跳过。
    Converted,
}

/// 零用量落账规则的唯一实现。
pub(crate) fn should_record_usage(usage: &TokenUsage, source: UsageSource) -> bool {
    source == UsageSource::Passthrough || usage.has_billable_tokens()
}

/// 使用量记录的唯一入口（PROXY-18）。
///
/// 持有记账所需请求字段的快照（`Send + 'static`，可 move 进流结束回调），统一
/// 模型归因顺序（响应/usage 回显 → 映射后的出站模型 → 客户端请求模型）、
/// 零用量规则与「按请求计价」锚点（出站模型）。
#[derive(Clone)]
pub(crate) struct UsageLogTarget {
    state: ProxyState,
    provider_id: String,
    app_type: &'static str,
    request_model: String,
    /// 「按请求计价」锚点与归因兜底：映射后的出站模型，无映射时等于 request_model
    outbound_model: String,
    session_id: String,
    start_time: std::time::Instant,
    source: UsageSource,
}

impl UsageLogTarget {
    pub(crate) fn from_ctx(state: &ProxyState, ctx: &RequestContext, source: UsageSource) -> Self {
        Self {
            state: state.clone(),
            provider_id: ctx.provider.id.clone(),
            app_type: ctx.app_type_str,
            request_model: ctx.request_model.clone(),
            outbound_model: ctx
                .outbound_model
                .clone()
                .filter(|m| !m.is_empty())
                .unwrap_or_else(|| ctx.request_model.clone()),
            session_id: ctx.session_id.clone(),
            start_time: ctx.start_time,
            source,
        }
    }

    /// 上游未回显模型时的归因兜底。
    pub(crate) fn fallback_model(&self) -> &str {
        &self.outbound_model
    }

    /// 取第一个非空候选模型名，否则落到兜底模型。空字符串视为缺失
    /// （转换层对无回显上游会合成 `model: ""`）。
    pub(crate) fn attribute_model<'a>(
        &self,
        candidates: impl IntoIterator<Item = Option<&'a str>>,
    ) -> String {
        candidates
            .into_iter()
            .flatten()
            .find(|m| !m.is_empty())
            .unwrap_or(&self.outbound_model)
            .to_string()
    }

    /// 从完整响应体解析 usage 与归因模型，并应用零用量规则。
    ///
    /// 透传来源在解析不到 usage 时仍返回零用量行（保留请求可见性）。
    pub(crate) fn usage_from_response(
        &self,
        parser: ResponseUsageParser,
        response: &Value,
    ) -> Option<(TokenUsage, String)> {
        let response_model = response.get("model").and_then(Value::as_str);
        match parser(response) {
            Some(usage) => {
                if !should_record_usage(&usage, self.source) {
                    log::debug!("[{}] usage 全 0，跳过消费记录", self.app_type);
                    return None;
                }
                let model = self.attribute_model([usage.model.as_deref(), response_model]);
                Some((usage, model))
            }
            None if self.source == UsageSource::Passthrough => {
                log::debug!("[{}] 未能解析 usage 信息，记录零用量行", self.app_type);
                Some((
                    TokenUsage::default(),
                    self.attribute_model([response_model]),
                ))
            }
            None => None,
        }
    }

    /// 从收集到的流式事件解析 usage 与归因模型，并应用零用量规则。
    pub(crate) fn usage_from_stream(
        &self,
        parser: StreamUsageParser,
        events: &[Value],
    ) -> Option<(TokenUsage, String)> {
        let usage = parser(events).unwrap_or_default();
        if !should_record_usage(&usage, self.source) {
            log::debug!(
                "[{}] 流式响应 usage 全 0 或缺失，跳过消费记录",
                self.app_type
            );
            return None;
        }
        let model = self.attribute_model([usage.model.as_deref()]);
        Some((usage, model))
    }

    /// 异步写入一行使用量（不阻塞响应）。
    pub(crate) fn spawn(
        &self,
        usage: TokenUsage,
        model: String,
        first_token_ms: Option<u64>,
        is_streaming: bool,
        status_code: u16,
    ) {
        if !usage_logging_enabled(&self.state) {
            return;
        }
        let target = self.clone();
        tokio::spawn(async move {
            target
                .write(usage, model, first_token_ms, is_streaming, status_code)
                .await;
        });
    }

    /// 同步等待写入一行使用量（用于随后立即返回错误的路径）。
    pub(crate) async fn write(
        &self,
        usage: TokenUsage,
        model: String,
        first_token_ms: Option<u64>,
        is_streaming: bool,
        status_code: u16,
    ) {
        if !usage_logging_enabled(&self.state) {
            return;
        }
        let latency_ms = self.start_time.elapsed().as_millis() as u64;
        log_usage_internal(
            &self.state,
            &self.provider_id,
            self.app_type,
            &model,
            &self.request_model,
            &self.outbound_model,
            usage,
            latency_ms,
            first_token_ms,
            is_streaming,
            status_code,
            Some(self.session_id.clone()),
        )
        .await;
    }
}

/// 转换后流式响应的使用量收集器（Converted 来源，零用量不落账）。
pub(crate) fn converted_stream_usage_collector(
    state: &ProxyState,
    ctx: &RequestContext,
    status_code: u16,
    filter: StreamUsageEventFilter,
    parser: StreamUsageParser,
) -> Option<SseUsageCollector> {
    if !usage_logging_enabled(state) {
        return None;
    }
    let target = UsageLogTarget::from_ctx(state, ctx, UsageSource::Converted);
    Some(SseUsageCollector::new(
        ctx.start_time,
        Some(filter),
        move |events, first_token_ms| {
            if let Some((usage, model)) = target.usage_from_stream(parser, &events) {
                target.spawn(usage, model, first_token_ms, true, status_code);
            }
        },
    ))
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
) {
    use super::usage::logger::UsageLogger;

    let logger = UsageLogger::new(&state.db);
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

    if let Err(e) = logger.log_with_calculation(
        request_id,
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
        log::warn!("[USG-001] 记录使用量失败: {e}");
    }
}

/// 创建带日志记录和超时控制的透传流
pub fn create_logged_passthrough_stream(
    stream: impl Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
    tag: &'static str,
    usage_collector: Option<SseUsageCollector>,
    timeout_config: StreamingTimeoutConfig,
    connection_guard: Option<ActiveConnectionGuard>,
    error_format: StreamErrorFormat,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut conn_guard = connection_guard;
        // 已发送字节是否停在 SSE 事件边界；不在边界时终止事件前先补空行收尾。
        let mut at_event_boundary = true;
        let mut sse_buffer = SseBlockBuffer::new();
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
                            // 超时：计入 provider 健康度，并给客户端补一个协议内的终止错误事件，
                            // 而不是直接截断 chunked body（PROXY-09）
                            let message = if is_first_chunk {
                                format!("流式响应首字节超时: {}秒无数据", duration.as_secs())
                            } else {
                                ProxyError::StreamIdleTimeout(duration.as_secs()).to_string()
                            };
                            log::error!("[{tag}] {message}");
                            if let Some(guard) = conn_guard.as_mut() {
                                guard.report_stream_failure(&message);
                            }
                            yield Ok(stream_error_event(error_format, &message, at_event_boundary));
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
                    }
                    is_first_chunk = false;
                    if !bytes.is_empty() {
                        at_event_boundary = bytes.ends_with(b"\n\n");
                    }
                    if inspect_sse_events {
                        sse_buffer.push(&bytes);

                        // 尝试解析并记录完整的 SSE 事件（块以 ASCII 分隔符结尾，
                        // 不会切断多字节字符；合法 UTF-8 时 from_utf8_lossy 零拷贝）
                        while let Some(block) = sse_buffer.next_block() {
                            let event_text = String::from_utf8_lossy(&block);
                            {
                                // 提取 data 部分；只有 usage collector 存在时才解析 JSON。
                                for line in event_text.lines() {
                                    if let Some(data) = strip_sse_field(line, "data") {
                                        if data.trim() != "[DONE]" {
                                            let collected = match &collector {
                                                Some(c) if c.should_collect(data) => {
                                                    match serde_json::from_str::<Value>(data) {
                                                        Ok(json_value) => {
                                                            c.push(json_value).await;
                                                            true
                                                        }
                                                        Err(_) => false,
                                                    }
                                                }
                                                _ => false,
                                            };
                                            if collected {
                                                log::debug!(
                                                    "[{tag}] <<< SSE 事件: {}",
                                                    super::http_client::log_body_excerpt(data)
                                                );
                                            } else {
                                                log::debug!(
                                                    "[{tag}] <<< SSE 数据: {}",
                                                    super::http_client::log_body_excerpt(data)
                                                );
                                            }
                                        } else {
                                            log::debug!("[{tag}] <<< SSE: [DONE]");
                                        }
                                    }
                                }
                            }
                        }
                    }

                    yield Ok(bytes);
                }
                Some(Err(e)) => {
                    log::error!("[{tag}] 流错误: {e}");
                    let message = format!("上游流式响应中断: {e}");
                    if let Some(guard) = conn_guard.as_mut() {
                        guard.report_stream_failure(&message);
                    }
                    yield Ok(stream_error_event(error_format, &message, at_event_boundary));
                    break;
                }
                None => {
                    // 流正常结束
                    if let Some(guard) = conn_guard.as_mut() {
                        guard.report_stream_success();
                    }
                    break;
                }
            }
        }

        if let Some(c) = collector.take() {
            c.finish().await;
        }
        if let Some(guard) = &mut finish_guard {
            guard.disarm();
        }
    }
}

/// 流中途失败时补发给客户端的协议内终止错误事件（PROXY-09）。
///
/// `at_event_boundary == false` 时先补一个空行，结束可能被截断的半个事件，
/// 保证错误事件被客户端解析为独立事件。
pub(crate) fn stream_error_event(
    format: StreamErrorFormat,
    message: &str,
    at_event_boundary: bool,
) -> Bytes {
    let payload = match format {
        StreamErrorFormat::Anthropic => format!(
            "event: error\ndata: {}\n\n",
            serde_json::json!({
                "type": "error",
                "error": {"type": "api_error", "message": message}
            })
        ),
        StreamErrorFormat::OpenAiChat => format!(
            "data: {}\n\n",
            serde_json::json!({
                "error": {
                    "message": message,
                    "type": "proxy_error",
                    "code": "cc_switch_stream_interrupted"
                }
            })
        ),
        StreamErrorFormat::Responses => format!(
            "event: response.failed\ndata: {}\n\n",
            serde_json::json!({
                "type": "response.failed",
                "response": {
                    "status": "failed",
                    "error": {"code": "cc_switch_stream_interrupted", "message": message}
                }
            })
        ),
    };
    if at_event_boundary {
        Bytes::from(payload)
    } else {
        Bytes::from(format!("\n\n{payload}"))
    }
}

fn format_headers(headers: &HeaderMap) -> String {
    super::http_client::format_headers_redacted(headers)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::database::Database;
    use crate::error::AppError;
    use crate::provider::ProviderMeta;
    use crate::proxy::failover_switch::FailoverSwitchManager;
    use crate::proxy::provider_router::ProviderRouter;
    use crate::proxy::providers::{
        codex_chat_history::CodexChatHistoryStore, gemini_shadow::GeminiShadowStore,
    };
    use crate::proxy::types::{ProxyConfig, ProxyStatus};
    use rust_decimal::Decimal;
    use std::collections::HashMap;
    use std::str::FromStr;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[test]
    fn test_strip_sse_field_accepts_optional_space() {
        assert_eq!(
            super::strip_sse_field("data: {\"ok\":true}", "data"),
            Some("{\"ok\":true}")
        );
        assert_eq!(
            super::strip_sse_field("data:{\"ok\":true}", "data"),
            Some("{\"ok\":true}")
        );
        assert_eq!(
            super::strip_sse_field("event: message_start", "event"),
            Some("message_start")
        );
        assert_eq!(
            super::strip_sse_field("event:message_start", "event"),
            Some("message_start")
        );
        assert_eq!(super::strip_sse_field("id:1", "data"), None);
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

    pub(crate) fn build_state(db: Arc<Database>) -> ProxyState {
        ProxyState {
            db: db.clone(),
            config: Arc::new(RwLock::new(ProxyConfig::default())),
            status: Arc::new(RwLock::new(ProxyStatus::default())),
            start_time: Arc::new(RwLock::new(None)),
            current_providers: Arc::new(RwLock::new(HashMap::new())),
            provider_router: Arc::new(ProviderRouter::new(db.clone())),
            gemini_shadow: Arc::new(GeminiShadowStore::default()),
            codex_chat_history: Arc::new(CodexChatHistoryStore::default()),
            app_handle: None,
            failover_manager: Arc::new(FailoverSwitchManager::new(db)),
        }
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

    async fn collect_with(config: &UsageParserConfig, events: &[Value]) -> Vec<Value> {
        let captured = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = captured.clone();
        let collector = SseUsageCollector::with_retention(
            std::time::Instant::now(),
            config.stream_event_filter,
            config.stream_event_retention,
            move |events, _| {
                *sink.lock().unwrap() = events;
            },
        );
        for event in events {
            let data = event.to_string();
            if collector.should_collect(&data) {
                collector.push(event.clone()).await;
            }
        }
        collector.finish().await;
        let events = captured.lock().unwrap().clone();
        events
    }

    #[tokio::test]
    async fn openai_collector_retains_bounded_events_with_same_usage() {
        use crate::proxy::handler_config::OPENAI_PARSER_CONFIG;
        let mut events: Vec<Value> = (0..1000)
            .map(|i| {
                serde_json::json!({
                    "id": "chatcmpl-1",
                    "model": "gpt-first",
                    "choices": [{"index": 0, "delta": {"content": format!("tok{i}")}}],
                    "usage": null
                })
            })
            .collect();
        events.push(serde_json::json!({
            "id": "chatcmpl-1",
            "model": "gpt-last",
            "choices": [],
            "usage": {"prompt_tokens": 11, "completion_tokens": 7}
        }));

        let retained = collect_with(&OPENAI_PARSER_CONFIG, &events).await;
        assert!(retained.len() <= 2, "retained {} events", retained.len());

        let full = (OPENAI_PARSER_CONFIG.stream_parser)(&events).unwrap();
        let bounded = (OPENAI_PARSER_CONFIG.stream_parser)(&retained).unwrap();
        assert_eq!(bounded.input_tokens, full.input_tokens);
        assert_eq!(bounded.output_tokens, full.output_tokens);
        assert_eq!(bounded.model, full.model);
        assert_eq!(
            (OPENAI_PARSER_CONFIG.model_extractor)(&retained, "fallback"),
            (OPENAI_PARSER_CONFIG.model_extractor)(&events, "fallback"),
        );
    }

    #[tokio::test]
    async fn gemini_collector_keeps_first_model_and_last_usage() {
        use crate::proxy::handler_config::GEMINI_PARSER_CONFIG;
        let mut events = vec![serde_json::json!({
            "modelVersion": "gemini-first",
            "usageMetadata": {"promptTokenCount": 5, "totalTokenCount": 6}
        })];
        for total in 7..500 {
            events.push(serde_json::json!({
                "modelVersion": "gemini-later",
                "usageMetadata": {"promptTokenCount": 5, "totalTokenCount": total}
            }));
        }

        let retained = collect_with(&GEMINI_PARSER_CONFIG, &events).await;
        assert!(retained.len() <= 2, "retained {} events", retained.len());

        let full = (GEMINI_PARSER_CONFIG.stream_parser)(&events).unwrap();
        let bounded = (GEMINI_PARSER_CONFIG.stream_parser)(&retained).unwrap();
        assert_eq!(bounded.input_tokens, full.input_tokens);
        assert_eq!(bounded.output_tokens, full.output_tokens);
        assert_eq!(bounded.model.as_deref(), Some("gemini-first"));
        assert_eq!(bounded.model, full.model);
    }

    #[tokio::test]
    async fn claude_collector_keeps_all_selected_events() {
        use crate::proxy::handler_config::CLAUDE_PARSER_CONFIG;
        let events = vec![
            serde_json::json!({"type": "message_start", "message": {"id": "m", "model": "c", "usage": {"input_tokens": 3}}}),
            serde_json::json!({"type": "content_block_delta", "delta": {"text": "x"}}),
            serde_json::json!({"type": "message_delta", "usage": {"output_tokens": 4}}),
        ];
        let retained = collect_with(&CLAUDE_PARSER_CONFIG, &events).await;
        assert_eq!(retained.len(), 2);
        let usage = (CLAUDE_PARSER_CONFIG.stream_parser)(&retained).unwrap();
        assert_eq!((usage.input_tokens, usage.output_tokens), (3, 4));
    }

    #[test]
    fn stream_error_events_are_protocol_shaped() {
        use crate::proxy::handler_config::StreamErrorFormat;
        let anthropic = stream_error_event(StreamErrorFormat::Anthropic, "boom", true);
        let text = std::str::from_utf8(&anthropic).unwrap();
        assert!(text.starts_with("event: error\ndata: "));
        let data: Value = serde_json::from_str(
            text.trim_end()
                .strip_prefix("event: error\ndata: ")
                .unwrap(),
        )
        .unwrap();
        assert_eq!(data["type"], "error");
        assert_eq!(data["error"]["message"], "boom");

        let chat = stream_error_event(StreamErrorFormat::OpenAiChat, "boom", false);
        let text = std::str::from_utf8(&chat).unwrap();
        assert!(
            text.starts_with("\n\ndata: {"),
            "closes a partial event first"
        );
        assert!(text.contains("\"code\":\"cc_switch_stream_interrupted\""));

        let responses = stream_error_event(StreamErrorFormat::Responses, "boom", true);
        let text = std::str::from_utf8(&responses).unwrap();
        assert!(text.starts_with("event: response.failed\n"));
        assert!(text.contains("\"status\":\"failed\""));
    }

    #[tokio::test]
    async fn upstream_error_mid_stream_yields_terminal_event_not_io_error() {
        use crate::proxy::handler_config::StreamErrorFormat;
        let upstream = futures::stream::iter(vec![
            Ok(Bytes::from_static(b"data: {\"partial\"")),
            Err(std::io::Error::other("connection reset")),
        ]);
        let stream = create_logged_passthrough_stream(
            upstream,
            "test",
            None,
            StreamingTimeoutConfig {
                first_byte_timeout: 0,
                idle_timeout: 0,
            },
            None,
            StreamErrorFormat::OpenAiChat,
        );
        let items: Vec<_> = stream.collect().await;
        assert_eq!(items.len(), 2);
        let last = items[1].as_ref().expect("terminal event, not an io error");
        let text = std::str::from_utf8(last).unwrap();
        assert!(text.starts_with("\n\ndata: "));
        assert!(text.contains("connection reset"));
    }

    #[test]
    fn zero_usage_rule_is_source_dependent() {
        let zero = TokenUsage::default();
        let cached = TokenUsage {
            cache_read_tokens: 5,
            ..TokenUsage::default()
        };
        assert!(should_record_usage(&zero, UsageSource::Passthrough));
        assert!(!should_record_usage(&zero, UsageSource::Converted));
        assert!(should_record_usage(&cached, UsageSource::Converted));
    }

    fn usage_target(outbound: Option<&str>, source: UsageSource) -> UsageLogTarget {
        let db = Arc::new(Database::memory().expect("db"));
        UsageLogTarget {
            state: build_state(db),
            provider_id: "p".to_string(),
            app_type: "claude",
            request_model: "client-alias".to_string(),
            outbound_model: outbound.unwrap_or("client-alias").to_string(),
            session_id: "s".to_string(),
            start_time: std::time::Instant::now(),
            source,
        }
    }

    #[test]
    fn model_attribution_prefers_echo_then_outbound_then_request() {
        let target = usage_target(Some("mapped-upstream"), UsageSource::Converted);
        assert_eq!(target.attribute_model([Some(""), Some("echoed")]), "echoed");
        assert_eq!(target.attribute_model([None, Some("")]), "mapped-upstream");
        let target = usage_target(None, UsageSource::Converted);
        assert_eq!(target.attribute_model([]), "client-alias");
    }

    #[test]
    fn usage_from_response_applies_one_rule_for_all_paths() {
        let body = serde_json::json!({
            "model": "",
            "usage": {"input_tokens": 0, "output_tokens": 0}
        });
        // Converted: synthesised all-zero usage is skipped.
        let converted = usage_target(Some("mapped"), UsageSource::Converted);
        assert!(converted
            .usage_from_response(TokenUsage::from_claude_response, &body)
            .is_none());
        // Passthrough: the row is kept so the request stays visible.
        let passthrough = usage_target(Some("mapped"), UsageSource::Passthrough);
        let (usage, model) = passthrough
            .usage_from_response(TokenUsage::from_claude_response, &body)
            .expect("row");
        assert!(!usage.has_billable_tokens());
        assert_eq!(model, "mapped");
        // Unparseable usage: passthrough still writes a zero row, converted does not.
        let no_usage = serde_json::json!({"model": "echo"});
        assert_eq!(
            passthrough
                .usage_from_response(TokenUsage::from_claude_response, &no_usage)
                .map(|(_, m)| m),
            Some("echo".to_string())
        );
        assert!(converted
            .usage_from_response(TokenUsage::from_claude_response, &no_usage)
            .is_none());

        let billable = serde_json::json!({
            "id": "msg_1",
            "model": "echo",
            "usage": {"input_tokens": 3, "output_tokens": 4}
        });
        let (usage, model) = converted
            .usage_from_response(TokenUsage::from_claude_response, &billable)
            .expect("row");
        assert_eq!((usage.input_tokens, usage.output_tokens), (3, 4));
        assert_eq!(model, "echo");
    }
}
