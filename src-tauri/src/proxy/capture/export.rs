//! 把一条抓包导出为 golden fixture 目录结构的 zip。
//!
//! ```text
//! <bridge>/<case>/
//!   meta.toml                        intent、source_commit、options、invariants
//!   client_request.json              客户端请求体（脱敏）
//!   upstream_response.sse | .json    上游响应体（脱敏）
//!   captured_client_response.sse | .json
//!                                    代理当时发给客户端的响应（用于对比回放结果）
//!   capture.json                     请求头、上游请求、状态码与每个 chunk 的时间
//! ```
//!
//! 解压到 `src-tauri/tests/fixtures/proxy/` 后即可被 `tests/proxy_golden.rs` 加载，
//! testkit 只读取前三类文件，其余文件只供排查时参考。

use std::io::{Cursor, Write};

use serde::Serialize;
use serde_json::{json, Value};
use zip::write::SimpleFileOptions;

use super::redact::{redact_bytes, redact_json};
use super::{CaptureBridge, CaptureRecord, CapturedBody, CapturedRequest};
use crate::error::AppError;
use crate::proxy::providers::GeminiGenerateEndpoint;
use crate::proxy::sse::{strip_sse_field, take_sse_block};

/// capture.json 的格式版本
const CAPTURE_FORMAT_VERSION: u32 = 1;

/// 写入 meta.toml 的 source_commit：没有 git 信息时用应用版本标识
const SOURCE_COMMIT: &str = concat!("v", env!("CARGO_PKG_VERSION"));

#[derive(Serialize)]
struct ExportMeta {
    intent: String,
    source_commit: String,
    #[serde(skip_serializing_if = "ExportOptions::is_empty")]
    options: ExportOptions,
    #[serde(skip_serializing_if = "ExportInvariants::is_empty")]
    invariants: ExportInvariants,
}

#[derive(Default, Serialize)]
struct ExportOptions {
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    preserve_reasoning_content: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    upstream_error: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    codex_oauth: bool,
    /// Gemini 客户端：URL 中的模型名（请求体不含模型）
    #[serde(skip_serializing_if = "Option::is_none")]
    client_model: Option<String>,
    /// Gemini 客户端：URL 是否为 `:streamGenerateContent`
    #[serde(skip_serializing_if = "Option::is_none")]
    client_stream: Option<bool>,
}

impl ExportOptions {
    fn is_empty(&self) -> bool {
        !self.preserve_reasoning_content
            && self.upstream_error.is_none()
            && !self.codex_oauth
            && self.client_model.is_none()
            && self.client_stream.is_none()
    }
}

#[derive(Default, Serialize)]
struct ExportInvariants {
    #[serde(skip_serializing_if = "Option::is_none")]
    terminal: Option<&'static str>,
}

impl ExportInvariants {
    fn is_empty(&self) -> bool {
        self.terminal.is_none()
    }
}

fn zip_error(error: impl std::fmt::Display) -> AppError {
    AppError::Message(format!("写入缺陷报告 zip 失败: {error}"))
}

/// fixture 目录名：`capture_<UTC 时间>_<抓包 ID 前 8 位>`
pub(crate) fn case_name(record: &CaptureRecord) -> String {
    let time = chrono::DateTime::from_timestamp_millis(record.started_at_ms)
        .unwrap_or_default()
        .format("%Y%m%d_%H%M%S");
    let short_id: String = record
        .id
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(8)
        .collect();
    format!("capture_{time}_{short_id}")
}

/// 构建缺陷报告 zip
pub(crate) fn build_zip(record: &CaptureRecord) -> Result<Vec<u8>, AppError> {
    let bridge = record.bridge.ok_or_else(|| {
        AppError::localized(
            "proxy.capture.not_bridged",
            "该请求未经过协议转换桥，无法导出为缺陷报告",
            "This request did not go through a protocol bridge and cannot be exported",
        )
    })?;
    let files = build_files(record, bridge)?;

    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());
    let dir = format!("{}/{}", bridge.dir_name(), case_name(record));
    for (name, contents) in files {
        writer
            .start_file(format!("{dir}/{name}"), options)
            .map_err(zip_error)?;
        writer.write_all(&contents).map_err(zip_error)?;
    }
    Ok(writer.finish().map_err(zip_error)?.into_inner())
}

/// 构建 fixture 目录下的全部文件 `(文件名, 内容)`；所有内容都已脱敏
pub(crate) fn build_files(
    record: &CaptureRecord,
    bridge: CaptureBridge,
) -> Result<Vec<(String, Vec<u8>)>, AppError> {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();

    let client_body = record
        .client_request
        .as_ref()
        .map(|request| &request.body)
        .filter(|body| body.is_object());
    let upstream = BodyFile::from_body(&record.upstream);
    if client_body.is_none() && upstream.is_none() {
        return Err(AppError::localized(
            "proxy.capture.empty",
            "抓包记录中没有请求体或上游响应，无法导出",
            "The capture has neither a request body nor an upstream response",
        ));
    }

    let client_response = BodyFile::from_body(&record.client);
    let gemini_endpoint = record
        .client_request
        .as_ref()
        .filter(|_| bridge == CaptureBridge::GeminiAnthropic)
        .and_then(|request| GeminiGenerateEndpoint::parse(&request.url));
    let meta = ExportMeta {
        intent: intent(record, bridge),
        source_commit: SOURCE_COMMIT.to_string(),
        options: ExportOptions {
            preserve_reasoning_content: bridge == CaptureBridge::AnthropicOpenAiChat
                && record
                    .upstream_request
                    .as_ref()
                    .is_some_and(|request| replays_reasoning_content(&request.body)),
            // testkit 只能在 SSE 体之后追加传输错误，非流式响应的读取错误无从回放
            upstream_error: record
                .upstream
                .error
                .clone()
                .filter(|_| record.upstream.is_stream),
            codex_oauth: record.codex_oauth && bridge == CaptureBridge::AnthropicResponses,
            client_model: gemini_endpoint
                .as_ref()
                .map(|endpoint| endpoint.model.clone()),
            client_stream: gemini_endpoint.as_ref().map(|endpoint| endpoint.stream),
        },
        invariants: ExportInvariants {
            terminal: client_response.as_ref().and_then(BodyFile::terminal),
        },
    };
    let meta_toml = toml::to_string(&meta).map_err(|e| AppError::Message(e.to_string()))?;
    files.push((
        "meta.toml".to_string(),
        meta_header(record, bridge, &meta_toml).into_bytes(),
    ));

    if let Some(body) = client_body {
        files.push(("client_request.json".to_string(), pretty_json(body)?));
    }
    if let Some(upstream) = &upstream {
        files.push((
            upstream.file_name("upstream_response"),
            upstream.contents()?,
        ));
    }
    if let Some(client) = &client_response {
        files.push((
            client.file_name("captured_client_response"),
            client.contents()?,
        ));
    }
    files.push((
        "capture.json".to_string(),
        pretty_json(&capture_json(record, bridge))?,
    ));
    Ok(files)
}

fn intent(record: &CaptureRecord, bridge: CaptureBridge) -> String {
    let mut intent = format!(
        "Bug report captured from a live {} request through {} (request {}). Describe the observed bug here.",
        record.app_type,
        bridge.dir_name(),
        record.request_id.as_deref().unwrap_or("unknown"),
    );
    if record.upstream.truncated || record.client.truncated {
        intent.push_str(" The capture was truncated at the size limit.");
    }
    intent
}

fn meta_header(record: &CaptureRecord, bridge: CaptureBridge, meta_toml: &str) -> String {
    let captured_at = chrono::DateTime::from_timestamp_millis(record.started_at_ms)
        .map(|time| time.to_rfc3339())
        .unwrap_or_default();
    format!(
        "# Bug report captured by CC Switch {SOURCE_COMMIT} at {captured_at}.\n\
         # Replay: unzip into src-tauri/tests/fixtures/proxy/ (creates {}/{}) and run\n\
         #   INSTA_UPDATE=always cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden\n\
         # then review the new snapshots. Secrets were redacted on export; capture.json\n\
         # keeps the request headers, the upstream request and per-chunk timings.\n\
         {meta_toml}",
        bridge.dir_name(),
        case_name(record),
    )
}

fn pretty_json(value: &Value) -> Result<Vec<u8>, AppError> {
    let mut out =
        serde_json::to_vec_pretty(value).map_err(|source| AppError::JsonSerialize { source })?;
    out.push(b'\n');
    Ok(out)
}

/// Chat 桥是否回放了 assistant 的 `reasoning_content`（对应 testkit 的
/// `preserve_reasoning_content` 选项，代理对 DeepSeek/Kimi 等推理供应商开启）
fn replays_reasoning_content(upstream_body: &Value) -> bool {
    upstream_body
        .get("messages")
        .and_then(Value::as_array)
        .is_some_and(|messages| {
            messages
                .iter()
                .any(|message| message.get("reasoning_content").is_some())
        })
}

/// 一个导出的响应体文件
enum BodyFile {
    Sse(Vec<u8>),
    Json(Value),
}

impl BodyFile {
    fn from_body(body: &CapturedBody) -> Option<Self> {
        if body.chunks.is_empty() {
            return None;
        }
        let bytes = body.concat();
        if !body.is_stream {
            if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
                return Some(BodyFile::Json(redact_json(&value)));
            }
        }
        Some(BodyFile::Sse(redact_bytes(&bytes)))
    }

    fn file_name(&self, stem: &str) -> String {
        match self {
            BodyFile::Sse(_) => format!("{stem}.sse"),
            BodyFile::Json(_) => format!("{stem}.json"),
        }
    }

    fn contents(&self) -> Result<Vec<u8>, AppError> {
        match self {
            BodyFile::Sse(bytes) => Ok(bytes.clone()),
            BodyFile::Json(value) => pretty_json(value),
        }
    }

    /// 由客户端流的最后一个事件推断 testkit 的 `invariants.terminal`；
    /// 正常结束（默认值）时返回 None
    fn terminal(&self) -> Option<&'static str> {
        let BodyFile::Sse(bytes) = self else {
            return None;
        };
        let mut buffer = String::from_utf8_lossy(bytes).into_owned();
        buffer.push_str("\n\n");
        let mut last_type = None;
        while let Some(block) = take_sse_block(&mut buffer) {
            let data: Vec<&str> = block
                .lines()
                .filter_map(|line| strip_sse_field(line, "data"))
                .collect();
            if let Ok(value) = serde_json::from_str::<Value>(&data.join("\n")) {
                if let Some(kind) = value.get("type").and_then(Value::as_str) {
                    last_type = Some(kind.to_string());
                }
            }
        }
        match last_type.as_deref() {
            Some("message_stop" | "response.completed" | "response.incomplete") => None,
            Some("error" | "response.failed") => Some("error"),
            _ => Some("none"),
        }
    }
}

fn request_json(request: &CapturedRequest, with_body: bool) -> Value {
    let headers: serde_json::Map<String, Value> = request
        .headers
        .iter()
        .map(|(name, value)| (name.clone(), Value::String(value.clone())))
        .collect();
    let mut out = json!({
        "method": request.method,
        "url": request.url,
        "headers": headers,
    });
    if with_body {
        out["body"] = request.body.clone();
    }
    out
}

fn body_json(body: &CapturedBody) -> Value {
    let chunks: Vec<Value> = body
        .chunks
        .iter()
        .map(|(offset_ms, chunk)| json!({ "t_ms": offset_ms, "bytes": chunk.len() }))
        .collect();
    json!({
        "stream": body.is_stream,
        "bytes": body.bytes,
        "truncated": body.truncated,
        "error": body.error,
        "first_chunk_ms": body.chunks.first().map(|(offset_ms, _)| offset_ms),
        "last_chunk_ms": body.chunks.last().map(|(offset_ms, _)| offset_ms),
        "chunks": chunks,
    })
}

fn capture_json(record: &CaptureRecord, bridge: CaptureBridge) -> Value {
    json!({
        "format": CAPTURE_FORMAT_VERSION,
        "capture_id": record.id,
        "request_id": record.request_id,
        "captured_at": chrono::DateTime::from_timestamp_millis(record.started_at_ms)
            .map(|time| time.to_rfc3339()),
        "app_version": env!("CARGO_PKG_VERSION"),
        "app_type": record.app_type,
        "provider_id": record.provider_id,
        "bridge": bridge.dir_name(),
        "codex_oauth": record.codex_oauth,
        "client_request": record.client_request.as_ref().map(|r| request_json(r, false)),
        "upstream_request": record.upstream_request.as_ref().map(|r| request_json(r, true)),
        "upstream_status": record.upstream_status,
        "upstream_response": body_json(&record.upstream),
        "client_response": body_json(&record.client),
    })
}
