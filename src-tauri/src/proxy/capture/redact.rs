//! 抓包脱敏：请求头按名称整体抹掉，JSON 按键名抹掉，其余文本按密钥形态替换。
//!
//! 覆盖的密钥形态：`sk-` / `sk-ant-`（OpenAI、Anthropic 等）、`AIza`（Google）、
//! `gh?_` / `github_pat_`（GitHub / Copilot）、`Bearer <token>`、`x-api-key` 等
//! 头部写法、`Cookie` / `Set-Cookie`，以及 URL 查询参数中的 `key=` / `token=`。
//! 替换后的文本保持 JSON / SSE 结构不变（只替换字符串内部的密钥字符）。

use std::sync::LazyLock;

use http::HeaderMap;
use regex::Regex;
use serde_json::Value;

/// 替换密钥用的占位符
pub const REDACTED: &str = "[REDACTED]";

/// 值需要整体抹掉的请求头 / JSON 键（小写，`-` 视同 `_`）
const SENSITIVE_KEYS: &[&str] = &[
    "authorization",
    "proxy_authorization",
    "x_api_key",
    "api_key",
    "apikey",
    "x_goog_api_key",
    "cookie",
    "set_cookie",
    "access_token",
    "refresh_token",
    "id_token",
    "token",
    "client_secret",
    "secret",
    "password",
];

/// 键名片段：含这些片段的请求头一律抹掉（如 `x-auth-token`、`openai-api-key`）
const SENSITIVE_HEADER_FRAGMENTS: &[&str] = &["auth", "token", "secret", "api_key", "cookie"];

fn normalize_key(key: &str) -> String {
    key.trim().to_ascii_lowercase().replace('-', "_")
}

fn is_sensitive_key(key: &str) -> bool {
    SENSITIVE_KEYS.contains(&normalize_key(key).as_str())
}

fn is_sensitive_header(name: &str) -> bool {
    let name = normalize_key(name);
    SENSITIVE_KEYS.contains(&name.as_str())
        || SENSITIVE_HEADER_FRAGMENTS
            .iter()
            .any(|fragment| name.contains(fragment))
}

struct Rule {
    pattern: Regex,
    replacement: &'static str,
}

fn rule(pattern: &str, replacement: &'static str) -> Rule {
    Rule {
        pattern: Regex::new(pattern).expect("valid redaction pattern"),
        replacement,
    }
}

/// 密钥起始边界（捕获组 1，替换时原样保留）：文本开头、非单词字符，或 JSON
/// 转义序列 `\n` / `\t` 等。原始 SSE 字节里换行写作 `\n`，`\b` 在 `n` 与密钥
/// 首字母之间不成立，行首的密钥会漏掉。regex crate 不支持后行断言，故用捕获组。
const KEY_START: &str = r"(^|[^A-Za-z0-9_]|\\[nrtbf])";

/// 裸密钥规则的替换：保留边界字符，只替换密钥本身
const KEEP_START: &str = "${1}[REDACTED]";

/// 按顺序应用的文本规则。先处理"键: 值"形式（整段抹掉），再处理裸密钥形态。
static TEXT_RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        // JSON 字符串里的敏感键值：`"api_key": "..."`（含转义引号）
        rule(
            r#"(?i)("(?:authorization|proxy-authorization|x-api-key|x_api_key|api[-_]?key|x-goog-api-key|cookie|set-cookie|access[-_]?token|refresh[-_]?token|id[-_]?token|client[-_]?secret|password)"\s*:\s*)"(?:[^"\\]|\\.)*""#,
            r#"$1"[REDACTED]""#,
        ),
        // 嵌在 JSON 字符串里的 JSON（`\"api_key\":\"...\"`）
        rule(
            r#"(?i)(\\"(?:authorization|x-api-key|api[-_]?key|x-goog-api-key|cookie|access[-_]?token|refresh[-_]?token|client[-_]?secret|password)\\"\s*:\s*)\\"(?:[^"\\]|\\[^"])*\\""#,
            r#"$1\"[REDACTED]\""#,
        ),
        // 文本中的头部行：`x-api-key: ...`、`Cookie: a=b; c=d`
        rule(
            &format!(
                r#"(?i){KEY_START}((?:proxy-)?authorization|x-api-key|x-goog-api-key|api-key|set-cookie|cookie)(\s*:\s*)[^\r\n"\\]+"#
            ),
            "${1}${2}${3}[REDACTED]",
        ),
        // Bearer token
        rule(
            &format!(r"(?i){KEY_START}(bearer)\s+[A-Za-z0-9._~+/=\-]+"),
            "${1}${2} [REDACTED]",
        ),
        // URL 查询参数
        rule(
            r#"(?i)([?&](?:key|api[-_]?key|access[-_]?token|token|auth)=)[^&\s"'#\\]+"#,
            "$1[REDACTED]",
        ),
        // Anthropic / OpenAI 风格（sk-ant-…、sk-proj-…、sk-…）
        rule(&format!(r"{KEY_START}sk-[A-Za-z0-9_\-]{{8,}}"), KEEP_START),
        // Google API key
        rule(
            &format!(r"{KEY_START}AIza[0-9A-Za-z_\-]{{20,}}"),
            KEEP_START,
        ),
        // GitHub token（ghu_ / ghp_ / gho_ / ghs_ / ghr_）与 fine-grained PAT
        rule(
            &format!(r"{KEY_START}gh[pousr]_[A-Za-z0-9]{{16,}}"),
            KEEP_START,
        ),
        rule(
            &format!(r"{KEY_START}github_pat_[A-Za-z0-9_]{{20,}}"),
            KEEP_START,
        ),
    ]
});

/// 替换文本中的密钥形态
pub fn redact_text(text: &str) -> String {
    let mut out = std::borrow::Cow::Borrowed(text);
    for rule in TEXT_RULES.iter() {
        if let std::borrow::Cow::Owned(replaced) = rule.pattern.replace_all(&out, rule.replacement)
        {
            out = std::borrow::Cow::Owned(replaced);
        }
    }
    out.into_owned()
}

/// 递归脱敏 JSON：敏感键的字符串值整体抹掉，其余字符串按文本规则替换
pub fn redact_json(value: &Value) -> Value {
    match value {
        Value::String(text) => Value::String(redact_text(text)),
        Value::Array(items) => Value::Array(items.iter().map(redact_json).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    let value = match value {
                        Value::String(_) if is_sensitive_key(key) => {
                            Value::String(REDACTED.to_string())
                        }
                        other => redact_json(other),
                    };
                    (key.clone(), value)
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

/// 脱敏请求头：敏感头整体抹掉，其余头的值按文本规则替换
pub fn redact_headers(headers: &HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .map(|(name, value)| {
            let value = if is_sensitive_header(name.as_str()) {
                REDACTED.to_string()
            } else {
                redact_text(&String::from_utf8_lossy(value.as_bytes()))
            };
            (name.as_str().to_string(), value)
        })
        .collect()
}

/// 脱敏原始字节（SSE 体等）：按 UTF-8 解码后替换；非 UTF-8 字节按 lossy 处理
pub fn redact_bytes(bytes: &[u8]) -> Vec<u8> {
    redact_text(&String::from_utf8_lossy(bytes)).into_bytes()
}
