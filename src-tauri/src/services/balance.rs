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

fn detect_provider(base_url: &str) -> Option<BalanceProvider> {
    let url = base_url.to_lowercase();
    if url.contains("api.deepseek.com") {
        Some(BalanceProvider::DeepSeek)
    } else if url.contains("api.stepfun.ai") || url.contains("api.stepfun.com") {
        Some(BalanceProvider::StepFun)
    } else if url.contains("api.siliconflow.cn") {
        Some(BalanceProvider::SiliconFlow)
    } else if url.contains("api.siliconflow.com") {
        Some(BalanceProvider::SiliconFlowEn)
    } else if url.contains("openrouter.ai") {
        Some(BalanceProvider::OpenRouter)
    } else if url.contains("api.novita.ai") {
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

// ── DeepSeek ────────────────────────────────────────────────
// GET https://api.deepseek.com/user/balance
// Response: { balance_infos: [{ currency, total_balance, granted_balance, topped_up_balance }], is_available }

async fn query_deepseek(api_key: &str) -> Result<UsageResult, String> {
    let client = crate::proxy::http_client::get();

    let resp = client
        .get("https://api.deepseek.com/user/balance")
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
        return Ok(make_auth_error(status));
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Ok(make_error(format!("API error (HTTP {status}): {body}")));
    }

    // 先 bytes() 再解析：读体失败（超时/连接中断）是瞬时 → Err；拿到完整响应体
    // 后解析失败才是确定性。reqwest 的 json() 把读体错误也包成 decode，无法区分。
    let raw = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => return Err(format!("Failed to read response: {e}")),
    };
    let body: serde_json::Value = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(e) => return Ok(make_error(format!("Failed to parse response: {e}"))),
    };

    Ok(parse_deepseek_balance(&body))
}

/// 解析 DeepSeek 余额响应体
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

async fn query_stepfun(api_key: &str) -> Result<UsageResult, String> {
    let client = crate::proxy::http_client::get();

    let resp = client
        .get("https://api.stepfun.com/v1/accounts")
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
        return Ok(make_auth_error(status));
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Ok(make_error(format!("API error (HTTP {status}): {body}")));
    }

    // 先 bytes() 再解析：读体失败（超时/连接中断）是瞬时 → Err；拿到完整响应体
    // 后解析失败才是确定性。reqwest 的 json() 把读体错误也包成 decode，无法区分。
    let raw = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => return Err(format!("Failed to read response: {e}")),
    };
    let body: serde_json::Value = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(e) => return Ok(make_error(format!("Failed to parse response: {e}"))),
    };

    Ok(parse_stepfun_balance(&body))
}

/// 解析 StepFun 余额响应体
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

async fn query_siliconflow(api_key: &str, is_cn: bool) -> Result<UsageResult, String> {
    let client = crate::proxy::http_client::get();

    let domain = if is_cn {
        "api.siliconflow.cn"
    } else {
        "api.siliconflow.com"
    };
    let url = format!("https://{domain}/v1/user/info");

    let resp = client
        .get(&url)
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
        return Ok(make_auth_error(status));
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Ok(make_error(format!("API error (HTTP {status}): {body}")));
    }

    // 先 bytes() 再解析：读体失败（超时/连接中断）是瞬时 → Err；拿到完整响应体
    // 后解析失败才是确定性。reqwest 的 json() 把读体错误也包成 decode，无法区分。
    let raw = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => return Err(format!("Failed to read response: {e}")),
    };
    let body: serde_json::Value = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(e) => return Ok(make_error(format!("Failed to parse response: {e}"))),
    };

    Ok(parse_siliconflow_balance(&body, is_cn))
}

/// 解析 SiliconFlow 余额响应体（is_cn 区分 .cn / .com 站点）
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

async fn query_openrouter(api_key: &str) -> Result<UsageResult, String> {
    let client = crate::proxy::http_client::get();

    let resp = client
        .get("https://openrouter.ai/api/v1/credits")
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
        return Ok(make_auth_error(status));
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Ok(make_error(format!("API error (HTTP {status}): {body}")));
    }

    // 先 bytes() 再解析：读体失败（超时/连接中断）是瞬时 → Err；拿到完整响应体
    // 后解析失败才是确定性。reqwest 的 json() 把读体错误也包成 decode，无法区分。
    let raw = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => return Err(format!("Failed to read response: {e}")),
    };
    let body: serde_json::Value = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(e) => return Ok(make_error(format!("Failed to parse response: {e}"))),
    };

    Ok(parse_openrouter_balance(&body))
}

/// 解析 OpenRouter 额度响应体（剩余 = total_credits - total_usage）
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

async fn query_novita(api_key: &str) -> Result<UsageResult, String> {
    let client = crate::proxy::http_client::get();

    let resp = client
        .get("https://api.novita.ai/v3/user/balance")
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
        return Ok(make_auth_error(status));
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Ok(make_error(format!("API error (HTTP {status}): {body}")));
    }

    // 先 bytes() 再解析：读体失败（超时/连接中断）是瞬时 → Err；拿到完整响应体
    // 后解析失败才是确定性。reqwest 的 json() 把读体错误也包成 decode，无法区分。
    let raw = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => return Err(format!("Failed to read response: {e}")),
    };
    let body: serde_json::Value = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(e) => return Ok(make_error(format!("Failed to parse response: {e}"))),
    };

    Ok(parse_novita_balance(&body))
}

/// 解析 Novita AI 余额响应体
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

    match provider {
        BalanceProvider::DeepSeek => query_deepseek(api_key).await,
        BalanceProvider::StepFun => query_stepfun(api_key).await,
        BalanceProvider::SiliconFlow => query_siliconflow(api_key, true).await,
        BalanceProvider::SiliconFlowEn => query_siliconflow(api_key, false).await,
        BalanceProvider::OpenRouter => query_openrouter(api_key).await,
        BalanceProvider::NovitaAI => query_novita(api_key).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn single(result: &UsageResult) -> &UsageData {
        let data = result.data.as_ref().expect("data present");
        assert_eq!(data.len(), 1);
        &data[0]
    }

    #[test]
    fn detect_provider_matches_known_hosts_case_insensitively() {
        assert!(matches!(
            detect_provider("https://API.DeepSeek.com/anthropic"),
            Some(BalanceProvider::DeepSeek)
        ));
        assert!(matches!(
            detect_provider("https://api.stepfun.ai/v1"),
            Some(BalanceProvider::StepFun)
        ));
        assert!(matches!(
            detect_provider("https://api.stepfun.com/v1"),
            Some(BalanceProvider::StepFun)
        ));
        assert!(matches!(
            detect_provider("https://api.siliconflow.cn/v1"),
            Some(BalanceProvider::SiliconFlow)
        ));
        assert!(matches!(
            detect_provider("https://api.siliconflow.com/v1"),
            Some(BalanceProvider::SiliconFlowEn)
        ));
        assert!(matches!(
            detect_provider("https://openrouter.ai/api/v1"),
            Some(BalanceProvider::OpenRouter)
        ));
        assert!(matches!(
            detect_provider("https://api.novita.ai/openai"),
            Some(BalanceProvider::NovitaAI)
        ));
        assert!(detect_provider("https://api.anthropic.com").is_none());
    }

    #[test]
    fn parse_f64_field_accepts_numbers_and_numeric_strings() {
        let obj = json!({ "n": 1.5, "s": "2.25", "bad": "abc", "b": true });
        assert_eq!(parse_f64_field(&obj, "n"), Some(1.5));
        assert_eq!(parse_f64_field(&obj, "s"), Some(2.25));
        assert_eq!(parse_f64_field(&obj, "bad"), None);
        assert_eq!(parse_f64_field(&obj, "b"), None);
        assert_eq!(parse_f64_field(&obj, "missing"), None);
    }

    #[test]
    fn deepseek_parses_each_currency_entry() {
        let body = json!({
            "is_available": true,
            "balance_infos": [
                { "currency": "CNY", "total_balance": "110.00" },
                { "currency": "USD", "total_balance": 3.5 }
            ]
        });
        let result = parse_deepseek_balance(&body);
        assert!(result.success);
        let data = result.data.expect("data");
        assert_eq!(data.len(), 2);
        assert_eq!(data[0].plan_name.as_deref(), Some("CNY"));
        assert_eq!(data[0].remaining, Some(110.0));
        assert_eq!(data[0].is_valid, Some(true));
        assert!(data[0].invalid_message.is_none());
        assert_eq!(data[1].unit.as_deref(), Some("USD"));
        assert_eq!(data[1].remaining, Some(3.5));
    }

    #[test]
    fn deepseek_marks_unavailable_and_handles_empty_infos() {
        let body = json!({
            "is_available": false,
            "balance_infos": [{ "total_balance": "0" }]
        });
        let result = parse_deepseek_balance(&body);
        let entry = single(&result);
        assert_eq!(entry.unit.as_deref(), Some("CNY"));
        assert_eq!(entry.is_valid, Some(false));
        assert_eq!(
            entry.invalid_message.as_deref(),
            Some("Insufficient balance")
        );

        let empty = parse_deepseek_balance(&json!({ "balance_infos": [] }));
        assert!(empty.success);
        assert!(empty.data.is_none());
    }

    #[test]
    fn stepfun_reads_balance_with_zero_fallback() {
        let result = parse_stepfun_balance(&json!({ "balance": "12.5" }));
        let entry = single(&result);
        assert_eq!(entry.remaining, Some(12.5));
        assert_eq!(entry.unit.as_deref(), Some("CNY"));

        let missing = parse_stepfun_balance(&json!({}));
        assert_eq!(single(&missing).remaining, Some(0.0));
    }

    #[test]
    fn siliconflow_uses_total_balance_and_site_currency() {
        let body = json!({ "code": 20000, "data": { "balance": "1", "totalBalance": "8.88" } });

        let cn = parse_siliconflow_balance(&body, true);
        let entry = single(&cn);
        assert_eq!(entry.plan_name.as_deref(), Some("SiliconFlow"));
        assert_eq!(entry.remaining, Some(8.88));
        assert_eq!(entry.unit.as_deref(), Some("CNY"));

        let en = parse_siliconflow_balance(&body, false);
        let entry = single(&en);
        assert_eq!(entry.plan_name.as_deref(), Some("SiliconFlow (EN)"));
        assert_eq!(entry.unit.as_deref(), Some("USD"));
    }

    #[test]
    fn siliconflow_missing_data_is_error() {
        let result = parse_siliconflow_balance(&json!({ "code": 20000 }), true);
        assert!(!result.success);
        assert!(result.data.is_none());
        assert_eq!(
            result.error.as_deref(),
            Some("Missing 'data' field in response")
        );
    }

    #[test]
    fn openrouter_computes_remaining_from_credits_and_usage() {
        let body = json!({ "data": { "total_credits": 20.0, "total_usage": 5.5 } });
        let result = parse_openrouter_balance(&body);
        let entry = single(&result);
        assert_eq!(entry.total, Some(20.0));
        assert_eq!(entry.used, Some(5.5));
        assert_eq!(entry.remaining, Some(14.5));
        assert_eq!(entry.is_valid, Some(true));

        // 无 data 包裹时直接读取顶层字段
        let exhausted = parse_openrouter_balance(&json!({
            "total_credits": 10,
            "total_usage": 10
        }));
        let entry = single(&exhausted);
        assert_eq!(entry.remaining, Some(0.0));
        assert_eq!(entry.is_valid, Some(false));
        assert_eq!(
            entry.invalid_message.as_deref(),
            Some("No credits remaining")
        );
    }

    #[test]
    fn novita_converts_ten_thousandths_to_usd() {
        let result = parse_novita_balance(&json!({ "availableBalance": 123456 }));
        let entry = single(&result);
        assert_eq!(entry.remaining, Some(12.3456));
        assert_eq!(entry.unit.as_deref(), Some("USD"));
        assert_eq!(entry.is_valid, Some(true));

        let empty = parse_novita_balance(&json!({ "availableBalance": "0" }));
        let entry = single(&empty);
        assert_eq!(entry.is_valid, Some(false));
        assert_eq!(
            entry.invalid_message.as_deref(),
            Some("No balance remaining")
        );
    }

    #[test]
    fn auth_error_marks_entry_invalid() {
        let result = make_auth_error(reqwest::StatusCode::UNAUTHORIZED);
        assert!(!result.success);
        let entry = single(&result);
        assert_eq!(entry.is_valid, Some(false));
        assert!(result.error.as_deref().unwrap().contains("401"));
    }

    #[tokio::test]
    async fn get_balance_rejects_empty_key_and_unknown_provider() {
        let empty = get_balance("https://api.deepseek.com", "  ")
            .await
            .expect("deterministic failure is Ok");
        assert!(!empty.success);
        assert_eq!(empty.error.as_deref(), Some("API key is empty"));

        let unknown = get_balance("https://example.com", "sk-test")
            .await
            .expect("deterministic failure is Ok");
        assert!(!unknown.success);
        assert_eq!(unknown.error.as_deref(), Some("Unknown balance provider"));
    }
}
