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

use crate::provider::{UsageData, UsageResult};
use std::time::Duration;

// ── 供应商检测 ──────────────────────────────────────────────

enum BalanceProvider {
    DeepSeek,
    StepFun,
    SiliconFlow,
    SiliconFlowEn,
    OpenRouter,
    NovitaAI,
}

/// Host of `base_url`, lower-cased. Accepts scheme-less input such as
/// `api.deepseek.com/v1` by assuming https.
fn base_url_host(base_url: &str) -> Option<String> {
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        return None;
    }
    let parsed = url::Url::parse(trimmed)
        .ok()
        .filter(|u| u.host_str().is_some())
        .or_else(|| url::Url::parse(&format!("https://{trimmed}")).ok())?;
    parsed
        .host_str()
        .map(|h| h.trim_end_matches('.').to_ascii_lowercase())
}

/// True when `host` is `domain` itself or one of its subdomains.
fn host_matches(host: &str, domain: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

/// Match on the parsed host only: the API key is sent to the vendor's own
/// balance endpoint, so a relay whose path or query merely mentions a vendor
/// domain must never be treated as that vendor.
fn detect_provider(base_url: &str) -> Option<BalanceProvider> {
    let host = base_url_host(base_url)?;
    let is = |domain: &str| host_matches(&host, domain);
    if is("api.deepseek.com") {
        Some(BalanceProvider::DeepSeek)
    } else if is("api.stepfun.ai") || is("api.stepfun.com") {
        Some(BalanceProvider::StepFun)
    } else if is("api.siliconflow.cn") {
        Some(BalanceProvider::SiliconFlow)
    } else if is("api.siliconflow.com") {
        Some(BalanceProvider::SiliconFlowEn)
    } else if is("openrouter.ai") {
        Some(BalanceProvider::OpenRouter)
    } else if is("api.novita.ai") {
        Some(BalanceProvider::NovitaAI)
    } else {
        None
    }
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

// ── HTTP 边界 ───────────────────────────────────────────────

/// GET `url` with the bearer key and decode the JSON body.
///
/// - `Err(String)`: transient transport failure (send / body read).
/// - `Ok(Err(UsageResult))`: deterministic failure (auth, non-2xx, bad JSON),
///   already shaped for the caller to return as-is.
/// - `Ok(Ok(Value))`: the decoded response body, ready for a pure parser.
async fn fetch_balance_json(
    url: &str,
    api_key: &str,
) -> Result<Result<serde_json::Value, UsageResult>, String> {
    let client = crate::proxy::http_client::get();

    let resp = client
        .get(url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Accept", "application/json")
        .timeout(Duration::from_secs(15))
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

fn balance_endpoint(provider: &BalanceProvider) -> &'static str {
    match provider {
        BalanceProvider::DeepSeek => "https://api.deepseek.com/user/balance",
        BalanceProvider::StepFun => "https://api.stepfun.com/v1/accounts",
        BalanceProvider::SiliconFlow => "https://api.siliconflow.cn/v1/user/info",
        BalanceProvider::SiliconFlowEn => "https://api.siliconflow.com/v1/user/info",
        BalanceProvider::OpenRouter => "https://openrouter.ai/api/v1/credits",
        BalanceProvider::NovitaAI => "https://api.novita.ai/v3/user/balance",
    }
}

fn parse_balance(provider: &BalanceProvider, body: &serde_json::Value) -> UsageResult {
    match provider {
        BalanceProvider::DeepSeek => parse_deepseek_balance(body),
        BalanceProvider::StepFun => parse_stepfun_balance(body),
        BalanceProvider::SiliconFlow => parse_siliconflow_balance(body, true),
        BalanceProvider::SiliconFlowEn => parse_siliconflow_balance(body, false),
        BalanceProvider::OpenRouter => parse_openrouter_balance(body),
        BalanceProvider::NovitaAI => parse_novita_balance(body),
    }
}

// ── 响应解析（纯函数）──────────────────────────────────────
//
// Each parser maps an already-decoded 2xx response body to a `UsageResult`.
// They never fail transiently, so they return the deterministic shape directly.

// ── DeepSeek ────────────────────────────────────────────────
// GET https://api.deepseek.com/user/balance
// Response: { balance_infos: [{ currency, total_balance, granted_balance, topped_up_balance }], is_available }

fn parse_deepseek_balance(body: &serde_json::Value) -> UsageResult {
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

    UsageResult {
        success: true,
        data: if data.is_empty() { None } else { Some(data) },
        error: None,
    }
}

// ── StepFun ─────────────────────────────────────────────────
// GET https://api.stepfun.com/v1/accounts
// Response: { object, type, balance, total_cash_balance, total_voucher_balance }

fn parse_stepfun_balance(body: &serde_json::Value) -> UsageResult {
    let balance = parse_f64_field(body, "balance").unwrap_or(0.0);

    UsageResult {
        success: true,
        data: Some(vec![UsageData {
            plan_name: Some("StepFun".to_string()),
            remaining: Some(balance),
            total: None,
            used: None,
            unit: Some("CNY".to_string()),
            is_valid: Some(true),
            invalid_message: None,
            extra: None,
        }]),
        error: None,
    }
}

// ── SiliconFlow ─────────────────────────────────────────────
// GET https://api.siliconflow.cn/v1/user/info (or .com for EN)
// Response: { code, data: { balance, chargeBalance, totalBalance, status } }

fn parse_siliconflow_balance(body: &serde_json::Value, is_cn: bool) -> UsageResult {
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

    UsageResult {
        success: true,
        data: Some(vec![UsageData {
            plan_name: Some(plan_name.to_string()),
            remaining: Some(total_balance),
            total: None,
            used: None,
            unit: Some(unit.to_string()),
            is_valid: Some(true),
            invalid_message: None,
            extra: None,
        }]),
        error: None,
    }
}

// ── OpenRouter ──────────────────────────────────────────────
// GET https://openrouter.ai/api/v1/credits
// Response: { data: { total_credits, total_usage } }

fn parse_openrouter_balance(body: &serde_json::Value) -> UsageResult {
    let data = body.get("data").unwrap_or(body);
    let total_credits = parse_f64_field(data, "total_credits").unwrap_or(0.0);
    let total_usage = parse_f64_field(data, "total_usage").unwrap_or(0.0);
    let remaining = total_credits - total_usage;

    UsageResult {
        success: true,
        data: Some(vec![UsageData {
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
        }]),
        error: None,
    }
}

// ── Novita AI ───────────────────────────────────────────────
// GET https://api.novita.ai/v3/user/balance
// Response: { availableBalance, cashBalance, creditLimit, outstandingInvoices }
// 金额单位：0.0001 USD

fn parse_novita_balance(body: &serde_json::Value) -> UsageResult {
    // Novita 金额单位为 0.0001 USD，需除以 10000 转为 USD
    let available = parse_f64_field(body, "availableBalance").unwrap_or(0.0) / 10000.0;

    UsageResult {
        success: true,
        data: Some(vec![UsageData {
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
        }]),
        error: None,
    }
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

    Ok(
        match fetch_balance_json(balance_endpoint(&provider), api_key).await? {
            Ok(body) => parse_balance(&provider, &body),
            Err(failure) => failure,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn detect_provider_by_base_url_case_insensitively() {
        assert!(matches!(
            detect_provider("https://api.deepseek.com/anthropic"),
            Some(BalanceProvider::DeepSeek)
        ));
        assert!(matches!(
            detect_provider("HTTPS://API.STEPFUN.COM/v1/"),
            Some(BalanceProvider::StepFun)
        ));
        assert!(matches!(
            detect_provider("https://api.stepfun.ai/v1"),
            Some(BalanceProvider::StepFun)
        ));
        assert!(matches!(
            detect_provider("https://openrouter.ai/api/v1"),
            Some(BalanceProvider::OpenRouter)
        ));
        assert!(matches!(
            detect_provider("https://api.novita.ai/openai"),
            Some(BalanceProvider::NovitaAI)
        ));
    }

    #[test]
    fn detect_provider_distinguishes_siliconflow_regions() {
        assert!(matches!(
            detect_provider("https://api.siliconflow.cn/v1"),
            Some(BalanceProvider::SiliconFlow)
        ));
        assert!(matches!(
            detect_provider("https://api.siliconflow.com/v1"),
            Some(BalanceProvider::SiliconFlowEn)
        ));
    }

    #[test]
    fn detect_provider_returns_none_for_unknown_or_empty() {
        assert!(detect_provider("").is_none());
        assert!(detect_provider("https://api.anthropic.com").is_none());
        // Bare marketing domains are not the API host
        assert!(detect_provider("https://deepseek.com").is_none());
    }

    #[test]
    fn detect_provider_matches_host_not_path_or_query() {
        // Relays that only mention a vendor domain in the path/query/userinfo
        // must not have their key sent to that vendor.
        for url in [
            "https://relay.example.com/openrouter.ai/api/v1",
            "https://relay.example.com/v1?upstream=api.deepseek.com",
            "https://proxy.example.com/api.siliconflow.cn/v1",
            "https://user:api.novita.ai@relay.example.com/v1",
        ] {
            assert!(detect_provider(url).is_none(), "{url}");
        }
        // Look-alike hosts sharing a suffix or prefix are different domains
        assert!(detect_provider("https://evilopenrouter.ai/api/v1").is_none());
        assert!(detect_provider("https://api.deepseek.com.evil.example/v1").is_none());
    }

    #[test]
    fn detect_provider_accepts_subdomains_ports_and_schemeless_urls() {
        assert!(matches!(
            detect_provider("https://eu.openrouter.ai/api/v1"),
            Some(BalanceProvider::OpenRouter)
        ));
        assert!(matches!(
            detect_provider("https://api.deepseek.com:443/v1"),
            Some(BalanceProvider::DeepSeek)
        ));
        assert!(matches!(
            detect_provider("  api.deepseek.com/anthropic  "),
            Some(BalanceProvider::DeepSeek)
        ));
        assert!(matches!(
            detect_provider("https://api.deepseek.com./v1"),
            Some(BalanceProvider::DeepSeek)
        ));
    }

    #[test]
    fn parse_f64_field_accepts_numbers_and_numeric_strings() {
        let obj = json!({
            "num": 12.5,
            "int": 3,
            "str": "110.00",
            "neg": "-0.5",
            "bad": "N/A",
            "empty": "",
            "nested": {"x": 1},
            "null": null
        });
        assert_eq!(parse_f64_field(&obj, "num"), Some(12.5));
        assert_eq!(parse_f64_field(&obj, "int"), Some(3.0));
        assert_eq!(parse_f64_field(&obj, "str"), Some(110.0));
        assert_eq!(parse_f64_field(&obj, "neg"), Some(-0.5));
        assert_eq!(parse_f64_field(&obj, "bad"), None);
        assert_eq!(parse_f64_field(&obj, "empty"), None);
        assert_eq!(parse_f64_field(&obj, "nested"), None);
        assert_eq!(parse_f64_field(&obj, "null"), None);
        assert_eq!(parse_f64_field(&obj, "missing"), None);
    }

    #[tokio::test]
    async fn get_balance_reports_deterministic_failures_without_network() {
        let empty_key = get_balance("https://api.deepseek.com", "  ").await.unwrap();
        assert!(!empty_key.success);
        assert_eq!(empty_key.error.as_deref(), Some("API key is empty"));

        let unknown = get_balance("https://example.com", "sk-x").await.unwrap();
        assert!(!unknown.success);
        assert_eq!(unknown.error.as_deref(), Some("Unknown balance provider"));
    }

    // ── Pure response parsers (fixtures mirror the fields each parser reads) ──

    fn only_row(result: &UsageResult) -> &UsageData {
        assert!(result.success, "expected success: {:?}", result.error);
        let rows = result.data.as_ref().expect("data rows");
        assert_eq!(rows.len(), 1);
        &rows[0]
    }

    #[test]
    fn deepseek_one_row_per_currency_with_string_amounts() {
        let body = json!({
            "is_available": true,
            "balance_infos": [
                {"currency": "CNY", "total_balance": "110.00", "granted_balance": "10.00", "topped_up_balance": "100.00"},
                {"currency": "USD", "total_balance": "5.25", "granted_balance": "0.00", "topped_up_balance": "5.25"}
            ]
        });
        let result = parse_deepseek_balance(&body);
        assert!(result.success);
        assert!(result.error.is_none());
        let rows = result.data.expect("rows");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].plan_name.as_deref(), Some("CNY"));
        assert_eq!(rows[0].unit.as_deref(), Some("CNY"));
        assert_eq!(rows[0].remaining, Some(110.0));
        assert_eq!(rows[0].is_valid, Some(true));
        assert!(rows[0].invalid_message.is_none());
        assert_eq!(rows[1].unit.as_deref(), Some("USD"));
        assert_eq!(rows[1].remaining, Some(5.25));
    }

    #[test]
    fn deepseek_unavailable_marks_rows_invalid_and_defaults_currency() {
        let body = json!({
            "is_available": false,
            "balance_infos": [{"total_balance": "0.00"}]
        });
        let result = parse_deepseek_balance(&body);
        let row = only_row(&result);
        assert_eq!(row.unit.as_deref(), Some("CNY"));
        assert_eq!(row.remaining, Some(0.0));
        assert_eq!(row.is_valid, Some(false));
        assert_eq!(row.invalid_message.as_deref(), Some("Insufficient balance"));
    }

    #[test]
    fn deepseek_without_balance_infos_succeeds_with_no_data() {
        let result = parse_deepseek_balance(&json!({"is_available": true, "balance_infos": []}));
        assert!(result.success);
        assert!(result.data.is_none());
        assert!(parse_deepseek_balance(&json!({})).data.is_none());
    }

    #[test]
    fn stepfun_reads_top_level_balance() {
        let body = json!({
            "object": "account",
            "type": "prepaid",
            "balance": 42.5,
            "total_cash_balance": 40.0,
            "total_voucher_balance": 2.5
        });
        let result = parse_stepfun_balance(&body);
        let row = only_row(&result);
        assert_eq!(row.plan_name.as_deref(), Some("StepFun"));
        assert_eq!(row.remaining, Some(42.5));
        assert_eq!(row.unit.as_deref(), Some("CNY"));
        assert_eq!(row.is_valid, Some(true));

        let missing = parse_stepfun_balance(&json!({"object": "account"}));
        assert_eq!(only_row(&missing).remaining, Some(0.0));
    }

    #[test]
    fn siliconflow_cn_and_en_differ_in_unit_and_plan_name() {
        let body = json!({
            "code": 20000,
            "data": {"balance": "0.88", "chargeBalance": "88.00", "totalBalance": "88.88", "status": "normal"}
        });

        let cn = parse_siliconflow_balance(&body, true);
        let row = only_row(&cn);
        assert_eq!(row.plan_name.as_deref(), Some("SiliconFlow"));
        assert_eq!(row.unit.as_deref(), Some("CNY"));
        assert_eq!(row.remaining, Some(88.88));

        let en = parse_siliconflow_balance(&body, false);
        let row = only_row(&en);
        assert_eq!(row.plan_name.as_deref(), Some("SiliconFlow (EN)"));
        assert_eq!(row.unit.as_deref(), Some("USD"));
        assert_eq!(row.remaining, Some(88.88));
    }

    #[test]
    fn siliconflow_missing_data_is_a_deterministic_error() {
        let result = parse_siliconflow_balance(&json!({"code": 50000, "message": "x"}), true);
        assert!(!result.success);
        assert!(result.data.is_none());
        assert_eq!(
            result.error.as_deref(),
            Some("Missing 'data' field in response")
        );
    }

    #[test]
    fn openrouter_remaining_is_credits_minus_usage() {
        let body = json!({"data": {"total_credits": 20.0, "total_usage": 7.5}});
        let result = parse_openrouter_balance(&body);
        let row = only_row(&result);
        assert_eq!(row.plan_name.as_deref(), Some("OpenRouter"));
        assert_eq!(row.total, Some(20.0));
        assert_eq!(row.used, Some(7.5));
        assert_eq!(row.remaining, Some(12.5));
        assert_eq!(row.unit.as_deref(), Some("USD"));
        assert_eq!(row.is_valid, Some(true));
        assert!(row.invalid_message.is_none());
    }

    #[test]
    fn openrouter_exhausted_credits_and_unwrapped_body() {
        // Without a `data` wrapper the fields are read from the top level.
        let body = json!({"total_credits": 10, "total_usage": "10"});
        let result = parse_openrouter_balance(&body);
        let row = only_row(&result);
        assert_eq!(row.remaining, Some(0.0));
        assert_eq!(row.is_valid, Some(false));
        assert_eq!(row.invalid_message.as_deref(), Some("No credits remaining"));
    }

    #[test]
    fn novita_converts_ten_thousandths_of_usd() {
        let body = json!({
            "availableBalance": "123456",
            "cashBalance": "123456",
            "creditLimit": "0",
            "outstandingInvoices": 0
        });
        let result = parse_novita_balance(&body);
        let row = only_row(&result);
        assert_eq!(row.plan_name.as_deref(), Some("Novita AI"));
        assert_eq!(row.remaining, Some(12.3456));
        assert_eq!(row.unit.as_deref(), Some("USD"));
        assert_eq!(row.is_valid, Some(true));

        let empty = parse_novita_balance(&json!({"availableBalance": 0}));
        let row = only_row(&empty);
        assert_eq!(row.remaining, Some(0.0));
        assert_eq!(row.is_valid, Some(false));
        assert_eq!(row.invalid_message.as_deref(), Some("No balance remaining"));
    }

    #[test]
    fn endpoints_and_parsers_are_wired_per_vendor() {
        assert_eq!(
            balance_endpoint(&BalanceProvider::SiliconFlow),
            "https://api.siliconflow.cn/v1/user/info"
        );
        assert_eq!(
            balance_endpoint(&BalanceProvider::SiliconFlowEn),
            "https://api.siliconflow.com/v1/user/info"
        );
        let body = json!({"data": {"totalBalance": 1}});
        let en = parse_balance(&BalanceProvider::SiliconFlowEn, &body);
        assert_eq!(only_row(&en).unit.as_deref(), Some("USD"));
        let cn = parse_balance(&BalanceProvider::SiliconFlow, &body);
        assert_eq!(only_row(&cn).unit.as_deref(), Some("CNY"));
    }
}
