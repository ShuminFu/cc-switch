//! 供应商余额查询服务
//!
//! 支持 DeepSeek、StepFun、SiliconFlow、OpenRouter、Novita AI 的账户余额查询。
//! 返回 UsageResult 格式，与现有用量系统无缝对接。
//!
//! 错误通道语义（与 coding_plan / subscription 两个服务保持一致）：
//! - `Err(String)` = 瞬时传输失败（网络不可达/超时/读体中断）。前端 invoke reject，
//!   react-query 触发 retry 并保留上一次成功的 data（天然 keep-last-good）。
//! - `Ok(success:false)` = 确定性失败（空 key/未知供应商/鉴权/非 2xx/响应体非法 JSON），
//!   立即透出错误文案。判定按 reqwest 错误种类在折叠点完成，不依赖错误文案匹配。
//!
//! SVC-30: every provider is one [`BalanceProvider`] registry entry (match
//! rule, endpoint, response parser); the HTTP round-trip, timeout and the
//! auth/status/JSON error mapping are shared by [`fetch_json`]. The parsers
//! are pure functions and each provider has a success and a 401 test against
//! a local mock server.

use crate::provider::{UsageData, UsageResult};
use serde_json::Value;
use std::time::Duration;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

// ── 供应商注册表 ────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BalanceProvider {
    DeepSeek,
    StepFun,
    SiliconFlow,
    SiliconFlowEn,
    OpenRouter,
    NovitaAI,
}

impl BalanceProvider {
    /// Registry order is match order (first hit wins).
    const ALL: [Self; 6] = [
        Self::DeepSeek,
        Self::StepFun,
        Self::SiliconFlow,
        Self::SiliconFlowEn,
        Self::OpenRouter,
        Self::NovitaAI,
    ];

    /// Host fragments of the provider's `base_url`.
    fn hosts(self) -> &'static [&'static str] {
        match self {
            Self::DeepSeek => &["api.deepseek.com"],
            Self::StepFun => &["api.stepfun.ai", "api.stepfun.com"],
            Self::SiliconFlow => &["api.siliconflow.cn"],
            Self::SiliconFlowEn => &["api.siliconflow.com"],
            Self::OpenRouter => &["openrouter.ai"],
            Self::NovitaAI => &["api.novita.ai"],
        }
    }

    /// Balance endpoint.
    fn endpoint(self) -> &'static str {
        match self {
            // Response: { balance_infos: [{ currency, total_balance, granted_balance, topped_up_balance }], is_available }
            Self::DeepSeek => "https://api.deepseek.com/user/balance",
            // Response: { object, type, balance, total_cash_balance, total_voucher_balance }
            Self::StepFun => "https://api.stepfun.com/v1/accounts",
            // Response: { code, data: { balance, chargeBalance, totalBalance, status } }
            Self::SiliconFlow => "https://api.siliconflow.cn/v1/user/info",
            Self::SiliconFlowEn => "https://api.siliconflow.com/v1/user/info",
            // Response: { data: { total_credits, total_usage } }
            Self::OpenRouter => "https://openrouter.ai/api/v1/credits",
            // Response: { availableBalance, cashBalance, creditLimit, outstandingInvoices }
            Self::NovitaAI => "https://api.novita.ai/v3/user/balance",
        }
    }

    fn parse(self, body: &Value) -> UsageResult {
        match self {
            Self::DeepSeek => parse_deepseek(body),
            Self::StepFun => parse_stepfun(body),
            Self::SiliconFlow => parse_siliconflow(body, true),
            Self::SiliconFlowEn => parse_siliconflow(body, false),
            Self::OpenRouter => parse_openrouter(body),
            Self::NovitaAI => parse_novita(body),
        }
    }
}

fn detect_provider(base_url: &str) -> Option<BalanceProvider> {
    let url = base_url.to_lowercase();
    BalanceProvider::ALL
        .into_iter()
        .find(|provider| provider.hosts().iter().any(|host| url.contains(host)))
}

fn make_error(msg: String) -> UsageResult {
    UsageResult {
        success: false,
        data: None,
        error: Some(msg),
    }
}

fn make_auth_error(status: reqwest::StatusCode) -> UsageResult {
    UsageResult {
        success: false,
        data: Some(vec![UsageData {
            plan_name: None,
            remaining: None,
            total: None,
            used: None,
            unit: None,
            is_valid: Some(false),
            invalid_message: Some(format!("Authentication failed (HTTP {status})")),
            extra: None,
        }]),
        error: Some(format!("Authentication failed (HTTP {status})")),
    }
}

fn success(data: Vec<UsageData>) -> UsageResult {
    UsageResult {
        success: true,
        data: if data.is_empty() { None } else { Some(data) },
        error: None,
    }
}

// ── 共享 HTTP ───────────────────────────────────────────────

/// GET `url` with a bearer token. `Err` = transient transport failure;
/// `Ok(Err(result))` = deterministic failure (auth, non-2xx, invalid JSON)
/// already shaped as a `UsageResult`; `Ok(Ok(body))` = parsed JSON.
async fn fetch_json(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
) -> Result<Result<Value, UsageResult>, String> {
    let resp = client
        .get(url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Accept", "application/json")
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return Err(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Ok(Err(make_auth_error(status)));
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Ok(Err(make_error(format!(
            "API error (HTTP {status}): {body}"
        ))));
    }

    // 先 bytes() 再解析：读体失败（超时/连接中断）是瞬时 → Err；拿到完整响应体
    // 后解析失败才是确定性。reqwest 的 json() 把读体错误也包成 decode，无法区分。
    let raw = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => return Err(format!("Failed to read response: {e}")),
    };
    match serde_json::from_slice(&raw) {
        Ok(v) => Ok(Ok(v)),
        Err(e) => Ok(Err(make_error(format!("Failed to parse response: {e}")))),
    }
}

async fn query_at(
    client: &reqwest::Client,
    provider: BalanceProvider,
    url: &str,
    api_key: &str,
) -> Result<UsageResult, String> {
    Ok(match fetch_json(client, url, api_key).await? {
        Ok(body) => provider.parse(&body),
        Err(failure) => failure,
    })
}

// ── 响应解析 ────────────────────────────────────────────────

fn parse_deepseek(body: &Value) -> UsageResult {
    let is_available = body
        .get("is_available")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let mut data = Vec::new();

    if let Some(infos) = body.get("balance_infos").and_then(|v| v.as_array()) {
        for info in infos {
            let currency = info
                .get("currency")
                .and_then(|v| v.as_str())
                .unwrap_or("CNY");
            let total = parse_f64_field(info, "total_balance");

            data.push(UsageData {
                plan_name: Some(currency.to_string()),
                remaining: total,
                total: None,
                used: None,
                unit: Some(currency.to_string()),
                is_valid: Some(is_available),
                invalid_message: if !is_available {
                    Some("Insufficient balance".to_string())
                } else {
                    None
                },
                extra: None,
            });
        }
    }

    success(data)
}

fn parse_stepfun(body: &Value) -> UsageResult {
    let balance = parse_f64_field(body, "balance").unwrap_or(0.0);
    success(vec![UsageData {
        plan_name: Some("StepFun".to_string()),
        remaining: Some(balance),
        total: None,
        used: None,
        unit: Some("CNY".to_string()),
        is_valid: Some(true),
        invalid_message: None,
        extra: None,
    }])
}

fn parse_siliconflow(body: &Value, is_cn: bool) -> UsageResult {
    let data = match body.get("data") {
        Some(d) => d,
        None => return make_error("Missing 'data' field in response".to_string()),
    };

    let total_balance = parse_f64_field(data, "totalBalance").unwrap_or(0.0);

    let unit = if is_cn { "CNY" } else { "USD" };
    let plan_name = if is_cn {
        "SiliconFlow"
    } else {
        "SiliconFlow (EN)"
    };

    success(vec![UsageData {
        plan_name: Some(plan_name.to_string()),
        remaining: Some(total_balance),
        total: None,
        used: None,
        unit: Some(unit.to_string()),
        is_valid: Some(true),
        invalid_message: None,
        extra: None,
    }])
}

fn parse_openrouter(body: &Value) -> UsageResult {
    let data = body.get("data").unwrap_or(body);
    let total_credits = parse_f64_field(data, "total_credits").unwrap_or(0.0);
    let total_usage = parse_f64_field(data, "total_usage").unwrap_or(0.0);
    let remaining = total_credits - total_usage;

    success(vec![UsageData {
        plan_name: Some("OpenRouter".to_string()),
        remaining: Some(remaining),
        total: Some(total_credits),
        used: Some(total_usage),
        unit: Some("USD".to_string()),
        is_valid: Some(remaining > 0.0),
        invalid_message: if remaining <= 0.0 {
            Some("No credits remaining".to_string())
        } else {
            None
        },
        extra: None,
    }])
}

fn parse_novita(body: &Value) -> UsageResult {
    // Novita 金额单位为 0.0001 USD，需除以 10000 转为 USD
    let available = parse_f64_field(body, "availableBalance").unwrap_or(0.0) / 10000.0;

    success(vec![UsageData {
        plan_name: Some("Novita AI".to_string()),
        remaining: Some(available),
        total: None,
        used: None,
        unit: Some("USD".to_string()),
        is_valid: Some(available > 0.0),
        invalid_message: if available <= 0.0 {
            Some("No balance remaining".to_string())
        } else {
            None
        },
        extra: None,
    }])
}

// ── 工具函数 ────────────────────────────────────────────────

/// 解析 JSON 字段为 f64，兼容数字和字符串格式
fn parse_f64_field(obj: &serde_json::Value, field: &str) -> Option<f64> {
    obj.get(field).and_then(|v| {
        v.as_f64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
    })
}

// ── 公开入口 ────────────────────────────────────────────────

/// 查询余额。瞬时传输失败返回 `Err`（前端 reject → retry + 保留上次成功值），
/// 确定性失败返回 `Ok(success:false)`（见模块级文档）。
pub async fn get_balance(base_url: &str, api_key: &str) -> Result<UsageResult, String> {
    if api_key.trim().is_empty() {
        return Ok(UsageResult {
            success: false,
            data: None,
            error: Some("API key is empty".to_string()),
        });
    }

    let provider = match detect_provider(base_url) {
        Some(p) => p,
        None => {
            return Ok(UsageResult {
                success: false,
                data: None,
                error: Some("Unknown balance provider".to_string()),
            })
        }
    };

    let client = crate::proxy::http_client::get();
    query_at(&client, provider, provider.endpoint(), api_key).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// One-shot local HTTP server answering `status` + `body`; returns its URL
    /// and the request it received (for header assertions).
    async fn mock_server(
        status: u16,
        body: &'static str,
    ) -> (String, tokio::task::JoinHandle<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/balance", listener.local_addr().unwrap());
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let n = socket.read(&mut buf).await.unwrap();
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            let response = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.shutdown().await.ok();
            request
        });
        (url, handle)
    }

    fn client() -> reqwest::Client {
        reqwest::Client::builder().no_proxy().build().unwrap()
    }

    const FIXTURES: [(BalanceProvider, &str); 6] = [
        (
            BalanceProvider::DeepSeek,
            r#"{"is_available":true,"balance_infos":[{"currency":"CNY","total_balance":"12.50"}]}"#,
        ),
        (BalanceProvider::StepFun, r#"{"balance":"8"}"#),
        (
            BalanceProvider::SiliconFlow,
            r#"{"data":{"totalBalance":"3.5"}}"#,
        ),
        (
            BalanceProvider::SiliconFlowEn,
            r#"{"data":{"totalBalance":2}}"#,
        ),
        (
            BalanceProvider::OpenRouter,
            r#"{"data":{"total_credits":10,"total_usage":4}}"#,
        ),
        (BalanceProvider::NovitaAI, r#"{"availableBalance":50000}"#),
    ];

    #[tokio::test]
    async fn every_provider_parses_its_success_fixture() {
        let expected = [12.5, 8.0, 3.5, 2.0, 6.0, 5.0];
        for ((provider, body), remaining) in FIXTURES.into_iter().zip(expected) {
            let (url, server) = mock_server(200, body).await;
            let result = query_at(&client(), provider, &url, "sk-test")
                .await
                .expect("no transport error");
            assert!(result.success, "{provider:?}: {:?}", result.error);
            let data = result.data.expect("data");
            assert_eq!(data[0].remaining, Some(remaining), "{provider:?}");
            let request = server.await.unwrap();
            assert!(
                request
                    .to_ascii_lowercase()
                    .contains("authorization: bearer sk-test"),
                "{provider:?} sends the bearer token"
            );
        }
    }

    #[tokio::test]
    async fn every_provider_maps_401_to_an_auth_failure() {
        for (provider, _) in FIXTURES {
            let (url, server) = mock_server(401, r#"{"error":"bad key"}"#).await;
            let result = query_at(&client(), provider, &url, "sk-bad")
                .await
                .expect("401 is deterministic, not transient");
            assert!(!result.success, "{provider:?}");
            assert_eq!(
                result.error.as_deref(),
                Some("Authentication failed (HTTP 401 Unauthorized)"),
                "{provider:?}"
            );
            assert_eq!(result.data.unwrap()[0].is_valid, Some(false));
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn server_errors_and_bad_json_are_deterministic_failures() {
        let (url, server) = mock_server(500, "boom").await;
        let result = query_at(&client(), BalanceProvider::StepFun, &url, "k")
            .await
            .unwrap();
        assert!(result.error.unwrap().starts_with("API error (HTTP 500"));
        server.await.unwrap();

        let (url, server) = mock_server(200, "not json").await;
        let result = query_at(&client(), BalanceProvider::StepFun, &url, "k")
            .await
            .unwrap();
        assert!(result
            .error
            .unwrap()
            .starts_with("Failed to parse response"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn unreachable_endpoint_is_a_transient_error() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/balance", listener.local_addr().unwrap());
        drop(listener);
        let err = query_at(&client(), BalanceProvider::DeepSeek, &url, "k")
            .await
            .expect_err("connection refused is transient");
        assert!(err.starts_with("Network error"), "{err}");
    }

    #[test]
    fn registry_detects_providers_by_host() {
        assert_eq!(
            detect_provider("https://api.deepseek.com/anthropic"),
            Some(BalanceProvider::DeepSeek)
        );
        assert_eq!(
            detect_provider("https://API.STEPFUN.AI/v1"),
            Some(BalanceProvider::StepFun)
        );
        assert_eq!(
            detect_provider("https://api.siliconflow.com/v1"),
            Some(BalanceProvider::SiliconFlowEn)
        );
        assert_eq!(detect_provider("https://relay.example.com"), None);
        for provider in BalanceProvider::ALL {
            assert!(provider.endpoint().starts_with("https://"));
            assert_eq!(
                detect_provider(provider.endpoint()),
                Some(provider),
                "each endpoint is detected as its own provider"
            );
        }
        let body = json!({});
        assert!(!BalanceProvider::SiliconFlow.parse(&body).success);
    }
}
