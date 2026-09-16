//! Port of `src/utils/tomlUtils.ts`, plus the TOML parsing / error-reporting
//! helpers the other modules share.
//!
//! The TypeScript original parses with `smol-toml`; this port parses with the
//! `toml` crate (TOML 1.0) into [`toml::Value`] trees, which gives the same
//! "parse the whole document, then look things up" semantics. Parse errors are
//! surfaced with the parser's own line/column-annotated message.

use std::fmt;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use toml::Value as TomlValue;

use crate::text::normalize_toml_text;

/// A TOML syntax error with a 1-based `line` / `column` and the parser message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TomlSyntaxError {
    /// 1-based line of the error start (0 when the parser reported no span).
    pub line: usize,
    /// 1-based column (in characters) of the error start (0 when unknown).
    pub column: usize,
    /// The parser's short message (without the code snippet), e.g. `invalid table header`.
    pub message: String,
    /// The parser's full rendered report, with the `line X, column Y` header and a snippet.
    pub report: String,
}

impl fmt::Display for TomlSyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line > 0 {
            write!(
                f,
                "line {}, column {}: {}",
                self.line, self.column, self.message
            )
        } else {
            f.write_str(&self.message)
        }
    }
}

impl std::error::Error for TomlSyntaxError {}

/// Convert a byte offset into 1-based `(line, column)` where the column counts
/// characters from the start of the line.
fn line_column_at(text: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(text.len());
    let before = &text[..offset];
    let line = before.matches('\n').count() + 1;
    let line_start = before.rfind('\n').map_or(0, |idx| idx + 1);
    let column = before[line_start..].chars().count() + 1;
    (line, column)
}

fn syntax_error(text: &str, error: &toml::de::Error) -> TomlSyntaxError {
    let (line, column) = error
        .span()
        .map_or((0, 0), |span| line_column_at(text, span.start));
    let message = error.message().trim().to_string();
    TomlSyntaxError {
        line,
        column,
        message,
        report: error.to_string(),
    }
}

/// Parse `text` (already normalised by the caller) as a TOML table.
///
/// On failure the error carries a friendly 1-based line / column plus the
/// parser's message. `text` is parsed as-is: callers that accept user input
/// should run [`normalize_toml_text`] first, as `validate_toml` does.
pub fn parse_toml_table(text: &str) -> Result<toml::Table, TomlSyntaxError> {
    toml::from_str::<toml::Table>(text).map_err(|error| syntax_error(text, &error))
}

/// Parse `text` with quote normalisation applied first. Equivalent to
/// `parseToml(normalizeTomlText(text))` in the TypeScript sources.
pub fn parse_normalized_toml_table(text: &str) -> Result<toml::Table, TomlSyntaxError> {
    parse_toml_table(&normalize_toml_text(text))
}

/// Error returned by [`validate_toml`] when the parsed root is not a table
/// (unreachable for TOML documents, kept for parity with the TypeScript i18n key).
pub const TOML_MUST_BE_OBJECT: &str = "mustBeObject";

/// Fallback returned by [`validate_toml`] when the parser reports no message.
pub const TOML_PARSE_ERROR: &str = "parseError";

/// Port of `validateToml` (`tomlUtils.ts`).
///
/// Returns `""` for blank input and for a valid TOML table. A parse failure
/// returns the parser's raw report (which includes `line X, column Y` and a
/// snippet), or [`TOML_PARSE_ERROR`] if the parser produced no message. Quotes
/// are normalised before parsing.
pub fn validate_toml(text: &str) -> String {
    if text.trim().is_empty() {
        return String::new();
    }
    match toml::from_str::<TomlValue>(&normalize_toml_text(text)) {
        Ok(TomlValue::Table(_)) => String::new(),
        Ok(_) => TOML_MUST_BE_OBJECT.to_string(),
        Err(error) => {
            let report = error.to_string();
            if report.trim().is_empty() {
                TOML_PARSE_ERROR.to_string()
            } else {
                report
            }
        }
    }
}

/// Port of the `McpServerSpec` interface (`src/types.ts`).
///
/// Known fields are typed; every other key is preserved verbatim in `extra`
/// (the TypeScript type has an `[key: string]: any` index signature so
/// extension fields such as `timeout_ms` survive round trips).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct McpServerSpec {
    /// `"stdio" | "http" | "sse"`; optional because community `.mcp.json`
    /// stdio entries often omit it.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub server_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<IndexMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<IndexMap<String, String>>,
    /// Extension fields, kept in insertion order.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, JsonValue>,
}

/// JavaScript truthiness for a TOML value (`if (parsed.type || ...)`).
fn is_truthy(value: &TomlValue) -> bool {
    match value {
        TomlValue::String(s) => !s.is_empty(),
        TomlValue::Integer(i) => *i != 0,
        TomlValue::Float(f) => *f != 0.0 && !f.is_nan(),
        TomlValue::Boolean(b) => *b,
        TomlValue::Datetime(_) | TomlValue::Array(_) | TomlValue::Table(_) => true,
    }
}

/// Format an `f64` the way JavaScript's `String(number)` does for the common cases.
fn js_number_string(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if value == value.trunc() && value.abs() < 1e21 {
        return format!("{}", value as i128);
    }
    format!("{value}")
}

/// `String(value)` for a TOML value, mirroring how the TypeScript code
/// stringifies `args` / `env` / `headers` entries.
pub fn toml_value_js_string(value: &TomlValue) -> String {
    match value {
        TomlValue::String(s) => s.clone(),
        TomlValue::Integer(i) => i.to_string(),
        TomlValue::Float(f) => js_number_string(*f),
        TomlValue::Boolean(b) => b.to_string(),
        TomlValue::Datetime(dt) => dt.to_string(),
        TomlValue::Array(items) => items
            .iter()
            .map(toml_value_js_string)
            .collect::<Vec<_>>()
            .join(","),
        TomlValue::Table(_) => "[object Object]".to_string(),
    }
}

/// Convert a TOML value into a JSON value (datetimes become their string form).
pub fn toml_value_to_json(value: &TomlValue) -> JsonValue {
    match value {
        TomlValue::String(s) => JsonValue::String(s.clone()),
        TomlValue::Integer(i) => JsonValue::from(*i),
        TomlValue::Float(f) => serde_json::Number::from_f64(*f)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        TomlValue::Boolean(b) => JsonValue::Bool(*b),
        TomlValue::Datetime(dt) => JsonValue::String(dt.to_string()),
        TomlValue::Array(items) => JsonValue::Array(items.iter().map(toml_value_to_json).collect()),
        TomlValue::Table(table) => JsonValue::Object(
            table
                .iter()
                .map(|(k, v)| (k.clone(), toml_value_to_json(v)))
                .collect(),
        ),
    }
}

/// Convert a JSON value into a TOML value. `null` has no TOML representation
/// and yields `None`; nulls nested in arrays / objects are dropped.
pub fn json_value_to_toml(value: &JsonValue) -> Option<TomlValue> {
    match value {
        JsonValue::Null => None,
        JsonValue::Bool(b) => Some(TomlValue::Boolean(*b)),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(TomlValue::Integer(i))
            } else if let Some(u) = n.as_u64() {
                i64::try_from(u)
                    .map(TomlValue::Integer)
                    .ok()
                    .or_else(|| Some(TomlValue::Float(u as f64)))
            } else {
                n.as_f64().map(TomlValue::Float)
            }
        }
        JsonValue::String(s) => Some(TomlValue::String(s.clone())),
        JsonValue::Array(items) => Some(TomlValue::Array(
            items.iter().filter_map(json_value_to_toml).collect(),
        )),
        JsonValue::Object(map) => Some(TomlValue::Table(
            map.iter()
                .filter_map(|(k, v)| json_value_to_toml(v).map(|v| (k.clone(), v)))
                .collect(),
        )),
    }
}

/// Port of `mcpServerToToml` (`tomlUtils.ts`).
///
/// Serialises the spec (including extension fields) to TOML and trims the
/// result for display in a text box. Keys whose value is `undefined` in the
/// TypeScript object are absent here (`None` / `null` values are dropped).
/// Escaping and nested tables are handled by the serializer; there are no
/// comments to preserve because the text is generated.
pub fn mcp_server_to_toml(server: &McpServerSpec) -> String {
    let json = serde_json::to_value(server).unwrap_or(JsonValue::Object(Default::default()));
    let table = match json_value_to_toml(&json) {
        Some(TomlValue::Table(table)) => table,
        _ => toml::Table::new(),
    };
    toml::to_string(&table)
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Error message: blank input to [`toml_to_mcp_server`].
pub const MCP_TOML_EMPTY: &str = "TOML 内容不能为空";
/// Error message: no recognised MCP server shape in [`toml_to_mcp_server`].
pub const MCP_TOML_UNRECOGNIZED: &str =
    "无法识别的 TOML 格式。请提供单个 MCP 服务器配置，或使用 [mcp_servers.<id>] 格式";
/// Error message: the server entry is not a table.
pub const MCP_SERVER_NOT_OBJECT: &str = "服务器配置必须是对象";
/// Error message: a stdio server without a string `command`.
pub const MCP_STDIO_COMMAND_REQUIRED: &str = "stdio 类型的 MCP 服务器必须包含 command 字段";

/// First `(key, value)` of an object-like value the way `Object.keys(x)[0]`
/// sees it: tables by insertion order, arrays by index (`"0"`), anything else
/// has no keys.
fn first_entry(value: &TomlValue) -> Option<(String, &TomlValue)> {
    match value {
        TomlValue::Table(table) => table.iter().next().map(|(k, v)| (k.clone(), v)),
        TomlValue::Array(items) => items.first().map(|v| ("0".to_string(), v)),
        _ => None,
    }
}

/// `x && typeof x === "object"` for a TOML value (tables, arrays and datetimes
/// are objects in JavaScript; datetimes have no enumerable keys though).
fn is_object_like(value: &TomlValue) -> bool {
    matches!(
        value,
        TomlValue::Table(_) | TomlValue::Array(_) | TomlValue::Datetime(_)
    )
}

fn nested_first_entry<'a>(root: &'a toml::Table, path: &[&str]) -> Option<(String, &'a TomlValue)> {
    let mut current: &TomlValue = root.get(path[0])?;
    for key in &path[1..] {
        if !is_object_like(current) {
            return None;
        }
        current = match current {
            TomlValue::Table(table) => table.get(*key)?,
            TomlValue::Array(items) => items.get(key.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    if !is_object_like(current) {
        return None;
    }
    first_entry(current)
}

/// Port of `tomlToMcpServer` (`tomlUtils.ts`).
///
/// Accepts, in order: (1) a bare server table (detected by a truthy
/// `type` / `command` / `url` / `args` / `env`), (2) `[mcp_servers.<id>]` — the
/// **first** id in key order, (3) the malformed `[mcp.servers.<id>]` shape.
/// Errors are the exact Chinese messages of the TypeScript implementation.
pub fn toml_to_mcp_server(toml_text: &str) -> Result<McpServerSpec, String> {
    if toml_text.trim().is_empty() {
        return Err(MCP_TOML_EMPTY.to_string());
    }

    let parsed = parse_normalized_toml_table(toml_text).map_err(|e| e.report)?;

    let looks_like_server = ["type", "command", "url", "args", "env"]
        .iter()
        .any(|key| parsed.get(*key).is_some_and(is_truthy));
    if looks_like_server {
        return normalize_server_config(&TomlValue::Table(parsed));
    }

    if let Some((_, first_server)) = nested_first_entry(&parsed, &["mcp_servers"]) {
        return normalize_server_config(first_server);
    }

    if let Some((_, first_server)) = nested_first_entry(&parsed, &["mcp", "servers"]) {
        return normalize_server_config(first_server);
    }

    Err(MCP_TOML_UNRECOGNIZED.to_string())
}

fn string_map_from(value: &TomlValue) -> IndexMap<String, String> {
    match value {
        TomlValue::Table(table) => table
            .iter()
            .map(|(k, v)| (k.clone(), toml_value_js_string(v)))
            .collect(),
        TomlValue::Array(items) => items
            .iter()
            .enumerate()
            .map(|(i, v)| (i.to_string(), toml_value_js_string(v)))
            .collect(),
        _ => IndexMap::new(),
    }
}

/// Port of `normalizeServerConfig` (`tomlUtils.ts`, not exported there).
fn normalize_server_config(config: &TomlValue) -> Result<McpServerSpec, String> {
    // Arrays are `typeof "object"` in JavaScript, so they pass the guard and
    // then fail the type-specific checks below; anything else is rejected.
    let empty = toml::Table::new();
    let table: &toml::Table = match config {
        TomlValue::Table(table) => table,
        TomlValue::Array(_) => &empty,
        _ => return Err(MCP_SERVER_NOT_OBJECT.to_string()),
    };

    let type_value = table.get("type").filter(|v| is_truthy(v));
    let type_display = type_value.map_or_else(|| "stdio".to_string(), toml_value_js_string);
    let type_str = match type_value {
        None => Some("stdio"),
        Some(TomlValue::String(s)) => Some(s.as_str()),
        Some(_) => None,
    };

    let mut server = McpServerSpec::default();
    let mut known: Vec<&str> = Vec::new();

    match type_str {
        Some("stdio") => {
            let command = match table.get("command") {
                Some(TomlValue::String(s)) if !s.is_empty() => s.clone(),
                _ => return Err(MCP_STDIO_COMMAND_REQUIRED.to_string()),
            };
            server.server_type = Some("stdio".to_string());
            server.command = Some(command);
            known.extend(["type", "command"]);

            if let Some(TomlValue::Array(args)) = table.get("args") {
                server.args = Some(args.iter().map(toml_value_js_string).collect());
                known.push("args");
            }
            if let Some(env) = table
                .get("env")
                .filter(|v| is_truthy(v) && is_object_like(v))
            {
                server.env = Some(string_map_from(env));
                known.push("env");
            }
            if let Some(TomlValue::String(cwd)) = table.get("cwd") {
                if !cwd.is_empty() {
                    server.cwd = Some(cwd.clone());
                    known.push("cwd");
                }
            }
        }
        Some(kind @ ("http" | "sse")) => {
            let url = match table.get("url") {
                Some(TomlValue::String(s)) if !s.is_empty() => s.clone(),
                _ => return Err(format!("{kind} 类型的 MCP 服务器必须包含 url 字段")),
            };
            server.server_type = Some(kind.to_string());
            server.url = Some(url);
            known.extend(["type", "url"]);

            if let Some(headers) = table
                .get("headers")
                .filter(|v| is_truthy(v) && is_object_like(v))
            {
                server.headers = Some(string_map_from(headers));
                known.push("headers");
            }
        }
        _ => return Err(format!("不支持的 MCP 服务器类型: {type_display}")),
    }

    for (key, value) in table {
        if !known.contains(&key.as_str()) {
            server.extra.insert(key.clone(), toml_value_to_json(value));
        }
    }

    Ok(server)
}

/// Port of `extractIdFromToml` (`tomlUtils.ts`).
///
/// Best-effort id suggestion: the first key under `mcp_servers`, else the
/// first under `mcp.servers`, else the last path segment of a top-level
/// `command` string with a trailing `.exe|.bat|.sh|.js|.py` (case-insensitive)
/// stripped. Parse failure or nothing found → `""`.
pub fn extract_id_from_toml(toml_text: &str) -> String {
    let Ok(parsed) = parse_normalized_toml_table(toml_text) else {
        return String::new();
    };

    if let Some((id, _)) = nested_first_entry(&parsed, &["mcp_servers"]) {
        return id;
    }
    if let Some((id, _)) = nested_first_entry(&parsed, &["mcp", "servers"]) {
        return id;
    }

    if let Some(TomlValue::String(command)) = parsed.get("command") {
        if !command.is_empty() {
            // `command.split(/[\\/]/).pop() || ""`
            let last = command.rsplit(['/', '\\']).next().unwrap_or_default();
            return strip_executable_extension(last).to_string();
        }
    }

    String::new()
}

/// `cmd.replace(/\.(exe|bat|sh|js|py)$/i, "")`
fn strip_executable_extension(name: &str) -> &str {
    for ext in [".exe", ".bat", ".sh", ".js", ".py"] {
        if name.len() >= ext.len() && name.is_char_boundary(name.len() - ext.len()) {
            let (stem, tail) = name.split_at(name.len() - ext.len());
            if tail.eq_ignore_ascii_case(ext) {
                return stem;
            }
        }
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_toml_accepts_blank_and_valid_tables() {
        assert_eq!(validate_toml(""), "");
        assert_eq!(validate_toml("   \n"), "");
        assert_eq!(validate_toml("a = 1\n[t]\nb = \"x\"\n"), "");
    }

    #[test]
    fn validate_toml_normalizes_quotes_before_parsing() {
        assert_eq!(validate_toml("a = “x”\n"), "");
    }

    #[test]
    fn validate_toml_reports_line_and_column() {
        let report = validate_toml("[models");
        assert!(report.contains("line 1, column 8"), "{report}");
        assert!(report.contains("invalid table header"), "{report}");
    }

    #[test]
    fn parse_toml_table_reports_friendly_location() {
        let err = parse_toml_table("a = 1\nb = \n").unwrap_err();
        assert_eq!(err.line, 2);
        assert!(err.column >= 4, "{err:?}");
        assert!(!err.message.is_empty());
        assert!(err.report.contains("line 2, column"));
        assert!(err.to_string().starts_with("line 2, column "));
    }

    #[test]
    fn line_column_counts_characters() {
        assert_eq!(line_column_at("ab\ncd", 4), (2, 2));
        assert_eq!(line_column_at("日本\nx", 7), (2, 1));
        assert_eq!(line_column_at("", 0), (1, 1));
    }

    #[test]
    fn mcp_server_to_toml_round_trips_a_stdio_server() {
        let mut spec = McpServerSpec {
            server_type: Some("stdio".to_string()),
            command: Some("npx".to_string()),
            args: Some(vec!["-y".to_string(), "server".to_string()]),
            ..Default::default()
        };
        spec.extra
            .insert("timeout_ms".to_string(), JsonValue::from(5000));
        let text = mcp_server_to_toml(&spec);
        assert!(!text.ends_with('\n'));
        assert!(text.contains("type = \"stdio\""));
        assert!(text.contains("command = \"npx\""));
        assert!(text.contains("timeout_ms = 5000"));
        let back = toml_to_mcp_server(&text).unwrap();
        assert_eq!(back, spec);
    }

    #[test]
    fn mcp_server_to_toml_writes_env_as_a_table_and_skips_none() {
        let mut env = IndexMap::new();
        env.insert("B".to_string(), "2".to_string());
        env.insert("A".to_string(), "1".to_string());
        let spec = McpServerSpec {
            command: Some("run".to_string()),
            env: Some(env),
            ..Default::default()
        };
        let text = mcp_server_to_toml(&spec);
        assert!(text.contains("[env]\nB = \"2\"\nA = \"1\""), "{text}");
        assert!(!text.contains("type"));
        assert!(!text.contains("args"));
    }

    #[test]
    fn toml_to_mcp_server_rejects_blank() {
        assert_eq!(toml_to_mcp_server("   ").unwrap_err(), MCP_TOML_EMPTY);
    }

    #[test]
    fn toml_to_mcp_server_reads_a_bare_stdio_server() {
        let spec = toml_to_mcp_server(
            "command = \"node\"\nargs = [\"index.js\", 1]\n\n[env]\nPORT = 8080\n",
        )
        .unwrap();
        assert_eq!(spec.server_type.as_deref(), Some("stdio"));
        assert_eq!(spec.command.as_deref(), Some("node"));
        assert_eq!(
            spec.args,
            Some(vec!["index.js".to_string(), "1".to_string()])
        );
        assert_eq!(spec.env.unwrap()["PORT"], "8080");
        assert!(spec.extra.is_empty());
    }

    #[test]
    fn toml_to_mcp_server_keeps_extension_fields_and_cwd() {
        let spec = toml_to_mcp_server(
            "type = \"stdio\"\ncommand = \"run.sh\"\ncwd = \"/tmp\"\ntimeout_ms = 5000\n",
        )
        .unwrap();
        assert_eq!(spec.cwd.as_deref(), Some("/tmp"));
        assert_eq!(spec.extra["timeout_ms"], JsonValue::from(5000));
    }

    #[test]
    fn toml_to_mcp_server_requires_command_for_stdio() {
        assert_eq!(
            toml_to_mcp_server("type = \"stdio\"\n").unwrap_err(),
            MCP_STDIO_COMMAND_REQUIRED
        );
        assert_eq!(
            toml_to_mcp_server("type = \"stdio\"\ncommand = \"\"\n").unwrap_err(),
            MCP_STDIO_COMMAND_REQUIRED
        );
    }

    #[test]
    fn toml_to_mcp_server_reads_http_and_sse_servers() {
        let http = toml_to_mcp_server(
            "type = \"http\"\nurl = \"https://x\"\n\n[headers]\nAuthorization = \"Bearer t\"\n",
        )
        .unwrap();
        assert_eq!(http.server_type.as_deref(), Some("http"));
        assert_eq!(http.url.as_deref(), Some("https://x"));
        assert_eq!(http.headers.unwrap()["Authorization"], "Bearer t");

        assert_eq!(
            toml_to_mcp_server("type = \"sse\"\n").unwrap_err(),
            "sse 类型的 MCP 服务器必须包含 url 字段"
        );
        assert_eq!(
            toml_to_mcp_server("type = \"http\"\nurl = \"\"\n").unwrap_err(),
            "http 类型的 MCP 服务器必须包含 url 字段"
        );
    }

    #[test]
    fn toml_to_mcp_server_rejects_unknown_types() {
        assert_eq!(
            toml_to_mcp_server("type = \"grpc\"\n").unwrap_err(),
            "不支持的 MCP 服务器类型: grpc"
        );
        assert_eq!(
            toml_to_mcp_server("type = 5\n").unwrap_err(),
            "不支持的 MCP 服务器类型: 5"
        );
    }

    #[test]
    fn toml_to_mcp_server_takes_first_mcp_servers_entry() {
        let spec = toml_to_mcp_server(
            "[mcp_servers.alpha]\ncommand = \"a\"\n\n[mcp_servers.beta]\ncommand = \"b\"\n",
        )
        .unwrap();
        assert_eq!(spec.command.as_deref(), Some("a"));
    }

    #[test]
    fn toml_to_mcp_server_tolerates_mcp_dot_servers() {
        let spec =
            toml_to_mcp_server("[mcp.servers.demo]\ntype = \"http\"\nurl = \"u\"\n").unwrap();
        assert_eq!(spec.url.as_deref(), Some("u"));
    }

    #[test]
    fn toml_to_mcp_server_rejects_unrecognized_shapes() {
        assert_eq!(
            toml_to_mcp_server("[other]\nx = 1\n").unwrap_err(),
            MCP_TOML_UNRECOGNIZED
        );
        assert_eq!(
            toml_to_mcp_server("[mcp_servers]\n").unwrap_err(),
            MCP_TOML_UNRECOGNIZED
        );
        assert_eq!(
            toml_to_mcp_server("[mcp_servers]\ndemo = \"str\"\n").unwrap_err(),
            MCP_SERVER_NOT_OBJECT
        );
    }

    #[test]
    fn toml_to_mcp_server_surfaces_parse_errors() {
        let err = toml_to_mcp_server("[mcp_servers").unwrap_err();
        assert!(err.contains("line 1, column"), "{err}");
    }

    #[test]
    fn extract_id_prefers_mcp_servers_key() {
        assert_eq!(
            extract_id_from_toml("[mcp_servers.demo]\ncommand = \"x\"\n"),
            "demo"
        );
        assert_eq!(
            extract_id_from_toml("[mcp.servers.legacy]\ncommand = \"x\"\n"),
            "legacy"
        );
    }

    #[test]
    fn extract_id_derives_from_command_path() {
        assert_eq!(
            extract_id_from_toml("command = \"/usr/bin/server.py\"\n"),
            "server"
        );
        assert_eq!(
            extract_id_from_toml("command = 'C:\\\\tools\\\\Tool.EXE'\n"),
            "Tool"
        );
        assert_eq!(extract_id_from_toml("command = \"npx\"\n"), "npx");
        assert_eq!(extract_id_from_toml("command = \"a.tar.gz\"\n"), "a.tar.gz");
    }

    #[test]
    fn extract_id_returns_empty_on_failure() {
        assert_eq!(extract_id_from_toml(""), "");
        assert_eq!(extract_id_from_toml("[broken"), "");
        assert_eq!(extract_id_from_toml("x = 1\n"), "");
        assert_eq!(extract_id_from_toml("command = \"\"\n"), "");
    }

    #[test]
    fn js_string_conversion_matches_javascript() {
        assert_eq!(toml_value_js_string(&TomlValue::Float(1.0)), "1");
        assert_eq!(toml_value_js_string(&TomlValue::Float(1.5)), "1.5");
        assert_eq!(toml_value_js_string(&TomlValue::Boolean(true)), "true");
        assert_eq!(
            toml_value_js_string(&TomlValue::Array(vec![
                TomlValue::Integer(1),
                TomlValue::String("b".into())
            ])),
            "1,b"
        );
        assert_eq!(
            toml_value_js_string(&TomlValue::Table(toml::Table::new())),
            "[object Object]"
        );
    }
}
