//! 全局出站代理（CCS-181 自命令层迁入）
//!
//! 读取 / 设置 / 测试全局出站代理 URL。设置时先验证、再写库、最后应用到运行态，
//! 保证写库失败时运行态与持久化不会不一致。

use std::time::Instant;

use serde::Serialize;

use crate::error::AppError;
use crate::proxy::http_client;
use crate::store::AppState;

/// 当前配置的代理 URL，None 表示直连
pub fn get_url(state: &AppState) -> Result<Option<String>, AppError> {
    let result = state.db.get_global_proxy_url()?;
    log::debug!(
        "[GlobalProxy] [GP-010] Read from database: {}",
        result
            .as_ref()
            .map(|u| http_client::mask_url(u))
            .unwrap_or_else(|| "None".to_string())
    );
    Ok(result)
}

/// 设置代理 URL；空字符串清除代理（直连）
pub fn set_url(state: &AppState, url: &str) -> Result<(), AppError> {
    // 调试：显示接收到的 URL 信息（不包含敏感内容）
    let has_auth = url.contains('@') && (url.starts_with("http://") || url.starts_with("socks"));
    log::debug!(
        "[GlobalProxy] [GP-011] Received URL: length={}, has_auth={}",
        url.len(),
        has_auth
    );

    let url_opt = if url.trim().is_empty() {
        None
    } else {
        Some(url)
    };

    // 1. 先验证代理配置是否有效（不应用）
    http_client::validate_proxy(url_opt).map_err(AppError::Message)?;
    // 2. 验证成功后保存到数据库
    state.db.set_global_proxy_url(url_opt)?;
    // 3. DB 写入成功后再应用到运行态
    http_client::apply_proxy(url_opt).map_err(AppError::Message)?;

    log::info!(
        "[GlobalProxy] [GP-009] Configuration updated: {}",
        url_opt
            .map(http_client::mask_url)
            .unwrap_or_else(|| "direct connection".to_string())
    );
    Ok(())
}

/// 代理测试结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyTestResult {
    /// 是否连接成功
    pub success: bool,
    /// 延迟（毫秒）
    pub latency_ms: u64,
    /// 错误信息
    pub error: Option<String>,
}

/// 测试代理连接
///
/// 通过指定的代理 URL 发送测试请求，返回连接结果和延迟。
/// 使用多个测试目标，任一成功即认为代理可用。
pub async fn test_url(url: &str) -> Result<ProxyTestResult, AppError> {
    if url.trim().is_empty() {
        return Err(AppError::Message("Proxy URL is empty".to_string()));
    }

    let start = Instant::now();

    // 构建带代理的临时客户端
    let proxy = reqwest::Proxy::all(url)
        .map_err(|e| AppError::Message(format!("Invalid proxy URL: {e}")))?;

    crate::proxy::http_client::ensure_tls_provider();
    let client = reqwest::Client::builder()
        .proxy(proxy)
        .timeout(std::time::Duration::from_secs(10))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| AppError::Message(format!("Failed to build client: {e}")))?;

    // 使用多个测试目标，提高兼容性
    // 优先使用 httpbin（专门用于 HTTP 测试），回退到其他公共端点
    let test_urls = [
        "https://httpbin.org/get",
        "https://www.google.com",
        "https://api.anthropic.com",
    ];

    let mut last_error = None;

    for test_url in test_urls {
        match client.head(test_url).send().await {
            Ok(resp) => {
                let latency = start.elapsed().as_millis() as u64;
                log::debug!(
                    "[GlobalProxy] Test successful: {} -> {} via {} ({}ms)",
                    http_client::mask_url(url),
                    test_url,
                    resp.status(),
                    latency
                );
                return Ok(ProxyTestResult {
                    success: true,
                    latency_ms: latency,
                    error: None,
                });
            }
            Err(e) => {
                log::debug!("[GlobalProxy] Test to {test_url} failed: {e}");
                last_error = Some(e);
            }
        }
    }

    // 所有测试目标都失败
    let latency = start.elapsed().as_millis() as u64;
    let error_msg = last_error
        .map(|e| e.to_string())
        .unwrap_or_else(|| "All test targets failed".to_string());

    log::debug!(
        "[GlobalProxy] Test failed: {} -> {} ({}ms)",
        http_client::mask_url(url),
        error_msg,
        latency
    );

    Ok(ProxyTestResult {
        success: false,
        latency_ms: latency,
        error: Some(error_msg),
    })
}
