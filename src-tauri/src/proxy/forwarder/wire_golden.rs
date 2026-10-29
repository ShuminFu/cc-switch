//! 出站请求线上字节的 golden 测试
//!
//! 对若干典型路由（原生 Claude raw 写、Claude→Chat 转换、Codex→Anthropic 伪装、
//! Codex→Chat、GET 无 body）调用 `forward()`，在本地 TCP 上游抓取完整请求字节
//! （请求行、头部顺序与大小写、body），与 `tests/fixtures/forwarder/upstream_request.expected`
//! 逐字节比对。请求管线的任何重构都必须保持这些字节不变。
//!
//! 有意的行为变更后用 `CC_SWITCH_UPDATE_EXPECTED=1 cargo test --lib wire_golden` 重新生成并审阅 diff。

use super::{ForwarderDeps, RequestForwarder, RequestScope};
use crate::app_config::AppType;
use crate::database::Database;
use crate::provider::{LocalProxyRequestOverrides, Provider, ProviderMeta};
use crate::proxy::failover_switch::FailoverSwitchManager;
use crate::proxy::hyper_client::OriginalHeaderCases;
use crate::proxy::protocol::WireFormat;
use crate::proxy::provider_router::ProviderRouter;
use crate::proxy::providers::{
    codex_chat_history::CodexChatHistoryStore, gemini_shadow::GeminiShadowStore, get_adapter,
};
use crate::proxy::types::{CopilotOptimizerConfig, OptimizerConfig, ProxyStatus, RectifierConfig};
use http::{Extensions, HeaderMap, HeaderName, HeaderValue};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::RwLock;

const REPLY_BODY: &str =
    r#"{"type":"message","id":"msg_1","content":[],"status":"completed","output":[]}"#;

fn expected_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/forwarder/upstream_request.expected")
}

/// 本地上游：记录每个请求的原始字节，统一回 200 JSON
async fn spawn_capture_upstream() -> (SocketAddr, Arc<Mutex<Vec<Vec<u8>>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind capture upstream");
    let addr = listener.local_addr().expect("capture upstream address");
    let captured = Arc::new(Mutex::new(Vec::new()));
    let sink = captured.clone();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let sink = sink.clone();
            tokio::spawn(async move {
                let raw = read_request(&mut stream).await;
                sink.lock().unwrap().push(raw);
                let reply = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{REPLY_BODY}",
                    REPLY_BODY.len()
                );
                let _ = stream.write_all(reply.as_bytes()).await;
                let _ = stream.shutdown().await;
            });
        }
    });
    (addr, captured)
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> Vec<u8> {
    let mut raw = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = stream.read(&mut buf).await.expect("read request");
        if n == 0 {
            return raw;
        }
        raw.extend_from_slice(&buf[..n]);
        let Some(head_end) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
            continue;
        };
        let head = String::from_utf8_lossy(&raw[..head_end]).to_ascii_lowercase();
        let content_length = head
            .lines()
            .find_map(|line| line.strip_prefix("content-length:"))
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        if raw.len() >= head_end + 4 + content_length {
            return raw;
        }
    }
}

fn forwarder(client_format: WireFormat) -> RequestForwarder {
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
            client_format,
            current_provider_id_at_start: String::new(),
            session_id: String::new(),
            session_client_provided: false,
            rectifier_config: RectifierConfig::default(),
            optimizer_config: OptimizerConfig::default(),
            copilot_optimizer_config: CopilotOptimizerConfig::default(),
            non_streaming_timeout: Duration::from_secs(10),
            streaming_first_byte_timeout: Duration::from_secs(10),
            max_attempts: 1,
            capture: None,
        },
    )
}

fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in pairs {
        map.append(
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        );
    }
    map
}

struct Case {
    name: &'static str,
    app_type: AppType,
    method: http::Method,
    endpoint: &'static str,
    provider: Provider,
    body: Value,
    headers: HeaderMap,
    extensions: Extensions,
}

fn claude_provider(addr: SocketAddr, meta: ProviderMeta) -> Provider {
    let mut provider = Provider::with_id(
        "claude-p".to_string(),
        "Claude P".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": format!("http://{addr}"),
                "ANTHROPIC_AUTH_TOKEN": "sk-upstream",
            }
        }),
        None,
    );
    provider.meta = Some(meta);
    provider
}

fn codex_provider(addr: SocketAddr, base_path: &str, meta: ProviderMeta) -> Provider {
    let mut provider = Provider::with_id(
        "codex-p".to_string(),
        "Codex P".to_string(),
        json!({
            "base_url": format!("http://{addr}{base_path}"),
            "env": { "OPENAI_API_KEY": "sk-codex-upstream" }
        }),
        None,
    );
    provider.meta = Some(meta);
    provider
}

fn cases(addr: SocketAddr) -> Vec<Case> {
    let claude_body = json!({
        "model": "claude-sonnet-4-5",
        "max_tokens": 64,
        "_private": "drop me",
        "metadata": { "user_id": "u1" },
        "messages": [{ "role": "user", "content": "hi" }]
    });
    let mut raw_cases = Extensions::new();
    raw_cases.insert(OriginalHeaderCases::from_raw_bytes(
        b"POST /v1/messages HTTP/1.1\r\nHost: client.local\r\nUser-Agent: claude-cli/2.0\r\nX-Api-Key: sk-client\r\nAnthropic-Version: 2023-06-01\r\nAnthropic-Beta: fine-grained-tool-streaming-2025-05-14\r\nAccept-Encoding: gzip, br\r\nX-Custom: 1\r\nContent-Type: application/json\r\n\r\n",
    ));

    vec![
        Case {
            name: "claude_native_raw_write",
            app_type: AppType::Claude,
            method: http::Method::POST,
            endpoint: "/v1/messages?beta=true",
            provider: claude_provider(
                addr,
                ProviderMeta {
                    api_format: Some("anthropic".to_string()),
                    ..Default::default()
                },
            ),
            body: claude_body.clone(),
            headers: headers(&[
                ("host", "client.local"),
                ("user-agent", "claude-cli/2.0"),
                ("x-api-key", "sk-client"),
                ("authorization", "Bearer client"),
                ("anthropic-version", "2023-06-01"),
                ("anthropic-beta", "fine-grained-tool-streaming-2025-05-14"),
                ("accept-encoding", "gzip, br"),
                ("x-request-id", "req-1"),
                ("traceparent", "00-abc-def-01"),
                ("cf-ray", "ray"),
                ("x-custom", "1"),
                ("content-type", "application/json"),
                ("content-length", "999"),
            ]),
            extensions: raw_cases,
        },
        Case {
            name: "claude_to_openai_chat_with_overrides",
            app_type: AppType::Claude,
            method: http::Method::POST,
            endpoint: "/v1/messages?beta=true&x-id=1",
            provider: claude_provider(
                addr,
                ProviderMeta {
                    api_format: Some("openai_chat".to_string()),
                    custom_user_agent: Some("my-agent/1.0".to_string()),
                    local_proxy_request_overrides: Some(LocalProxyRequestOverrides {
                        headers: HashMap::from([
                            ("X-Extra".to_string(), "1".to_string()),
                            ("Authorization".to_string(), "Bearer bad".to_string()),
                        ]),
                        body: Some(json!({ "temperature": 0.5, "stream": true })),
                    }),
                    ..Default::default()
                },
            ),
            body: claude_body.clone(),
            headers: headers(&[
                ("user-agent", "claude-cli/2.0"),
                ("accept", "application/json"),
                ("accept-encoding", "gzip"),
                ("anthropic-version", "2023-06-01"),
                ("anthropic-beta", "fine-grained-tool-streaming-2025-05-14"),
                ("x-api-key", "sk-client"),
                ("x-stainless-lang", "js"),
            ]),
            extensions: Extensions::new(),
        },
        Case {
            name: "claude_native_get_without_body",
            app_type: AppType::Claude,
            method: http::Method::GET,
            endpoint: "/v1/models",
            provider: claude_provider(
                addr,
                ProviderMeta {
                    api_format: Some("anthropic".to_string()),
                    ..Default::default()
                },
            ),
            body: json!({}),
            headers: headers(&[("accept", "application/json"), ("x-api-key", "sk-client")]),
            extensions: Extensions::new(),
        },
        Case {
            name: "codex_to_anthropic_impersonating_claude_code",
            app_type: AppType::Codex,
            method: http::Method::POST,
            endpoint: "/v1/responses?x=1",
            provider: codex_provider(
                addr,
                "",
                ProviderMeta {
                    api_format: Some("anthropic".to_string()),
                    impersonate_claude_code: Some(true),
                    max_output_tokens: Some(4096),
                    ..Default::default()
                },
            ),
            body: json!({
                "model": "claude-sonnet-4-5[1m]",
                "instructions": "Be terse",
                "input": "hi",
                "stream": false
            }),
            headers: headers(&[
                ("user-agent", "codex_cli_rs/0.1"),
                ("accept", "text/event-stream"),
                ("originator", "codex_cli_rs"),
                ("session_id", "s1"),
                ("x-app", "codex"),
                ("openai-beta", "responses=v1"),
                ("x-stainless-runtime", "rust"),
                ("authorization", "Bearer client"),
                ("accept-encoding", "gzip"),
                ("accept", "application/xml"),
            ]),
            extensions: Extensions::new(),
        },
        Case {
            name: "codex_to_anthropic_plain_full_endpoint",
            app_type: AppType::Codex,
            method: http::Method::POST,
            endpoint: "/responses",
            provider: codex_provider(
                addr,
                "/v1/messages",
                ProviderMeta {
                    api_format: Some("anthropic".to_string()),
                    api_key_field: Some("ANTHROPIC_API_KEY".to_string()),
                    ..Default::default()
                },
            ),
            body: json!({ "model": "claude-haiku-4-5", "input": "hi" }),
            headers: headers(&[
                ("anthropic-version", "2024-01-01"),
                ("x-app", "codex"),
                ("user-agent", "codex_cli_rs/0.1"),
            ]),
            extensions: Extensions::new(),
        },
        Case {
            name: "codex_to_openai_chat",
            app_type: AppType::Codex,
            method: http::Method::POST,
            endpoint: "/v1/responses?x=1",
            provider: codex_provider(
                addr,
                "/v1",
                ProviderMeta {
                    api_format: Some("openai_chat".to_string()),
                    ..Default::default()
                },
            ),
            body: json!({
                "model": "gpt-5",
                "instructions": "Be terse",
                "input": [{ "role": "user", "content": "hi" }],
                "_internal": 1
            }),
            headers: headers(&[
                ("host", "client.local"),
                ("authorization", "Bearer client"),
                ("originator", "codex_cli_rs"),
                ("accept-encoding", "gzip"),
                ("user-agent", "codex_cli_rs/0.1"),
                ("x-b3-traceid", "t"),
            ]),
            extensions: Extensions::new(),
        },
    ]
}

/// 抓到的请求只用 CRLF 换行（JSON body 不含换行），这里把 CRLF 写成 LF，
/// 使期望文件经 git 换行归一化后仍可逐字节比对；任何裸 CR / LF 都会让断言失败
fn render(case_name: &str, raw: &[u8], addr: SocketAddr) -> String {
    let text = String::from_utf8_lossy(raw).replace(&addr.port().to_string(), "<PORT>");
    let normalized = text.replace("\r\n", "\n");
    assert_eq!(
        normalized.matches('\n').count(),
        text.matches("\r\n").count(),
        "{case_name}: request must use CRLF line endings only"
    );
    assert!(
        !normalized.contains('\r'),
        "{case_name}: stray CR in request"
    );
    format!("===== {case_name} =====\n{normalized}\n")
}

#[tokio::test]
async fn upstream_request_bytes_match_golden() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (addr, captured) = spawn_capture_upstream().await;

    let mut rendered = String::new();
    for case in cases(addr) {
        let forwarder = forwarder(match case.app_type {
            AppType::Codex => WireFormat::OpenAiResponses,
            _ => WireFormat::AnthropicMessages,
        });
        let adapter = get_adapter(&case.app_type);
        let before = captured.lock().unwrap().len();
        let result = forwarder
            .forward_once(
                &case.app_type,
                &case.method,
                &case.provider,
                case.endpoint,
                &case.body,
                &case.headers,
                &case.extensions,
                adapter.as_ref(),
            )
            .await;
        if let Err(error) = result {
            panic!("{}: forward failed: {error:?}", case.name);
        }
        let raw = captured.lock().unwrap()[before].clone();
        rendered.push_str(&render(case.name, &raw, addr));
    }

    let path = expected_path();
    if std::env::var_os("CC_SWITCH_UPDATE_EXPECTED").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &rendered).unwrap();
    }
    let expected =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    if rendered != expected {
        panic!("upstream request bytes drifted from golden.\n--- expected\n{expected}\n--- actual\n{rendered}");
    }
}

/// OpenAI 兼容上游只收到端到端头：逐跳头、Connection 点名的头、cookie 与 OpenAI
/// 账号身份头都被剔除，content-type 与 adapter 认证头保留
#[tokio::test]
async fn openai_compatible_upstream_never_sees_hop_by_hop_cookie_or_identity_headers() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (addr, captured) = spawn_capture_upstream().await;
    let provider = codex_provider(
        addr,
        "/v1",
        ProviderMeta {
            api_format: Some("openai_chat".to_string()),
            ..Default::default()
        },
    );
    let client_headers = headers(&[
        ("connection", "keep-alive, x-foo"),
        ("x-foo", "connection-scoped"),
        ("keep-alive", "timeout=5"),
        ("proxy-authorization", "Basic cHJveHk="),
        ("cookie", "session=abc"),
        ("chatgpt-account-id", "acct-1"),
        ("openai-organization", "org-1"),
        ("authorization", "Bearer client"),
        ("content-type", "application/json"),
        ("x-custom", "1"),
    ]);
    let adapter = get_adapter(&AppType::Codex);

    forwarder(WireFormat::OpenAiResponses)
        .forward_once(
            &AppType::Codex,
            &http::Method::POST,
            &provider,
            "/v1/responses",
            &json!({ "model": "gpt-5", "input": "hi" }),
            &client_headers,
            &Extensions::new(),
            adapter.as_ref(),
        )
        .await
        .unwrap_or_else(|error| panic!("forward failed: {error:?}"));

    let raw = captured.lock().unwrap()[0].clone();
    let text = String::from_utf8_lossy(&raw).to_ascii_lowercase();
    let head = &text[..text.find("\r\n\r\n").expect("request head")];
    let names: Vec<&str> = head
        .lines()
        .skip(1)
        .filter_map(|line| line.split_once(':').map(|(name, _)| name.trim()))
        .collect();

    for forbidden in [
        "connection",
        "x-foo",
        "keep-alive",
        "proxy-authorization",
        "cookie",
        "chatgpt-account-id",
        "openai-organization",
    ] {
        assert!(
            !names.contains(&forbidden),
            "{forbidden} leaked upstream:\n{head}"
        );
    }
    assert!(
        head.contains("\r\ncontent-type: application/json\r\n"),
        "{head}"
    );
    assert!(
        head.contains("\r\nauthorization: bearer sk-codex-upstream\r\n"),
        "{head}"
    );
    assert!(head.contains("\r\nx-custom: 1\r\n"), "{head}");
}
