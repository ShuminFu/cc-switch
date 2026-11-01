//! MCP server health check (PRODUCT-23, first increment).
//!
//! `test_server` performs the MCP handshake against one server definition and
//! lists its tools:
//!
//! - **stdio**: spawns `command args…` (with `env` / `cwd`), writes
//!   newline-delimited JSON-RPC `initialize`, `notifications/initialized` and
//!   `tools/list` to stdin and reads the replies from stdout. The child is
//!   killed afterwards, and on timeout.
//! - **http** (streamable HTTP): POSTs the same messages to `url` with the
//!   configured `headers`, accepting a JSON or an SSE (`data:`) reply and
//!   echoing `Mcp-Session-Id`.
//! - **sse** (legacy transport): GETs `url` and reports whether the event
//!   stream opens; tools are not listed.
//!
//! The result never contains header values or `env` values.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};

/// Default overall timeout for one test.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);
/// Protocol version cc-switch offers in `initialize`.
const PROTOCOL_VERSION: &str = "2025-06-18";
/// Bytes of stderr kept for the error report.
const STDERR_TAIL_BYTES: usize = 2048;

/// One tool advertised by the server.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct McpToolInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Outcome of `test_mcp_server`.
#[derive(Debug, Clone, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct McpTestResult {
    pub ok: bool,
    /// `stdio`, `http` or `sse`.
    pub transport: String,
    /// Wall time of the whole test.
    pub latency_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol_version: Option<String>,
    pub tools: Vec<McpToolInfo>,
    /// English error text when `ok` is false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Last bytes the stdio server wrote to stderr (a typo in `args` usually
    /// shows up here).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr_tail: Option<String>,
}

fn transport_of(spec: &Value) -> &str {
    spec.get("type").and_then(Value::as_str).unwrap_or("stdio")
}

fn initialize_request() -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": { "name": "cc-switch", "version": env!("CARGO_PKG_VERSION") }
        }
    })
}

fn initialized_notification() -> Value {
    json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })
}

fn tools_list_request() -> Value {
    json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} })
}

/// Fill server info from an `initialize` result.
fn apply_initialize(result: &mut McpTestResult, reply: &Value) -> Result<(), String> {
    if let Some(err) = reply.get("error") {
        return Err(format!("initialize failed: {}", rpc_error_text(err)));
    }
    let res = reply
        .get("result")
        .ok_or_else(|| "initialize reply has no result".to_string())?;
    result.protocol_version = res
        .get("protocolVersion")
        .and_then(Value::as_str)
        .map(str::to_string);
    let info = res.get("serverInfo");
    result.server_name = info
        .and_then(|i| i.get("name"))
        .and_then(Value::as_str)
        .map(str::to_string);
    result.server_version = info
        .and_then(|i| i.get("version"))
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok(())
}

/// Fill tools from a `tools/list` result.
fn apply_tools(result: &mut McpTestResult, reply: &Value) -> Result<(), String> {
    if let Some(err) = reply.get("error") {
        return Err(format!("tools/list failed: {}", rpc_error_text(err)));
    }
    result.tools = reply
        .pointer("/result/tools")
        .and_then(Value::as_array)
        .map(|tools| {
            tools
                .iter()
                .filter_map(|t| {
                    Some(McpToolInfo {
                        name: t.get("name")?.as_str()?.to_string(),
                        description: t
                            .get("description")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(())
}

fn rpc_error_text(err: &Value) -> String {
    let code = err.get("code").and_then(Value::as_i64).unwrap_or_default();
    let message = err
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("unknown error");
    format!("{message} (code {code})")
}

/// Run the test for one server definition (blocking; call it off the UI
/// thread).
pub fn test_server(spec: &Value, timeout: Duration) -> McpTestResult {
    let started = Instant::now();
    let transport = transport_of(spec).to_string();
    let mut result = McpTestResult {
        transport: transport.clone(),
        ..Default::default()
    };
    let outcome = match crate::mcp::validate_server_spec(spec) {
        Err(e) => Err(e.to_string()),
        Ok(()) => match transport.as_str() {
            "http" => test_http(spec, timeout, &mut result),
            "sse" => test_sse(spec, timeout),
            _ => test_stdio(spec, timeout, &mut result),
        },
    };
    result.latency_ms = started.elapsed().as_millis() as u64;
    match outcome {
        Ok(()) => result.ok = true,
        Err(e) => result.error = Some(e),
    }
    result
}

fn string_map(spec: &Value, field: &str) -> Vec<(String, String)> {
    spec.get(field)
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .map(|(k, v)| {
                    let v = v
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| v.to_string());
                    (k.clone(), v)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn build_command(spec: &Value) -> Command {
    let program = spec
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let args: Vec<String> = spec
        .get("args")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| v.to_string())
                })
                .collect()
        })
        .unwrap_or_default();

    // On Windows `npx`, `uvx` and friends are `.cmd` shims that CreateProcess
    // cannot start directly; go through `cmd /C` like the CLIs do.
    #[cfg(target_os = "windows")]
    let mut cmd = {
        use std::os::windows::process::CommandExt;
        let has_ext = std::path::Path::new(program).extension().is_some();
        let mut c = if has_ext {
            Command::new(program)
        } else {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(program);
            c
        };
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        c
    };
    #[cfg(not(target_os = "windows"))]
    let mut cmd = Command::new(program);

    cmd.args(args);
    for (k, v) in string_map(spec, "env") {
        cmd.env(k, v);
    }
    if let Some(cwd) = spec.get("cwd").and_then(Value::as_str) {
        if !cwd.trim().is_empty() {
            cmd.current_dir(cwd);
        }
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

/// Wait for the JSON-RPC reply with `id`, skipping notifications, logs and
/// server-initiated requests.
fn wait_for_reply(
    lines: &mpsc::Receiver<String>,
    id: i64,
    deadline: Instant,
) -> Result<Value, String> {
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("timed out waiting for the server to answer".to_string());
        }
        let line = match lines.recv_timeout(remaining) {
            Ok(line) => line,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return Err("timed out waiting for the server to answer".to_string())
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("the server exited before answering".to_string())
            }
        };
        let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
            continue; // a stray log line on stdout
        };
        if value.get("id").and_then(Value::as_i64) == Some(id)
            && (value.get("result").is_some() || value.get("error").is_some())
        {
            return Ok(value);
        }
    }
}

fn test_stdio(spec: &Value, timeout: Duration, result: &mut McpTestResult) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    let mut child = build_command(spec)
        .spawn()
        .map_err(|e| format!("failed to start the server: {e}"))?;

    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");
    let mut stdin = child.stdin.take().expect("stdin piped");

    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    // The stderr tail is shared rather than joined: a server that spawned
    // grandchildren (npx → node) can keep the pipe open after the kill.
    let stderr_tail = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    let tail_writer = std::sync::Arc::clone(&stderr_tail);
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        let mut stderr = stderr;
        while let Ok(n) = stderr.read(&mut buf) {
            if n == 0 {
                break;
            }
            let Ok(mut tail) = tail_writer.lock() else {
                break;
            };
            tail.extend_from_slice(&buf[..n]);
            if tail.len() > STDERR_TAIL_BYTES * 2 {
                let excess = tail.len() - STDERR_TAIL_BYTES;
                tail.drain(..excess);
            }
        }
    });

    let mut send = |msg: Value| -> Result<(), String> {
        let mut line = msg.to_string();
        line.push('\n');
        stdin
            .write_all(line.as_bytes())
            .and_then(|_| stdin.flush())
            .map_err(|e| format!("failed to write to the server: {e}"))
    };

    let outcome = (|| {
        send(initialize_request())?;
        let init = wait_for_reply(&rx, 1, deadline)?;
        apply_initialize(result, &init)?;
        send(initialized_notification())?;
        send(tools_list_request())?;
        let tools = wait_for_reply(&rx, 2, deadline)?;
        apply_tools(result, &tools)
    })();

    let _ = child.kill();
    let _ = child.wait();
    if outcome.is_err() {
        // Give the reader a moment to drain what the server wrote last.
        std::thread::sleep(Duration::from_millis(100));
        if let Ok(tail) = stderr_tail.lock() {
            let start = tail.len().saturating_sub(STDERR_TAIL_BYTES);
            let text = String::from_utf8_lossy(&tail[start..]).trim().to_string();
            if !text.is_empty() {
                result.stderr_tail = Some(text);
            }
        }
    }
    outcome
}

/// Parse a streamable-HTTP reply: plain JSON, or the first SSE `data:` event
/// that carries a JSON-RPC message with `id`.
fn parse_http_reply(content_type: &str, body: &str, id: i64) -> Result<Value, String> {
    if content_type.contains("text/event-stream") {
        let mut data = String::new();
        for line in body.lines().chain(std::iter::once("")) {
            if let Some(rest) = line.strip_prefix("data:") {
                data.push_str(rest.trim_start());
            } else if line.trim().is_empty() && !data.is_empty() {
                if let Ok(v) = serde_json::from_str::<Value>(&data) {
                    if v.get("id").and_then(Value::as_i64) == Some(id) {
                        return Ok(v);
                    }
                }
                data.clear();
            }
        }
        return Err("the event stream carried no reply".to_string());
    }
    serde_json::from_str(body).map_err(|e| format!("the reply is not JSON: {e}"))
}

fn http_client(timeout: Duration) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|e| format!("failed to build HTTP client: {e}"))
}

fn header_map(spec: &Value) -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    for (k, v) in string_map(spec, "headers") {
        if let (Ok(name), Ok(value)) = (
            reqwest::header::HeaderName::from_bytes(k.as_bytes()),
            reqwest::header::HeaderValue::from_str(&v),
        ) {
            headers.insert(name, value);
        }
    }
    headers
}

fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    tauri::async_runtime::block_on(fut)
}

fn test_http(spec: &Value, timeout: Duration, result: &mut McpTestResult) -> Result<(), String> {
    let url = spec.get("url").and_then(Value::as_str).unwrap_or_default();
    let client = http_client(timeout)?;
    let headers = header_map(spec);
    block_on(async {
        let post = |body: Value, session: Option<String>| {
            let mut req = client
                .post(url)
                .headers(headers.clone())
                .header("Accept", "application/json, text/event-stream")
                .json(&body);
            if let Some(s) = session {
                req = req.header("Mcp-Session-Id", s);
            }
            req.send()
        };
        let read = |resp: reqwest::Response, id: i64| async move {
            let status = resp.status();
            let ct = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_string();
            let body = resp.text().await.map_err(|e| e.to_string())?;
            if !status.is_success() {
                return Err(format!("HTTP {status}"));
            }
            parse_http_reply(&ct, &body, id)
        };

        let resp = post(initialize_request(), None)
            .await
            .map_err(|e| format!("request failed: {e}"))?;
        let session = resp
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let init = read(resp, 1).await?;
        apply_initialize(result, &init)?;
        let _ = post(initialized_notification(), session.clone()).await;
        let resp = post(tools_list_request(), session)
            .await
            .map_err(|e| format!("request failed: {e}"))?;
        let tools = read(resp, 2).await?;
        apply_tools(result, &tools)
    })
}

fn test_sse(spec: &Value, timeout: Duration) -> Result<(), String> {
    let url = spec.get("url").and_then(Value::as_str).unwrap_or_default();
    let client = http_client(timeout)?;
    let headers = header_map(spec);
    block_on(async {
        let resp = client
            .get(url)
            .headers(headers)
            .header("Accept", "text/event-stream")
            .send()
            .await
            .map_err(|e| format!("request failed: {e}"))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(format!("HTTP {}", resp.status()))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_command_is_reported_without_spawning() {
        let r = test_server(&json!({ "type": "stdio", "command": "" }), DEFAULT_TIMEOUT);
        assert!(!r.ok);
        assert!(r.error.is_some());
    }

    #[test]
    fn nonexistent_executable_fails_to_start() {
        let r = test_server(
            &json!({ "command": "cc-switch-definitely-not-a-binary-xyz" }),
            Duration::from_secs(5),
        );
        assert!(!r.ok);
        assert_eq!(r.transport, "stdio");
    }

    #[cfg(unix)]
    #[test]
    fn stdio_handshake_lists_tools() {
        // A tiny shell "server": answers initialize and tools/list in order.
        let script = r#"
read -r init
printf '%s\n' 'log line that is not json'
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18","serverInfo":{"name":"fake","version":"1.2.3"},"capabilities":{}}}'
read -r initialized
read -r list
printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/message","params":{}}'
printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"echo","description":"Echo back"},{"name":"add"}]}}'
sleep 5
"#;
        let r = test_server(
            &json!({ "command": "sh", "args": ["-c", script] }),
            Duration::from_secs(10),
        );
        assert!(r.ok, "{r:?}");
        assert_eq!(r.server_name.as_deref(), Some("fake"));
        assert_eq!(r.server_version.as_deref(), Some("1.2.3"));
        assert_eq!(
            r.tools.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
            vec!["echo", "add"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn stdio_server_that_exits_reports_stderr() {
        // What a typo in `args` typically looks like: the server prints usage
        // to stderr and exits.
        let r = test_server(
            &json!({ "command": "sh", "args": ["-c", "echo 'unknown option --prot' >&2; exit 2"] }),
            Duration::from_secs(10),
        );
        assert!(!r.ok);
        assert!(r.error.is_some());
        assert!(r.stderr_tail.unwrap().contains("unknown option --prot"));
    }

    #[cfg(unix)]
    #[test]
    fn stdio_timeout_kills_a_silent_server() {
        let started = Instant::now();
        let r = test_server(
            &json!({ "command": "sh", "args": ["-c", "sleep 30"] }),
            Duration::from_millis(500),
        );
        assert!(!r.ok);
        assert!(r.error.unwrap().contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn rpc_errors_are_surfaced() {
        let mut r = McpTestResult::default();
        let err = apply_initialize(
            &mut r,
            &json!({"id":1,"error":{"code":-32602,"message":"bad version"}}),
        )
        .unwrap_err();
        assert_eq!(err, "initialize failed: bad version (code -32602)");
    }

    #[test]
    fn http_reply_parses_json_and_sse() {
        let v = parse_http_reply("application/json", r#"{"id":1,"result":{}}"#, 1).unwrap();
        assert_eq!(v["id"], 1);
        let sse =
            "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"tools\":[]}}\n\n";
        let v = parse_http_reply("text/event-stream; charset=utf-8", sse, 2).unwrap();
        assert_eq!(v["id"], 2);
        assert!(parse_http_reply("text/event-stream", "data: {}\n\n", 2).is_err());
    }
}
