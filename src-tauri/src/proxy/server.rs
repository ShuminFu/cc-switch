//! HTTP代理服务器
//!
//! 基于Axum的HTTP服务器，处理代理请求
//!
//! Uses a manual hyper HTTP/1.1 accept loop with `preserve_header_case(true)` so
//! that the original header-name casing from the CLI client is captured in a
//! `HeaderCaseMap` extension.  This map is later forwarded to the upstream via
//! the hyper-based HTTP client, producing wire-level header casing identical to
//! a direct (non-proxied) CLI request.

pub(crate) mod access_guard;
mod header_capture;
mod ip_limit;

use super::{
    failover_switch::FailoverSwitchManager,
    handlers,
    log_codes::srv as log_srv,
    provider_router::ProviderRouter,
    providers::{codex_chat_history::CodexChatHistoryStore, gemini_shadow::GeminiShadowStore},
    types::*,
    ProxyError,
};
use crate::database::Database;
use access_guard::{AccessContext, AccessGuard, PeerInfo, MAX_REQUEST_BODY_BYTES};
use axum::{
    extract::{DefaultBodyLimit, State},
    response::IntoResponse,
    routing::{any, get, post},
    Extension, Json, Router,
};
use hyper_util::rt::{TokioIo, TokioTimer};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{oneshot, RwLock, Semaphore};
use tokio::task::JoinHandle;

/// Maximum time a client may take to send a complete request head (also closes
/// idle keep-alive connections). SEC-14.
const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(30);
/// Maximum number of simultaneously open client connections. SEC-14.
const MAX_CONCURRENT_CONNECTIONS: usize = 1024;
/// How long `stop()` lets in-flight requests finish before aborting them (PROXY-14).
const DRAIN_TIMEOUT: Duration = Duration::from_secs(4);
/// Upper bound for the whole stop sequence (drain + task shutdown).
const STOP_TIMEOUT: Duration = Duration::from_secs(5);

/// 代理服务器状态（共享）
#[derive(Clone)]
pub struct ProxyState {
    pub db: Arc<Database>,
    pub config: Arc<RwLock<ProxyConfig>>,
    /// Mirror of `config.enable_logging` (PERF-49), readable without the
    /// config lock so a concurrent config write can never flip logging on.
    pub logging_enabled: Arc<std::sync::atomic::AtomicBool>,
    /// Server-level status (running, address, port).
    pub status: Arc<RwLock<ProxyStatus>>,
    /// Request counters (PROXY-34), merged into the status on read.
    pub metrics: Arc<super::pipeline::metrics::ProxyMetrics>,
    pub start_time: Arc<RwLock<Option<std::time::Instant>>>,
    /// 每个应用类型当前使用的 provider (app_type -> (provider_id, provider_name))
    pub current_providers: Arc<RwLock<std::collections::HashMap<String, (String, String)>>>,
    /// 共享的 ProviderRouter（持有熔断器状态，跨请求保持）
    pub provider_router: Arc<ProviderRouter>,
    /// Gemini Native shadow state，用于 thoughtSignature / tool call 回放
    pub gemini_shadow: Arc<GeminiShadowStore>,
    /// Codex Chat bridge history，用于恢复 previous_response_id 指向的 tool call
    pub codex_chat_history: Arc<CodexChatHistoryStore>,
    /// AppHandle，用于发射事件和更新托盘菜单
    pub app_handle: Option<tauri::AppHandle>,
    /// 故障转移切换管理器
    pub failover_manager: Arc<FailoverSwitchManager>,
}

impl ProxyState {
    /// Server status with the request counters filled in.
    pub async fn status_snapshot(&self) -> ProxyStatus {
        let mut status = self.status.read().await.clone();
        self.metrics.apply_to(&mut status);
        status
    }
}

/// 代理HTTP服务器
pub struct ProxyServer {
    config: ProxyConfig,
    state: ProxyState,
    shutdown_tx: Arc<RwLock<Option<oneshot::Sender<()>>>>,
    /// 服务器任务句柄，用于等待服务器实际关闭
    server_handle: Arc<RwLock<Option<JoinHandle<()>>>>,
}

impl ProxyServer {
    pub fn new(
        config: ProxyConfig,
        db: Arc<Database>,
        app_handle: Option<tauri::AppHandle>,
    ) -> Self {
        // 创建共享的 ProviderRouter（熔断器状态将跨所有请求保持）
        let provider_router = Arc::new(ProviderRouter::new(db.clone()));
        // 创建故障转移切换管理器
        let failover_manager = Arc::new(FailoverSwitchManager::new(db.clone()));

        let state = ProxyState {
            db,
            config: Arc::new(RwLock::new(config.clone())),
            logging_enabled: Arc::new(std::sync::atomic::AtomicBool::new(config.enable_logging)),
            status: Arc::new(RwLock::new(ProxyStatus::default())),
            metrics: super::pipeline::metrics::ProxyMetrics::new(),
            start_time: Arc::new(RwLock::new(None)),
            current_providers: Arc::new(RwLock::new(std::collections::HashMap::new())),
            provider_router,
            gemini_shadow: Arc::new(GeminiShadowStore::default()),
            codex_chat_history: Arc::new(CodexChatHistoryStore::default()),
            app_handle,
            failover_manager,
        };

        Self {
            config,
            state,
            shutdown_tx: Arc::new(RwLock::new(None)),
            server_handle: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn start(&self) -> Result<ProxyServerInfo, ProxyError> {
        // 检查是否已在运行
        if self.shutdown_tx.read().await.is_some() {
            return Err(ProxyError::AlreadyRunning);
        }

        let addr: SocketAddr =
            format!("{}:{}", self.config.listen_address, self.config.listen_port)
                .parse()
                .map_err(|e| ProxyError::BindFailed(format!("无效的地址: {e}")))?;

        // 创建关闭通道
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        // 访问控制：非本机请求需要访问令牌（SEC-01 / PROXY-03）
        let access_token = self
            .state
            .db
            .get_or_create_proxy_access_token()
            .map_err(|e| ProxyError::BindFailed(format!("无法加载访问令牌: {e}")))?;
        if !addr.ip().is_loopback() {
            log::warn!(
                "[{}] 代理监听非回环地址 {addr}，来自其他主机的请求需要访问令牌",
                log_srv::STARTED
            );
        }

        // 构建路由
        let app = self.build_router(AccessGuard::new(access_token));

        // 绑定监听器
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|e| ProxyError::BindFailed(e.to_string()))?;
        let local_addr = listener
            .local_addr()
            .map_err(|e| ProxyError::BindFailed(e.to_string()))?;
        let actual_port = local_addr.port();

        log::info!("[{}] 代理服务器启动于 {local_addr}", log_srv::STARTED);

        // 更新全局代理端口，用于系统代理检测
        crate::proxy::http_client::set_proxy_port(actual_port);

        // 保存关闭句柄
        *self.shutdown_tx.write().await = Some(shutdown_tx);

        // 更新状态
        let mut status = self.state.status.write().await;
        status.running = true;
        status.address = self.config.listen_address.clone();
        status.port = actual_port;
        drop(status);

        // 记录启动时间
        *self.state.start_time.write().await = Some(std::time::Instant::now());

        // 启动服务器 — 使用手动 hyper HTTP/1.1 accept loop
        // 开启 preserve_header_case 以捕获客户端请求头的原始大小写
        let state = self.state.clone();
        let connection_slots = Arc::new(Semaphore::new(MAX_CONCURRENT_CONNECTIONS));
        let per_ip = ip_limit::PerIpLimiter::new(ip_limit::MAX_CONNECTIONS_PER_REMOTE_IP);
        let handle = tokio::spawn(async move {
            let mut shutdown_rx = shutdown_rx;
            // PROXY-14: connection tasks are tracked so stop() can drain them.
            let mut connections = tokio::task::JoinSet::new();
            let (drain_tx, drain_rx) = tokio::sync::watch::channel(false);
            loop {
                tokio::select! {
                    Some(_) = connections.join_next(), if !connections.is_empty() => {}
                    result = listener.accept() => {
                        let (stream, remote_addr) = match result {
                            Ok(v) => v,
                            Err(e) => {
                                log::error!("[{SRV}] accept 失败: {e}", SRV = log_srv::ACCEPT_ERR);
                                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                                continue;
                            }
                        };

                        // SEC-14: bound concurrently open connections.
                        let Ok(permit) = connection_slots.clone().try_acquire_owned() else {
                            log::warn!(
                                "[{SRV}] 连接数已达上限 {MAX_CONCURRENT_CONNECTIONS}，拒绝 {remote_addr}",
                                SRV = log_srv::CONN_ERR
                            );
                            drop(stream);
                            continue;
                        };
                        // One non-loopback host cannot take every slot.
                        let Some(ip_permit) = per_ip.try_acquire(remote_addr.ip()) else {
                            log::warn!(
                                "[{SRV}] {} 的连接数已达上限 {}，拒绝",
                                remote_addr.ip(),
                                ip_limit::MAX_CONNECTIONS_PER_REMOTE_IP,
                                SRV = log_srv::CONN_ERR
                            );
                            drop(stream);
                            continue;
                        };
                        let peer_info = PeerInfo {
                            remote: remote_addr,
                            local: stream.local_addr().unwrap_or(local_addr),
                        };

                        let app = app.clone();
                        let mut drain_rx = drain_rx.clone();
                        connections.spawn(async move {
                            let _permit = permit;
                            let _ip_permit = ip_permit;
                            // Wait (bounded by the header timeout) for the client to
                            // send something so silent connections cannot park here.
                            if tokio::time::timeout(HEADER_READ_TIMEOUT, stream.readable()).await.is_err() {
                                log::debug!("[ProxyServer] connection from {remote_addr} sent nothing before timeout");
                                return;
                            }
                            // Record the raw bytes hyper reads so each request's
                            // original header casing/order is captured when it is
                            // dispatched (per request, not per connection: SEC-25).
                            let recorder = header_capture::HeaderRecorder::default();
                            let recorder_io = recorder.clone();

                            // service_fn 将 axum Router（tower::Service）桥接到 hyper
                            let service = hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                                let mut router = app.clone();
                                let cases = recorder.take_request_cases(
                                    req.method().as_str(),
                                    &req.uri().to_string(),
                                );
                                log::debug!(
                                    "[ProxyServer] captured {} header casings",
                                    cases.cases.len()
                                );
                                async move {
                                    // 将 hyper::body::Incoming 转为 axum::body::Body，保留 extensions
                                    let (mut parts, body) = req.into_parts();

                                    // Insert our own header case map alongside hyper's internal one
                                    parts.extensions.insert(cases);
                                    // Connection endpoints for the access guard
                                    parts.extensions.insert(peer_info);

                                    let body = axum::body::Body::new(body);
                                    let axum_req = http::Request::from_parts(parts, body);
                                    <Router as tower::Service<http::Request<axum::body::Body>>>::call(&mut router, axum_req).await
                                }
                            });

                            let builder = {
                                let mut builder = hyper::server::conn::http1::Builder::new();
                                builder
                                    .preserve_header_case(true)
                                    .timer(TokioTimer::new())
                                    .header_read_timeout(HEADER_READ_TIMEOUT);
                                builder
                            };
                            let conn = builder.serve_connection(
                                TokioIo::new(header_capture::RecordingIo::new(stream, recorder_io)),
                                service,
                            );
                            tokio::pin!(conn);
                            let served = tokio::select! {
                                result = conn.as_mut() => result,
                                _ = drain_rx.changed() => {
                                    // Finish the in-flight request, then close.
                                    conn.as_mut().graceful_shutdown();
                                    conn.as_mut().await
                                }
                            };
                            if let Err(e) = served {
                                // Connection reset / broken pipe 等在代理场景下很常见，debug 级别
                                log::debug!("[{SRV}] connection error: {e}", SRV = log_srv::CONN_ERR);
                            }
                        });
                    }
                    _ = &mut shutdown_rx => {
                        break;
                    }
                }
            }

            // Stop accepting (frees the port), then drain open connections.
            drop(listener);
            let open = connections.len();
            let _ = drain_tx.send(true);
            if open > 0 {
                log::info!(
                    "[{}] 等待 {open} 个连接完成（最多 {}s）",
                    log_srv::STOPPED,
                    DRAIN_TIMEOUT.as_secs()
                );
                let drained = tokio::time::timeout(DRAIN_TIMEOUT, async {
                    while connections.join_next().await.is_some() {}
                })
                .await;
                if drained.is_err() {
                    log::warn!(
                        "[{}] {} 个连接未在 {}s 内结束，已中断",
                        log_srv::STOP_TIMEOUT,
                        connections.len(),
                        DRAIN_TIMEOUT.as_secs()
                    );
                    connections.abort_all();
                    while connections.join_next().await.is_some() {}
                }
            }

            // 服务器停止后更新状态
            state.status.write().await.running = false;
            *state.start_time.write().await = None;
        });

        // 保存服务器任务句柄
        *self.server_handle.write().await = Some(handle);

        Ok(ProxyServerInfo {
            address: self.config.listen_address.clone(),
            port: actual_port,
            started_at: chrono::Utc::now().to_rfc3339(),
        })
    }

    /// Stop the server: close the listener at once, let in-flight requests finish
    /// for up to [`DRAIN_TIMEOUT`], then abort the rest (PROXY-14).
    ///
    /// A drain that overruns is not an error: the listener is already closed and
    /// the remaining connections are aborted, so callers (config updates, restarts)
    /// can proceed.
    pub async fn stop(&self) -> Result<(), ProxyError> {
        // 1. 发送关闭信号
        if let Some(tx) = self.shutdown_tx.write().await.take() {
            let _ = tx.send(());
        } else {
            return Err(ProxyError::NotRunning);
        }

        // 2. 等待 accept 循环与连接排空
        let Some(mut handle) = self.server_handle.write().await.take() else {
            return Ok(());
        };
        match tokio::time::timeout(STOP_TIMEOUT, &mut handle).await {
            Ok(Ok(())) => {
                log::info!("[{}] 代理服务器已完全停止", log_srv::STOPPED);
                Ok(())
            }
            Ok(Err(e)) => {
                log::warn!("[{}] 代理服务器任务异常终止: {e}", log_srv::TASK_ERROR);
                Err(ProxyError::StopFailed(e.to_string()))
            }
            Err(_) => {
                log::warn!(
                    "[{}] 代理服务器停止超时（{}秒），已强制中断",
                    log_srv::STOP_TIMEOUT,
                    STOP_TIMEOUT.as_secs()
                );
                handle.abort();
                let _ = handle.await;
                self.state.status.write().await.running = false;
                *self.state.start_time.write().await = None;
                Ok(())
            }
        }
    }

    pub async fn get_status(&self) -> ProxyStatus {
        let mut status = self.state.status_snapshot().await;

        // 计算运行时间
        if let Some(start) = *self.state.start_time.read().await {
            status.uptime_seconds = start.elapsed().as_secs();
        }

        // 从 current_providers HashMap 获取每个应用类型当前正在使用的 provider
        let current_providers = self.state.current_providers.read().await;
        status.active_targets = current_providers
            .iter()
            .map(|(app_type, (provider_id, provider_name))| ActiveTarget {
                app_type: app_type.clone(),
                provider_id: provider_id.clone(),
                provider_name: provider_name.clone(),
            })
            .collect();

        status
    }

    /// 更新某个应用类型当前“目标供应商”（用于 UI 展示 active_targets）
    ///
    /// 注意：这不代表该供应商一定已经处理过请求，而是用于“热切换/启用故障转移立即切 P1”
    /// 等场景下，让 UI 能立刻反映最新目标。
    pub async fn set_active_target(&self, app_type: &str, provider_id: &str, provider_name: &str) {
        let mut current_providers = self.state.current_providers.write().await;
        current_providers.insert(
            app_type.to_string(),
            (provider_id.to_string(), provider_name.to_string()),
        );
    }

    fn build_router(&self, guard: AccessGuard) -> Router {
        Router::new()
            // 健康检查
            .route("/health", get(handlers::health_check))
            .route("/status", get(status_endpoint))
            // Prometheus text metrics (PROXY-20)
            .route("/metrics", get(metrics_endpoint))
            // Claude API (支持带前缀和不带前缀两种格式)
            .route("/v1/messages", post(handlers::handle_messages))
            .route("/claude/v1/messages", post(handlers::handle_messages))
            // Claude Desktop 3P 本地 gateway（独立 provider namespace）
            .route(
                "/claude-desktop/v1/models",
                get(handlers::handle_claude_desktop_models),
            )
            .route(
                "/claude-desktop/v1/messages",
                post(handlers::handle_claude_desktop_messages),
            )
            // OpenAI Chat Completions API (Codex CLI，支持带前缀和不带前缀)
            .route("/chat/completions", post(handlers::handle_chat_completions))
            .route(
                "/v1/chat/completions",
                post(handlers::handle_chat_completions),
            )
            .route(
                "/v1/v1/chat/completions",
                post(handlers::handle_chat_completions),
            )
            .route(
                "/codex/v1/chat/completions",
                post(handlers::handle_chat_completions),
            )
            // OpenAI Models API (Codex CLI reachability check)
            .route("/models", get(handlers::handle_models))
            .route("/v1/models", get(handlers::handle_models))
            // OpenAI Responses API (Codex CLI，支持带前缀和不带前缀)
            .route("/responses", post(handlers::handle_responses))
            .route("/v1/responses", post(handlers::handle_responses))
            .route("/v1/v1/responses", post(handlers::handle_responses))
            .route("/codex/v1/responses", post(handlers::handle_responses))
            // Grok Build uses the Responses protocol but has an independent
            // provider namespace and failover queue.
            .route(
                "/grokbuild/v1/responses",
                post(handlers::handle_grokbuild_responses),
            )
            // OpenAI Responses Compact API (Codex CLI 远程压缩，透传)
            .route(
                "/responses/compact",
                post(handlers::handle_responses_compact),
            )
            .route(
                "/v1/responses/compact",
                post(handlers::handle_responses_compact),
            )
            .route(
                "/v1/v1/responses/compact",
                post(handlers::handle_responses_compact),
            )
            .route(
                "/codex/v1/responses/compact",
                post(handlers::handle_responses_compact),
            )
            .route(
                "/grokbuild/v1/responses/compact",
                post(handlers::handle_grokbuild_responses_compact),
            )
            // Gemini API (支持带前缀和不带前缀)
            //
            // 用 `any(..)` 覆盖所有 HTTP 方法：除了 POST `:generateContent` /
            // `:streamGenerateContent` / `:countTokens` 之外，Gemini SDK / CLI 还会发
            // GET `/models`、GET `/models/<id>` 等只读端点。如果只挂 POST，这些 GET
            // 请求会在路由层 404，绕过本地代理的统计、整流和故障转移。
            .route("/v1beta/{*path}", any(handlers::handle_gemini))
            .route("/gemini/v1beta/{*path}", any(handlers::handle_gemini))
            // Gemini 的 GA 版本也叫 /v1，给原 SDK 留一条出口
            .route("/gemini/v1/{*path}", any(handlers::handle_gemini))
            // 请求体大小限制（SEC-14）；access guard 还会对原始 body 施加同样的上限
            .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES))
            // 访问控制：Origin/Host 校验 + 非本机请求的访问令牌（在读取 body 之前执行）
            .layer(axum::middleware::from_fn_with_state(
                guard,
                access_guard::enforce,
            ))
            .with_state(self.state.clone())
    }

    /// 在不重启服务的情况下更新运行时配置
    pub async fn apply_runtime_config(&self, config: &ProxyConfig) {
        let mut guard = self.state.config.write().await;
        *guard = config.clone();
        self.state
            .logging_enabled
            .store(config.enable_logging, std::sync::atomic::Ordering::Relaxed);
    }

    /// 热更新熔断器配置
    ///
    /// 将新配置应用到所有已创建的熔断器实例
    pub async fn update_circuit_breaker_configs(
        &self,
        config: super::circuit_breaker::CircuitBreakerConfig,
    ) {
        self.state.provider_router.update_all_configs(config).await;
    }

    pub async fn update_circuit_breaker_config_for_app(
        &self,
        app_type: &str,
        config: super::circuit_breaker::CircuitBreakerConfig,
    ) {
        self.state
            .provider_router
            .update_app_configs(app_type, config)
            .await;
    }

    /// 重置指定 Provider 的熔断器
    pub async fn reset_provider_circuit_breaker(&self, provider_id: &str, app_type: &str) {
        self.state
            .provider_router
            .reset_provider_breaker(provider_id, app_type)
            .await;
    }
}

impl Drop for ProxyServer {
    /// A server dropped without `stop()` still releases its port: signal the
    /// accept loop, which closes the listener and drains connections (PROXY-14).
    fn drop(&mut self) {
        if let Ok(mut tx) = self.shutdown_tx.try_write() {
            if let Some(tx) = tx.take() {
                let _ = tx.send(());
            }
        }
    }
}

/// `/status`: full details (provider names/ids, last error) only for requests that
/// presented the access token; everyone else gets liveness information only. SEC-15.
async fn status_endpoint(
    State(state): State<ProxyState>,
    ctx: Option<Extension<AccessContext>>,
) -> axum::response::Response {
    let authenticated = ctx.is_some_and(|Extension(ctx)| ctx.token_authenticated);
    if authenticated {
        return match handlers::get_status(State(state)).await {
            Ok(json) => json.into_response(),
            Err(e) => e.into_response(),
        };
    }
    let status = state.status.read().await;
    Json(public_status(&status)).into_response()
}

/// `/metrics`: Prometheus text exposition of the proxy counters, per-provider
/// attempt counters / latency histograms and circuit-breaker states.
///
/// Provider ids and names are not public, so the full body is only served to
/// token-authenticated callers or peers on this machine.
async fn metrics_endpoint(
    State(state): State<ProxyState>,
    ctx: Option<Extension<AccessContext>>,
    peer: Option<Extension<PeerInfo>>,
) -> axum::response::Response {
    let authenticated = ctx.is_some_and(|Extension(ctx)| ctx.token_authenticated);
    let local = peer.is_some_and(|Extension(p)| p.remote.ip().to_canonical().is_loopback());
    if !(authenticated || local) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            "metrics require the CC Switch access token\n",
        )
            .into_response();
    }

    let mut breakers = Vec::new();
    for (key, breaker) in super::circuit_breaker::registered_breakers() {
        let Some((app, provider_id)) = super::circuit_breaker::split_breaker_key(&key) else {
            continue;
        };
        let state = match breaker.get_state().await {
            super::circuit_breaker::CircuitState::Closed => 0,
            super::circuit_breaker::CircuitState::HalfOpen => 1,
            super::circuit_breaker::CircuitState::Open => 2,
        };
        breakers.push(super::pipeline::metrics::BreakerGauge {
            app_type: app.to_string(),
            provider_id: provider_id.to_string(),
            state,
        });
    }
    breakers.sort_by(|a, b| (&a.app_type, &a.provider_id).cmp(&(&b.app_type, &b.provider_id)));

    let uptime = state
        .start_time
        .read()
        .await
        .map(|t| t.elapsed().as_secs())
        .unwrap_or(0);
    let history = state.codex_chat_history.stats().await;
    let body = state.metrics.render_prometheus(
        &breakers,
        &[
            (
                "ccswitch_proxy_uptime_seconds",
                "Seconds since the proxy server started.",
                uptime,
            ),
            (
                "ccswitch_codex_chat_history_responses",
                "Responses held in the Codex chat bridge history cache.",
                history.responses as u64,
            ),
            (
                "ccswitch_codex_chat_history_bytes",
                "Approximate bytes held in the Codex chat bridge history cache.",
                history.bytes as u64,
            ),
        ],
    );
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
        .into_response()
}

fn public_status(status: &ProxyStatus) -> serde_json::Value {
    serde_json::json!({
        "running": status.running,
        "port": status.port,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_status_hides_provider_details() {
        let status = ProxyStatus {
            running: true,
            port: 15721,
            current_provider: Some("My Relay".into()),
            current_provider_id: Some("p1".into()),
            last_error: Some("Provider My Relay 失败: secret body".into()),
            active_targets: vec![ActiveTarget {
                app_type: "claude".into(),
                provider_name: "My Relay".into(),
                provider_id: "p1".into(),
            }],
            ..Default::default()
        };
        let value = public_status(&status);
        assert_eq!(value, serde_json::json!({"running": true, "port": 15721}));
        assert!(!value.to_string().contains("My Relay"));
    }

    fn test_server() -> ProxyServer {
        let db = Arc::new(Database::memory().expect("memory db"));
        let config = ProxyConfig {
            listen_address: "127.0.0.1".into(),
            listen_port: 0,
            ..ProxyConfig::default()
        };
        ProxyServer::new(config, db, None)
    }

    async fn wait_until_port_is_free(port: u16) -> bool {
        for _ in 0..50 {
            if tokio::net::TcpListener::bind(("127.0.0.1", port))
                .await
                .is_ok()
            {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        false
    }

    #[tokio::test]
    async fn stop_closes_the_listener_and_aborts_hanging_connections_within_budget() {
        let server = test_server();
        let info = server.start().await.expect("start");
        // An idle client connection that never sends a request.
        let _idle = tokio::net::TcpStream::connect(("127.0.0.1", info.port))
            .await
            .expect("connect");
        tokio::time::sleep(Duration::from_millis(50)).await;

        let started = std::time::Instant::now();
        server
            .stop()
            .await
            .expect("stop is not an error even when draining");
        assert!(started.elapsed() < STOP_TIMEOUT + Duration::from_secs(1));
        assert!(wait_until_port_is_free(info.port).await, "port still bound");
        assert!(!server.get_status().await.running);
    }

    /// axum 0.8 changed wildcard syntax from `/*path` to `/{*path}` and panics
    /// on the old form when the router is built; this pins that every Gemini
    /// route still reaches `handle_gemini` instead of the 404 fallback.
    #[tokio::test]
    async fn gemini_wildcard_routes_match_nested_paths() {
        const TOKEN: &str = "ccsp-test-token-0123456789abcdef";
        let server = test_server();
        let status_of = |path: &'static str| {
            let mut router = server.build_router(AccessGuard::new(TOKEN.to_string()));
            async move {
                let req = axum::extract::Request::builder()
                    .method("POST")
                    .uri(path)
                    .header(access_guard::ACCESS_TOKEN_HEADER, TOKEN)
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from("{}"))
                    .unwrap();
                let resp =
                    <Router as tower::Service<axum::extract::Request>>::call(&mut router, req)
                        .await
                        .unwrap_or_else(|never| match never {});
                resp.status()
            }
        };
        for path in [
            "/v1beta/models/gemini-2.5-pro:generateContent",
            "/gemini/v1beta/models/gemini-2.5-pro:streamGenerateContent",
            "/gemini/v1/models/gemini-2.5-pro:countTokens",
        ] {
            assert_ne!(
                status_of(path).await,
                axum::http::StatusCode::NOT_FOUND,
                "{path} must reach the Gemini handler"
            );
        }
        assert_eq!(
            status_of("/v1beta-unknown/x").await,
            axum::http::StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn dropping_a_running_server_frees_its_port() {
        let server = test_server();
        let info = server.start().await.expect("start");
        drop(server);
        assert!(wait_until_port_is_free(info.port).await, "port still bound");
    }
}
