use rquickjs::{CaughtError, Context, Ctx, Function, Object, Runtime};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use url::{Host, Url};

use crate::error::AppError;

/// QuickJS 堆内存上限：用量脚本只做简单的请求拼装与字段提取，64 MiB 绰绰有余
const SCRIPT_MEMORY_LIMIT: usize = 64 * 1024 * 1024;
/// QuickJS 栈上限：防止深递归耗尽阻塞线程的原生栈（tokio 阻塞线程默认 2 MiB）
const SCRIPT_MAX_STACK_SIZE: usize = 512 * 1024;
/// 用量接口响应体上限，防止异常/恶意服务端返回超大响应耗尽内存
const MAX_RESPONSE_BODY_BYTES: usize = 2 * 1024 * 1024;

/// 注入脚本的变量（以数据形式传入，而非拼接进源码）
#[derive(Debug, Clone)]
struct ScriptVars {
    api_key: String,
    base_url: String,
    access_token: Option<String>,
    user_id: Option<String>,
}

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
    execute_usage_script_with_client(
        crate::proxy::http_client::get_no_redirect(),
        script_code,
        api_key,
        base_url,
        timeout_secs,
        access_token,
        user_id,
        template_type,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn execute_usage_script_with_client(
    client: reqwest::Client,
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

    // 1. 验证 base_url 的安全性（仅当提供了 base_url 时）
    // 自定义模板模式下，用户可能不使用模板变量，而是直接在脚本中写完整 URL
    if should_validate_base_url(base_url, is_custom_template) {
        validate_base_url(base_url)?;
    }

    // 约束超时范围，防止异常配置导致长时间阻塞（最小 2 秒，最大 30 秒）。
    // 同一个值既用于 HTTP 请求，也用于每个 JS 执行阶段的执行时间上限。
    let timeout = Duration::from_secs(timeout_secs.clamp(2, 30));

    let vars = ScriptVars {
        api_key: api_key.to_string(),
        base_url: base_url.to_string(),
        access_token: access_token.map(str::to_string),
        user_id: user_id.map(str::to_string),
    };
    let script = build_script_with_vars(script_code, &vars);

    // 2. QuickJS 是同步执行的：放到阻塞线程上运行，避免占住 async 运行时的 worker。
    // Runtime 不是 Send，因此整个生命周期（单次 eval → HTTP → extractor）都留在该线程，
    // HTTP 请求通过 Handle::block_on 在该线程上驱动。
    let handle = tokio::runtime::Handle::current();
    let job = tokio::task::spawn_blocking(move || {
        run_script_blocking(
            &handle,
            &client,
            &script,
            &vars,
            is_custom_template,
            timeout,
        )
    });

    // 外层兜底：两个 JS 阶段 + 一次 HTTP 请求，再留一些余量。
    // 正常情况下中断处理器会先触发，这里只防御极端情况。
    let outer_timeout = timeout * 3 + Duration::from_secs(5);
    let result = match tokio::time::timeout(outer_timeout, job).await {
        Ok(Ok(result)) => result?,
        Ok(Err(e)) => {
            return Err(AppError::localized(
                "usage_script.worker_failed",
                format!("脚本执行线程异常: {e}"),
                format!("Usage script worker failed: {e}"),
            ))
        }
        Err(_) => return Err(script_timeout_error(timeout)),
    };

    // 3. 验证返回值格式
    validate_result(&result)?;

    Ok(result)
}

/// 中断处理器使用的执行期限：仅在 JS 执行阶段生效，HTTP 请求期间解除
#[derive(Default)]
struct ScriptDeadline {
    deadline: Mutex<Option<Instant>>,
    fired: AtomicBool,
}

impl ScriptDeadline {
    fn arm(&self, timeout: Duration) {
        if let Ok(mut d) = self.deadline.lock() {
            *d = Some(Instant::now() + timeout);
        }
    }

    fn disarm(&self) {
        if let Ok(mut d) = self.deadline.lock() {
            *d = None;
        }
    }

    fn should_interrupt(&self) -> bool {
        let expired = self
            .deadline
            .lock()
            .map(|d| d.is_some_and(|deadline| Instant::now() >= deadline))
            .unwrap_or(true);
        if expired {
            self.fired.store(true, Ordering::SeqCst);
        }
        expired
    }

    fn fired(&self) -> bool {
        self.fired.load(Ordering::SeqCst)
    }
}

fn script_timeout_error(timeout: Duration) -> AppError {
    let secs = timeout.as_secs();
    AppError::localized(
        "usage_script.script_timeout",
        format!("脚本执行超时（超过 {secs} 秒）"),
        format!("Usage script timed out (exceeded {secs}s)"),
    )
}

/// 把 rquickjs 错误转换为本地化错误：区分超时、内存超限、栈溢出与普通脚本异常
fn js_error(
    ctx: &Ctx<'_>,
    guard: &ScriptDeadline,
    timeout: Duration,
    err: rquickjs::Error,
    key: &'static str,
    zh_prefix: &str,
    en_prefix: &str,
) -> AppError {
    if guard.fired() {
        return script_timeout_error(timeout);
    }
    // 只保留消息与前几帧调用栈，避免深递归等场景产生超长错误文案
    let message = CaughtError::from_error(ctx, err)
        .to_string()
        .lines()
        .take(4)
        .collect::<Vec<_>>()
        .join("\n");
    if message.contains("out of memory") || message.contains("Allocation failed") {
        let mib = SCRIPT_MEMORY_LIMIT / 1024 / 1024;
        return AppError::localized(
            "usage_script.memory_limit_exceeded",
            format!("脚本内存使用超出上限（{mib} MiB）"),
            format!("Usage script exceeded the memory limit ({mib} MiB)"),
        );
    }
    if message.contains("stack overflow") || message.contains("Maximum call stack size exceeded") {
        return AppError::localized(
            "usage_script.stack_overflow",
            "脚本调用栈溢出",
            "Usage script exceeded the stack size limit",
        );
    }
    AppError::localized(
        key,
        format!("{zh_prefix}: {message}"),
        format!("{en_prefix}: {message}"),
    )
}

/// 将变量以数据形式注入为全局变量：`apiKey`、`baseUrl`、`accessToken`、`userId`
/// （未配置的 accessToken / userId 为 undefined）
fn set_script_globals(ctx: &Ctx<'_>, vars: &ScriptVars) -> rquickjs::Result<()> {
    let globals = ctx.globals();
    globals.set("apiKey", vars.api_key.as_str())?;
    globals.set("baseUrl", vars.base_url.as_str())?;
    globals.set("accessToken", vars.access_token.as_deref())?;
    globals.set("userId", vars.user_id.as_deref())?;
    Ok(())
}

/// 在当前（阻塞）线程上完成一次完整的脚本执行：创建受限运行时 → 单次 eval →
/// 发送请求 → 在同一上下文中调用 extractor
fn run_script_blocking(
    handle: &tokio::runtime::Handle,
    client: &reqwest::Client,
    script: &str,
    vars: &ScriptVars,
    is_custom_template: bool,
    timeout: Duration,
) -> Result<Value, AppError> {
    let runtime = Runtime::new().map_err(|e| {
        AppError::localized(
            "usage_script.runtime_create_failed",
            format!("创建 JS 运行时失败: {e}"),
            format!("Failed to create JS runtime: {e}"),
        )
    })?;
    runtime.set_memory_limit(SCRIPT_MEMORY_LIMIT);
    runtime.set_max_stack_size(SCRIPT_MAX_STACK_SIZE);

    let guard = Arc::new(ScriptDeadline::default());
    {
        let guard = Arc::clone(&guard);
        runtime.set_interrupt_handler(Some(Box::new(move || guard.should_interrupt())));
    }

    let context = Context::full(&runtime).map_err(|e| {
        AppError::localized(
            "usage_script.context_create_failed",
            format!("创建 JS 上下文失败: {e}"),
            format!("Failed to create JS context: {e}"),
        )
    })?;

    context.with(|ctx| {
        set_script_globals(&ctx, vars).map_err(|e| {
            AppError::localized(
                "usage_script.context_create_failed",
                format!("创建 JS 上下文失败: {e}"),
                format!("Failed to create JS context: {e}"),
            )
        })?;

        // 阶段 1：执行用户代码（仅一次），提取 request 配置
        guard.arm(timeout);
        let phase1 = extract_request(&ctx, &guard, timeout, script);
        guard.disarm();
        let (config, request_json) = phase1?;

        let request: RequestConfig = serde_json::from_str(&request_json).map_err(|e| {
            AppError::localized(
                "usage_script.request_format_invalid",
                format!("request 配置格式错误: {e}"),
                format!("Invalid request config format: {e}"),
            )
        })?;

        // 验证请求 URL（HTTPS 强制 + 同源检查）
        validate_request_url(&request.url, &vars.base_url, is_custom_template)?;

        // 发送 HTTP 请求（此时不执行 JS，期限已解除）
        let response_data = handle.block_on(send_http_request(
            client,
            &request,
            timeout,
            &vars.base_url,
            is_custom_template,
        ))?;

        // 阶段 2：在同一上下文中调用 extractor（无需重新 eval 脚本）
        guard.arm(timeout);
        let phase2 = run_extractor(&ctx, &guard, timeout, config, &response_data);
        guard.disarm();
        phase2
    })
}

fn extract_request<'js>(
    ctx: &Ctx<'js>,
    guard: &ScriptDeadline,
    timeout: Duration,
    script: &str,
) -> Result<(Object<'js>, String), AppError> {
    // 执行用户代码，获取配置对象
    let config: Object = ctx.eval(script).map_err(|e| {
        js_error(
            ctx,
            guard,
            timeout,
            e,
            "usage_script.config_parse_failed",
            "解析配置失败",
            "Failed to parse config",
        )
    })?;

    // 提取 request 配置
    let request: Object = config.get("request").map_err(|e| {
        js_error(
            ctx,
            guard,
            timeout,
            e,
            "usage_script.request_missing",
            "缺少 request 配置",
            "Missing request config",
        )
    })?;

    // 将 request 转换为 JSON 字符串
    let request_json = stringify(
        ctx,
        guard,
        timeout,
        request.into_value(),
        (
            "usage_script.request_serialize_failed",
            "序列化 request 失败",
            "Failed to serialize request",
        ),
    )?;
    Ok((config, request_json))
}

fn run_extractor<'js>(
    ctx: &Ctx<'js>,
    guard: &ScriptDeadline,
    timeout: Duration,
    config: Object<'js>,
    response_data: &str,
) -> Result<Value, AppError> {
    // 提取 extractor 函数
    let extractor: Function = config.get("extractor").map_err(|e| {
        js_error(
            ctx,
            guard,
            timeout,
            e,
            "usage_script.extractor_missing",
            "缺少 extractor 函数",
            "Missing extractor function",
        )
    })?;

    // 将响应数据转换为 JS 值
    let response_js: rquickjs::Value = ctx.json_parse(response_data).map_err(|e| {
        js_error(
            ctx,
            guard,
            timeout,
            e,
            "usage_script.response_parse_failed",
            "解析响应 JSON 失败",
            "Failed to parse response JSON",
        )
    })?;

    // 调用 extractor(response)
    let result_js: rquickjs::Value = extractor.call((response_js,)).map_err(|e| {
        js_error(
            ctx,
            guard,
            timeout,
            e,
            "usage_script.extractor_exec_failed",
            "执行 extractor 失败",
            "Failed to execute extractor",
        )
    })?;

    // 转换为 JSON 字符串并解析为 serde_json::Value
    let result_json = stringify(
        ctx,
        guard,
        timeout,
        result_js,
        (
            "usage_script.result_serialize_failed",
            "序列化结果失败",
            "Failed to serialize result",
        ),
    )?;
    serde_json::from_str(&result_json).map_err(|e| {
        AppError::localized(
            "usage_script.json_parse_failed",
            format!("JSON 解析失败: {e}"),
            format!("JSON parse failed: {e}"),
        )
    })
}

fn stringify<'js>(
    ctx: &Ctx<'js>,
    guard: &ScriptDeadline,
    timeout: Duration,
    value: rquickjs::Value<'js>,
    (key, zh, en): (&'static str, &str, &str),
) -> Result<String, AppError> {
    ctx.json_stringify(value)
        .map_err(|e| js_error(ctx, guard, timeout, e, key, zh, en))?
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
        })
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

fn response_too_large_error() -> AppError {
    let mib = MAX_RESPONSE_BODY_BYTES / 1024 / 1024;
    AppError::localized(
        "usage_script.response_too_large",
        format!("响应体过大（超过 {mib} MiB）"),
        format!("Response body too large (exceeds {mib} MiB)"),
    )
}

/// 读取响应体，超过上限立即中止（同时检查 Content-Length 与实际流式长度）
async fn read_body_capped(mut resp: reqwest::Response, limit: usize) -> Result<String, AppError> {
    if resp.content_length().is_some_and(|len| len > limit as u64) {
        return Err(response_too_large_error());
    }
    let mut buf: Vec<u8> = Vec::new();
    loop {
        let chunk = resp.chunk().await.map_err(|e| {
            AppError::localized(
                "usage_script.read_response_failed",
                format!("读取响应失败: {e}"),
                format!("Failed to read response: {e}"),
            )
        })?;
        let Some(chunk) = chunk else { break };
        if buf.len() + chunk.len() > limit {
            return Err(response_too_large_error());
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// 发送 HTTP 请求
async fn send_http_request(
    client: &reqwest::Client,
    config: &RequestConfig,
    request_timeout: Duration,
    base_url: &str,
    is_custom_template: bool,
) -> Result<String, AppError> {
    // 严格校验 HTTP 方法，非法值不回退为 GET
    let method: reqwest::Method = config.method.parse().map_err(|_| {
        AppError::localized(
            "usage_script.invalid_http_method",
            format!("不支持的 HTTP 方法: {}", config.method),
            format!("Unsupported HTTP method: {}", config.method),
        )
    })?;

    // 客户端不自动跟随重定向（见 http_client::get_no_redirect）：每一跳的目标
    // 都先做 HTTPS/同源校验再发送，避免脚本的自定义请求头（API Key 等）被
    // 发往未经校验的重定向目标。
    const MAX_REDIRECTS: usize = 5;
    let deadline = std::time::Instant::now() + request_timeout;
    let original_origin = Url::parse(&config.url).ok().map(|u| u.origin());
    let mut url = config.url.clone();
    let mut method = method;
    let mut body = config.body.clone();
    let mut redirects = 0usize;

    let resp = loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(AppError::localized(
                "usage_script.request_failed",
                "请求失败: 超时",
                "Request failed: timed out",
            ));
        }
        let mut req = client.request(method.clone(), &url).timeout(remaining);

        // 添加请求头：仅发往与原始请求同源的地址
        let same_origin = Url::parse(&url).ok().map(|u| u.origin()) == original_origin;
        if same_origin {
            for (k, v) in &config.headers {
                req = req.header(k, v);
            }
        }

        // 添加请求体
        if let Some(body) = &body {
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
        let location = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok());
        let Some(location) = location.filter(|_| status.is_redirection()) else {
            break resp;
        };

        if redirects >= MAX_REDIRECTS {
            return Err(AppError::localized(
                "usage_script.too_many_redirects",
                format!("重定向次数过多（超过 {MAX_REDIRECTS} 次）"),
                format!("Too many redirects (more than {MAX_REDIRECTS})"),
            ));
        }
        let next = resp.url().join(location).map_err(|e| {
            AppError::localized(
                "usage_script.request_url_invalid",
                format!("无效的重定向地址: {e}"),
                format!("Invalid redirect location: {e}"),
            )
        })?;
        // 发送前校验重定向目标，拒绝跳转到其他主机
        validate_request_url(next.as_str(), base_url, is_custom_template)?;

        // 与浏览器/reqwest 一致：303 以及 POST 的 301/302 改为不带请求体的 GET
        let to_get = status == reqwest::StatusCode::SEE_OTHER
            || (method == reqwest::Method::POST
                && matches!(
                    status,
                    reqwest::StatusCode::MOVED_PERMANENTLY | reqwest::StatusCode::FOUND
                ));
        if to_get && method != reqwest::Method::HEAD {
            method = reqwest::Method::GET;
            body = None;
        }
        url = next.to_string();
        redirects += 1;
    };

    let status = resp.status();
    let text = read_body_capped(resp, MAX_RESPONSE_BODY_BYTES).await?;

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

/// 兼容旧版脚本的 `{{apiKey}}` 等占位符。
///
/// 变量的权威传入方式是全局数据（见 [`set_script_globals`]）。旧脚本把占位符写在
/// 字符串字面量里（如 `"Bearer {{apiKey}}"`），为保持其语义不变，这里只把占位符
/// 替换为“字符串安全”的转义片段：除少量无害 ASCII 字符外，所有字符都写成
/// `\uXXXX`，因此值中的引号、反斜杠、换行、`${` 等无法跳出单引号/双引号/模板
/// 字符串上下文，也无法注入代码。
fn build_script_with_vars(script_code: &str, vars: &ScriptVars) -> String {
    let mut replaced = script_code
        .replace("{{apiKey}}", &js_string_fragment(&vars.api_key))
        .replace("{{baseUrl}}", &js_string_fragment(&vars.base_url));

    if let Some(token) = &vars.access_token {
        replaced = replaced.replace("{{accessToken}}", &js_string_fragment(token));
    }
    if let Some(uid) = &vars.user_id {
        replaced = replaced.replace("{{userId}}", &js_string_fragment(uid));
    }

    replaced
}

/// 将任意字符串编码为可安全嵌入 JS 字符串字面量（'…'、"…"、`…`）内部的片段
fn js_string_fragment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        if c.is_ascii_alphanumeric()
            || matches!(
                c,
                '-' | '_' | '.' | ':' | '@' | '%' | '+' | '=' | '~' | ',' | ' '
            )
        {
            out.push(c);
        } else {
            let mut units = [0u16; 2];
            for unit in c.encode_utf16(&mut units) {
                out.push_str(&format!("\\u{unit:04X}"));
            }
        }
    }
    out
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn test_client() -> reqwest::Client {
        // 与生产一致：不自动跟随重定向（http_client::get_no_redirect）
        reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap()
    }

    fn error_key(err: &AppError) -> &'static str {
        match err {
            AppError::Localized { key, .. } => key,
            other => panic!("expected localized error, got {other}"),
        }
    }

    /// 极简 HTTP 服务：按顺序为每个连接返回一条预置的原始响应（`{port}` 会被替换为端口）
    async fn spawn_server(responses: Vec<String>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            for response in responses {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let mut buf = [0u8; 4096];
                let mut seen = Vec::new();
                loop {
                    match sock.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            seen.extend_from_slice(&buf[..n]);
                            if seen.windows(4).any(|w| w == b"\r\n\r\n") {
                                break;
                            }
                        }
                    }
                }
                let response = response.replace("{port}", &port.to_string());
                let _ = sock.write_all(response.as_bytes()).await;
                let _ = sock.shutdown().await;
            }
        });
        format!("http://127.0.0.1:{port}")
    }

    fn json_response(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    async fn run(script: &str, api_key: &str, base_url: &str) -> Result<Value, AppError> {
        execute_usage_script_with_client(
            test_client(),
            script,
            api_key,
            base_url,
            2,
            None,
            None,
            Some("general"),
        )
        .await
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn infinite_loop_script_times_out() {
        let start = Instant::now();
        let err = run(
            "try { while (true) {} } catch (e) {} ({ request: {} })",
            "k",
            "https://api.example.com",
        )
        .await
        .unwrap_err();
        assert_eq!(error_key(&err), "usage_script.script_timeout", "{err}");
        assert!(
            start.elapsed() < Duration::from_secs(6),
            "timeout took {:?}",
            start.elapsed()
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn huge_allocation_fails_cleanly() {
        let err = run(
            "new Array(1e9).fill(0); ({ request: {} })",
            "k",
            "https://api.example.com",
        )
        .await
        .unwrap_err();
        assert_eq!(
            error_key(&err),
            "usage_script.memory_limit_exceeded",
            "{err}"
        );

        let err = run(
            "let s = 'x'; while (true) { s = s + s; } ({ request: {} })",
            "k",
            "https://api.example.com",
        )
        .await
        .unwrap_err();
        assert!(
            matches!(
                error_key(&err),
                "usage_script.memory_limit_exceeded" | "usage_script.config_parse_failed"
            ),
            "{err}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn deep_recursion_fails_cleanly() {
        let err = run(
            "function f() { return f() + 1; } f(); ({ request: {} })",
            "k",
            "https://api.example.com",
        )
        .await
        .unwrap_err();
        assert_eq!(error_key(&err), "usage_script.stack_overflow", "{err}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn normal_script_runs_and_evaluates_once() {
        let base = spawn_server(vec![json_response(r#"{"balance": 12.5}"#)]).await;
        let script = r#"
            globalThis.counter = (globalThis.counter || 0) + 1;
            ({
              request: {
                url: "{{baseUrl}}/user/balance",
                method: "GET",
                headers: { "Authorization": "Bearer {{apiKey}}" }
              },
              extractor: function (response) {
                return { remaining: response.balance, unit: "USD", total: globalThis.counter };
              }
            })
        "#;
        let value = run(script, "sk-test", &base).await.unwrap();
        assert_eq!(value["remaining"], 12.5);
        assert_eq!(value["unit"], "USD");
        assert_eq!(value["total"], 1, "script must be evaluated exactly once");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn api_key_with_quotes_and_newlines_is_passed_as_data() {
        let base = spawn_server(vec![json_response("{}")]).await;
        let key = "a\";\nthrow 1;//'`${x}\\\u{2028}é😀";
        let script = r#"({
            request: { url: "{{baseUrl}}/x", method: "GET", headers: { "X-Test": "1" } },
            extractor: function () {
              return { planName: "{{apiKey}}", extra: apiKey, invalidMessage: `{{apiKey}}` };
            }
        })"#;
        let value = run(script, key, &base).await.unwrap();
        assert_eq!(value["planName"], key);
        assert_eq!(value["extra"], key);
        assert_eq!(value["invalidMessage"], key);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn extractor_infinite_loop_times_out() {
        let base = spawn_server(vec![json_response("{}")]).await;
        let script = r#"({
            request: { url: "{{baseUrl}}/x", method: "GET" },
            extractor: function () { while (true) {} }
        })"#;
        let err = run(script, "k", &base).await.unwrap_err();
        assert_eq!(error_key(&err), "usage_script.script_timeout", "{err}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn oversized_response_body_is_rejected() {
        // 无 Content-Length，靠流式累计长度触发上限
        let big = "x".repeat(MAX_RESPONSE_BODY_BYTES + 1024);
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n\"{big}\""
        );
        let base = spawn_server(vec![response]).await;
        let script = r#"({
            request: { url: "{{baseUrl}}/x", method: "GET" },
            extractor: function (r) { return { remaining: 1 }; }
        })"#;
        let err = run(script, "k", &base).await.unwrap_err();
        assert_eq!(error_key(&err), "usage_script.response_too_large", "{err}");

        // 声明了超大 Content-Length 也直接拒绝
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            10 * 1024 * 1024
        );
        let base = spawn_server(vec![response]).await;
        let err = run(script, "k", &base).await.unwrap_err();
        assert_eq!(error_key(&err), "usage_script.response_too_large", "{err}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cross_host_redirect_is_refused() {
        let redirect = "HTTP/1.1 302 Found\r\nLocation: http://localhost:{port}/final\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string();
        let base = spawn_server(vec![redirect, json_response(r#"{"balance": 1}"#)]).await;
        let script = r#"({
            request: { url: "{{baseUrl}}/x", method: "GET" },
            extractor: function (r) { return { remaining: r.balance }; }
        })"#;
        let err = run(script, "k", &base).await.unwrap_err();
        assert_eq!(
            error_key(&err),
            "usage_script.request_host_mismatch",
            "{err}"
        );
    }

    /// 记录每个请求原始字节的 HTTP 服务，始终返回 `response`
    async fn spawn_recording_server(
        response: String,
    ) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let record = seen.clone();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                let mut buf = [0u8; 4096];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                record
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buf[..n]).into_owned());
                let _ = sock.write_all(response.as_bytes()).await;
                let _ = sock.shutdown().await;
            }
        });
        (format!("http://127.0.0.1:{port}"), seen)
    }

    fn redirect_to(location: &str) -> String {
        format!(
            "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn same_origin_redirect_is_followed() {
        let redirect = redirect_to("http://127.0.0.1:{port}/final");
        let base = spawn_server(vec![redirect, json_response(r#"{"balance": 7}"#)]).await;
        let script = r#"({
            request: { url: "{{baseUrl}}/x", method: "GET" },
            extractor: function (r) { return { remaining: r.balance }; }
        })"#;
        let value = run(script, "k", &base).await.expect("redirect followed");
        assert_eq!(value["remaining"], 7, "{value}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cross_origin_redirect_target_never_receives_the_request() {
        let (target, target_seen) =
            spawn_recording_server(json_response(r#"{"balance": 1}"#)).await;
        let base = spawn_server(vec![redirect_to(&format!("{target}/steal"))]).await;
        let script = r#"({
            request: { url: "{{baseUrl}}/x", method: "GET", headers: { "X-Api-Key": "{{apiKey}}" } },
            extractor: function (r) { return { remaining: r.balance }; }
        })"#;
        let err = run(script, "sk-secret", &base).await.unwrap_err();
        assert_eq!(
            error_key(&err),
            "usage_script.request_port_mismatch",
            "{err}"
        );
        assert!(target_seen.lock().unwrap().is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn custom_template_cross_origin_redirect_drops_script_headers() {
        let (target, target_seen) =
            spawn_recording_server(json_response(r#"{"balance": 3}"#)).await;
        let base = spawn_server(vec![redirect_to(&format!("{target}/next"))]).await;
        let script = format!(
            r#"({{
            request: {{ url: "{base}/x", method: "GET", headers: {{ "X-Custom-Key": "sk-secret" }} }},
            extractor: function (r) {{ return {{ remaining: r.balance }}; }}
        }})"#
        );
        let value = execute_usage_script_with_client(
            test_client(),
            &script,
            "",
            "",
            2,
            None,
            None,
            Some("custom"),
        )
        .await
        .expect("custom template may follow cross-origin redirects");
        assert_eq!(value["remaining"], 3, "{value}");
        let seen = target_seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert!(
            !seen[0].to_ascii_lowercase().contains("sk-secret"),
            "script header leaked to redirect target: {}",
            seen[0]
        );
    }

    #[test]
    fn js_string_fragment_escapes_breakout_characters() {
        let encoded = js_string_fragment("ab\"';\n`${}\\/");
        assert!(encoded.starts_with("ab"));
        for bad in ['"', '\'', ';', '\n', '`', '$', '{', '}', '/'] {
            assert!(!encoded.contains(bad), "{bad:?} not escaped in {encoded}");
        }
        assert_eq!(js_string_fragment("sk-ABC_123.x"), "sk-ABC_123.x");
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
