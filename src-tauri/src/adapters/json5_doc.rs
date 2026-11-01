//! Comment- and order-preserving JSON5/JSONC documents (CFG-07, CFG-09, CFG-37).
//!
//! [`Json5Doc`] wraps the `json_five` round-trip AST. Instead of serialising a
//! whole `serde_json::Value` (which drops comments and reorders keys), callers
//! compute the new value as usual and [`Json5Doc::apply`] patches only the
//! parts of the document that differ:
//!
//! * unchanged subtrees, including their comments and whitespace, are kept
//!   byte-for-byte;
//! * changed scalars/arrays are re-rendered in place with the indentation of
//!   the surrounding document;
//! * new keys are appended to their object, removed keys are cut out together
//!   with their trailing comma.
//!
//! Key quoting follows the document: a file that only uses `"quoted"` keys (a
//! plain JSON / JSONC file such as `opencode.json`) never gets bare JSON5
//! identifiers.

use json_five::rt::parser::{
    from_str as rt_from_str, JSONKeyValuePair, JSONObjectContext, JSONText, JSONValue,
    KeyValuePairContext,
};
use serde_json::Value;

use crate::error::AppError;

/// Formatting conventions sniffed from the document.
#[derive(Debug, Clone)]
pub(crate) struct DocStyle {
    pub eol: String,
    pub unit: String,
    pub identifier_keys: bool,
}

pub struct Json5Doc {
    text: JSONText,
    style: DocStyle,
}

impl Json5Doc {
    /// Parse a JSON5/JSONC/JSON document (a UTF-8 BOM is tolerated).
    pub fn parse(source: &str) -> Result<Self, AppError> {
        let source = super::live_file::strip_bom(source);
        let mut text = rt_from_str(source).map_err(|e| {
            AppError::Config(format!(
                "Failed to parse JSON5 document (line {}, column {}): {}",
                e.lineno, e.colno, e.message
            ))
        })?;
        repair_block_comments(&mut text, source);
        // Only edit documents the AST reproduces exactly; callers fall back to
        // a plain rewrite (with backup) otherwise.
        if text.to_string() != source {
            return Err(AppError::Config(
                "JSON5 document cannot be edited without changing its formatting".to_string(),
            ));
        }
        let style = detect_style(source, &text.value);
        Ok(Self { text, style })
    }

    /// The document as a `serde_json::Value`.
    pub fn to_value(&self) -> Result<Value, AppError> {
        rt_to_value(&self.text.value)
    }

    /// Patch the document so that it represents `new_root`, touching only the
    /// parts that differ.
    pub fn apply(&mut self, new_root: &Value) -> Result<(), AppError> {
        apply_diff(&mut self.text.value, new_root, "", &self.style)
    }

    /// Patch only the subtree at the given object path (missing objects along
    /// the path are created).
    pub fn apply_at(&mut self, path: &[&str], new_value: &Value) -> Result<(), AppError> {
        let mut root = self.to_value()?;
        let mut cursor = &mut root;
        for key in path {
            if !cursor.is_object() {
                *cursor = Value::Object(Default::default());
            }
            cursor = cursor
                .as_object_mut()
                .expect("object ensured above")
                .entry(key.to_string())
                .or_insert_with(|| Value::Object(Default::default()));
        }
        *cursor = new_value.clone();
        self.apply(&root)
    }

    pub fn render(&self) -> String {
        self.text.to_string()
    }
}

/// Patch a raw root value in place (used by adapters that keep their own
/// `JSONText`, e.g. OpenClaw).
pub(crate) fn apply_to_root(
    root: &mut JSONValue,
    source: &str,
    new_root: &Value,
) -> Result<(), AppError> {
    let style = detect_style(source, root);
    apply_diff(root, new_root, "", &style)
}

/// `json_five` 0.3.1 drops the closing `/` of every block comment from the
/// round-trip AST (its token span ends one byte early), so `/* c */` would be
/// written back as `/* c *`. Collect the block comments from the source and
/// put the missing byte back, walking the AST in document order.
pub(crate) fn repair_block_comments(text: &mut JSONText, source: &str) {
    let comments = scan_block_comments(source);
    if comments.is_empty() {
        return;
    }
    let mut queue = comments.into_iter();
    let mut fix = |ws: &mut String| repair_ws(ws, &mut queue);
    if let Some(ctx) = text.context.as_mut() {
        fix(&mut ctx.wsc.0);
    }
    visit_ws(&mut text.value, &mut fix);
    if let Some(ctx) = text.context.as_mut() {
        fix(&mut ctx.wsc.1);
    }
}

fn repair_ws(ws: &mut String, queue: &mut impl Iterator<Item = String>) {
    if !ws.contains("/*") {
        return;
    }
    let mut out = String::with_capacity(ws.len() + 2);
    let mut rest = ws.as_str();
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("//") {
            let end = after.find('\n').map(|i| i + 2).unwrap_or(rest.len());
            out.push_str(&rest[..end]);
            rest = &rest[end..];
        } else if rest.starts_with("/*") {
            let Some(original) = queue.next() else {
                out.push_str(rest);
                break;
            };
            let broken = &original[..original.len() - 1];
            // The AST always holds the truncated form (`/* c *`).
            if rest.starts_with(broken) {
                out.push_str(&original);
                rest = &rest[broken.len()..];
            } else {
                out.push_str(rest);
                break;
            }
        } else {
            let ch = rest.chars().next().expect("non-empty");
            out.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
    }
    *ws = out;
}

fn visit_ws(value: &mut JSONValue, f: &mut dyn FnMut(&mut String)) {
    match value {
        JSONValue::JSONObject {
            key_value_pairs,
            context,
        } => {
            if let Some(ctx) = context.as_mut() {
                f(&mut ctx.wsc.0);
            }
            for pair in key_value_pairs {
                if let Some(ctx) = pair.context.as_mut() {
                    f(&mut ctx.wsc.0);
                    f(&mut ctx.wsc.1);
                }
                visit_ws(&mut pair.value, f);
                if let Some(ctx) = pair.context.as_mut() {
                    f(&mut ctx.wsc.2);
                    if let Some(after) = ctx.wsc.3.as_mut() {
                        f(after);
                    }
                }
            }
        }
        JSONValue::JSONArray { values, context } => {
            if let Some(ctx) = context.as_mut() {
                f(&mut ctx.wsc.0);
            }
            for item in values {
                visit_ws(&mut item.value, f);
                if let Some(ctx) = item.context.as_mut() {
                    f(&mut ctx.wsc.0);
                    if let Some(after) = ctx.wsc.1.as_mut() {
                        f(after);
                    }
                }
            }
        }
        _ => {}
    }
}

/// Block comments of a JSON5 source in document order (strings skipped).
fn scan_block_comments(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' | b'\'' => {
                let quote = bytes[i];
                i += 1;
                while i < bytes.len() && bytes[i] != quote {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i += 1;
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let start = i;
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
                out.push(source[start..i].to_string());
            }
            _ => i += 1,
        }
    }
    out
}

pub(crate) fn rt_to_value(value: &JSONValue) -> Result<Value, AppError> {
    json_five::from_str(&value.to_string())
        .map_err(|e| AppError::Config(format!("Failed to read JSON5 value: {e}")))
}

fn detect_style(source: &str, root: &JSONValue) -> DocStyle {
    let eol = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
    .to_string();
    let unit = match root {
        JSONValue::JSONObject {
            context: Some(ctx), ..
        } if ctx.wsc.0.contains('\n') => trailing_indent(&ctx.wsc.0),
        _ => String::new(),
    };
    let unit = if unit.is_empty() {
        "  ".to_string()
    } else {
        unit
    };
    let mut identifier_keys = false;
    visit_keys(root, &mut |key| {
        if matches!(key, JSONValue::Identifier(_)) {
            identifier_keys = true;
        }
    });
    DocStyle {
        eol,
        unit,
        identifier_keys,
    }
}

fn visit_keys(value: &JSONValue, f: &mut dyn FnMut(&JSONValue)) {
    match value {
        JSONValue::JSONObject {
            key_value_pairs, ..
        } => {
            for pair in key_value_pairs {
                f(&pair.key);
                visit_keys(&pair.value, f);
            }
        }
        JSONValue::JSONArray { values, .. } => {
            for v in values {
                visit_keys(&v.value, f);
            }
        }
        _ => {}
    }
}

/// Indentation after the last newline of a whitespace/comment run.
fn trailing_indent(ws: &str) -> String {
    ws.rsplit_once('\n')
        .map(|(_, tail)| tail.trim_end_matches('\r').to_string())
        .unwrap_or_default()
}

/// Split at the first newline: `(same-line part, rest starting with the newline)`.
fn split_first_line(ws: &str) -> (&str, &str) {
    match ws.find('\n') {
        Some(idx) => {
            let head = &ws[..idx];
            let head = head.strip_suffix('\r').unwrap_or(head);
            let cut = head.len();
            (&ws[..cut], &ws[cut..])
        }
        None => (ws, ""),
    }
}

/// Split at the last newline: `(content before it, newline + closing indent)`.
fn split_last_line(ws: &str) -> (&str, &str) {
    match ws.rfind('\n') {
        Some(idx) => {
            let cut = if idx > 0 && ws.as_bytes()[idx - 1] == b'\r' {
                idx - 1
            } else {
                idx
            };
            (&ws[..cut], &ws[cut..])
        }
        None => ("", ws),
    }
}

fn key_name(key: &JSONValue) -> Option<String> {
    match key {
        JSONValue::Identifier(name) => Some(name.clone()),
        JSONValue::DoubleQuotedString(raw) => {
            serde_json::from_str::<String>(&format!("\"{raw}\"")).ok()
        }
        JSONValue::SingleQuotedString(raw) => {
            json_five::from_str::<String>(&format!("'{raw}'")).ok()
        }
        _ => None,
    }
}

fn make_key(key: &str, style: &DocStyle) -> JSONValue {
    let is_identifier = {
        let mut chars = key.chars();
        chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
    };
    if style.identifier_keys && is_identifier {
        JSONValue::Identifier(key.to_string())
    } else {
        let quoted = serde_json::to_string(key).unwrap_or_else(|_| format!("\"{key}\""));
        JSONValue::DoubleQuotedString(quoted[1..quoted.len() - 1].to_string())
    }
}

/// Render a value as a JSON5 AST node whose continuation lines are indented
/// relative to `indent`.
fn render_value(
    value: &Value,
    indent: &str,
    multiline: bool,
    style: &DocStyle,
) -> Result<JSONValue, AppError> {
    let source = if multiline {
        let mut buf = Vec::new();
        let formatter = serde_json::ser::PrettyFormatter::with_indent(style.unit.as_bytes());
        let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
        serde::Serialize::serialize(value, &mut ser)
            .map_err(|e| AppError::JsonSerialize { source: e })?;
        let pretty = String::from_utf8(buf).map_err(|e| AppError::Message(e.to_string()))?;
        let mut out = String::with_capacity(pretty.len());
        for (i, line) in pretty.split('\n').enumerate() {
            if i > 0 {
                out.push_str(&style.eol);
                out.push_str(indent);
            }
            out.push_str(line);
        }
        out
    } else {
        serde_json::to_string(value).map_err(|e| AppError::JsonSerialize { source: e })?
    };
    let text = rt_from_str(&source)
        .map_err(|e| AppError::Config(format!("Failed to build JSON5 value: {}", e.message)))?;
    Ok(text.value)
}

fn ensure_ctx(pair: &mut JSONKeyValuePair) -> &mut KeyValuePairContext {
    pair.context.get_or_insert_with(|| KeyValuePairContext {
        wsc: (String::new(), " ".to_string(), String::new(), None),
    })
}

fn apply_diff(
    target: &mut JSONValue,
    new: &Value,
    indent: &str,
    style: &DocStyle,
) -> Result<(), AppError> {
    let old = rt_to_value(target)?;
    if old == *new {
        return Ok(());
    }

    let (
        JSONValue::JSONObject {
            key_value_pairs,
            context,
        },
        Value::Object(new_map),
    ) = (&mut *target, new)
    else {
        let multiline = !matches!(new, Value::Object(m) if m.is_empty())
            && !matches!(new, Value::Array(a) if a.is_empty());
        *target = render_value(new, indent, multiline, style)?;
        return Ok(());
    };

    let leading = context
        .as_ref()
        .map(|c| c.wsc.0.clone())
        .unwrap_or_default();
    let multiline = key_value_pairs.is_empty() || leading.contains('\n');
    let child_indent = if !key_value_pairs.is_empty() && leading.contains('\n') {
        trailing_indent(&leading)
    } else {
        format!("{indent}{}", style.unit)
    };

    // 1) removals (iterate backwards so indices stay valid)
    let mut idx = key_value_pairs.len();
    while idx > 0 {
        idx -= 1;
        let keep = key_name(&key_value_pairs[idx].key)
            .map(|k| new_map.contains_key(&k))
            .unwrap_or(true);
        if !keep {
            remove_pair(key_value_pairs, context, idx);
        }
    }

    // 2) updates and insertions, in the order of the new map
    for (key, value) in new_map {
        let existing = key_value_pairs
            .iter_mut()
            .rev()
            .find(|pair| key_name(&pair.key).as_deref() == Some(key.as_str()));
        if let Some(pair) = existing {
            apply_diff(&mut pair.value, value, &child_indent, style)?;
            continue;
        }
        let rendered = render_value(value, &child_indent, multiline, style)?;
        insert_pair(
            key_value_pairs,
            context,
            make_key(key, style),
            rendered,
            indent,
            &child_indent,
            multiline,
            style,
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn insert_pair(
    pairs: &mut Vec<JSONKeyValuePair>,
    context: &mut Option<JSONObjectContext>,
    key: JSONValue,
    value: JSONValue,
    indent: &str,
    child_indent: &str,
    multiline: bool,
    style: &DocStyle,
) {
    let separator = if multiline {
        format!("{}{child_indent}", style.eol)
    } else {
        " ".to_string()
    };

    let closing = if let Some(last) = pairs.last_mut() {
        let ctx = ensure_ctx(last);
        // Whatever followed the previous last value (a same-line comment, the
        // newline before `}`) is split: the comment stays with that value, the
        // closing whitespace moves after the new pair.
        let after = match ctx.wsc.3.take() {
            Some(after_comma) => after_comma,
            None => std::mem::take(&mut ctx.wsc.2),
        };
        let (head, closing) = if multiline {
            let (head, tail) = split_last_line(&after);
            (head.to_string(), tail.to_string())
        } else {
            (String::new(), after)
        };
        ctx.wsc.3 = Some(format!("{head}{separator}"));
        closing
    } else {
        let ctx = context.get_or_insert_with(|| JSONObjectContext {
            wsc: (String::new(),),
        });
        if multiline {
            let (head, _) = split_last_line(&ctx.wsc.0);
            let head = if ctx.wsc.0.contains('\n') {
                head.to_string()
            } else {
                // `{}` or `{ /* c */ }`: keep any inline comment
                ctx.wsc.0.trim_end().to_string()
            };
            ctx.wsc.0 = format!("{head}{separator}");
            format!("{}{indent}", style.eol)
        } else {
            String::new()
        }
    };

    pairs.push(JSONKeyValuePair {
        key,
        value,
        context: Some(KeyValuePairContext {
            wsc: (String::new(), " ".to_string(), closing, None),
        }),
    });
}

fn remove_pair(
    pairs: &mut Vec<JSONKeyValuePair>,
    context: &mut Option<JSONObjectContext>,
    idx: usize,
) {
    let len = pairs.len();
    let mut removed = pairs.remove(idx);
    let removed_ctx = ensure_ctx(&mut removed).clone();

    if len == 1 {
        if let Some(ctx) = context.as_mut() {
            ctx.wsc.0.clear();
        }
        return;
    }

    if idx == len - 1 {
        // Removing the last pair: the previous pair becomes last and inherits
        // the closing whitespace before `}`.
        let prev = ensure_ctx(&mut pairs[idx - 1]);
        let prev_after = prev.wsc.3.take().unwrap_or_default();
        let (head, _) = split_first_line(&prev_after);
        // keep a same-line comment of the previous value, drop plain spaces
        let head = if head.trim().is_empty() {
            String::new()
        } else {
            head.to_string()
        };
        match removed_ctx.wsc.3 {
            Some(after) => {
                let (_, tail) = split_first_line(&after);
                let tail = if tail.is_empty() {
                    after.as_str()
                } else {
                    tail
                };
                prev.wsc.3 = Some(format!("{head}{tail}"));
            }
            None => {
                let closing = removed_ctx.wsc.2;
                let (_, tail) = split_first_line(&closing);
                let mut tail = if tail.is_empty() {
                    closing.clone()
                } else {
                    tail.to_string()
                };
                if head.contains("//") && !tail.starts_with('\n') && !tail.starts_with("\r\n") {
                    tail = format!("\n{tail}");
                }
                prev.wsc.2 = format!("{}{head}{tail}", prev.wsc.2);
            }
        }
        return;
    }

    // Removing a pair in the middle/front: the whitespace in front of it is
    // replaced by the whitespace that preceded the following pair, so the
    // removed key's comments go with it.
    let after = removed_ctx.wsc.3.unwrap_or_default();
    let (_, tail) = split_first_line(&after);
    if tail.is_empty() {
        // inline object (`{"a": 1, "b": 2, "c": 3}`): the whitespace in front
        // of the removed key already separates its neighbours
        return;
    }
    let preceding: &mut String = if idx == 0 {
        &mut context
            .get_or_insert_with(|| JSONObjectContext {
                wsc: (String::new(),),
            })
            .wsc
            .0
    } else {
        ensure_ctx(&mut pairs[idx - 1])
            .wsc
            .3
            .get_or_insert_with(String::new)
    };
    let (head, _) = split_first_line(preceding);
    let head = if head.trim().is_empty() { "" } else { head };
    *preceding = format!("{head}{tail}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// CI-15: the serde side of JSON5 parsing moved from the `json5` crate to
    /// `json_five::from_str`; pin the JSON5 syntax real configs use.
    #[test]
    fn json_five_serde_parses_json5_syntax_like_the_json5_crate() {
        let source = r#"// leading comment
        {
          /* block */ unquoted: 'single',
          "double": "a\"b",
          hex: 0x1F,
          neg: -3,
          plus: +4,
          frac: .5,
          big: 18446744073709551615,
          list: [1, 2,],
          nested: { ok: true, none: null, },
        }"#;
        let value: Value = json_five::from_str(source).unwrap();
        assert_eq!(
            value,
            json!({
                "unquoted": "single",
                "double": "a\"b",
                "hex": 31,
                "neg": -3,
                "plus": 4,
                "frac": 0.5,
                "big": 18446744073709551615u64,
                "list": [1, 2],
                "nested": { "ok": true, "none": null },
            })
        );
        assert!(json_five::from_str::<Value>("{ invalid json5").is_err());
        assert_eq!(
            json_five::from_str::<String>("'it\\'s'").unwrap(),
            "it's".to_string()
        );
    }

    fn patch(source: &str, f: impl FnOnce(&mut Value)) -> String {
        let mut doc = Json5Doc::parse(source).unwrap();
        let mut value = doc.to_value().unwrap();
        f(&mut value);
        doc.apply(&value).unwrap();
        let out = doc.render();
        // always re-parses to the requested value
        let reparsed: Value = json_five::from_str(&out).unwrap();
        assert_eq!(reparsed, value, "rendered:\n{out}");
        out
    }

    const COMMENTED: &str = r#"{
  // OpenCode config
  "$schema": "https://opencode.ai/config.json",
  "provider": {
    // my main provider
    "zeta": { "npm": "@ai-sdk/openai" }, // trailing note
    "alpha": {
      "options": { "baseURL": "https://a" }
    }
  },
  /* agents kept in this order on purpose */
  "agent": { "build": {}, "plan": {} }
}
"#;

    #[test]
    fn unchanged_value_is_byte_identical() {
        assert_eq!(patch(COMMENTED, |_| {}), COMMENTED);
    }

    #[test]
    fn updating_one_provider_keeps_comments_and_order() {
        let out = patch(COMMENTED, |v| {
            v["provider"]["alpha"]["options"]["baseURL"] = json!("https://b");
        });
        assert_eq!(out, COMMENTED.replace("https://a", "https://b"));
    }

    #[test]
    fn adding_a_key_appends_with_document_indent_and_quotes() {
        let out = patch(COMMENTED, |v| {
            v["provider"]["new"] = json!({"npm": "x"});
        });
        assert!(out.contains("// my main provider"));
        assert!(out.contains("// trailing note"));
        assert!(out.contains("/* agents kept in this order on purpose */"));
        assert!(
            out.contains("    },\n    \"new\": {\n      \"npm\": \"x\"\n    }\n  },"),
            "{out}"
        );
        // order of the untouched keys is preserved
        let zeta = out.find("\"zeta\"").unwrap();
        let alpha = out.find("\"alpha\"").unwrap();
        assert!(zeta < alpha);
    }

    #[test]
    fn removing_keys_keeps_neighbour_comments() {
        let out = patch(COMMENTED, |v| {
            v["provider"].as_object_mut().unwrap().shift_remove("zeta");
        });
        assert!(!out.contains("zeta"));
        assert!(!out.contains("trailing note"));
        assert!(out.contains("\"alpha\""));
        assert!(out.contains("/* agents kept"));

        let out = patch(COMMENTED, |v| {
            v["provider"].as_object_mut().unwrap().shift_remove("alpha");
        });
        assert!(out.contains("\"zeta\": { \"npm\": \"@ai-sdk/openai\" }"));
        assert!(!out.contains("alpha"));

        let out = patch(COMMENTED, |v| {
            v.as_object_mut().unwrap().shift_remove("agent");
        });
        assert!(!out.contains("agent"));
        assert!(out.contains("// my main provider"));
    }

    #[test]
    fn crlf_and_four_space_indent_are_kept() {
        let src = "{\r\n    \"a\": 1,\r\n    \"b\": {}\r\n}\r\n";
        let out = patch(src, |v| {
            v["b"]["c"] = json!([1, 2]);
            v["d"] = json!("x");
        });
        assert!(!out.replace("\r\n", "").contains('\n'), "{out:?}");
        assert!(out.contains("\r\n        \"c\": [\r\n"), "{out:?}");
        assert!(out.contains("\r\n    \"d\": \"x\"\r\n}"), "{out:?}");
    }

    #[test]
    fn json5_documents_keep_identifier_keys() {
        let src = "{\n  models: {\n    mode: 'merge',\n    providers: {},\n  },\n}\n";
        let out = patch(src, |v| {
            v["models"]["providers"]["p1"] = json!({"baseUrl": "u"});
        });
        assert!(out.contains("mode: 'merge'"));
        assert!(out.contains("p1: {"), "{out}");
    }

    #[test]
    fn empty_and_inline_objects() {
        assert_eq!(patch("{}", |v| v["a"] = json!(1)), "{\n  \"a\": 1\n}");
        assert_eq!(
            patch("{\"x\": {\"a\": 1}}", |v| v["x"]["b"] = json!(2)),
            "{\"x\": {\"a\": 1, \"b\": 2}}"
        );
        assert_eq!(
            patch("{\"a\": 1, \"b\": 2, \"c\": 3}", |v| {
                v.as_object_mut().unwrap().shift_remove("b");
            }),
            "{\"a\": 1, \"c\": 3}"
        );
        assert_eq!(
            patch("{\n  \"only\": 1\n}\n", |v| {
                v.as_object_mut().unwrap().shift_remove("only");
            }),
            "{}\n"
        );
    }

    #[test]
    fn apply_at_creates_missing_parents() {
        let mut doc = Json5Doc::parse("{\n  // c\n  \"x\": 1\n}\n").unwrap();
        doc.apply_at(&["mcp", "srv"], &json!({"type": "local"}))
            .unwrap();
        let out = doc.render();
        assert!(out.contains("// c"));
        let v: Value = json_five::from_str(&out).unwrap();
        assert_eq!(v["mcp"]["srv"]["type"], json!("local"));
    }

    #[test]
    fn invalid_source_is_an_error() {
        assert!(Json5Doc::parse("{ \"a\": ").is_err());
    }
}
