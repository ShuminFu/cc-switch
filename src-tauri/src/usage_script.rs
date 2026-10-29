use rquickjs::{Context, Ctx, Function, Runtime};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use url::{Host, Url};

use crate::error::AppError;

/// 单个 JS 阶段（解析配置 / 执行 extractor）允许的最长执行时间
const JS_EXEC_TIMEOUT: Duration = Duration::from_secs(5);
/// 外层 tokio 超时相对 JS 中断截止时间的宽限，用于兜底单个长耗时原生操作
const JS_TIMEOUT_GRACE: Duration = Duration::from_secs(1);
/// 每个 JS 运行时的内存上限
const JS_MEMORY_LIMIT: usize = 32 * 1024 * 1024;
/// 每个 JS 运行时的最大栈大小
const JS_MAX_STACK_SIZE: usize = 512 * 1024;

/// 执行用量查询脚本
pub async fn execute_usage_script(
    script_code: &str,
    api_key: &str,
    base_url: &str,
    timeout_secs: u64,
    access_token: Option<&str>,
    user_id: Option<&str>,
    template_type: Option<&str>,
) -> Result<Value, AppError> {
    // 检测是否为自定义模板模式
    // 优先使用前端传递的 template_type
    let is_custom_template = template_type.map(|t| t == "custom").unwrap_or(false);

    // 1. 替换模板变量，避免泄露敏感信息
    let script_with_vars =
        build_script_with_vars(script_code, api_key, base_url, access_token, user_id);

    // 2. 验证 base_url 的安全性（仅当提供了 base_url 时）
    // 自定义模板模式下，用户可能不使用模板变量，而是直接在脚本中写完整 URL
    if should_validate_base_url(base_url, is_custom_template) {
        validate_base_url(base_url)?;
    }

    // 3. 在沙箱运行时中提取 request 配置（阻塞线程 + 中断/内存/栈限制）
    let request_config = run_sandboxed(script_with_vars.clone(), JS_EXEC_TIMEOUT, |ctx, script| {
        // 执行用户代码，获取配置对象
        let config: rquickjs::Object = ctx.eval(script).map_err(|e| {
            AppError::localized(
                "usage_script.config_parse_failed",
                format!("解析配置失败: {e}"),
                format!("Failed to parse config: {e}"),
            )
        })?;

        // 提取 request 配置
        let request: rquickjs::Object = config.get("request").map_err(|e| {
            AppError::localized(
                "usage_script.request_missing",
                format!("缺少 request 配置: {e}"),
                format!("Missing request config: {e}"),
            )
        })?;

        // 将 request 转换为 JSON 字符串
        let request_json: String = ctx
            .json_stringify(request)
            .map_err(|e| {
                AppError::localized(
                    "usage_script.request_serialize_failed",
                    format!("序列化 request 失败: {e}"),
                    format!("Failed to serialize request: {e}"),
                )
            })?
            .ok_or_else(|| {
                AppError::localized(
                    "usage_script.serialize_none",
                    "序列化返回 None",
                    "Serialization returned None",
                )
            })?
            .get()
            .map_err(|e| {
                AppError::localized(
                    "usage_script.get_string_failed",
                    format!("获取字符串失败: {e}"),
                    format!("Failed to get string: {e}"),
                )
            })?;

        Ok::<_, AppError>(request_json)
    })
    .await?;

    // 4. 解析 request 配置
    let request: RequestConfig = serde_json::from_str(&request_config).map_err(|e| {
        AppError::localized(
            "usage_script.request_format_invalid",
            format!("request 配置格式错误: {e}"),
            format!("Invalid request config format: {e}"),
        )
    })?;

    // 5. 验证请求 URL（HTTPS 强制 + 同源检查）
    validate_request_url(&request.url, base_url, is_custom_template)?;

    // 6. 发送 HTTP 请求
    let response_data = send_http_request(&request, timeout_secs).await?;

    // 7. 在沙箱运行时中执行 extractor（阻塞线程 + 中断/内存/栈限制）
    let result: Value = run_sandboxed(script_with_vars, JS_EXEC_TIMEOUT, move |ctx, script| {
        // 重新 eval 获取配置对象
        let config: rquickjs::Object = ctx.eval(script).map_err(|e| {
            AppError::localized(
                "usage_script.config_reparse_failed",
                format!("重新解析配置失败: {e}"),
                format!("Failed to re-parse config: {e}"),
            )
        })?;

        // 提取 extractor 函数
        let extractor: Function = config.get("extractor").map_err(|e| {
            AppError::localized(
                "usage_script.extractor_missing",
                format!("缺少 extractor 函数: {e}"),
                format!("Missing extractor function: {e}"),
            )
        })?;

        // 将响应数据转换为 JS 值
        let response_js: rquickjs::Value = ctx.json_parse(response_data.as_str()).map_err(|e| {
            AppError::localized(
                "usage_script.response_parse_failed",
                format!("解析响应 JSON 失败: {e}"),
                format!("Failed to parse response JSON: {e}"),
            )
        })?;

        // 调用 extractor(response)
        let result_js: rquickjs::Value = extractor.call((response_js,)).map_err(|e| {
            AppError::localized(
                "usage_script.extractor_exec_failed",
                format!("执行 extractor 失败: {e}"),
                format!("Failed to execute extractor: {e}"),
            )
        })?;

        // 转换为 JSON 字符串
        let result_json: String = ctx
            .json_stringify(result_js)
            .map_err(|e| {
                AppError::localized(
                    "usage_script.result_serialize_failed",
                    format!("序列化结果失败: {e}"),
                    format!("Failed to serialize result: {e}"),
                )
            })?
            .ok_or_else(|| {
                AppError::localized(
                    "usage_script.serialize_none",
                    "序列化返回 None",
                    "Serialization returned None",
                )
            })?
            .get()
            .map_err(|e| {
                AppError::localized(
                    "usage_script.get_string_failed",
                    format!("获取字符串失败: {e}"),
                    format!("Failed to get string: {e}"),
                )
            })?;

        // 解析为 serde_json::Value
        serde_json::from_str(&result_json).map_err(|e| {
            AppError::localized(
                "usage_script.json_parse_failed",
                format!("JSON 解析失败: {e}"),
                format!("JSON parse failed: {e}"),
            )
        })
    })
    .await?;

    // 8. 验证返回值格式
    validate_result(&result)?;

    Ok(result)
}

fn script_timed_out_error(timeout: Duration) -> AppError {
    let secs = timeout.as_secs();
    AppError::localized(
        "usage_script.timed_out",
        format!("脚本执行超时（超过 {secs} 秒）"),
        format!("Script timed out (exceeded {secs}s)"),
    )
}

/// 在受限的 QuickJS 运行时中执行一个 JS 阶段。
///
/// - 运行在 `spawn_blocking` 线程上，避免死循环脚本占住 tokio 工作线程；
/// - 运行时设置内存上限、栈上限和基于截止时间的中断处理器；
/// - 外层再包一层 `tokio::time::timeout` 兜底。
async fn run_sandboxed<T, F>(script: String, timeout: Duration, phase: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: for<'js> FnOnce(Ctx<'js>, String) -> Result<T, AppError> + Send + 'static,
{
    let interrupted = Arc::new(AtomicBool::new(false));
    let interrupted_flag = interrupted.clone();

    let handle = tokio::task::spawn_blocking(move || {
        let runtime = Runtime::new().map_err(|e| {
            AppError::localized(
                "usage_script.runtime_create_failed",
                format!("创建 JS 运行时失败: {e}"),
                format!("Failed to create JS runtime: {e}"),
            )
        })?;
        runtime.set_memory_limit(JS_MEMORY_LIMIT);
        runtime.set_max_stack_size(JS_MAX_STACK_SIZE);
        let deadline = Instant::now() + timeout;
        runtime.set_interrupt_handler(Some(Box::new(move || {
            if Instant::now() >= deadline {
                interrupted_flag.store(true, Ordering::SeqCst);
                true
            } else {
                false
            }
        })));

        let context = Context::full(&runtime).map_err(|e| {
            AppError::localized(
                "usage_script.context_create_failed",
                format!("创建 JS 上下文失败: {e}"),
                format!("Failed to create JS context: {e}"),
            )
        })?;

        context.with(|ctx| phase(ctx, script))
    }); // Runtime 和 Context 在阻塞线程内被 drop

    let outcome = match tokio::time::timeout(timeout + JS_TIMEOUT_GRACE, handle).await {
        Ok(Ok(result)) => result,
        Ok(Err(e)) => Err(AppError::localized(
            "usage_script.sandbox_failed",
            format!("脚本执行线程异常: {e}"),
            format!("Script execution thread failed: {e}"),
        )),
        Err(_) => return Err(script_timed_out_error(timeout)),
    };

    // 被中断处理器终止的脚本统一报告为超时，而不是笼统的解析/执行失败
    match outcome {
        Err(_) if interrupted.load(Ordering::SeqCst) => Err(script_timed_out_error(timeout)),
        other => other,
    }
}

/// 请求配置结构
#[derive(Debug, serde::Deserialize)]
struct RequestConfig {
    url: String,
    method: String,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    body: Option<String>,
}

/// 发送 HTTP 请求
async fn send_http_request(config: &RequestConfig, timeout_secs: u64) -> Result<String, AppError> {
    // 使用全局 HTTP 客户端（已包含代理配置）
    let client = crate::proxy::http_client::get();
    // 约束超时范围，防止异常配置导致长时间阻塞（最小 2 秒，最大 30 秒）
    let request_timeout = std::time::Duration::from_secs(timeout_secs.clamp(2, 30));

    // 严格校验 HTTP 方法，非法值不回退为 GET
    let method: reqwest::Method = config.method.parse().map_err(|_| {
        AppError::localized(
            "usage_script.invalid_http_method",
            format!("不支持的 HTTP 方法: {}", config.method),
            format!("Unsupported HTTP method: {}", config.method),
        )
    })?;

    let mut req = client
        .request(method.clone(), &config.url)
        .timeout(request_timeout);

    // 添加请求头
    for (k, v) in &config.headers {
        req = req.header(k, v);
    }

    // 添加请求体
    if let Some(body) = &config.body {
        req = req.body(body.clone());
    }

    // 发送请求
    let resp = req.send().await.map_err(|e| {
        AppError::localized(
            "usage_script.request_failed",
            format!("请求失败: {e}"),
            format!("Request failed: {e}"),
        )
    })?;

    let status = resp.status();
    let text = resp.text().await.map_err(|e| {
        AppError::localized(
            "usage_script.read_response_failed",
            format!("读取响应失败: {e}"),
            format!("Failed to read response: {e}"),
        )
    })?;

    if !status.is_success() {
        let preview = if text.len() > 200 {
            let mut safe_cut = 200usize;
            while !text.is_char_boundary(safe_cut) {
                safe_cut = safe_cut.saturating_sub(1);
            }
            format!("{}...", &text[..safe_cut])
        } else {
            text.clone()
        };
        return Err(AppError::localized(
            "usage_script.http_error",
            format!("HTTP {status} : {preview}"),
            format!("HTTP {status} : {preview}"),
        ));
    }

    Ok(text)
}

/// 验证脚本返回值（支持单对象或数组）
fn validate_result(result: &Value) -> Result<(), AppError> {
    // 如果是数组，验证每个元素
    if let Some(arr) = result.as_array() {
        if arr.is_empty() {
            return Err(AppError::localized(
                "usage_script.empty_array",
                "脚本返回的数组不能为空",
                "Script returned empty array",
            ));
        }
        for (idx, item) in arr.iter().enumerate() {
            validate_single_usage(item).map_err(|e| {
                AppError::localized(
                    "usage_script.array_validation_failed",
                    format!("数组索引[{idx}]验证失败: {e}"),
                    format!("Validation failed at index [{idx}]: {e}"),
                )
            })?;
        }
        return Ok(());
    }

    // 如果是单对象，直接验证（向后兼容）
    validate_single_usage(result)
}

/// 验证单个用量数据对象
fn validate_single_usage(result: &Value) -> Result<(), AppError> {
    let obj = result.as_object().ok_or_else(|| {
        AppError::localized(
            "usage_script.must_return_object",
            "脚本必须返回对象或对象数组",
            "Script must return object or array of objects",
        )
    })?;

    // 所有字段均为可选，只进行类型检查
    if obj.contains_key("isValid")
        && !result["isValid"].is_null()
        && !result["isValid"].is_boolean()
    {
        return Err(AppError::localized(
            "usage_script.isvalid_type_error",
            "isValid 必须是布尔值或 null",
            "isValid must be boolean or null",
        ));
    }
    if obj.contains_key("invalidMessage")
        && !result["invalidMessage"].is_null()
        && !result["invalidMessage"].is_string()
    {
        return Err(AppError::localized(
            "usage_script.invalidmessage_type_error",
            "invalidMessage 必须是字符串或 null",
            "invalidMessage must be string or null",
        ));
    }
    if obj.contains_key("remaining")
        && !result["remaining"].is_null()
        && !result["remaining"].is_number()
    {
        return Err(AppError::localized(
            "usage_script.remaining_type_error",
            "remaining 必须是数字或 null",
            "remaining must be number or null",
        ));
    }
    if obj.contains_key("unit") && !result["unit"].is_null() && !result["unit"].is_string() {
        return Err(AppError::localized(
            "usage_script.unit_type_error",
            "unit 必须是字符串或 null",
            "unit must be string or null",
        ));
    }
    if obj.contains_key("total") && !result["total"].is_null() && !result["total"].is_number() {
        return Err(AppError::localized(
            "usage_script.total_type_error",
            "total 必须是数字或 null",
            "total must be number or null",
        ));
    }
    if obj.contains_key("used") && !result["used"].is_null() && !result["used"].is_number() {
        return Err(AppError::localized(
            "usage_script.used_type_error",
            "used 必须是数字或 null",
            "used must be number or null",
        ));
    }
    if obj.contains_key("planName")
        && !result["planName"].is_null()
        && !result["planName"].is_string()
    {
        return Err(AppError::localized(
            "usage_script.planname_type_error",
            "planName 必须是字符串或 null",
            "planName must be string or null",
        ));
    }
    if obj.contains_key("extra") && !result["extra"].is_null() && !result["extra"].is_string() {
        return Err(AppError::localized(
            "usage_script.extra_type_error",
            "extra 必须是字符串或 null",
            "extra must be string or null",
        ));
    }

    Ok(())
}

/// 构建替换变量后的脚本，保持与旧版脚本的兼容性
fn build_script_with_vars(
    script_code: &str,
    api_key: &str,
    base_url: &str,
    access_token: Option<&str>,
    user_id: Option<&str>,
) -> String {
    let mut replaced = script_code
        .replace("{{apiKey}}", api_key)
        .replace("{{baseUrl}}", base_url);

    if let Some(token) = access_token {
        replaced = replaced.replace("{{accessToken}}", token);
    }
    if let Some(uid) = user_id {
        replaced = replaced.replace("{{userId}}", uid);
    }

    replaced
}

/// 验证 base_url 的基本安全性
fn validate_base_url(base_url: &str) -> Result<(), AppError> {
    if base_url.is_empty() {
        return Err(AppError::localized(
            "usage_script.base_url_empty",
            "base_url 不能为空",
            "base_url cannot be empty",
        ));
    }

    // 解析 URL
    let parsed_url = Url::parse(base_url).map_err(|e| {
        AppError::localized(
            "usage_script.base_url_invalid",
            format!("无效的 base_url: {e}"),
            format!("Invalid base_url: {e}"),
        )
    })?;

    let is_loopback = is_loopback_host(&parsed_url);

    // 必须是 HTTPS（允许 localhost 用于开发）
    if parsed_url.scheme() != "https" && !is_loopback {
        return Err(AppError::localized(
            "usage_script.base_url_https_required",
            "base_url 必须使用 HTTPS 协议（localhost 除外）",
            "base_url must use HTTPS (localhost allowed)",
        ));
    }

    // 检查主机名格式有效性
    let hostname = parsed_url.host_str().ok_or_else(|| {
        AppError::localized(
            "usage_script.base_url_hostname_missing",
            "base_url 必须包含有效的主机名",
            "base_url must include a valid hostname",
        )
    })?;

    // 基本的主机名格式检查
    if hostname.is_empty() {
        return Err(AppError::localized(
            "usage_script.base_url_hostname_empty",
            "base_url 主机名不能为空",
            "base_url hostname cannot be empty",
        ));
    }

    Ok(())
}

fn should_validate_base_url(base_url: &str, is_custom_template: bool) -> bool {
    !base_url.is_empty() && !is_custom_template
}

/// 验证请求 URL 是否安全（HTTPS 强制 + 同源检查）
fn validate_request_url(
    request_url: &str,
    base_url: &str,
    is_custom_template: bool,
) -> Result<(), AppError> {
    // 解析请求 URL
    let parsed_request = Url::parse(request_url).map_err(|e| {
        AppError::localized(
            "usage_script.request_url_invalid",
            format!("无效的请求 URL: {e}"),
            format!("Invalid request URL: {e}"),
        )
    })?;

    let is_request_loopback = is_loopback_host(&parsed_request);

    // 必须使用 HTTPS（允许 localhost 用于开发）
    // 自定义模板模式下，允许用户自行决定是否使用 HTTP（用户需自行承担安全风险）
    if !is_custom_template && parsed_request.scheme() != "https" && !is_request_loopback {
        return Err(AppError::localized(
            "usage_script.request_https_required",
            "请求 URL 必须使用 HTTPS 协议（localhost 除外）",
            "Request URL must use HTTPS (localhost allowed)",
        ));
    }

    // 如果提供了 base_url（非空），则进行同源检查
    // 🔧 自定义模板模式下，用户可以自由访问任意 HTTPS 域名，跳过同源检查
    if !base_url.is_empty() && !is_custom_template {
        // 解析 base URL
        let parsed_base = Url::parse(base_url).map_err(|e| {
            AppError::localized(
                "usage_script.base_url_invalid",
                format!("无效的 base_url: {e}"),
                format!("Invalid base_url: {e}"),
            )
        })?;

        // 核心安全检查：必须与 base_url 同源（相同域名和端口）
        if parsed_request.host_str() != parsed_base.host_str() {
            return Err(AppError::localized(
                "usage_script.request_host_mismatch",
                format!(
                    "请求域名 {} 与 base_url 域名 {} 不匹配（必须是同源请求）",
                    parsed_request.host_str().unwrap_or("unknown"),
                    parsed_base.host_str().unwrap_or("unknown")
                ),
                format!(
                    "Request host {} must match base_url host {} (same-origin required)",
                    parsed_request.host_str().unwrap_or("unknown"),
                    parsed_base.host_str().unwrap_or("unknown")
                ),
            ));
        }

        // 检查端口是否匹配（考虑默认端口）
        // 使用 port_or_known_default() 会自动处理默认端口（http->80, https->443）
        match (
            parsed_request.port_or_known_default(),
            parsed_base.port_or_known_default(),
        ) {
            (Some(request_port), Some(base_port)) if request_port == base_port => {
                // 端口匹配，继续执行
            }
            (Some(request_port), Some(base_port)) => {
                return Err(AppError::localized(
                    "usage_script.request_port_mismatch",
                    format!("请求端口 {request_port} 必须与 base_url 端口 {base_port} 匹配"),
                    format!("Request port {request_port} must match base_url port {base_port}"),
                ));
            }
            _ => {
                // 理论上不会发生，因为 port_or_known_default() 应该总是返回 Some
                return Err(AppError::localized(
                    "usage_script.request_port_unknown",
                    "无法确定端口号",
                    "Unable to determine port number",
                ));
            }
        }
    }

    Ok(())
}

/// 判断 URL 是否指向本机（localhost / loopback）
fn is_loopback_host(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 在独立线程中运行，并以硬超时兜底：即使脚本卡住执行线程，测试也会失败而不是挂起
    fn run_with_hard_timeout<T, F>(limit: Duration, f: F) -> T
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        rx.recv_timeout(limit)
            .expect("usage script did not finish within the hard test timeout")
    }

    fn run_custom_script(script: &'static str) -> (Result<Value, AppError>, Duration) {
        run_with_hard_timeout(Duration::from_secs(10), move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let started = Instant::now();
            let result = rt.block_on(execute_usage_script(
                script,
                "",
                "",
                10,
                None,
                None,
                Some("custom"),
            ));
            (result, started.elapsed())
        })
    }

    fn error_key(err: &AppError) -> &'static str {
        match err {
            AppError::Localized { key, .. } => key,
            other => panic!("expected localized error, got {other}"),
        }
    }

    #[test]
    fn usage_script_infinite_loop_is_interrupted_near_deadline() {
        let (result, elapsed) = run_custom_script("while(true){}");
        let err = result.expect_err("infinite loop must not succeed");
        assert_eq!(error_key(&err), "usage_script.timed_out");
        assert!(
            elapsed >= JS_EXEC_TIMEOUT - Duration::from_millis(500)
                && elapsed < JS_EXEC_TIMEOUT + JS_TIMEOUT_GRACE + Duration::from_secs(2),
            "loop should stop near the {JS_EXEC_TIMEOUT:?} deadline, took {elapsed:?}"
        );
    }

    #[test]
    fn usage_script_huge_allocation_hits_memory_limit() {
        // 未设内存上限时该脚本可以成功解析并走到 HTTP 请求阶段
        let (result, _) = run_custom_script(
            "const s = 'a'.repeat(1e9); \
             ({ request: { url: 'http://127.0.0.1:1/' + s.length, method: 'GET' }, \
                extractor: function (r) { return r; } })",
        );
        let err = result.expect_err("1e9-char string must exceed the memory limit");
        assert_eq!(error_key(&err), "usage_script.config_parse_failed");
    }

    #[test]
    fn usage_script_infinite_recursion_errors_without_stack_overflow() {
        let (result, _) = run_custom_script("function f() { return f() + 1; } f();");
        let err = result.expect_err("unbounded recursion must fail");
        assert_eq!(error_key(&err), "usage_script.config_parse_failed");
    }

    #[test]
    fn usage_script_extractor_phase_is_sandboxed() {
        let result = run_with_hard_timeout(Duration::from_secs(10), || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(run_sandboxed(
                "({ extractor: function () { for (;;) {} } })".to_string(),
                Duration::from_millis(300),
                |ctx, script| {
                    let config: rquickjs::Object = ctx
                        .eval(script)
                        .map_err(|e| AppError::Message(e.to_string()))?;
                    let extractor: Function = config
                        .get("extractor")
                        .map_err(|e| AppError::Message(e.to_string()))?;
                    extractor
                        .call::<_, rquickjs::Value>(())
                        .map(|_| ())
                        .map_err(|e| AppError::Message(e.to_string()))
                },
            ))
        });
        let err = result.expect_err("looping extractor must be interrupted");
        assert_eq!(error_key(&err), "usage_script.timed_out");
    }

    #[test]
    fn usage_script_sandbox_still_returns_normal_results() {
        let value = run_with_hard_timeout(Duration::from_secs(10), || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(run_sandboxed(
                "[1, 2, 3].map(function (x) { return x * 2; }).join(',')".to_string(),
                JS_EXEC_TIMEOUT,
                |ctx, script| {
                    ctx.eval::<String, _>(script)
                        .map_err(|e| AppError::Message(e.to_string()))
                },
            ))
        })
        .expect("plain script should evaluate");
        assert_eq!(value, "2,4,6");
    }

    #[test]
    fn usage_script_looping_scripts_do_not_starve_runtime_workers() {
        let sleep_elapsed = run_with_hard_timeout(Duration::from_secs(10), || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                let loops: Vec<_> = (0..4)
                    .map(|_| {
                        tokio::spawn(execute_usage_script(
                            "while(true){}",
                            "",
                            "",
                            10,
                            None,
                            None,
                            Some("custom"),
                        ))
                    })
                    .collect();
                // 让循环脚本先占住执行线程
                tokio::time::sleep(Duration::from_millis(200)).await;

                let started = Instant::now();
                tokio::spawn(tokio::time::sleep(Duration::from_millis(100)))
                    .await
                    .unwrap();
                let sleep_elapsed = started.elapsed();

                for handle in loops {
                    let err = handle.await.unwrap().expect_err("loop must time out");
                    assert_eq!(error_key(&err), "usage_script.timed_out");
                }
                sleep_elapsed
            })
        });
        assert!(
            sleep_elapsed < Duration::from_secs(2),
            "unrelated sleep task was starved for {sleep_elapsed:?}"
        );
    }

    #[test]
    fn test_https_bypass_prevention() {
        // 非本地域名的 HTTP 应该被拒绝
        let result = validate_base_url("http://127.0.0.1.evil.com/api");
        assert!(
            result.is_err(),
            "Should reject HTTP for non-localhost domains"
        );
    }

    #[test]
    fn test_custom_template_allows_http_lan_request_with_different_base_url() {
        assert!(
            !should_validate_base_url("http://10.37.192.156:8090/anthropic", true),
            "Custom scripts should not validate an unused provider base_url fallback"
        );

        let result = validate_request_url(
            "http://10.37.192.156:18344/user/balance",
            "http://10.37.192.156:8090/anthropic",
            true,
        );
        assert!(
            result.is_ok(),
            "Custom usage scripts should be able to call an explicit HTTP quota endpoint"
        );
    }

    #[test]
    fn test_port_comparison() {
        // 测试端口比较逻辑是否正确处理默认端口和显式端口

        // 测试用例：(base_url, request_url, should_match)
        let test_cases = vec![
            // HTTPS默认端口测试
            (
                "https://api.example.com",
                "https://api.example.com/v1/test",
                true,
            ),
            (
                "https://api.example.com",
                "https://api.example.com:443/v1/test",
                true,
            ),
            (
                "https://api.example.com:443",
                "https://api.example.com/v1/test",
                true,
            ),
            (
                "https://api.example.com:443",
                "https://api.example.com:443/v1/test",
                true,
            ),
            // 端口不匹配测试
            (
                "https://api.example.com",
                "https://api.example.com:8443/v1/test",
                false,
            ),
            (
                "https://api.example.com:443",
                "https://api.example.com:8443/v1/test",
                false,
            ),
        ];

        for (base_url, request_url, should_match) in test_cases {
            let result = validate_request_url(request_url, base_url, false);

            if should_match {
                assert!(
                    result.is_ok(),
                    "应该匹配的URL被拒绝: base_url={}, request_url={}, error={}",
                    base_url,
                    request_url,
                    result.unwrap_err()
                );
            } else {
                assert!(
                    result.is_err(),
                    "应该不匹配的URL被允许: base_url={}, request_url={}",
                    base_url,
                    request_url
                );
            }
        }
    }
}
