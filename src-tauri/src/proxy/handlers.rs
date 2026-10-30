//! 请求处理器
//!
//! 处理各种API端点的HTTP请求
//!
//! 重构后的结构：
//! - 端点差异由 `endpoints::CLIENT_ENDPOINTS` 描述，统一走 `handle_proxied`
//! - 通用逻辑提取到 `handler_context` 和 `response_processor` 模块
//! - 转发层错误按客户端线协议由 `errors::render` 渲染
//! - Claude 的格式转换逻辑保留在此文件（用于 OpenRouter 旧接口回退）

use super::{
    capture::{tap_stream, CaptureBridge, CaptureSide},
    content_encoding::{decompress_body, get_content_encoding, is_supported_content_encoding},
    endpoints::{BodyDecoding, ClientEndpoint, UpstreamEndpoint},
    error_mapper::{get_error_message, map_proxy_error_to_status},
    errors::render::{
        handle_codex_chat_error_response, render_forward_error, render_selection_error,
    },
    forwarder::ActiveConnectionGuard,
    handler_config::{
        parser_config_for, CLAUDE_PARSER_CONFIG, CODEX_PARSER_CONFIG, GEMINI_PARSER_CONFIG,
    },
    handler_context::RequestContext,
    protocol::{ClaudeApiFormat, WireFormat},
    providers::{
        transform_codex_chat, transform_gemini, transform_responses, GeminiGenerateEndpoint,
    },
    response_processor::{
        process_response, read_decoded_body, rebuilt_json_response, response_usage_log_task,
        spawn_response_usage_log, transformed_sse_response,
    },
    routing::{ResponseDecode, RoutePlan},
    server::{ProxyState, MAX_REQUEST_BODY_BYTES},
    stream::{
        aggregate::{parse_upstream_json, responses_sse_to_response_value, SseAggregator},
        codec::{
            anthropic_decoder, decode_upstream, AnthropicToGemini, AnthropicToResponses,
            ChatToResponses, GeminiToAnthropic, StreamDecoder, StreamEncoder,
        },
    },
    types::*,
    usage::parity::raw_usage_block,
    ProxyError,
};
use axum::{extract::State, http::StatusCode, Json};
use bytes::Bytes;
use serde_json::{json, Value};

// ============================================================================
// 请求体读取
// ============================================================================

/// 以 [`MAX_REQUEST_BODY_BYTES`] 为上限读取入站请求体
///
/// handler 直接接收 `axum::extract::Request`，路由层的 `DefaultBodyLimit` 对其不生效，
/// 因此必须在这里自行限流：超限返回 413，读取失败（如客户端中途断开）返回 400。
async fn read_request_body(body: axum::body::Body) -> Result<Bytes, ProxyError> {
    read_request_body_with_limit(body, MAX_REQUEST_BODY_BYTES).await
}

async fn read_request_body_with_limit(
    body: axum::body::Body,
    limit: usize,
) -> Result<Bytes, ProxyError> {
    axum::body::to_bytes(body, limit).await.map_err(|e| {
        if is_length_limit_error(&e) {
            ProxyError::PayloadTooLarge(limit)
        } else {
            ProxyError::InvalidRequest(format!("Failed to read request body: {e}"))
        }
    })
}

fn is_length_limit_error(error: &axum::Error) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(err) = source {
        if err.is::<http_body_util::LengthLimitError>() {
            return true;
        }
        source = err.source();
    }
    false
}

/// 把请求体解析为 JSON；格式错误属于客户端问题，返回 400 而不是 500
fn parse_request_json(body_bytes: &[u8]) -> Result<Value, ProxyError> {
    serde_json::from_slice(body_bytes)
        .map_err(|e| ProxyError::InvalidRequest(format!("Failed to parse request body: {e}")))
}

// ============================================================================
// 健康检查和状态查询（简单端点）
// ============================================================================

/// 健康检查
pub async fn health_check() -> (StatusCode, Json<Value>) {
    (
        StatusCode::OK,
        Json(json!({
            "status": "healthy",
            "timestamp": chrono::Utc::now().to_rfc3339(),
        })),
    )
}

/// 获取服务状态
pub async fn get_status(State(state): State<ProxyState>) -> Result<Json<ProxyStatus>, ProxyError> {
    let status = state.status.read().await.clone();
    Ok(Json(status))
}

/// GET /v1/models — Codex model list (reachability check)
///
/// Codex CLI probes this endpoint at startup and deserializes the response as a
/// catalog with a top-level `models` field.  Return the cc-switch–managed model
/// catalog file directly so the format always matches what the current version
/// of Codex expects.
///
/// Only serves the catalog when the live config.toml still references the
/// cc-switch–owned `model_catalog_json`, using the same path ownership rules as
/// Codex live-setting import.
pub async fn handle_models() -> Result<Json<Value>, ProxyError> {
    let generated_path = crate::codex_config::get_codex_model_catalog_path();
    let active_catalog_path = match crate::codex_config::read_codex_config_text() {
        Ok(config_text) => {
            crate::codex_config::resolve_cc_switch_catalog_path(&config_text, &generated_path)
        }
        Err(_) => None,
    };

    let catalog = if let Some(catalog_path) =
        active_catalog_path.as_ref().filter(|path| path.exists())
    {
        let text = std::fs::read_to_string(catalog_path).unwrap_or_default();
        serde_json::from_str(&text).unwrap_or(json!({"models": []}))
    } else {
        if active_catalog_path.is_none() {
            log::debug!(
                "[models] stale guard: catalog not served (model_catalog_json not set to cc-switch catalog)"
            );
        }
        json!({"models": []})
    };
    Ok(Json(catalog))
}

// ============================================================================
// 统一代理入口
// ============================================================================

/// 处理 [`CLIENT_ENDPOINTS`](super::endpoints::CLIENT_ENDPOINTS) 中全部端点的请求
///
/// 端点之间的差异（请求体解码、上游端点、错误渲染、响应处理分支）都由
/// [`ClientEndpoint`] 描述。Claude 分支包含独特的格式转换逻辑：
/// - 过去用于 OpenRouter 的 OpenAI Chat Completions 兼容接口（Anthropic ↔ OpenAI 转换）
/// - 现在 OpenRouter 已推出 Claude Code 兼容接口，默认不再启用该转换（逻辑保留以备回退）
pub(crate) async fn handle_proxied(
    state: ProxyState,
    request: axum::extract::Request,
    client: &'static ClientEndpoint,
) -> Result<axum::response::Response, ProxyError> {
    if client.claude_desktop_gateway_auth {
        validate_claude_desktop_gateway_auth(&state, request.headers())?;
    }

    let (parts, req_body) = request.into_parts();
    let method = parts.method;
    let uri = parts.uri;
    let mut headers = parts.headers;
    let extensions = parts.extensions;
    let body_bytes = read_request_body(req_body).await?;
    let body_bytes = match client.body_decoding {
        BodyDecoding::Plain => body_bytes,
        BodyDecoding::ContentEncoded => decode_codex_request_body(&mut headers, body_bytes)?,
    };
    // GET 类只读端点（/v1beta/models、/v1beta/models/<model> 等）没有请求体，
    // 不能强制 parse 为 JSON —— 否则空 body 会被拒绝。
    let mut body: Value = if client.allow_empty_body && body_bytes.is_empty() {
        Value::Null
    } else {
        parse_request_json(&body_bytes)?
    };

    let app_type = &client.app_type;
    let mut ctx = match RequestContext::new(
        &state,
        &body,
        &headers,
        app_type.clone(),
        client.tag,
        client.app_type_str,
    )
    .await
    {
        Ok(ctx) => ctx,
        // 选路失败（如所有供应商均已达到消费限额）按客户端线协议渲染
        Err(error) => return Ok(render_selection_error(client.client_format, error)),
    };
    if client.model_from_uri {
        // Gemini 的模型名称在 URI 中
        ctx = ctx.with_model_from_uri(&uri);
    }

    // 缺陷报告抓包：开启期间记录客户端请求（脱敏后）
    ctx.capture = state.capture.begin(ctx.app_type_str, &ctx.provider.id);
    if let Some(capture) = &ctx.capture {
        capture.record_client_request(&method, &uri, &headers, &body);
    }

    let endpoint = upstream_endpoint(&uri, client.upstream);
    let is_stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    // 请求追踪：始终记录（环形缓冲区，只保留最近的请求）
    let trace = state
        .traces
        .begin(ctx.app_type_str, &method, &uri, is_stream);
    trace.record_client_headers(&headers);
    ctx.trace = Some(trace);
    let codex_tool_context = (client.client_format == WireFormat::OpenAiResponses)
        .then(|| transform_codex_chat::build_codex_tool_context_from_request(&body));
    // Claude 格式转换需要原始请求体（工具 schema 提示），其余端点直接移交给转发器
    let forward_body = if client.client_format == WireFormat::AnthropicMessages {
        body.clone()
    } else {
        body.take()
    };

    let forwarder = ctx.create_forwarder(&state, client.client_format);
    let mut result = match forwarder
        .forward_with_retry(
            app_type,
            method,
            &endpoint,
            forward_body,
            headers,
            extensions,
            ctx.get_providers(),
        )
        .await
    {
        Ok(result) => result,
        Err(mut err) => {
            if let Some(provider) = err.provider.take() {
                ctx.provider = provider;
            }
            if let Some(capture) = ctx.capture.take() {
                state.capture.discard(&capture);
            }
            if let Some(trace) = &ctx.trace {
                trace.finish(
                    map_proxy_error_to_status(&err.error),
                    Some(&get_error_message(&err.error)),
                );
            }
            log_forward_error(&state, &ctx, is_stream, &err.error);
            return render_forward_error(client.client_format, &ctx, &endpoint, err.error);
        }
    };

    let connection_guard = result.connection_guard.take();
    ctx.outbound_model = result.outbound_model.take();
    ctx.provider = result.provider;
    // 响应解码只读成功那次尝试的路由计划，不再按最终 provider 重新推导格式
    let plan = result.plan;
    let response = result.response;
    if let Some(trace) = &ctx.trace {
        trace.finish(response.status().as_u16(), None);
    }
    // 只抓协议桥请求；透传请求的记录直接丢弃
    if let Some(capture) = ctx.capture.take() {
        match CaptureBridge::from_decode(plan.response_decode()) {
            Some(bridge) => {
                capture.set_route(&ctx.provider.id, bridge, plan.quirks.codex_oauth);
                capture.set_upstream_status(response.status().as_u16());
                ctx.capture = Some(capture);
            }
            None => state.capture.discard(&capture),
        }
    }

    let parser_config = match plan.response_decode() {
        ResponseDecode::AnthropicFrom(api_format) => {
            return handle_claude_transform(
                response,
                &ctx,
                &state,
                &body,
                is_stream,
                api_format,
                &plan,
                connection_guard,
            )
            .await;
        }
        ResponseDecode::ResponsesFromAnthropic => {
            return handle_codex_anthropic_to_responses_transform(
                response,
                &ctx,
                &state,
                is_stream,
                connection_guard,
                codex_tool_context.unwrap_or_default(),
            )
            .await;
        }
        ResponseDecode::ResponsesFromChat => {
            return handle_codex_chat_to_responses_transform(
                response,
                &ctx,
                &state,
                is_stream,
                connection_guard,
                codex_tool_context.unwrap_or_default(),
            )
            .await;
        }
        ResponseDecode::GeminiFromAnthropic => {
            // Gemini 请求体没有 `stream` 字段，流式由 URL 的 `:streamGenerateContent` 决定
            let is_stream = GeminiGenerateEndpoint::parse(&endpoint).is_some_and(|e| e.stream);
            return handle_gemini_anthropic_transform(
                response,
                &ctx,
                &state,
                is_stream,
                connection_guard,
            )
            .await;
        }
        ResponseDecode::Passthrough => parser_config_for(plan.client),
    };

    // 通用响应处理（透传模式）
    process_response(response, &ctx, &state, parser_config, connection_guard).await
}

/// 由入站 URI 推导转发给上游的端点
fn upstream_endpoint(uri: &axum::http::Uri, upstream: UpstreamEndpoint) -> String {
    match upstream {
        UpstreamEndpoint::Inbound { strip_prefix } => {
            let raw_endpoint = uri
                .path_and_query()
                .map(|path_and_query| path_and_query.as_str())
                .unwrap_or(uri.path());
            strip_prefix
                .and_then(|prefix| raw_endpoint.strip_prefix(prefix))
                .unwrap_or(raw_endpoint)
                .to_string()
        }
        UpstreamEndpoint::Fixed(path) => endpoint_with_query(uri, path),
    }
}

// ============================================================================
// Claude API 处理器（包含格式转换逻辑）
// ============================================================================

pub async fn handle_claude_desktop_models(
    State(state): State<ProxyState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, ProxyError> {
    validate_claude_desktop_gateway_auth(&state, &headers)?;
    let providers = state
        .provider_router
        .select_providers("claude-desktop")
        .await
        .map_err(|e| match e {
            // 当前供应商已达到消费限额：返回 429，而非误报为数据库错误
            crate::error::AppError::SpendCapExceeded(detail) => {
                ProxyError::SpendCapExceeded(detail)
            }
            e => ProxyError::DatabaseError(e.to_string()),
        })?;
    let provider = providers.first().ok_or(ProxyError::NoAvailableProvider)?;
    let response = crate::claude_desktop_config::model_list_response(provider)
        .map_err(|e| ProxyError::ConfigError(e.to_string()))?;
    Ok(Json(response))
}

fn validate_claude_desktop_gateway_auth(
    state: &ProxyState,
    headers: &axum::http::HeaderMap,
) -> Result<(), ProxyError> {
    let expected = state
        .runtime_config
        .claude_desktop_gateway_token()
        .map_err(|e| ProxyError::AuthError(e.to_string()))?;
    let Some(value) = headers.get(axum::http::header::AUTHORIZATION) else {
        return Err(ProxyError::AuthError(
            "Claude Desktop gateway 缺少 Authorization 头".to_string(),
        ));
    };
    let value = value
        .to_str()
        .map_err(|_| ProxyError::AuthError("Authorization 头格式无效".to_string()))?;
    let token = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
        .unwrap_or("")
        .trim();
    if token != expected {
        return Err(ProxyError::AuthError(
            "Claude Desktop gateway token 无效".to_string(),
        ));
    }
    Ok(())
}

/// Claude 格式转换处理（独有逻辑）
///
/// 支持 OpenAI Chat Completions、Responses API 与 Gemini Native 三种上游格式的转换
#[allow(clippy::too_many_arguments)]
async fn handle_claude_transform(
    response: super::hyper_client::ProxyResponse,
    ctx: &RequestContext,
    state: &ProxyState,
    original_body: &Value,
    is_stream: bool,
    api_format: ClaudeApiFormat,
    plan: &RoutePlan,
    connection_guard: Option<ActiveConnectionGuard>,
) -> Result<axum::response::Response, ProxyError> {
    let status = response.status();
    let is_codex_oauth = plan.quirks.codex_oauth;
    // Codex OAuth 会把 openai_responses 响应强制升级为 SSE，即使客户端发的是 stream:false。
    // should_use_claude_transform_streaming 默认会把这个组合路由到流式转换器——虽然能避免
    // JSON parse 报 422，但会让非流客户端收到 text/event-stream，违反 Anthropic 非流语义。
    // 这里为这个特定组合打开 override：把上游 SSE 聚合成 Anthropic JSON 回给客户端，其它
    // 场景（任意上游 is_sse、非 Codex OAuth 等）仍沿用原有流式兜底。
    let aggregate_codex_oauth_responses_sse =
        !is_stream && is_codex_oauth && api_format == ClaudeApiFormat::OpenAiResponses;
    let use_streaming = if aggregate_codex_oauth_responses_sse {
        false
    } else {
        should_use_claude_transform_streaming(
            is_stream,
            response.is_sse(),
            api_format,
            is_codex_oauth,
        )
    };
    let tool_schema_hints = transform_gemini::extract_anthropic_tool_schema_hints(original_body);
    let tool_schema_hints = (!tool_schema_hints.is_empty()).then_some(tool_schema_hints);

    if use_streaming {
        // 根据 api_format 选择流式解码器；usage tap 观测转换后的 Anthropic SSE
        let decoder = anthropic_decoder(
            api_format,
            GeminiToAnthropic {
                shadow_store: Some(state.gemini_shadow.clone()),
                provider_id: Some(ctx.provider.id.clone()),
                session_id: Some(ctx.session_id.clone()),
                tool_schema_hints: tool_schema_hints.clone(),
            },
        );
        let upstream = tap_stream(
            ctx.capture.clone(),
            CaptureSide::Upstream,
            response.bytes_stream(),
        );
        let (client_format, sse_stream) = decode_upstream(decoder, ctx.tag, Box::pin(upstream));
        return Ok(transformed_sse_response(
            sse_stream,
            client_format,
            ctx,
            state,
            status.as_u16(),
            connection_guard,
        ));
    }

    // 非流式响应转换 (OpenAI/Responses → Anthropic)
    let body_timeout = ctx.non_streaming_body_timeout();
    let (response_headers, _status, body_bytes) =
        read_decoded_body(response, ctx.tag, body_timeout).await?;
    if let Some(capture) = &ctx.capture {
        capture.record_body(CaptureSide::Upstream, &body_bytes);
    }

    let upstream_response: Value = if aggregate_codex_oauth_responses_sse {
        responses_sse_to_response_value(&String::from_utf8_lossy(&body_bytes))?
    } else {
        // 兜底嗅探（#2234）：部分网关对 stream:false 强制返回 SSE 体，却把
        // Content-Type 标成 application/json 等，is_sse() 的 header 检查失效。
        // 此时按 SSE 聚合成单个 JSON 再走既有非流转换器，客户端仍收到
        // Anthropic JSON，非流语义不变。gemini_native 暂无聚合器，落诊断错误。
        let aggregator = match api_format {
            ClaudeApiFormat::OpenAiResponses => Some(SseAggregator::Responses),
            ClaudeApiFormat::GeminiNative => None,
            ClaudeApiFormat::OpenAiChat | ClaudeApiFormat::Anthropic => Some(SseAggregator::Chat),
        };
        parse_upstream_json(
            ctx.tag,
            &body_bytes,
            &response_headers,
            aggregator,
            "Failed to parse upstream response",
        )?
    };

    // 转换前的上游原始 usage 块，供 usage parity 检查
    let upstream_usage = raw_usage_block(&upstream_response).cloned();

    // Preserve raw Responses usage so a post-upstream conversion failure still
    // records the tokens already consumed by the successful upstream request.
    let raw_usage_response = (api_format == ClaudeApiFormat::OpenAiResponses).then(|| {
        json!({
            "id": upstream_response.get("id").cloned().unwrap_or(Value::Null),
            "model": upstream_response.get("model").cloned().unwrap_or(Value::Null),
            "usage": transform_responses::build_anthropic_usage_from_responses(
                upstream_response.get("usage")
            )
        })
    });

    // 根据 api_format 选择非流式转换器
    let transform_result = if api_format == ClaudeApiFormat::OpenAiResponses {
        crate::proxy::codec::bridge::responses_to_anthropic_response(
            upstream_response,
            crate::settings::codec_engine(),
        )
    } else if api_format == ClaudeApiFormat::GeminiNative {
        crate::proxy::codec::bridge::gemini_to_anthropic_response(
            upstream_response,
            crate::proxy::codec::gemini::GeminiShadowSink::new(
                Some(state.gemini_shadow.clone()),
                Some(ctx.provider.id.clone()),
                Some(ctx.session_id.clone()),
            ),
            tool_schema_hints.as_ref(),
            crate::settings::codec_engine(),
        )
    } else {
        crate::proxy::codec::bridge::openai_chat_to_anthropic_response(
            upstream_response,
            crate::settings::codec_engine(),
        )
    };
    let anthropic_response = match transform_result {
        Ok(response) => response,
        Err(error) => {
            log::error!("[Claude] 转换响应失败: {error}");
            if let Some(task) = raw_usage_response.as_ref().and_then(|response| {
                response_usage_log_task(
                    state,
                    ctx,
                    CLAUDE_PARSER_CONFIG.response_parser,
                    response,
                    upstream_usage.clone(),
                    status.as_u16(),
                )
            }) {
                // The upstream request already succeeded and consumed tokens. Persist
                // usage before returning the terminal transform error to the client.
                task.await;
            }
            return Err(error);
        }
    };

    // 记录使用量
    // 全 0 usage 不落账（对齐 Codex 流式收集器的 skip）：SSE 聚合兜底救回的流
    // 在上游缺 stream_options.include_usage 时没有 usage，写入只会产生无意义空行
    spawn_response_usage_log(
        state,
        ctx,
        CLAUDE_PARSER_CONFIG.response_parser,
        &anthropic_response,
        upstream_usage,
        status.as_u16(),
    );

    if let Some(capture) = &ctx.capture {
        capture.record_json(CaptureSide::Client, &anthropic_response);
    }
    rebuilt_json_response(status, response_headers, &anthropic_response, ctx.tag)
}

fn endpoint_with_query(uri: &axum::http::Uri, endpoint: &str) -> String {
    match uri.query() {
        Some(query) => format!("{endpoint}?{query}"),
        None => endpoint.to_string(),
    }
}

/// Codex 客户端（尤其 Desktop 登录态）可能对请求体启用 zstd 压缩，使得后续
/// `serde_json::from_slice` 直接解析失败。这里在解析前解压，并剥掉已失真的实体头
/// （content-encoding / content-length / transfer-encoding）——转发层会基于解压后的
/// 明文 JSON 重新生成正确的头。
fn decode_codex_request_body(
    headers: &mut axum::http::HeaderMap,
    body_bytes: Bytes,
) -> Result<Bytes, ProxyError> {
    let Some(encoding) = get_content_encoding(headers) else {
        return Ok(body_bytes);
    };

    if !is_supported_content_encoding(&encoding) {
        return Err(ProxyError::InvalidRequest(format!(
            "Unsupported request content-encoding: {encoding}"
        )));
    }

    log::debug!("[Codex] 解压请求体: content-encoding={encoding}");
    let decompressed = match decompress_body(&encoding, &body_bytes) {
        Ok(Some(decompressed)) => decompressed,
        // is_supported_content_encoding 已确保编码受支持，正常不会返回 None；
        // 防御性兜底：宁可报错，也不能把压缩字节当 JSON 透传下去。
        Ok(None) => {
            return Err(ProxyError::InvalidRequest(format!(
                "Unsupported request content-encoding: {encoding}"
            )));
        }
        Err(e) => {
            log::warn!("[Codex] 请求体解压失败 ({encoding}): {e}");
            return Err(ProxyError::InvalidRequest(format!(
                "Failed to decompress request body ({encoding}): {e}"
            )));
        }
    };

    headers.remove(axum::http::header::CONTENT_ENCODING);
    headers.remove(axum::http::header::CONTENT_LENGTH);
    headers.remove(axum::http::header::TRANSFER_ENCODING);

    Ok(Bytes::from(decompressed))
}

// ============================================================================
// Codex API 处理器
// ============================================================================

async fn handle_codex_chat_to_responses_transform(
    response: super::hyper_client::ProxyResponse,
    ctx: &RequestContext,
    state: &ProxyState,
    is_stream: bool,
    connection_guard: Option<ActiveConnectionGuard>,
    tool_context: transform_codex_chat::CodexToolContext,
) -> Result<axum::response::Response, ProxyError> {
    let status = response.status();

    if !status.is_success() {
        // 上游 Chat 错误体形状与 Responses 不一致（如 MiniMax 的 base_resp、自定义 detail 字段）；
        // 直接透传会让 Codex 客户端无法识别错误码。这里统一转换为 Responses 风格
        // `{"error": {message, type, code, param}}`，保留原始 HTTP 状态码。
        return handle_codex_chat_error_response(response, ctx, status).await;
    }

    if is_stream || response.is_sse() {
        let decoder = Box::new(ChatToResponses {
            tool_context,
            history: state.codex_chat_history.clone(),
        });
        let upstream = tap_stream(
            ctx.capture.clone(),
            CaptureSide::Upstream,
            response.bytes_stream(),
        );
        let (client_format, sse_stream) = decode_upstream(decoder, ctx.tag, Box::pin(upstream));
        // 上游遵守 OpenAI 语义省略 usage 时，Chat→Responses 转换器会合成一个
        // 全 0 的 response.completed；全 0 usage 由 tap 收尾时跳过，不写空行。
        return Ok(transformed_sse_response(
            sse_stream,
            client_format,
            ctx,
            state,
            status.as_u16(),
            connection_guard,
        ));
    }

    let _connection_guard = connection_guard;
    let body_timeout = ctx.non_streaming_body_timeout();
    let (response_headers, status, body_bytes) =
        read_decoded_body(response, ctx.tag, body_timeout).await?;
    if let Some(capture) = &ctx.capture {
        capture.record_body(CaptureSide::Upstream, &body_bytes);
    }
    // 与 Claude 侧 handle_claude_transform 对称的兜底嗅探（#2234）：
    // 上游对 stream:false 返回未标记 Content-Type 的 SSE 体时按 SSE 聚合。
    let chat_response = parse_upstream_json(
        ctx.tag,
        &body_bytes,
        &response_headers,
        Some(SseAggregator::Chat),
        "Failed to parse upstream chat response",
    )?;
    let upstream_usage = raw_usage_block(&chat_response).cloned();
    let responses_response = crate::proxy::codec::bridge::chat_to_responses_response(
        chat_response,
        &tool_context,
        crate::settings::codec_engine(),
    )
    .map_err(|e| {
        log::error!("[Codex] Chat → Responses 响应转换失败: {e}");
        e
    })?;
    state
        .codex_chat_history
        .record_response(&responses_response);

    // 上游非流式 Chat 省略 usage 时，chat_usage_to_responses_usage 会合成全 0 usage，
    // from_codex_response 对 input/output 字段存在(哪怕=0)即返回 Some。全 0 由
    // has_billable_tokens 闸门跳过，避免空行虚增请求数——与流式分支对齐。
    spawn_response_usage_log(
        state,
        ctx,
        CODEX_PARSER_CONFIG.response_parser,
        &responses_response,
        upstream_usage,
        status.as_u16(),
    );

    if let Some(capture) = &ctx.capture {
        capture.record_json(CaptureSide::Client, &responses_response);
    }
    rebuilt_json_response(status, response_headers, &responses_response, ctx.tag)
}

/// Response-transform handler for the Codex (Responses) ↔ Anthropic Messages gateway.
///
/// Parallel to `handle_codex_chat_to_responses_transform`: the upstream speaks
/// Anthropic Messages, and this converts the response back into the Responses form
/// Codex expects (both streaming and non-streaming). Error bodies reuse
/// `handle_codex_chat_error_response` (whose extraction logic also works for
/// Anthropic's `{"error":{type,message}}`). It does not involve codex_chat_history
/// (tool ids round-trip natively through Anthropic).
async fn handle_codex_anthropic_to_responses_transform(
    response: super::hyper_client::ProxyResponse,
    ctx: &RequestContext,
    state: &ProxyState,
    is_stream: bool,
    connection_guard: Option<ActiveConnectionGuard>,
    codex_tool_context: transform_codex_chat::CodexToolContext,
) -> Result<axum::response::Response, ProxyError> {
    let status = response.status();

    if !status.is_success() {
        return handle_codex_chat_error_response(response, ctx, status).await;
    }

    let codec = Box::new(AnthropicToResponses {
        tool_context: codex_tool_context,
    });

    // Preserve live streaming when the gateway marks SSE correctly or omits an
    // explicit JSON media type. Explicit JSON is buffered below so 2xx error
    // envelopes and gateways that ignore stream:true can be converted faithfully.
    if response.is_sse() || (is_stream && !response.is_json()) {
        let upstream = tap_stream(
            ctx.capture.clone(),
            CaptureSide::Upstream,
            response.bytes_stream(),
        );
        let (client_format, sse_stream) = decode_upstream(codec, ctx.tag, Box::pin(upstream));
        return Ok(transformed_sse_response(
            sse_stream,
            client_format,
            ctx,
            state,
            status.as_u16(),
            connection_guard,
        ));
    }

    let body_timeout = ctx.non_streaming_body_timeout();
    let (response_headers, status, body_bytes) =
        read_decoded_body(response, ctx.tag, body_timeout).await?;
    if let Some(capture) = &ctx.capture {
        capture.record_body(CaptureSide::Upstream, &body_bytes);
    }
    // Fallback sniffing symmetric to the chat / claude side (#2234): when the
    // upstream returns an Anthropic SSE body with an unmarked Content-Type,
    // aggregate it back into a message before continuing the conversion.
    let anthropic_response = parse_upstream_json(
        ctx.tag,
        &body_bytes,
        &response_headers,
        Some(SseAggregator::Anthropic),
        "Failed to parse upstream anthropic response",
    )?;

    if is_stream {
        let client_format = StreamDecoder::client_format(&*codec);
        let sse_stream = codec.encode_stream(&anthropic_response);
        return Ok(transformed_sse_response(
            sse_stream,
            client_format,
            ctx,
            state,
            status.as_u16(),
            connection_guard,
        ));
    }

    let _connection_guard = connection_guard;
    let upstream_usage = raw_usage_block(&anthropic_response).cloned();
    let responses_response = crate::proxy::codec::bridge::anthropic_to_responses_response(
        anthropic_response,
        &codec.tool_context,
        crate::settings::codec_engine(),
    )
    .map_err(|e| {
        log::error!("[Codex] Failed to convert Anthropic response to Responses: {e}");
        e
    })?;

    spawn_response_usage_log(
        state,
        ctx,
        CODEX_PARSER_CONFIG.response_parser,
        &responses_response,
        upstream_usage,
        status.as_u16(),
    );

    if let Some(capture) = &ctx.capture {
        capture.record_json(CaptureSide::Client, &responses_response);
    }
    rebuilt_json_response(status, response_headers, &responses_response, ctx.tag)
}

/// Gemini 客户端（Gemini CLI）← Anthropic Messages 上游的响应转换（CCS-187）
///
/// 流式客户端：上游 SSE（或忽略 `stream: true` 返回的整段 JSON）逐帧转换为 Gemini SSE；
/// 非流式客户端：上游 JSON（或未标注 Content-Type 的 SSE 体，聚合后）转换为
/// `GenerateContentResponse`。usage 按 Gemini 客户端格式记录（`app_type = gemini`）。
async fn handle_gemini_anthropic_transform(
    response: super::hyper_client::ProxyResponse,
    ctx: &RequestContext,
    state: &ProxyState,
    is_stream: bool,
    connection_guard: Option<ActiveConnectionGuard>,
) -> Result<axum::response::Response, ProxyError> {
    let status = response.status();

    if is_stream {
        let upstream = tap_stream(
            ctx.capture.clone(),
            CaptureSide::Upstream,
            response.bytes_stream(),
        );
        let (client_format, sse_stream) =
            decode_upstream(Box::new(AnthropicToGemini), ctx.tag, Box::pin(upstream));
        return Ok(transformed_sse_response(
            sse_stream,
            client_format,
            ctx,
            state,
            status.as_u16(),
            connection_guard,
        ));
    }

    let _connection_guard = connection_guard;
    let body_timeout = ctx.non_streaming_body_timeout();
    let (response_headers, status, body_bytes) =
        read_decoded_body(response, ctx.tag, body_timeout).await?;
    if let Some(capture) = &ctx.capture {
        capture.record_body(CaptureSide::Upstream, &body_bytes);
    }
    let anthropic_response = parse_upstream_json(
        ctx.tag,
        &body_bytes,
        &response_headers,
        Some(SseAggregator::Anthropic),
        "Failed to parse upstream anthropic response",
    )?;
    let upstream_usage = raw_usage_block(&anthropic_response).cloned();
    let gemini_response =
        crate::proxy::codec::bridge::anthropic_to_gemini_client_response(anthropic_response)
            .map_err(|e| {
                log::error!("[Gemini] Failed to convert Anthropic response to Gemini: {e}");
                e
            })?;

    spawn_response_usage_log(
        state,
        ctx,
        GEMINI_PARSER_CONFIG.response_parser,
        &gemini_response,
        upstream_usage,
        status.as_u16(),
    );

    if let Some(capture) = &ctx.capture {
        capture.record_json(CaptureSide::Client, &gemini_response);
    }
    rebuilt_json_response(status, response_headers, &gemini_response, ctx.tag)
}

fn should_use_claude_transform_streaming(
    requested_streaming: bool,
    upstream_is_sse: bool,
    api_format: ClaudeApiFormat,
    is_codex_oauth: bool,
) -> bool {
    requested_streaming
        || upstream_is_sse
        || (is_codex_oauth && api_format == ClaudeApiFormat::OpenAiResponses)
}

// ============================================================================
// 失败请求记录
// ============================================================================

fn log_forward_error(
    state: &ProxyState,
    ctx: &RequestContext,
    is_streaming: bool,
    error: &ProxyError,
) {
    use super::usage::logger::UsageLogger;

    let logger = UsageLogger::new(&state.db).with_runtime_config(&state.runtime_config);
    let status_code = map_proxy_error_to_status(error);
    let error_message = get_error_message(error);
    let request_id = uuid::Uuid::new_v4().to_string();

    match logger.log_error_with_context(
        request_id.clone(),
        ctx.provider.id.clone(),
        ctx.app_type_str.to_string(),
        ctx.request_model.clone(),
        status_code,
        error_message,
        ctx.latency_ms(),
        is_streaming,
        Some(ctx.session_id.clone()),
        None,
    ) {
        Ok(()) => {
            if let Some(trace) = &ctx.trace {
                trace.link_request(&request_id, ctx.latency_ms(), None);
            }
        }
        Err(e) => log::warn!("记录失败请求日志失败: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        parse_request_json, read_request_body_with_limit, should_use_claude_transform_streaming,
    };
    use crate::proxy::protocol::ClaudeApiFormat;
    use crate::proxy::ProxyError;

    /// 与生产 handler 同样接收原始 Request、走 read_request_body + parse_request_json，
    /// 但上限可以设得很小，便于在测试里触发 413。
    fn body_limit_router(limit: usize) -> axum::Router {
        axum::Router::new().route(
            "/echo",
            axum::routing::post(move |request: axum::extract::Request| async move {
                let bytes = read_request_body_with_limit(request.into_body(), limit).await?;
                let body = parse_request_json(&bytes)?;
                Ok::<_, ProxyError>(axum::Json(body))
            }),
        )
    }

    async fn post_echo(limit: usize, body: impl Into<axum::body::Body>) -> axum::http::StatusCode {
        use tower::ServiceExt;
        let request = axum::http::Request::builder()
            .method("POST")
            .uri("/echo")
            .header("content-type", "application/json")
            .body(body.into())
            .unwrap();
        body_limit_router(limit)
            .oneshot(request)
            .await
            .unwrap()
            .status()
    }

    #[tokio::test]
    async fn request_body_over_limit_returns_413() {
        let oversized = format!("{{\"pad\":\"{}\"}}", "x".repeat(64));
        assert_eq!(
            post_echo(16, oversized).await,
            axum::http::StatusCode::PAYLOAD_TOO_LARGE
        );
    }

    #[tokio::test]
    async fn request_body_over_limit_returns_413_for_chunked_stream() {
        // 没有 Content-Length 的流式请求体也必须在累计超限时被截断
        let chunks: Vec<Result<bytes::Bytes, std::io::Error>> = (0..8)
            .map(|_| Ok(bytes::Bytes::from_static(b"0123456789")))
            .collect();
        let body = axum::body::Body::from_stream(futures::stream::iter(chunks));
        assert_eq!(
            post_echo(32, body).await,
            axum::http::StatusCode::PAYLOAD_TOO_LARGE
        );
    }

    #[tokio::test]
    async fn request_body_within_limit_is_parsed() {
        assert_eq!(
            post_echo(64, r#"{"model":"m"}"#).await,
            axum::http::StatusCode::OK
        );
    }

    #[tokio::test]
    async fn malformed_json_request_body_returns_400() {
        assert_eq!(
            post_echo(1024, "{not json").await,
            axum::http::StatusCode::BAD_REQUEST
        );
        assert!(matches!(
            parse_request_json(b"{not json"),
            Err(ProxyError::InvalidRequest(_))
        ));
    }

    #[test]
    fn codex_oauth_responses_force_streaming_even_if_client_sent_false() {
        assert!(should_use_claude_transform_streaming(
            false,
            false,
            ClaudeApiFormat::OpenAiResponses,
            true,
        ));
    }

    #[test]
    fn upstream_sse_response_always_uses_streaming_path() {
        assert!(should_use_claude_transform_streaming(
            false,
            true,
            ClaudeApiFormat::OpenAiChat,
            false,
        ));
    }

    #[test]
    fn non_streaming_response_stays_non_streaming_for_regular_openai_responses() {
        assert!(!should_use_claude_transform_streaming(
            false,
            false,
            ClaudeApiFormat::OpenAiResponses,
            false,
        ));
    }
}
