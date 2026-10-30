use super::*;
use crate::database::Database;
use crate::proxy::pipeline::transport::should_preserve_exact_header_case;
use axum::http::HeaderMap;
use http::StatusCode;
use serde_json::json;
use std::collections::HashMap;
use std::time::Duration;

impl RequestForwarder {
    /// 直接调用单次尝试（不经故障转移循环）
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn forward_once(
        &self,
        app_type: &AppType,
        method: &http::Method,
        provider: &Provider,
        endpoint: &str,
        body: &Value,
        headers: &axum::http::HeaderMap,
        extensions: &Extensions,
        adapter: &dyn ProviderAdapter,
    ) -> Result<ForwardSuccess, ProxyError> {
        let request = ClientRequest {
            app_type,
            method,
            endpoint,
            body,
            headers,
            extensions,
        };
        self.forward(&request, provider, adapter, &mut None).await
    }
}

fn test_forwarder(
    non_streaming_timeout: Duration,
    streaming_first_byte_timeout: Duration,
) -> RequestForwarder {
    let db = Arc::new(Database::memory().expect("memory db"));

    RequestForwarder::new(
        Arc::new(ForwarderDeps {
            router: Arc::new(ProviderRouter::new(db.clone())),
            status: Arc::new(RwLock::new(ProxyStatus::default())),
            current_providers: Arc::new(RwLock::new(HashMap::new())),
            gemini_shadow: Arc::new(GeminiShadowStore::new()),
            codex_chat_history: Arc::new(CodexChatHistoryStore::default()),
            failover_manager: Arc::new(FailoverSwitchManager::new(db)),
            app_handle: None,
        }),
        RequestScope {
            client_format: WireFormat::AnthropicMessages,
            current_provider_id_at_start: String::new(),
            session_id: String::new(),
            session_client_provided: false,
            rectifier_config: RectifierConfig::default(),
            optimizer_config: OptimizerConfig::default(),
            copilot_optimizer_config: CopilotOptimizerConfig::default(),
            non_streaming_timeout,
            streaming_first_byte_timeout,
            max_attempts: 1,
            capture: None,
            trace: None,
        },
    )
}

#[tokio::test]
async fn cancelled_half_open_probe_releases_the_permit() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let _ = rustls::crypto::ring::default_provider().install_default();

    // 挂起的上游：接受连接但从不响应
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind hanging upstream");
    let addr = listener.local_addr().expect("hanging upstream address");
    let connections = Arc::new(AtomicUsize::new(0));
    let accepted = connections.clone();
    let upstream = tokio::spawn(async move {
        let mut open = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            accepted.fetch_add(1, Ordering::SeqCst);
            open.push(stream);
        }
    });

    let db = Arc::new(Database::memory().expect("memory db"));
    db.get_proxy_config_for_app("claude")
        .await
        .expect("create proxy config");
    db.update_circuit_breaker_config(&crate::proxy::circuit_breaker::CircuitBreakerConfig {
        failure_threshold: 1,
        timeout_seconds: 0,
        ..Default::default()
    })
    .await
    .expect("write circuit config");

    let make_provider = |id: &str| {
        Provider::with_id(
            id.to_string(),
            format!("Provider {id}"),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": format!("http://{addr}"),
                    "ANTHROPIC_AUTH_TOKEN": "sk-test",
                }
            }),
            None,
        )
    };
    let providers = vec![make_provider("a"), make_provider("b")];
    for provider in &providers {
        db.save_provider("claude", provider).expect("save provider");
    }

    let router = Arc::new(ProviderRouter::new(db));
    let mut forwarder = test_forwarder(Duration::from_secs(30), Duration::from_secs(30));
    Arc::get_mut(&mut forwarder.shared)
        .expect("test forwarder deps are not shared yet")
        .router = router.clone();
    forwarder.req.max_attempts = 2;

    // A 熔断（timeout=0，下一次请求即进入 HalfOpen 探测）
    router
        .record_result("a", "claude", None, false, Some("boom".to_string()))
        .await
        .expect("record failure");

    let forward = forwarder.forward_with_retry(
        &AppType::Claude,
        http::Method::POST,
        "/v1/messages",
        json!({
            "model": "claude-sonnet-4-5",
            "max_tokens": 16,
            "messages": [{ "role": "user", "content": "hi" }]
        }),
        HeaderMap::new(),
        Extensions::new(),
        providers,
    );
    let cancelled = tokio::time::timeout(Duration::from_millis(300), forward).await;
    assert!(
        cancelled.is_err(),
        "forward against a hanging upstream must time out"
    );
    assert!(
        connections.load(Ordering::SeqCst) >= 1,
        "the half-open probe must have reached the upstream"
    );

    let next = router.allow_provider_request("a", "claude").await;
    assert!(
        next.allowed,
        "cancelling the probe must release the half-open permit"
    );
    assert!(next.used_half_open_permit());

    upstream.abort();
}

#[tokio::test]
async fn streaming_raw_path_uses_first_byte_timeout_for_headers() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    // 上游接受连接但从不返回响应头
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind silent upstream");
    let addr = listener.local_addr().expect("silent upstream address");
    let upstream = tokio::spawn(async move {
        let mut open = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            open.push(stream);
        }
    });

    let mut provider = Provider::with_id(
        "raw".to_string(),
        "Raw Claude".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": format!("http://{addr}"),
                "ANTHROPIC_AUTH_TOKEN": "sk-test",
            }
        }),
        None,
    );
    provider.meta = Some(crate::provider::ProviderMeta {
        api_format: Some("anthropic".to_string()),
        ..Default::default()
    });
    assert!(
        should_preserve_exact_header_case(
            "Claude",
            &provider,
            Some(ClaudeApiFormat::Anthropic),
            false
        ),
        "an anthropic-format Claude provider must use the raw hyper path"
    );

    let forwarder = test_forwarder(Duration::from_secs(30), Duration::from_secs(1));
    let adapter = get_adapter(&AppType::Claude);
    let mut extensions = Extensions::new();
    extensions.insert(
        crate::proxy::hyper_client::OriginalHeaderCases::from_raw_bytes(
            b"POST /v1/messages HTTP/1.1\r\nContent-Type: application/json\r\n\r\n",
        ),
    );
    let body = json!({
        "model": "claude-sonnet-4-5",
        "max_tokens": 16,
        "stream": true,
        "messages": [{ "role": "user", "content": "hi" }]
    });

    let started = std::time::Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        forwarder.forward_once(
            &AppType::Claude,
            &http::Method::POST,
            &provider,
            "/v1/messages",
            &body,
            &HeaderMap::new(),
            &extensions,
            adapter.as_ref(),
        ),
    )
    .await
    .expect("streaming header wait must be bounded by the first-byte timeout");
    let elapsed = started.elapsed();

    assert!(
        matches!(result, Err(ProxyError::Timeout(_))),
        "expected ProxyError::Timeout, got {:?}",
        result.err()
    );
    assert!(
        elapsed < Duration::from_secs(3),
        "first-byte timeout must fire within about 2s, took {elapsed:?}"
    );

    upstream.abort();
}

/// 本地上游：前 `failures` 次返回 500，之后返回 200
async fn spawn_flaky_upstream(
    failures: usize,
) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let remaining = Arc::new(AtomicUsize::new(failures));
    let app = axum::Router::new().route(
        "/v1/messages",
        axum::routing::post(move || {
            let remaining = remaining.clone();
            async move {
                let failed = remaining
                    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                    .is_ok();
                if failed {
                    (StatusCode::INTERNAL_SERVER_ERROR, "upstream boom")
                } else {
                    (StatusCode::OK, r#"{"type":"message","content":[]}"#)
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind flaky upstream");
    let addr = listener.local_addr().expect("flaky upstream address");
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (addr, handle)
}

fn upstream_provider(id: &str, addr: std::net::SocketAddr) -> Provider {
    Provider::with_id(
        id.to_string(),
        format!("Provider {id}"),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": format!("http://{addr}"),
                "ANTHROPIC_AUTH_TOKEN": "sk-test",
            }
        }),
        None,
    )
}

/// 与 `deps` 共享状态的 forwarder，模拟请求开始时当前供应商为 `current_at_start`
fn failover_forwarder(deps: &Arc<ForwarderDeps>, current_at_start: &str) -> RequestForwarder {
    let mut forwarder = test_forwarder(Duration::from_secs(30), Duration::from_secs(30));
    forwarder.shared = deps.clone();
    forwarder.req.max_attempts = 2;
    forwarder.req.current_provider_id_at_start = current_at_start.to_string();
    forwarder
}

async fn send(forwarder: &RequestForwarder, providers: &[Provider]) -> String {
    let result = forwarder
        .forward_with_retry(
            &AppType::Claude,
            http::Method::POST,
            "/v1/messages",
            json!({
                "model": "claude-sonnet-4-5",
                "max_tokens": 16,
                "messages": [{ "role": "user", "content": "hi" }]
            }),
            HeaderMap::new(),
            Extensions::new(),
            providers.to_vec(),
        )
        .await;
    match result {
        Ok(result) => result.provider.id,
        Err(e) => panic!("request must succeed, got {:?}", e.error),
    }
}

#[tokio::test]
async fn transient_failure_counts_one_failover_and_return_counts_recovery() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let (a_addr, a_upstream) = spawn_flaky_upstream(1).await;
    let (b_addr, b_upstream) = spawn_flaky_upstream(0).await;
    let providers = vec![
        upstream_provider("a", a_addr),
        upstream_provider("b", b_addr),
    ];
    let deps = test_forwarder(Duration::ZERO, Duration::ZERO).shared;

    // 请求 1：当前供应商 P1，P1 返回 5xx，P2 接手
    assert_eq!(send(&failover_forwarder(&deps, "a"), &providers).await, "b");
    assert_eq!(deps.status.read().await.failover_count, 1);
    assert_eq!(deps.status.read().await.recovery_count, 0);

    // 请求 2：热切换已把当前供应商改为 P2，P1 首个尝试即成功 —— 恢复而不是故障转移
    assert_eq!(send(&failover_forwarder(&deps, "b"), &providers).await, "a");
    let status = deps.status.read().await;
    assert_eq!(status.failover_count, 1);
    assert_eq!(status.recovery_count, 1);
    assert_eq!(status.success_requests, 2);
    drop(status);

    // 请求 3：仍停留在 P1，不再计数
    assert_eq!(send(&failover_forwarder(&deps, "a"), &providers).await, "a");
    let status = deps.status.read().await;
    assert_eq!(status.failover_count, 1);
    assert_eq!(status.recovery_count, 1);

    a_upstream.abort();
    b_upstream.abort();
}

#[tokio::test]
async fn parallel_failovers_before_the_switch_lands_count_once() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    const N: usize = 8;
    let (a_addr, a_upstream) = spawn_flaky_upstream(usize::MAX).await;
    let (b_addr, b_upstream) = spawn_flaky_upstream(0).await;
    let providers = vec![
        upstream_provider("a", a_addr),
        upstream_provider("b", b_addr),
    ];
    let deps = test_forwarder(Duration::ZERO, Duration::ZERO).shared;

    // 所有请求都在异步切换落地前开始：当前供应商仍是 P1
    let forwarders: Vec<_> = (0..N).map(|_| failover_forwarder(&deps, "a")).collect();
    let served = futures::future::join_all(forwarders.iter().map(|f| send(f, &providers))).await;

    assert!(served.iter().all(|id| id == "b"), "served by {served:?}");
    let status = deps.status.read().await;
    assert_eq!(status.success_requests, N as u64);
    assert_eq!(status.failover_count, 1);
    assert_eq!(status.recovery_count, 0);

    a_upstream.abort();
    b_upstream.abort();
}
