//! Shared live-file layer used by every per-app adapter (CFG-09).
//!
//! The helpers here give all adapters the same safety net instead of five
//! ad-hoc writers:
//!
//! * [`read_text_lenient`] tolerates a UTF-8/UTF-16 byte-order mark and
//!   records the file's [`TextStyle`] (BOM, line ending) so the writer can put
//!   it back (CFG-14);
//! * [`write_live_text`] skips writes that would not change the file, takes a
//!   backup of the previous content before replacing it and writes atomically
//!   (CFG-03/CFG-04);
//! * [`JsonStyle`] / [`to_json_string`] keep a JSON file's key order,
//!   indentation and trailing newline instead of alphabetising it (CFG-28);
//! * [`update_json_file`] is a read-modify-write with an optimistic
//!   concurrency check, so a change another program made to the file between
//!   our read and our write is never silently overwritten (CFG-39).

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::error::AppError;

const UTF8_BOM: &str = "\u{feff}";

/// Byte-level style of a text file that must survive a rewrite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextStyle {
    /// The file started with a UTF-8 byte-order mark.
    pub bom: bool,
    /// The file uses `\r\n` line endings.
    pub crlf: bool,
}

impl TextStyle {
    /// Detect the style of `raw` (BOM and the dominant line ending).
    pub fn detect(raw: &str) -> Self {
        let bom = raw.starts_with(UTF8_BOM);
        let crlf_count = raw.matches("\r\n").count();
        let lf_count = raw.matches('\n').count();
        Self {
            bom,
            // A file counts as CRLF when the majority of its newlines are CRLF.
            crlf: crlf_count > 0 && crlf_count * 2 >= lf_count,
        }
    }

    pub fn eol(&self) -> &'static str {
        if self.crlf {
            "\r\n"
        } else {
            "\n"
        }
    }

    /// Re-apply this style to text produced with `\n` line endings.
    ///
    /// Existing `\r\n` pairs are kept as-is, so already-styled text is not
    /// doubled up.
    pub fn apply(&self, text: &str) -> String {
        let body = strip_bom(text);
        let mut out = String::with_capacity(body.len() + 8);
        if self.bom {
            out.push_str(UTF8_BOM);
        }
        if self.crlf {
            out.push_str(&normalize_eol(body, "\r\n"));
        } else {
            out.push_str(&normalize_eol(body, "\n"));
        }
        out
    }
}

/// Convert every line ending in `text` to `eol`.
pub fn normalize_eol(text: &str, eol: &str) -> String {
    let lf = text.replace("\r\n", "\n");
    if eol == "\n" {
        lf
    } else {
        lf.replace('\n', eol)
    }
}

/// Strip a leading UTF-8 byte-order mark.
pub fn strip_bom(text: &str) -> &str {
    text.strip_prefix(UTF8_BOM).unwrap_or(text)
}

/// The decoded content of a live file plus the style needed to write it back.
#[derive(Debug, Clone)]
pub struct LiveText {
    /// Content without the BOM; line endings are left untouched.
    pub text: String,
    pub style: TextStyle,
}

/// Decode raw bytes: UTF-8 (with or without BOM), UTF-16 with BOM, and as a
/// last resort lossy UTF-8 so a stray byte never turns into a hard failure.
pub fn decode_text(bytes: &[u8], path: &Path) -> LiveText {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        let text = String::from_utf8_lossy(rest).into_owned();
        let mut style = TextStyle::detect(&text);
        style.bom = true;
        return LiveText { text, style };
    }
    let utf16 = |be: bool, rest: &[u8]| -> String {
        let units: Vec<u16> = rest
            .chunks_exact(2)
            .map(|c| {
                if be {
                    u16::from_be_bytes([c[0], c[1]])
                } else {
                    u16::from_le_bytes([c[0], c[1]])
                }
            })
            .collect();
        String::from_utf16_lossy(&units)
    };
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let text = utf16(false, rest);
        log::warn!(
            "{} is UTF-16 encoded; it will be rewritten as UTF-8",
            path.display()
        );
        let style = TextStyle::detect(&text);
        return LiveText { text, style };
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let text = utf16(true, rest);
        log::warn!(
            "{} is UTF-16 encoded; it will be rewritten as UTF-8",
            path.display()
        );
        let style = TextStyle::detect(&text);
        return LiveText { text, style };
    }
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => {
            log::warn!(
                "{} is not valid UTF-8; invalid bytes were replaced while reading",
                path.display()
            );
            String::from_utf8_lossy(bytes).into_owned()
        }
    };
    let style = TextStyle::detect(&text);
    LiveText { text, style }
}

/// Read a live file leniently. Returns `Ok(None)` when the file does not exist.
pub fn read_text_lenient(path: &Path) -> Result<Option<LiveText>, AppError> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(decode_text(&bytes, path))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(AppError::io(path, e)),
    }
}

/// How a live file must be written.
#[derive(Debug, Clone, Copy, Default)]
pub struct WriteOptions {
    /// Back up the previous content into `~/.cc-switch/backups/<app>/`.
    pub backup_app: Option<&'static str>,
    /// The file carries credentials: create it owner-only (0600).
    pub secret: bool,
}

impl WriteOptions {
    pub fn backup(app: &'static str) -> Self {
        Self {
            backup_app: Some(app),
            secret: false,
        }
    }

    pub fn secret(mut self) -> Self {
        self.secret = true;
        self
    }
}

/// Result of a live write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteOutcome {
    /// The file already had exactly this content; nothing was written.
    Unchanged,
    /// The file was (re)written; `backup` is the backup of the old content.
    Written { backup: Option<PathBuf> },
}

impl WriteOutcome {
    pub fn written(&self) -> bool {
        matches!(self, WriteOutcome::Written { .. })
    }
}

/// Refuse to write a secrets-vault placeholder (`cc-switch-secret:v1:…`) into
/// a live file. Placeholders only belong in cc-switch's database; one that
/// reaches a tool's config means a provider was not resolved before the
/// write, and the tool would send the placeholder as its API key.
pub fn ensure_no_vault_placeholder(path: &Path, content: &str) -> Result<(), AppError> {
    if content.contains(crate::secrets::provider_vault::PLACEHOLDER_PREFIX) {
        log::error!(
            "Refusing to write an unresolved secrets-vault placeholder into {}",
            path.display()
        );
        return Err(AppError::localized(
            "secrets.placeholderInLiveConfig",
            format!(
                "未写入 {}：配置中仍含有未解析的密钥库占位符，请解锁密钥库后重试。",
                path.display()
            ),
            format!(
                "{} was not written: the config still contains an unresolved secrets-vault placeholder. Unlock the vault and retry.",
                path.display()
            ),
        ));
    }
    Ok(())
}

/// Write `content` to a live file: no-op when unchanged, backup first, atomic
/// replace. Content carrying a vault placeholder is rejected.
pub fn write_live_text(
    path: &Path,
    content: &str,
    opts: WriteOptions,
) -> Result<WriteOutcome, AppError> {
    ensure_no_vault_placeholder(path, content)?;
    if let Ok(existing) = fs::read(path) {
        if existing == content.as_bytes() {
            return Ok(WriteOutcome::Unchanged);
        }
    }
    let backup = opts
        .backup_app
        .and_then(|app| crate::config::backup_live_file(app, path));
    if opts.secret {
        crate::config::atomic_write_secret(path, content.as_bytes())?;
    } else {
        crate::config::atomic_write(path, content.as_bytes())?;
    }
    Ok(WriteOutcome::Written { backup })
}

/// Formatting of a JSON document: indentation, trailing newline, BOM, EOL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonStyle {
    pub indent: String,
    pub trailing_newline: bool,
    pub text: TextStyle,
}

impl Default for JsonStyle {
    fn default() -> Self {
        Self {
            indent: "  ".to_string(),
            trailing_newline: true,
            text: TextStyle::default(),
        }
    }
}

impl JsonStyle {
    /// Sniff the style of an existing JSON document.
    pub fn detect(raw: &str) -> Self {
        let text = TextStyle::detect(raw);
        let body = strip_bom(raw);
        let indent = body
            .lines()
            .skip(1)
            .find_map(|line| {
                let trimmed = line.trim_start_matches([' ', '\t']);
                if trimmed.is_empty() || trimmed.len() == line.len() {
                    None
                } else {
                    Some(line[..line.len() - trimmed.len()].to_string())
                }
            })
            .unwrap_or_else(|| "  ".to_string());
        Self {
            indent,
            trailing_newline: body.ends_with('\n'),
            text,
        }
    }

    /// Style of the JSON file at `path`, or the default for a new file.
    pub fn of_file(path: &Path) -> Self {
        match read_text_lenient(path) {
            Ok(Some(live)) if !live.text.trim().is_empty() => {
                let mut style = Self::detect(&live.text);
                style.text.bom = live.style.bom;
                style
            }
            _ => Self::default(),
        }
    }
}

/// Serialise `data` as JSON in `style`, keeping the value's key order.
pub fn to_json_string<T: Serialize + ?Sized>(
    data: &T,
    style: &JsonStyle,
) -> Result<String, AppError> {
    let mut buf = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(style.indent.as_bytes());
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
    data.serialize(&mut ser)
        .map_err(|e| AppError::JsonSerialize { source: e })?;
    let mut text = String::from_utf8(buf).map_err(|e| AppError::Message(e.to_string()))?;
    if style.trailing_newline {
        text.push('\n');
    }
    Ok(style.text.apply(&text))
}

/// Write `value` to a JSON or JSONC live file, keeping the existing
/// document's comments, key order and formatting: only the parts that differ
/// are re-rendered (via [`super::json5_doc::Json5Doc`]). Falls back to a
/// style-preserving plain rewrite (previous content backed up per `opts`)
/// when the document cannot be edited in place.
pub fn write_json_preserving(
    path: &Path,
    value: &Value,
    opts: WriteOptions,
) -> Result<WriteOutcome, AppError> {
    let content = match read_text_lenient(path)? {
        Some(live) if !live.text.trim().is_empty() => {
            match super::json5_doc::Json5Doc::parse(&live.text) {
                Ok(mut doc) => {
                    doc.apply(value)?;
                    live.style.apply(&doc.render())
                }
                Err(err) => {
                    log::warn!(
                        "{} could not be edited in place ({err}); rewriting it",
                        path.display()
                    );
                    let mut style = JsonStyle::detect(&live.text);
                    style.text.bom = live.style.bom;
                    to_json_string(value, &style)?
                }
            }
        }
        _ => to_json_string(value, &JsonStyle::default())?,
    };
    write_live_text(path, &content, opts)
}

/// Parse JSON text, tolerating a BOM.
pub fn parse_json(text: &str, path: &Path) -> Result<Value, AppError> {
    serde_json::from_str(strip_bom(text)).map_err(|e| AppError::json(path, e))
}

/// How many times [`update_json_file`] re-applies its change when the file
/// keeps changing underneath it.
const MAX_CONCURRENT_RETRIES: usize = 3;

/// Read-modify-write a JSON live file.
///
/// * a missing file starts from `{}`; an unparsable file is an error and is
///   never overwritten;
/// * `mutate` may be called more than once: when the file changes between the
///   read and the write (another program owns it, e.g. Claude Code and
///   `~/.claude.json`), the change is re-applied to the fresh content instead
///   of overwriting the external edit;
/// * nothing is written when the result is identical to the file.
pub fn update_json_file<F>(
    path: &Path,
    opts: WriteOptions,
    mut mutate: F,
) -> Result<WriteOutcome, AppError>
where
    F: FnMut(&mut Value) -> Result<(), AppError>,
{
    for _ in 0..MAX_CONCURRENT_RETRIES {
        let original = fs::read(path).ok();
        let (mut value, style) = match &original {
            Some(bytes) => {
                let live = decode_text(bytes, path);
                if live.text.trim().is_empty() {
                    (Value::Object(Default::default()), JsonStyle::default())
                } else {
                    let mut style = JsonStyle::detect(&live.text);
                    style.text.bom = live.style.bom;
                    (parse_json(&live.text, path)?, style)
                }
            }
            None => (Value::Object(Default::default()), JsonStyle::default()),
        };
        let before = value.clone();
        mutate(&mut value)?;
        if value == before {
            return Ok(WriteOutcome::Unchanged);
        }
        let content = to_json_string(&value, &style)?;

        // Optimistic concurrency: only write when nobody changed the file
        // since we read it; otherwise start over from the new content.
        let current = fs::read(path).ok();
        if current != original {
            log::warn!(
                "{} changed while cc-switch was updating it; re-applying the change",
                path.display()
            );
            continue;
        }
        return write_live_text(path, &content, opts);
    }
    Err(AppError::Config(format!(
        "{} keeps changing on disk; cc-switch did not overwrite it. Please retry.",
        path.display()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn detects_bom_and_crlf() {
        let style = TextStyle::detect("\u{feff}{\r\n  \"a\": 1\r\n}\r\n");
        assert!(style.bom);
        assert!(style.crlf);
        assert_eq!(style.apply("{\n}\n"), "\u{feff}{\r\n}\r\n");
        assert!(!TextStyle::detect("a\nb\n").crlf);
    }

    #[test]
    fn decode_strips_utf8_bom_and_reads_utf16() {
        let path = Path::new("x.json");
        let live = decode_text(b"\xEF\xBB\xBF{}", path);
        assert_eq!(live.text, "{}");
        assert!(live.style.bom);

        let mut utf16 = vec![0xFF, 0xFE];
        for unit in "{\"a\":1}".encode_utf16() {
            utf16.extend_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(decode_text(&utf16, path).text, "{\"a\":1}");

        // invalid UTF-8 is decoded lossily instead of failing
        assert!(decode_text(b"{\"a\":\"\xFF\"}", path)
            .text
            .contains('\u{fffd}'));
    }

    #[test]
    fn json_style_round_trips_order_indent_and_newline() {
        let raw = "{\n    \"zeta\": 1,\n    \"alpha\": {\n        \"b\": 2,\n        \"a\": 1\n    }\n}\n";
        let value: Value = serde_json::from_str(raw).unwrap();
        let style = JsonStyle::detect(raw);
        assert_eq!(style.indent, "    ");
        assert!(style.trailing_newline);
        assert_eq!(to_json_string(&value, &style).unwrap(), raw);

        let crlf = raw.replace('\n', "\r\n");
        let style = JsonStyle::detect(&crlf);
        assert_eq!(to_json_string(&value, &style).unwrap(), crlf);

        let no_newline = "{\n\t\"b\": 1,\n\t\"a\": 2\n}";
        let value: Value = serde_json::from_str(no_newline).unwrap();
        assert_eq!(
            to_json_string(&value, &JsonStyle::detect(no_newline)).unwrap(),
            no_newline
        );
    }

    #[test]
    fn write_json_preserving_keeps_jsonc_comments() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let raw = "{\n  // theme picked by hand\n  \"theme\": \"dark\",\n  \"security\": {\n    \"auth\": { \"selectedType\": \"oauth-personal\" }\n  }\n}\n";
        fs::write(&path, raw).unwrap();
        let mut value: Value = json_five::from_str(raw).unwrap();
        value["security"]["auth"]["selectedType"] = json!("gemini-api-key");
        write_json_preserving(&path, &value, WriteOptions::default()).unwrap();
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.contains("// theme picked by hand"), "{written}");
        assert_eq!(written, raw.replace("oauth-personal", "gemini-api-key"));
    }

    #[test]
    fn write_live_text_refuses_vault_placeholders() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, "{}").unwrap();
        let content = format!(
            "{{\"env\":{{\"ANTHROPIC_AUTH_TOKEN\":\"{}abc\"}}}}",
            crate::secrets::provider_vault::PLACEHOLDER_PREFIX
        );
        assert!(write_live_text(&path, &content, WriteOptions::default()).is_err());
        assert!(crate::config::write_text_file_secret(&path, &content).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}");
    }

    #[test]
    fn write_live_text_skips_identical_content_and_backs_up() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.json");
        fs::write(&path, "old").unwrap();
        let outcome = write_live_text(&path, "old", WriteOptions::default()).unwrap();
        assert_eq!(outcome, WriteOutcome::Unchanged);
        let outcome = write_live_text(&path, "new", WriteOptions::default()).unwrap();
        assert!(outcome.written());
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
    }

    #[test]
    fn update_json_file_preserves_style_and_refuses_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, "\u{feff}{\r\n    \"z\": 1,\r\n    \"a\": 2\r\n}").unwrap();
        update_json_file(&path, WriteOptions::default(), |v| {
            v["m"] = json!(true);
            Ok(())
        })
        .unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "\u{feff}{\r\n    \"z\": 1,\r\n    \"a\": 2,\r\n    \"m\": true\r\n}"
        );

        // unchanged result -> no write
        let outcome = update_json_file(&path, WriteOptions::default(), |_| Ok(())).unwrap();
        assert_eq!(outcome, WriteOutcome::Unchanged);

        fs::write(&path, "{ not json").unwrap();
        assert!(update_json_file(&path, WriteOptions::default(), |_| Ok(())).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ not json");
    }

    #[test]
    fn update_json_file_reapplies_change_on_concurrent_edit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("claude.json");
        fs::write(&path, "{\"a\": 1}").unwrap();
        let mut calls = 0;
        update_json_file(&path, WriteOptions::default(), |v| {
            calls += 1;
            if calls == 1 {
                // simulate the owning tool writing the file between our read and write
                fs::write(&path, "{\"a\": 1, \"external\": true}").unwrap();
            }
            v["mine"] = json!(1);
            Ok(())
        })
        .unwrap();
        assert_eq!(calls, 2);
        let value: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(value["external"], json!(true));
        assert_eq!(value["mine"], json!(1));
    }
}
