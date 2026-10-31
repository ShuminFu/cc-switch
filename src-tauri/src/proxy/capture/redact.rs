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
    key.trim()
        .chars()
        .map(|c| {
            if c == '-' {
                '_'
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect()
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

/// 文本是否可能命中 [`TEXT_RULES`] 中的任一规则。
///
/// 每条规则都含有一个必需的字面量：`"`（JSON 键值）、`:`（头部行）、`=`（查询参数）、
/// `_`（GitHub token）、`sk-`、`AIza` 或不区分大小写的 `bearer`。都不含时没有规则能
/// 匹配，可跳过正则扫描——代理热路径上请求追踪会对每个请求头值调用 [`redact_text`]，
/// 而大多数头值（`application/json`、版本号等）不含这些字面量。
fn may_contain_secret(text: &str) -> bool {
    text.bytes().any(|b| matches!(b, b'"' | b':' | b'=' | b'_'))
        || text.contains("sk-")
        || text.contains("AIza")
        || text
            .as_bytes()
            .windows(6)
            .any(|window| window.eq_ignore_ascii_case(b"bearer"))
}

/// 替换文本中的密钥形态
pub fn redact_text(text: &str) -> String {
    if !may_contain_secret(text) {
        return text.to_string();
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 预检查只是快速路径：被它跳过的文本必须确实不会被任何规则匹配。
    /// 新增规则时若引入了新的必需字面量，这里会失败，提醒同步 [`may_contain_secret`]。
    #[test]
    fn may_contain_secret_never_skips_a_rule_match() {
        // 不含 `"` `:` `=` `_` 的片段（含大小写与 Unicode 折叠变体、转义与空白），
        // 让跳过正则的分支真正被覆盖
        let pieces: Vec<&str> = concat!(
            "a|b|e|r|s|k|A|B|E|R|S|K|I|z|0|9|-|?|&|/|.| |\\|\\n|\n|\t|ſ|\u{212a}|",
            "gh|ghp|ghu|github|pat|key|token|auth|cookie|Cookie|x-api-key|authorization|",
            "Bearer|bear|sk|sk-|AIza|AIz",
        )
        .split('|')
        .collect();
        let mut state: u64 = 0x5eed;
        let mut next = || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 33) as usize
        };
        let mut skipped = 0;
        for _ in 0..20_000 {
            let len = next() % 24;
            let text: String = (0..len).map(|_| pieces[next() % pieces.len()]).collect();
            if may_contain_secret(&text) {
                continue;
            }
            skipped += 1;
            for rule in TEXT_RULES.iter() {
                assert!(
                    !rule.pattern.is_match(&text),
                    "prefilter skipped {text:?} but {} matches",
                    rule.pattern.as_str()
                );
            }
        }
        assert!(
            skipped > 1_000,
            "only {skipped} samples exercised the skip path"
        );
    }

    #[test]
    fn may_contain_secret_accepts_every_rule_shape() {
        for text in [
            r#""api_key": "x""#,
            "x-api-key: abc",
            "BeArEr abcdefgh",
            "https://h/v1?key=abc",
            "sk-abcdefgh12",
            "AIzaSyD9tSrke72PouQMnMXa7eZSW0jkFMBWY",
            concat!("ghp", "_wWPw5k4aXcaT4fNP0UcnZwJUVFk6LO0pINUx"),
        ] {
            assert!(may_contain_secret(text), "{text}");
            assert!(redact_text(text).contains(REDACTED), "{text}");
        }
        for text in [
            "application/json",
            "2023-06-01",
            "claude-cli/1.0.0 (external, cli)",
        ] {
            assert!(!may_contain_secret(text), "{text}");
            assert_eq!(redact_text(text), text);
        }
    }
}
