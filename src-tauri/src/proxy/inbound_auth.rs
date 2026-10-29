//! 代理入站鉴权
//!
//! 本地代理会把用户真实的上游凭据附加到每个转发请求上，因此入站请求必须证明
//! 自己来自 CC Switch 接管写入的客户端配置：
//!
//! 1. Host 必须是回环地址或代理实际监听的地址:端口（防 DNS rebinding）；
//! 2. 拒绝带 `Origin` 的请求，以及 `Sec-Fetch-Site` 不是 `none` / `same-origin`
//!    的请求（防网页发起的 no-cors 请求）；
//! 3. POST/PUT/PATCH 必须是 `application/json`（text/plain 是 CORS simple request）；
//! 4. 必须在 `authorization` / `x-api-key` / `x-goog-api-key` 中携带每个安装独立的
//!    随机 token（常量时间比较）。旧版 `PROXY_MANAGED` 占位符只在回环监听时兼容，
//!    用于尚未重启的客户端过渡。
//!
//! 接管时各写入路径仍先写入 [`LEGACY_PROXY_TOKEN`] 作为哨兵，
//! 最终落盘前由 [`bind_inbound_token`] 统一替换为本安装的 token。

use crate::database::Database;
use crate::error::AppError;
use axum::{
    body::Body,
    extract::{Request, State},
    http::{header, HeaderMap, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::net::IpAddr;
use std::sync::Arc;

/// 旧版接管占位符（也是写入路径内部使用的哨兵值）
pub const LEGACY_PROXY_TOKEN: &str = "PROXY_MANAGED";

/// 每个安装 token 的前缀，便于在 Live 配置中识别接管状态
pub const INBOUND_TOKEN_PREFIX: &str = "ccs-proxy-";

/// token 随机部分长度（uuid v4 simple 格式，32 个十六进制字符）
const INBOUND_TOKEN_RANDOM_LEN: usize = 32;

const INBOUND_TOKEN_SETTING_KEY: &str = "proxy_inbound_token";

/// 不要求 token 的路径：健康检查不含敏感信息；Claude Desktop gateway 由
/// handler 校验自己的 gateway token。Host / Origin / Content-Type 检查仍然生效。
const TOKEN_EXEMPT_PATHS: [&str; 1] = ["/health"];
const TOKEN_EXEMPT_PREFIXES: [&str; 1] = ["/claude-desktop/"];

/// 是否为 CC Switch 写入的代理 token（新 token 或旧占位符）
pub fn is_proxy_token(value: &str) -> bool {
    let value = value.trim();
    value == LEGACY_PROXY_TOKEN || is_install_token(value)
}

/// 字符串（如请求头值）中是否包含 CC Switch 的代理 token
pub fn contains_proxy_token(value: &str) -> bool {
    value.contains(LEGACY_PROXY_TOKEN) || value.contains(INBOUND_TOKEN_PREFIX)
}

fn is_install_token(value: &str) -> bool {
    value
        .strip_prefix(INBOUND_TOKEN_PREFIX)
        .is_some_and(|rest| {
            rest.len() == INBOUND_TOKEN_RANDOM_LEN && rest.bytes().all(|b| b.is_ascii_hexdigit())
        })
}

fn generate_inbound_token() -> String {
    format!("{INBOUND_TOKEN_PREFIX}{}", uuid::Uuid::new_v4().simple())
}

/// 读取本安装的入站 token，不存在（或格式无效）时生成并写入数据库
pub fn get_or_create_inbound_token(db: &Database) -> Result<String, AppError> {
    if let Some(token) = db.get_setting(INBOUND_TOKEN_SETTING_KEY)? {
        let token = token.trim();
        if is_install_token(token) {
            return Ok(token.to_string());
        }
    }

    let token = generate_inbound_token();
    db.set_setting(INBOUND_TOKEN_SETTING_KEY, &token)?;
    Ok(token)
}

/// 把配置中的 [`LEGACY_PROXY_TOKEN`] 替换为本安装的 token
///
/// - JSON 字符串值恰好等于占位符时整体替换（Claude env、Codex auth、Gemini env）；
/// - 嵌入的 TOML 文本（Codex / Grok Build 的 `config`）中替换带引号的占位符字面量。
pub fn bind_inbound_token(value: &mut Value, token: &str) {
    match value {
        Value::String(text) => {
            if text == LEGACY_PROXY_TOKEN {
                *text = token.to_string();
            } else if text.contains(LEGACY_PROXY_TOKEN) {
                *text = text
                    .replace(
                        &format!("\"{LEGACY_PROXY_TOKEN}\""),
                        &format!("\"{token}\""),
                    )
                    .replace(&format!("'{LEGACY_PROXY_TOKEN}'"), &format!("'{token}'"));
            }
        }
        Value::Array(items) => {
            for item in items {
                bind_inbound_token(item, token);
            }
        }
        Value::Object(map) => {
            for (_, item) in map.iter_mut() {
                bind_inbound_token(item, token);
            }
        }
        _ => {}
    }
}

/// 是否为回环监听地址
pub fn is_loopback_listen_address(address: &str) -> bool {
    let address = address.trim();
    if address.eq_ignore_ascii_case("localhost") {
        return true;
    }
    address
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
        .is_ok_and(|ip| ip.is_loopback())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// 入站请求守卫（axum 中间件状态）
#[derive(Clone)]
pub struct InboundGuard {
    token: Arc<str>,
    /// 监听地址（`None` 表示 0.0.0.0 / :: 通配监听）
    listen_ip: Option<IpAddr>,
    port: u16,
    allow_legacy_token: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rejection {
    Host,
    Origin,
    FetchSite,
    ContentType,
    MissingToken,
    InvalidToken,
}

impl Rejection {
    fn status(self) -> StatusCode {
        match self {
            Rejection::Host | Rejection::Origin | Rejection::FetchSite => StatusCode::FORBIDDEN,
            Rejection::ContentType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Rejection::MissingToken | Rejection::InvalidToken => StatusCode::UNAUTHORIZED,
        }
    }

    fn message(self) -> &'static str {
        match self {
            Rejection::Host => "Host header is not allowed for the local proxy",
            Rejection::Origin => "Cross-origin requests to the local proxy are not allowed",
            Rejection::FetchSite => {
                "Browser cross-site requests to the local proxy are not allowed"
            }
            Rejection::ContentType => "Request body must be application/json",
            Rejection::MissingToken => "Missing local proxy token",
            Rejection::InvalidToken => "Invalid local proxy token",
        }
    }

    fn into_response(self) -> Response {
        let error_type = match self {
            Rejection::ContentType => "invalid_request_error",
            Rejection::MissingToken | Rejection::InvalidToken => "authentication_error",
            _ => "permission_error",
        };
        (
            self.status(),
            Json(json!({
                "type": "error",
                "error": { "type": error_type, "message": self.message() }
            })),
        )
            .into_response()
    }
}

impl InboundGuard {
    /// `port` 为代理实际监听的端口（端口 0 时为系统分配的端口）
    pub fn new(listen_address: &str, port: u16, token: String) -> Self {
        let trimmed = listen_address
            .trim()
            .trim_start_matches('[')
            .trim_end_matches(']');
        let listen_ip = if trimmed.eq_ignore_ascii_case("localhost") {
            Some(IpAddr::from([127, 0, 0, 1]))
        } else {
            trimmed
                .parse::<IpAddr>()
                .ok()
                .filter(|ip| !ip.is_unspecified())
        };
        Self {
            token: Arc::from(token),
            listen_ip,
            port,
            allow_legacy_token: is_loopback_listen_address(listen_address),
        }
    }

    fn check(&self, method: &Method, path: &str, headers: &HeaderMap) -> Result<(), Rejection> {
        self.check_host(headers)?;

        if headers.contains_key(header::ORIGIN) {
            return Err(Rejection::Origin);
        }
        if let Some(site) = headers.get("sec-fetch-site") {
            let site = site.to_str().unwrap_or("").trim();
            if !site.eq_ignore_ascii_case("none") && !site.eq_ignore_ascii_case("same-origin") {
                return Err(Rejection::FetchSite);
            }
        }

        if matches!(*method, Method::POST | Method::PUT | Method::PATCH)
            && !is_json_content_type(headers)
        {
            return Err(Rejection::ContentType);
        }

        if TOKEN_EXEMPT_PATHS.contains(&path)
            || TOKEN_EXEMPT_PREFIXES
                .iter()
                .any(|prefix| path.starts_with(prefix))
        {
            return Ok(());
        }

        self.check_token(headers)
    }

    fn check_host(&self, headers: &HeaderMap) -> Result<(), Rejection> {
        let host = headers
            .get(header::HOST)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .ok_or(Rejection::Host)?;
        let (name, port) = split_host_port(host).ok_or(Rejection::Host)?;

        let port_matches = match port {
            Some(port) => port == self.port,
            None => self.port == 80,
        };
        if !port_matches {
            return Err(Rejection::Host);
        }

        if name.eq_ignore_ascii_case("localhost") {
            return Ok(());
        }
        // 只接受 IP 字面量：DNS rebinding 需要攻击者控制的域名。
        let ip = name.parse::<IpAddr>().map_err(|_| Rejection::Host)?;
        let allowed = ip.is_loopback()
            || match self.listen_ip {
                // 通配监听时客户端可能使用任意本机网卡地址
                None => true,
                Some(listen_ip) => ip == listen_ip,
            };
        if allowed {
            Ok(())
        } else {
            Err(Rejection::Host)
        }
    }

    fn check_token(&self, headers: &HeaderMap) -> Result<(), Rejection> {
        let mut saw_credential = false;
        let candidates = headers
            .get_all(header::AUTHORIZATION)
            .iter()
            .chain(headers.get_all("x-api-key").iter())
            .chain(headers.get_all("x-goog-api-key").iter());

        for value in candidates {
            let Ok(value) = value.to_str() else {
                continue;
            };
            let value = value.trim();
            let credential = match value.split_once(' ') {
                Some((scheme, rest)) if scheme.eq_ignore_ascii_case("bearer") => rest.trim(),
                _ => value,
            };
            if credential.is_empty() {
                continue;
            }
            saw_credential = true;
            if constant_time_eq(credential.as_bytes(), self.token.as_bytes()) {
                return Ok(());
            }
            if self.allow_legacy_token
                && constant_time_eq(credential.as_bytes(), LEGACY_PROXY_TOKEN.as_bytes())
            {
                return Ok(());
            }
        }

        Err(if saw_credential {
            Rejection::InvalidToken
        } else {
            Rejection::MissingToken
        })
    }
}

fn split_host_port(host: &str) -> Option<(&str, Option<u16>)> {
    if host.is_empty() {
        return None;
    }
    if let Some(rest) = host.strip_prefix('[') {
        let (name, after) = rest.split_once(']')?;
        return match after {
            "" => Some((name, None)),
            _ => Some((name, Some(after.strip_prefix(':')?.parse().ok()?))),
        };
    }
    match host.split_once(':') {
        None => Some((host, None)),
        Some((name, port)) => {
            if port.contains(':') {
                return None;
            }
            Some((name, Some(port.parse().ok()?)))
        }
    }
}

fn is_json_content_type(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .is_some_and(|mime| mime.eq_ignore_ascii_case("application/json"))
}

/// axum 中间件：在路由 handler（以及转发器）之前拒绝未授权的入站请求
pub async fn enforce_inbound_auth(
    State(guard): State<Arc<InboundGuard>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    if let Err(rejection) = guard.check(request.method(), request.uri().path(), request.headers()) {
        log::warn!(
            "[{}] 拒绝入站请求 {} {}: {}",
            super::log_codes::srv::INBOUND_REJECTED,
            request.method(),
            request.uri().path(),
            rejection.message()
        );
        return rejection.into_response();
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        routing::{any, get, post},
        Router,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tower::ServiceExt;

    const PORT: u16 = 15721;
    const TOKEN: &str = "ccs-proxy-0123456789abcdef0123456789abcdef";

    fn router(listen_address: &str, hits: Arc<AtomicUsize>) -> Router {
        let guard = Arc::new(InboundGuard::new(listen_address, PORT, TOKEN.to_string()));
        let handler = move || {
            let hits = hits.clone();
            async move {
                hits.fetch_add(1, Ordering::SeqCst);
                "ok"
            }
        };
        Router::new()
            .route("/v1/messages", post(handler.clone()))
            .route("/health", get(handler.clone()))
            .route("/claude-desktop/v1/models", get(handler.clone()))
            .route("/v1beta/*path", any(handler))
            .layer(axum::middleware::from_fn_with_state(
                guard,
                enforce_inbound_auth,
            ))
    }

    fn post_request(content_type: &str) -> http::request::Builder {
        http::Request::builder()
            .method(Method::POST)
            .uri("/v1/messages")
            .header(header::HOST, format!("127.0.0.1:{PORT}"))
            .header(header::CONTENT_TYPE, content_type)
    }

    fn messages_request() -> http::request::Builder {
        post_request("application/json")
    }

    fn messages_request_for_host(host: &str) -> http::request::Builder {
        http::Request::builder()
            .method(Method::POST)
            .uri("/v1/messages")
            .header(header::HOST, host)
            .header(header::CONTENT_TYPE, "application/json")
    }

    async fn status_of(listen_address: &str, request: Request<Body>) -> (StatusCode, usize) {
        let hits = Arc::new(AtomicUsize::new(0));
        let response = router(listen_address, hits.clone())
            .oneshot(request)
            .await
            .expect("router response");
        (response.status(), hits.load(Ordering::SeqCst))
    }

    #[tokio::test]
    async fn valid_token_reaches_the_handler() {
        for (name, value) in [
            ("authorization", format!("Bearer {TOKEN}")),
            ("x-api-key", TOKEN.to_string()),
            ("x-goog-api-key", TOKEN.to_string()),
        ] {
            let request = messages_request()
                .header(name, value)
                .body(Body::from("{}"))
                .unwrap();
            assert_eq!(
                status_of("127.0.0.1", request).await,
                (StatusCode::OK, 1),
                "{name}"
            );
        }
    }

    #[tokio::test]
    async fn text_plain_post_with_foreign_origin_is_rejected() {
        let request = post_request("text/plain")
            .header(header::ORIGIN, "https://evil.example")
            .header("x-api-key", TOKEN)
            .body(Body::from("{}"))
            .unwrap();
        assert_eq!(
            status_of("127.0.0.1", request).await,
            (StatusCode::FORBIDDEN, 0)
        );
    }

    #[tokio::test]
    async fn text_plain_post_without_origin_is_rejected() {
        let request = post_request("text/plain")
            .header("x-api-key", TOKEN)
            .body(Body::from("{}"))
            .unwrap();
        assert_eq!(
            status_of("127.0.0.1", request).await,
            (StatusCode::UNSUPPORTED_MEDIA_TYPE, 0)
        );
    }

    #[tokio::test]
    async fn cross_site_fetch_metadata_is_rejected() {
        let request = messages_request()
            .header("sec-fetch-site", "cross-site")
            .header("x-api-key", TOKEN)
            .body(Body::from("{}"))
            .unwrap();
        assert_eq!(
            status_of("127.0.0.1", request).await,
            (StatusCode::FORBIDDEN, 0)
        );

        let request = messages_request()
            .header("sec-fetch-site", "none")
            .header("x-api-key", TOKEN)
            .body(Body::from("{}"))
            .unwrap();
        assert_eq!(status_of("127.0.0.1", request).await, (StatusCode::OK, 1));
    }

    #[tokio::test]
    async fn foreign_host_is_rejected() {
        for host in [
            format!("evil.example:{PORT}"),
            format!("attacker.test:{PORT}"),
            "127.0.0.1:1".to_string(),
            "127.0.0.1".to_string(),
            format!("192.168.1.20:{PORT}"),
        ] {
            let request = messages_request_for_host(&host)
                .header("x-api-key", TOKEN)
                .body(Body::from("{}"))
                .unwrap();
            assert_eq!(
                status_of("127.0.0.1", request).await,
                (StatusCode::FORBIDDEN, 0),
                "{host}"
            );
        }
    }

    #[tokio::test]
    async fn loopback_hosts_are_accepted() {
        for host in [
            format!("localhost:{PORT}"),
            format!("127.0.0.1:{PORT}"),
            format!("[::1]:{PORT}"),
        ] {
            let request = messages_request_for_host(&host)
                .header("x-api-key", TOKEN)
                .body(Body::from("{}"))
                .unwrap();
            assert_eq!(
                status_of("127.0.0.1", request).await,
                (StatusCode::OK, 1),
                "{host}"
            );
        }
    }

    #[tokio::test]
    async fn wildcard_bind_accepts_lan_ip_but_not_domains() {
        let request = messages_request_for_host(&format!("192.168.1.20:{PORT}"))
            .header("x-api-key", TOKEN)
            .body(Body::from("{}"))
            .unwrap();
        assert_eq!(status_of("0.0.0.0", request).await, (StatusCode::OK, 1));

        let request = messages_request_for_host(&format!("rebind.attacker.test:{PORT}"))
            .header("x-api-key", TOKEN)
            .body(Body::from("{}"))
            .unwrap();
        assert_eq!(
            status_of("0.0.0.0", request).await,
            (StatusCode::FORBIDDEN, 0)
        );
    }

    #[tokio::test]
    async fn missing_and_wrong_tokens_are_rejected() {
        let request = messages_request().body(Body::from("{}")).unwrap();
        assert_eq!(
            status_of("127.0.0.1", request).await,
            (StatusCode::UNAUTHORIZED, 0)
        );

        let request = messages_request()
            .header("x-api-key", "ccs-proxy-ffffffffffffffffffffffffffffffff")
            .body(Body::from("{}"))
            .unwrap();
        assert_eq!(
            status_of("127.0.0.1", request).await,
            (StatusCode::UNAUTHORIZED, 0)
        );

        let request = messages_request()
            .header(header::AUTHORIZATION, "Bearer sk-real-looking-key")
            .body(Body::from("{}"))
            .unwrap();
        assert_eq!(
            status_of("127.0.0.1", request).await,
            (StatusCode::UNAUTHORIZED, 0)
        );
    }

    #[tokio::test]
    async fn legacy_placeholder_only_accepted_on_loopback_bind() {
        let request = messages_request()
            .header("x-api-key", LEGACY_PROXY_TOKEN)
            .body(Body::from("{}"))
            .unwrap();
        assert_eq!(status_of("127.0.0.1", request).await, (StatusCode::OK, 1));

        let request = messages_request()
            .header("x-api-key", LEGACY_PROXY_TOKEN)
            .body(Body::from("{}"))
            .unwrap();
        assert_eq!(
            status_of("0.0.0.0", request).await,
            (StatusCode::UNAUTHORIZED, 0)
        );
    }

    #[tokio::test]
    async fn health_and_desktop_gateway_skip_only_the_token_check() {
        let request = http::Request::builder()
            .uri("/health")
            .header(header::HOST, format!("127.0.0.1:{PORT}"))
            .body(Body::empty())
            .unwrap();
        assert_eq!(status_of("127.0.0.1", request).await, (StatusCode::OK, 1));

        let request = http::Request::builder()
            .uri("/claude-desktop/v1/models")
            .header(header::HOST, format!("127.0.0.1:{PORT}"))
            .body(Body::empty())
            .unwrap();
        assert_eq!(status_of("127.0.0.1", request).await, (StatusCode::OK, 1));

        let request = http::Request::builder()
            .uri("/health")
            .header(header::HOST, format!("evil.example:{PORT}"))
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            status_of("127.0.0.1", request).await,
            (StatusCode::FORBIDDEN, 0)
        );
    }

    #[tokio::test]
    async fn gemini_get_requires_token_but_not_json() {
        let request = http::Request::builder()
            .uri("/v1beta/models")
            .header(header::HOST, format!("127.0.0.1:{PORT}"))
            .header("x-goog-api-key", TOKEN)
            .body(Body::empty())
            .unwrap();
        assert_eq!(status_of("127.0.0.1", request).await, (StatusCode::OK, 1));
    }

    #[test]
    fn token_helpers_recognise_install_and_legacy_tokens() {
        assert!(is_proxy_token(LEGACY_PROXY_TOKEN));
        assert!(is_proxy_token(TOKEN));
        assert!(!is_proxy_token("ccs-proxy-short"));
        assert!(!is_proxy_token("sk-ant-real"));
        assert!(contains_proxy_token(&format!("Bearer {TOKEN}")));
        let generated = generate_inbound_token();
        assert!(is_install_token(&generated), "{generated}");
        assert_ne!(generated, generate_inbound_token());
    }

    #[test]
    fn inbound_token_is_persisted_once() {
        let db = Database::memory().expect("memory db");
        let first = get_or_create_inbound_token(&db).expect("create token");
        let second = get_or_create_inbound_token(&db).expect("read token");
        assert_eq!(first, second);
        assert!(is_install_token(&first));
    }

    #[test]
    fn bind_inbound_token_replaces_json_values_and_toml_literals() {
        let mut config = json!({
            "env": { "ANTHROPIC_AUTH_TOKEN": LEGACY_PROXY_TOKEN, "OTHER": "keep" },
            "config": "model_provider = \"x\"\n[model_providers.x]\nexperimental_bearer_token = \"PROXY_MANAGED\"\n",
        });
        bind_inbound_token(&mut config, TOKEN);
        assert_eq!(config["env"]["ANTHROPIC_AUTH_TOKEN"], json!(TOKEN));
        assert_eq!(config["env"]["OTHER"], json!("keep"));
        assert!(config["config"]
            .as_str()
            .unwrap()
            .contains(&format!("experimental_bearer_token = \"{TOKEN}\"")));
        assert!(!config.to_string().contains(LEGACY_PROXY_TOKEN));
    }
}
