//! Hermes Agent 配置文件读写模块
//!
//! 处理 `~/.hermes/config.yaml` 配置文件的读写操作（YAML 格式）。
//! Hermes 使用累加式供应商管理，所有供应商配置共存于同一配置文件中。
//!
//! ## 配置结构示例
//!
//! ```yaml
//! model:
//!   default: "anthropic/claude-opus-4-8"
//!   provider: "openrouter"
//!   base_url: "https://openrouter.ai/api/v1"
//!
//! agent:
//!   max_turns: 50
//!   reasoning_effort: "high"
//!
//! custom_providers:
//!   - name: openrouter
//!     base_url: https://openrouter.ai/api/v1
//!     api_key: sk-or-...
//!     model: anthropic/claude-opus-4-8
//!     models:
//!       anthropic/claude-opus-4-8:
//!         context_length: 200000
//!
//! mcp_servers:
//!   filesystem:
//!     command: npx
//!     args: ["-y", "@modelcontextprotocol/server-filesystem"]
//! ```

use crate::config::{atomic_write, atomic_write_secret, get_app_config_dir};
use crate::error::AppError;
use crate::settings::{effective_backup_retain_count, get_hermes_override_dir};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

// ============================================================================
// Path Functions
// ============================================================================

/// 获取 Hermes 配置目录
///
/// 解析顺序对齐 Hermes 自身的 `get_hermes_home()`:
///   1. CCS 设置 `hermes_config_dir`(显式覆盖)
///   2. `HERMES_HOME` 环境变量(trim 后非空;按原样,不展开 `~`,与 Hermes `Path(val)` 一致)
///   3. 平台默认(Windows: `%LOCALAPPDATA%\hermes`,Mac/Linux: `~/.hermes`)
pub fn get_hermes_dir() -> PathBuf {
    if let Some(override_dir) = get_hermes_override_dir() {
        return override_dir;
    }

    // TEST-09: thread-isolated tests must not see an ambient HERMES_HOME.
    if let Some(raw) = std::env::var_os("HERMES_HOME").filter(|_| !crate::config::thread_isolated())
    {
        let value = raw.to_string_lossy();
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    default_hermes_dir()
}

/// 平台默认 Hermes 目录(Windows):对齐 Hermes `_get_platform_default_hermes_home()`——
/// 读 `LOCALAPPDATA` 环境变量,缺失/空时回退 `~\AppData\Local`,再拼 `hermes`。
#[cfg(target_os = "windows")]
fn default_hermes_dir() -> PathBuf {
    let localappdata =
        std::env::var_os("LOCALAPPDATA").filter(|_| !crate::config::thread_isolated());
    windows_local_hermes_dir(localappdata.as_deref(), &crate::config::get_home_dir())
}

/// 平台默认 Hermes 目录(Mac/Linux):`~/.hermes`。
#[cfg(not(target_os = "windows"))]
fn default_hermes_dir() -> PathBuf {
    crate::config::get_home_dir().join(".hermes")
}

/// Windows `%LOCALAPPDATA%\hermes` 路径计算(纯函数,便于跨平台单测)。
/// 对齐 Hermes 的 `os.environ.get("LOCALAPPDATA", "").strip()`:trim 后为空
/// (缺失/空/纯空白)则回退 `<home>\AppData\Local\hermes`。
#[cfg(any(target_os = "windows", test))]
fn windows_local_hermes_dir(localappdata: Option<&std::ffi::OsStr>, home: &Path) -> PathBuf {
    localappdata
        .map(|value| value.to_string_lossy().trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("AppData").join("Local"))
        .join("hermes")
}

/// 获取 Hermes 配置文件路径
///
/// 返回 `~/.hermes/config.yaml`
pub fn get_hermes_config_path() -> PathBuf {
    get_hermes_dir().join("config.yaml")
}

fn hermes_write_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

// ============================================================================
// Type Definitions
// ============================================================================

/// Hermes 写入结果
#[derive(Debug, Clone, Serialize, Deserialize, Default, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HermesWriteOutcome {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_path: Option<String>,
}

/// Hermes model section config
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HermesModelConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_length: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    /// Preserve unknown fields for forward compatibility
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

// ============================================================================
// Core YAML Read Functions
// ============================================================================

/// 读取 Hermes 配置文件为 serde_yaml::Value
///
/// 如果文件不存在，返回空 Mapping
pub fn read_hermes_config() -> Result<serde_yaml::Value, AppError> {
    let path = get_hermes_config_path();
    if !path.exists() {
        return Ok(serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
    }

    let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    if content.trim().is_empty() {
        return Ok(serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
    }

    // Heal duplicate top-level keys left behind by the pre-CRLF-fix append
    // bug (#3633); serde_yaml rejects them outright, which bricked the panel.
    let deduped = deduplicate_top_level_keys(&content);

    serde_yaml::from_str(&deduped)
        .map_err(|e| AppError::Config(format!("Failed to parse Hermes config as YAML: {e}")))
}

/// Remove duplicate top-level YAML sections, keeping the LAST occurrence of
/// each key.
///
/// Keep-last is deliberate, not arbitrary: the duplicates come from section
/// replacement degrading into appends (#3633), so the last block is the
/// newest data — and Hermes itself reads the file with PyYAML, whose
/// duplicate-key semantics are last-wins. Keeping the first occurrence would
/// silently roll the user back to stale config and diverge from what Hermes
/// actually runs with.
fn deduplicate_top_level_keys(raw: &str) -> String {
    use std::collections::HashMap;

    // Pass 1: locate every top-level key line as (key, byte offset).
    let mut sections: Vec<(&str, usize)> = Vec::new();
    let mut offset = 0;
    for line in raw.split('\n') {
        if is_top_level_key_line(line) {
            if let Some(colon_pos) = line.find(':') {
                sections.push((&line[..colon_pos], offset));
            }
        }
        offset += line.len() + 1;
    }

    let mut remaining: HashMap<&str, usize> = HashMap::new();
    for (key, _) in &sections {
        *remaining.entry(key).or_insert(0) += 1;
    }
    if remaining.values().all(|&count| count <= 1) {
        return raw.to_string();
    }

    // Pass 2: re-emit, dropping every section that has a later occurrence of
    // the same key. A section spans from its key line to the next top-level
    // key line (or EOF), matching find_yaml_section_range. Content before the
    // first section (comments, document markers) is always kept.
    let mut result = String::with_capacity(raw.len());
    let head_end = sections
        .first()
        .map(|&(_, start)| start)
        .unwrap_or(raw.len());
    result.push_str(&raw[..head_end]);

    for (i, &(key, start)) in sections.iter().enumerate() {
        let end = sections
            .get(i + 1)
            .map(|&(_, next_start)| next_start)
            .unwrap_or(raw.len());
        let count = remaining.get_mut(key).expect("key collected in pass 1");
        *count -= 1;
        if *count > 0 {
            log::warn!(
                "Hermes config: dropped duplicate top-level section '{key}' (keeping the last occurrence)"
            );
            continue;
        }
        result.push_str(&raw[start..end]);
    }

    result
}

// ============================================================================
// YAML Section-Level Replacement
// ============================================================================

/// Check if a line is a YAML top-level key (mapping key at column 0).
///
/// A top-level key line must:
/// - Start at column 0 (no leading whitespace)
/// - Not be empty or whitespace-only
/// - Not be a comment (starting with `#`)
/// - Not be a sequence item (starting with `-`)
/// - Contain `:` followed by space, tab, newline, or end-of-line
///
/// Lines may carry a trailing `\r` (CRLF files split on `\n`) or `\n`
/// (callers using `split_inclusive`); both count as end-of-line after the
/// colon. Rejecting `\r` here used to make every section lookup miss on
/// CRLF configs, turning section replacement into endless appends (#3633).
fn is_top_level_key_line(line: &str) -> bool {
    if line.is_empty() {
        return false;
    }
    let first_char = line.as_bytes()[0];
    if first_char == b' ' || first_char == b'\t' || first_char == b'#' || first_char == b'-' {
        return false;
    }
    if let Some(colon_pos) = line.find(':') {
        let after_colon = &line[colon_pos + 1..];
        after_colon.is_empty() || after_colon.starts_with([' ', '\t', '\r', '\n'])
    } else {
        false
    }
}

/// Find the byte range of a top-level YAML section.
///
/// A YAML top-level key is a line that starts at column 0 (no leading
/// whitespace), is not a comment, and contains `:` after the key name.
///
/// Returns `(start_byte_inclusive, end_byte_exclusive)` or `None` if not found.
fn find_yaml_section_range(raw: &str, section_key: &str) -> Option<(usize, usize)> {
    let target = format!("{}:", section_key);
    let mut section_start = None;
    let mut offset = 0;

    for line in raw.split('\n') {
        if section_start.is_none() && is_top_level_key_line(line) && line.starts_with(&target) {
            // Verify exact match: after "key:" must be whitespace or EOL
            let after_target = &line[target.len()..];
            if after_target.is_empty()
                || after_target.starts_with(' ')
                || after_target.starts_with('\t')
                || after_target.starts_with('\r')
            {
                section_start = Some(offset);
            }
        } else if section_start.is_some() && is_top_level_key_line(line) {
            // Found the next top-level key — this is the end of our section
            return Some((section_start.unwrap(), offset));
        }
        offset += line.len() + 1; // +1 for the \n
    }

    // Section extends to end of file
    section_start.map(|start| (start, raw.len()))
}

/// Find the byte range of a top-level section's *content*.
///
/// Like [`find_yaml_section_range`], but blank lines and comments after the
/// section's last content line are left out: they belong to whatever follows
/// (typically a comment describing the next section), so replacing this
/// section must not delete them (CFG-08).
fn find_yaml_section_content_range(raw: &str, section_key: &str) -> Option<(usize, usize)> {
    let (start, end) = find_yaml_section_range(raw, section_key)?;
    let mut content_end = start;
    let mut offset = start;
    for (i, line) in raw[start..end].split_inclusive('\n').enumerate() {
        offset += line.len();
        let trimmed = line.trim();
        if i == 0 || !(trimmed.is_empty() || trimmed.starts_with('#')) {
            content_end = offset;
        }
    }
    Some((start, content_end))
}

/// Serialize a section key + value into a YAML fragment like:
///
/// ```yaml
/// model:
///   default: "anthropic/claude-opus-4-8"
///   provider: "openrouter"
/// ```
fn serialize_yaml_section(key: &str, value: &serde_yaml::Value) -> Result<String, AppError> {
    let mut section = serde_yaml::Mapping::new();
    section.insert(serde_yaml::Value::String(key.to_string()), value.clone());
    let yaml_str = serde_yaml::to_string(&serde_yaml::Value::Mapping(section))
        .map_err(|e| AppError::Config(format!("Failed to serialize YAML section '{key}': {e}")))?;
    Ok(yaml_str)
}

/// Remove every top-level section with the given key from raw YAML text.
/// Used to clean residual duplicates of a key after replacing its first
/// occurrence; safe values come from the keep-last healed read, so dropping
/// all on-disk copies here loses nothing.
fn remove_all_sections(raw: &str, section_key: &str) -> String {
    let mut result = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some((start, end)) = find_yaml_section_range(rest, section_key) {
        result.push_str(&rest[..start]);
        rest = &rest[end..];
    }
    result.push_str(rest);
    result
}

/// Replace a YAML section in raw text, or append it if not found.
///
/// Format preservation (CFG-08):
/// - comments/blank lines after the section (usually describing the next
///   section) are kept, see [`find_yaml_section_content_range`];
/// - for mapping and sequence sections only the child entries that changed
///   are re-serialised; untouched entries keep their comments, quoting and
///   anchors byte-for-byte (see [`splice_yaml_section`]);
/// - newly generated text uses the file's line ending (CRLF files stay CRLF).
fn replace_yaml_section(
    raw: &str,
    section_key: &str,
    value: &serde_yaml::Value,
) -> Result<String, AppError> {
    let eol = crate::adapters::live_file::TextStyle::detect(raw).eol();

    if let Some((start, end)) = find_yaml_section_content_range(raw, section_key) {
        let rendered = splice_yaml_section(&raw[start..end], section_key, value, eol)?;
        let mut result = String::with_capacity(raw.len() + rendered.len());
        result.push_str(&raw[..start]);
        result.push_str(&rendered);
        // Drop duplicate sections of this key from the remainder — configs
        // written before the CRLF fix may carry several appended copies.
        let remainder = remove_all_sections(&raw[end..], section_key);
        // Ensure proper separation between sections
        if !rendered.ends_with('\n')
            && !remainder.is_empty()
            && !remainder.starts_with(['\n', '\r'])
        {
            result.push_str(eol);
        }
        result.push_str(&remainder);
        Ok(result)
    } else {
        // Section not found — append at end
        let serialized = crate::adapters::live_file::normalize_eol(
            &serialize_yaml_section(section_key, value)?,
            eol,
        );
        let mut result = raw.to_string();
        if !result.is_empty() && !result.ends_with('\n') {
            result.push_str(eol);
        }
        result.push_str(&serialized);
        if !result.ends_with('\n') {
            result.push_str(eol);
        }
        Ok(result)
    }
}

/// Render `section_key: value` reusing as much of `raw_section` as possible.
///
/// `raw_section` runs from the section's key line to its last content line.
/// When the section is a block mapping or block sequence whose entries can be
/// parsed one by one, entries that did not change are copied verbatim
/// (including the comments directly above them) and only changed/new entries
/// are serialised. Anything more exotic falls back to re-serialising the
/// whole section.
fn splice_yaml_section(
    raw_section: &str,
    section_key: &str,
    value: &serde_yaml::Value,
    eol: &str,
) -> Result<String, AppError> {
    use crate::adapters::live_file::normalize_eol;

    let old = serde_yaml::from_str::<serde_yaml::Mapping>(raw_section)
        .ok()
        .and_then(|m| {
            m.get(serde_yaml::Value::String(section_key.to_string()))
                .cloned()
        });
    if old.as_ref() == Some(value) {
        return Ok(raw_section.to_string());
    }
    if let Some(old) = old {
        if let Some(spliced) = splice_yaml_children(raw_section, section_key, &old, value, eol) {
            return Ok(spliced);
        }
    }
    Ok(normalize_eol(
        &serialize_yaml_section(section_key, value)?,
        eol,
    ))
}

/// One child entry of a block mapping/sequence section.
struct YamlChildBlock<'a> {
    /// Comment/blank lines directly above the entry.
    comments: Vec<&'a str>,
    /// The entry's own lines (anchor line + continuation lines).
    body: Vec<&'a str>,
    value: serde_yaml::Value,
}

fn yaml_is_comment_or_blank(line: &str) -> bool {
    let t = line.trim();
    t.is_empty() || t.starts_with('#')
}

fn yaml_indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches([' ', '\t']).len()
}

fn splice_yaml_children(
    raw_section: &str,
    section_key: &str,
    old: &serde_yaml::Value,
    new: &serde_yaml::Value,
    eol: &str,
) -> Option<String> {
    use crate::adapters::live_file::normalize_eol;

    let is_sequence = match (old, new) {
        (serde_yaml::Value::Mapping(o), serde_yaml::Value::Mapping(n))
            if !o.is_empty() && !n.is_empty() =>
        {
            false
        }
        (serde_yaml::Value::Sequence(o), serde_yaml::Value::Sequence(n))
            if !o.is_empty() && !n.is_empty() =>
        {
            true
        }
        _ => return None,
    };

    let lines: Vec<&str> = raw_section.split_inclusive('\n').collect();
    let (header, body) = lines.split_first()?;
    // Only block style headers (`key:` optionally followed by a comment).
    let after_key = header.strip_prefix(section_key)?.strip_prefix(':')?.trim();
    if !(after_key.is_empty() || after_key.starts_with('#')) {
        return None;
    }

    let first_content = body.iter().find(|l| !yaml_is_comment_or_blank(l))?;
    let child_indent = &first_content[..yaml_indent_of(first_content)];
    let is_anchor = |line: &str| {
        if yaml_is_comment_or_blank(line) || yaml_indent_of(line) != child_indent.len() {
            return false;
        }
        let content = line.trim_start_matches([' ', '\t']);
        content.starts_with('-') == is_sequence
    };

    // Split the body into blocks: leading comments + anchor + continuation.
    let mut blocks: Vec<(Vec<&str>, Vec<&str>)> = Vec::new();
    let mut pending: Vec<&str> = Vec::new();
    for line in body {
        if is_anchor(line) {
            blocks.push((std::mem::take(&mut pending), vec![*line]));
        } else if yaml_is_comment_or_blank(line) {
            pending.push(line);
        } else if let Some((_, block_body)) = blocks.last_mut() {
            // continuation line: comments seen in between belong to the entry
            block_body.append(&mut pending);
            block_body.push(line);
        } else {
            return None;
        }
    }
    // the content range never ends with comments, but stay safe
    if let Some((_, block_body)) = blocks.last_mut() {
        block_body.append(&mut pending);
    }

    let dedent = |lines: &[&str]| -> String {
        lines
            .iter()
            .map(|l| l.strip_prefix(child_indent).unwrap_or(l.trim_start()))
            .collect::<String>()
    };
    let mut parsed: Vec<YamlChildBlock> = Vec::with_capacity(blocks.len());
    for (comments, body) in blocks {
        let text = dedent(&body);
        let value = if is_sequence {
            let seq: serde_yaml::Sequence = serde_yaml::from_str(&text).ok()?;
            if seq.len() != 1 {
                return None;
            }
            seq.into_iter().next()?
        } else {
            let map: serde_yaml::Mapping = serde_yaml::from_str(&text).ok()?;
            if map.len() != 1 {
                return None;
            }
            serde_yaml::Value::Mapping(map)
        };
        parsed.push(YamlChildBlock {
            comments,
            body,
            value,
        });
    }

    // The blocks must describe exactly the old value, otherwise we do not
    // understand the layout well enough to splice it.
    let reconstructed = if is_sequence {
        serde_yaml::Value::Sequence(parsed.iter().map(|b| b.value.clone()).collect())
    } else {
        let mut map = serde_yaml::Mapping::new();
        for b in &parsed {
            if let serde_yaml::Value::Mapping(m) = &b.value {
                for (k, v) in m {
                    map.insert(k.clone(), v.clone());
                }
            }
        }
        serde_yaml::Value::Mapping(map)
    };
    if &reconstructed != old {
        return None;
    }

    let render = |entry: serde_yaml::Value| -> Option<String> {
        let text = serde_yaml::to_string(&entry).ok()?;
        let mut out = String::new();
        for line in text.split_inclusive('\n') {
            out.push_str(child_indent);
            out.push_str(line);
        }
        if !out.ends_with('\n') {
            out.push('\n');
        }
        Some(normalize_eol(&out, eol))
    };
    let push_lines = |out: &mut String, lines: &[&str]| {
        for line in lines {
            out.push_str(line);
        }
        if !lines.is_empty() && !out.ends_with('\n') {
            out.push_str(eol);
        }
    };

    let mut out = String::with_capacity(raw_section.len());
    out.push_str(header);
    if !out.ends_with('\n') {
        out.push_str(eol);
    }

    if is_sequence {
        let new_items = new.as_sequence()?;
        let mut used = vec![false; parsed.len()];
        for item in new_items {
            let exact = (0..parsed.len()).find(|&i| !used[i] && parsed[i].value == *item);
            if let Some(i) = exact {
                used[i] = true;
                push_lines(&mut out, &parsed[i].comments);
                push_lines(&mut out, &parsed[i].body);
                continue;
            }
            // Same `name` → edited entry: keep the comments above it.
            let same_name = item.get("name").and_then(|name| {
                (0..parsed.len()).find(|&i| !used[i] && parsed[i].value.get("name") == Some(name))
            });
            if let Some(i) = same_name {
                used[i] = true;
                push_lines(&mut out, &parsed[i].comments);
            }
            out.push_str(&render(serde_yaml::Value::Sequence(vec![item.clone()]))?);
        }
    } else {
        let new_map = new.as_mapping()?;
        let mut emitted = std::collections::HashSet::new();
        for block in &parsed {
            let serde_yaml::Value::Mapping(m) = &block.value else {
                return None;
            };
            let (key, old_value) = m.iter().next()?;
            match new_map.get(key) {
                Some(new_value) if new_value == old_value => {
                    push_lines(&mut out, &block.comments);
                    push_lines(&mut out, &block.body);
                }
                Some(new_value) => {
                    push_lines(&mut out, &block.comments);
                    let mut entry = serde_yaml::Mapping::new();
                    entry.insert(key.clone(), new_value.clone());
                    out.push_str(&render(serde_yaml::Value::Mapping(entry))?);
                }
                None => continue,
            }
            emitted.insert(key.clone());
        }
        for (key, value) in new_map {
            if emitted.contains(key) {
                continue;
            }
            let mut entry = serde_yaml::Mapping::new();
            entry.insert(key.clone(), value.clone());
            out.push_str(&render(serde_yaml::Value::Mapping(entry))?);
        }
    }

    // The rendered section must parse back to exactly the requested value.
    let check: serde_yaml::Mapping = serde_yaml::from_str(&out).ok()?;
    if check.get(serde_yaml::Value::String(section_key.to_string())) != Some(new) {
        return None;
    }
    // Keep "no trailing newline at EOF" files that way.
    if !raw_section.ends_with('\n') {
        while out.ends_with('\n') {
            out.pop();
            if out.ends_with('\r') {
                out.pop();
            }
        }
    }
    Some(out)
}

// ============================================================================
// Backup & Cleanup
// ============================================================================

fn create_hermes_backup(source: &str) -> Result<PathBuf, AppError> {
    let backup_dir = get_app_config_dir().join("backups").join("hermes");
    fs::create_dir_all(&backup_dir).map_err(|e| AppError::io(&backup_dir, e))?;

    let base_id = format!("hermes_{}", Local::now().format("%Y%m%d_%H%M%S"));
    let mut filename = format!("{base_id}.yaml");
    let mut backup_path = backup_dir.join(&filename);
    let mut counter = 1;

    while backup_path.exists() {
        filename = format!("{base_id}_{counter}.yaml");
        backup_path = backup_dir.join(&filename);
        counter += 1;
    }

    atomic_write_secret(&backup_path, source.as_bytes())?;
    cleanup_hermes_backups(&backup_dir)?;
    Ok(backup_path)
}

fn cleanup_hermes_backups(dir: &Path) -> Result<(), AppError> {
    let retain = effective_backup_retain_count();
    let mut entries = fs::read_dir(dir)
        .map_err(|e| AppError::io(dir, e))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .map(|ext| ext == "yaml" || ext == "yml")
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();

    if entries.len() <= retain {
        return Ok(());
    }

    entries.sort_by_key(|entry| entry.metadata().and_then(|m| m.modified()).ok());
    let remove_count = entries.len().saturating_sub(retain);
    for entry in entries.into_iter().take(remove_count) {
        if let Err(err) = fs::remove_file(entry.path()) {
            log::warn!(
                "Failed to remove old Hermes config backup {}: {err}",
                entry.path().display()
            );
        }
    }

    Ok(())
}

// ============================================================================
// High-level Write Helper
// ============================================================================

/// Write a single top-level YAML section to config.yaml using section-level replacement.
///
/// This preserves comments and unrelated sections while only modifying the
/// target section.
fn write_yaml_section_to_config(
    section_key: &str,
    value: &serde_yaml::Value,
) -> Result<HermesWriteOutcome, AppError> {
    let _guard = hermes_write_lock().lock()?;
    write_yaml_section_to_config_locked(section_key, value)
}

/// Inner write helper — caller must already hold the write lock.
fn write_yaml_section_to_config_locked(
    section_key: &str,
    value: &serde_yaml::Value,
) -> Result<HermesWriteOutcome, AppError> {
    let config_path = get_hermes_config_path();
    let raw = if config_path.exists() {
        fs::read_to_string(&config_path).map_err(|e| AppError::io(&config_path, e))?
    } else {
        String::new()
    };

    let new_raw = replace_yaml_section(&raw, section_key, value)?;

    if new_raw == raw {
        return Ok(HermesWriteOutcome::default());
    }

    let backup_path = if !raw.is_empty() {
        Some(create_hermes_backup(&raw)?)
    } else {
        None
    };

    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    atomic_write_secret(&config_path, new_raw.as_bytes())?;

    log::debug!(
        "Hermes config section '{}' written to {:?}",
        section_key,
        config_path
    );
    Ok(HermesWriteOutcome {
        backup_path: backup_path.map(|p| p.display().to_string()),
    })
}

// ============================================================================
// Provider Functions
// ============================================================================

/// Convert a provider's `models` field from a UI-friendly array to the YAML
/// dict shape that Hermes expects.
///
/// Input (from CC Switch UI / database):
/// ```json
/// "models": [{ "id": "foo", "context_length": 200000 }, { "id": "bar" }]
/// ```
///
/// Output (what we write to YAML):
/// ```json
/// "models": { "foo": { "context_length": 200000 }, "bar": {} }
/// ```
///
/// Entries with a missing or empty `id` are dropped. The top-level `id` key
/// is stripped from each value since it now lives on the parent as the map
/// key. Insertion order is preserved (serde_json uses IndexMap under the
/// `preserve_order` feature).
fn models_array_to_dict(array: Vec<serde_json::Value>) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for item in array {
        let serde_json::Value::Object(mut obj) = item else {
            continue;
        };
        let Some(id) = obj
            .remove("id")
            .and_then(|v| v.as_str().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        map.insert(id, serde_json::Value::Object(obj));
    }
    serde_json::Value::Object(map)
}

/// Inverse of [`models_array_to_dict`]. Converts the YAML dict shape back to
/// the UI-friendly ordered array, re-injecting `id` as an object field.
fn models_dict_to_array(dict: serde_json::Map<String, serde_json::Value>) -> serde_json::Value {
    let mut out = Vec::with_capacity(dict.len());
    for (id, value) in dict {
        let mut obj = match value {
            serde_json::Value::Object(obj) => obj,
            serde_json::Value::Null => serde_json::Map::new(),
            other => {
                log::warn!("Unexpected Hermes model entry for '{id}': {other:?}, skipping");
                continue;
            }
        };
        obj.insert("id".to_string(), serde_json::Value::String(id));
        out.push(serde_json::Value::Object(obj));
    }
    serde_json::Value::Array(out)
}

/// Rewrite historical camelCase keys to Hermes' snake_case schema.
///
/// Older DeepLink import paths emitted `baseUrl` / `apiKey` / `apiMode` /
/// `maxTokens` / `contextLength`, which do not belong to Hermes'
/// `_VALID_CUSTOM_PROVIDER_FIELDS` set. Writing those raw to YAML silently
/// poisons `custom_providers:` entries. This sanitiser runs defensively on
/// every `set_provider` call so stored data heals on the next activation;
/// unknown keys pass through untouched to keep forward-compat with new
/// Hermes fields (e.g. `request_timeout_seconds`).
fn sanitize_hermes_provider_keys(config: &mut serde_json::Value) {
    const KEY_ALIASES: &[(&str, &str)] = &[
        ("baseUrl", "base_url"),
        ("apiKey", "api_key"),
        ("apiMode", "api_mode"),
        ("maxTokens", "max_tokens"),
        ("contextLength", "context_length"),
    ];
    // Legacy DeepLink emitted `api: "openai-completions"` which is neither a
    // Hermes field nor mappable to `api_mode`. `_cc_source` / `provider_key`
    // are UI-only markers injected on read — they must never reach YAML.
    const LEGACY_FIELDS_TO_DROP: &[&str] = &["api", PROVIDER_SOURCE_FIELD, "provider_key"];

    let Some(obj) = config.as_object_mut() else {
        return;
    };

    for (from, to) in KEY_ALIASES {
        if let Some(val) = obj.remove(*from) {
            // snake_case wins when both are present; stale camelCase is dropped.
            obj.entry((*to).to_string()).or_insert(val);
        }
    }

    for field in LEGACY_FIELDS_TO_DROP {
        obj.remove(*field);
    }
}

/// If `config.models` is a JSON array, convert it in-place to the dict shape.
/// No-op when `models` is absent or already a dict.
fn normalize_provider_models_for_write(config: &mut serde_json::Value) {
    let Some(obj) = config.as_object_mut() else {
        return;
    };
    let Some(models_val) = obj.get_mut("models") else {
        return;
    };
    if models_val.is_array() {
        let taken = std::mem::take(models_val);
        if let serde_json::Value::Array(arr) = taken {
            *models_val = models_array_to_dict(arr);
        }
    }
}

/// If `config.models` is a JSON dict, convert it in-place to the ordered array
/// shape. No-op when `models` is absent or already an array.
fn denormalize_provider_models_for_read(config: &mut serde_json::Value) {
    let Some(obj) = config.as_object_mut() else {
        return;
    };
    let Some(models_val) = obj.get_mut("models") else {
        return;
    };
    if models_val.is_object() {
        let taken = std::mem::take(models_val);
        if let serde_json::Value::Object(map) = taken {
            *models_val = models_dict_to_array(map);
        }
    }
}

/// Marker field injected on provider payloads sourced from Hermes v12+
/// `providers:` dict. CC Switch treats those as read-only — writes have to
/// go through Hermes' own Web UI to keep its overlay semantics intact.
pub const PROVIDER_SOURCE_FIELD: &str = "_cc_source";
pub const PROVIDER_SOURCE_CUSTOM_LIST: &str = "custom_providers";
pub const PROVIDER_SOURCE_DICT: &str = "providers_dict";

/// Normalize a single entry from the v12+ `providers:` dict into the same
/// JSON shape that `custom_providers:` list entries take, mirroring upstream
/// `_normalize_custom_provider_entry` (hermes_cli/config.py).
///
/// Returns `None` when the entry is not a mapping or lacks any usable name.
fn normalize_providers_dict_entry(
    key: &str,
    entry: &serde_yaml::Value,
) -> Result<Option<serde_json::Value>, AppError> {
    if !entry.is_mapping() {
        return Ok(None);
    }
    let mut json_val = yaml_to_json(entry)?;
    let Some(obj) = json_val.as_object_mut() else {
        return Ok(None);
    };
    // Upstream prefers an explicit `name` when present, falling back to the
    // dict key. Always round-trip it to a trimmed non-empty string.
    let resolved_name = obj
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| key.trim().to_string());
    if resolved_name.is_empty() {
        return Ok(None);
    }
    obj.insert("name".to_string(), serde_json::json!(resolved_name));
    obj.insert("provider_key".to_string(), serde_json::json!(key));
    obj.insert(
        PROVIDER_SOURCE_FIELD.to_string(),
        serde_json::json!(PROVIDER_SOURCE_DICT),
    );
    Ok(Some(json_val))
}

/// Collect provider entries living under the v12+ `providers:` dict.
fn read_providers_dict_entries(config: &serde_yaml::Value) -> Vec<(String, serde_json::Value)> {
    let Some(mapping) = config.get("providers").and_then(|v| v.as_mapping()) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(mapping.len());
    for (k, v) in mapping {
        let Some(key_str) = k.as_str().map(str::trim).filter(|s| !s.is_empty()) else {
            continue;
        };
        match normalize_providers_dict_entry(key_str, v) {
            Ok(Some(entry)) => {
                let name = entry
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or(key_str)
                    .to_string();
                out.push((name, entry));
            }
            Ok(None) => {
                log::debug!("Skipping Hermes providers['{key_str}']: not a mapping");
            }
            Err(e) => {
                log::warn!("Failed to normalize Hermes providers['{key_str}']: {e}");
            }
        }
    }
    out
}

/// Get all providers as a JSON map keyed by provider name.
///
/// Unions two on-disk sources, matching upstream `get_compatible_custom_providers`:
/// - `custom_providers:` list entries (writable by CC Switch)
/// - `providers:` dict entries (v12+ schema, surfaced read-only with
///   `_cc_source = "providers_dict"` so the UI can disable edit/delete)
///
/// When a name appears in both, the list entry wins (upstream dedup order),
/// keeping CC Switch free to edit it. Models are denormalized from the YAML
/// dict shape to the UI-friendly ordered array.
pub fn get_providers() -> Result<serde_json::Map<String, serde_json::Value>, AppError> {
    let config = read_hermes_config()?;
    let mut map = serde_json::Map::new();

    if let Some(seq) = config.get("custom_providers").and_then(|v| v.as_sequence()) {
        for item in seq {
            if let Some(name) = item.get("name").and_then(|n| n.as_str()) {
                match yaml_to_json(item) {
                    Ok(mut json_val) => {
                        // Heal legacy camelCase records (from older DeepLink
                        // imports) before the UI sees them, so editing doesn't
                        // reveal stale `baseUrl` / `apiKey` fields.
                        sanitize_hermes_provider_keys(&mut json_val);
                        denormalize_provider_models_for_read(&mut json_val);
                        if let Some(obj) = json_val.as_object_mut() {
                            obj.insert(
                                PROVIDER_SOURCE_FIELD.to_string(),
                                serde_json::json!(PROVIDER_SOURCE_CUSTOM_LIST),
                            );
                        }
                        map.insert(name.to_string(), json_val);
                    }
                    Err(e) => {
                        log::warn!("Failed to convert Hermes provider '{name}' to JSON: {e}");
                    }
                }
            }
        }
    }

    for (name, mut entry) in read_providers_dict_entries(&config) {
        if map.contains_key(&name) {
            continue; // list wins over dict on duplicate names
        }
        denormalize_provider_models_for_read(&mut entry);
        map.insert(name, entry);
    }

    Ok(map)
}

/// Reject writes that would target a dict-only overlay entry.
///
/// `verb` is inlined into the user-facing error so both "edit" and "remove"
/// callers can share one implementation.
fn ensure_provider_writable(
    config: &serde_yaml::Value,
    name: &str,
    verb: &str,
) -> Result<(), AppError> {
    if is_dict_only_provider(config, name) {
        return Err(AppError::Config(format!(
            "Provider '{name}' is managed by Hermes' 'providers:' dict — {verb} via Hermes Web UI"
        )));
    }
    Ok(())
}

/// True when `name` appears in `providers:` dict but not in `custom_providers:`
/// list — i.e. it is a read-only overlay CC Switch must not touch.
fn is_dict_only_provider(config: &serde_yaml::Value, name: &str) -> bool {
    let list_has = config
        .get("custom_providers")
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .any(|item| item.get("name").and_then(|n| n.as_str()) == Some(name))
        })
        .unwrap_or(false);
    if list_has {
        return false;
    }
    config
        .get("providers")
        .and_then(|v| v.as_mapping())
        .map(|m| {
            m.iter().any(|(k, v)| {
                let key_matches = k.as_str() == Some(name);
                let name_matches = v
                    .get("name")
                    .and_then(|n| n.as_str())
                    .map(|s| s == name)
                    .unwrap_or(false);
                (key_matches || name_matches) && v.is_mapping()
            })
        })
        .unwrap_or(false)
}

/// Get a single custom provider by name.
pub fn get_provider(name: &str) -> Result<Option<serde_json::Value>, AppError> {
    Ok(get_providers()?.get(name).cloned())
}

/// Set (upsert) a custom provider by name.
///
/// Upserts into the `custom_providers:` YAML sequence (matched by `name`).
/// The entry includes:
///   - `name:` field matching the provider id
///   - singular `model:` field set to the first model id from the `models:`
///     dict — the Hermes runtime and `/model` picker both read this field
///     (runtime_provider.py reads it via `_normalize_custom_provider_entry`;
///     main.py:1436/1450 uses it for picker hints)
///   - plural `models:` dict carrying per-model `context_length` etc.
///
/// The entire read-modify-write is done under the write lock to prevent
/// TOCTOU races.
pub fn set_provider(
    name: &str,
    provider_config: serde_json::Value,
) -> Result<HermesWriteOutcome, AppError> {
    let _guard = hermes_write_lock().lock()?;

    let config = read_hermes_config()?;
    ensure_provider_writable(&config, name, "edit")?;
    let mut providers: Vec<serde_yaml::Value> = config
        .get("custom_providers")
        .and_then(|v| v.as_sequence())
        .cloned()
        .unwrap_or_default();

    // Rewrite any historical camelCase keys (e.g. from older DeepLink imports)
    // before touching models / YAML — avoids writing non-Hermes fields back.
    let mut normalized = provider_config;
    sanitize_hermes_provider_keys(&mut normalized);

    // `null` in the payload is an explicit delete (CFG-18): the key is dropped
    // from the entry and not carried over from disk below.
    let deleted_keys: Vec<String> = normalized
        .as_object_mut()
        .map(|obj| {
            let keys: Vec<String> = obj
                .iter()
                .filter(|(_, v)| v.is_null())
                .map(|(k, _)| k.clone())
                .collect();
            for key in &keys {
                obj.remove(key);
            }
            keys
        })
        .unwrap_or_default();

    // Normalize `models` from UI array to Hermes YAML dict before serializing.
    normalize_provider_models_for_write(&mut normalized);

    // Extract the first model id (now a key in the normalized dict) so we can
    // propagate it to the singular `model:` field Hermes reads.
    let first_model_id = normalized
        .get("models")
        .and_then(|v| v.as_object())
        .and_then(|obj| obj.keys().next())
        .cloned();

    let mut yaml_val: serde_yaml::Value = json_to_yaml(&normalized)?;
    if let serde_yaml::Value::Mapping(ref mut m) = yaml_val {
        m.insert(
            serde_yaml::Value::String("name".to_string()),
            serde_yaml::Value::String(name.to_string()),
        );
        if let Some(model_id) = first_model_id {
            m.insert(
                serde_yaml::Value::String("model".to_string()),
                serde_yaml::Value::String(model_id),
            );
        } else {
            m.remove(serde_yaml::Value::String("model".to_string()));
        }
    }

    if let Some(existing) = providers
        .iter_mut()
        .find(|p| p.get("name").and_then(|n| n.as_str()) == Some(name))
    {
        // Forward-compat: carry over any on-disk fields the UI payload didn't
        // include. Hermes keeps evolving (e.g. `request_timeout_seconds`,
        // `key_env`), and users may set those via Hermes Web UI — without
        // this merge, a CC Switch edit to an unrelated field would silently
        // strip them on write-back.
        if let (Some(existing_map), serde_yaml::Value::Mapping(new_map)) =
            (existing.as_mapping(), &mut yaml_val)
        {
            for (k, v) in existing_map {
                let deleted = k
                    .as_str()
                    .is_some_and(|key| deleted_keys.iter().any(|d| d == key));
                if !deleted {
                    new_map.entry(k.clone()).or_insert_with(|| v.clone());
                }
            }
        }
        *existing = yaml_val;
    } else {
        providers.push(yaml_val);
    }

    let providers_value = serde_yaml::Value::Sequence(providers);
    write_yaml_section_to_config_locked("custom_providers", &providers_value)
}

/// Remove a custom provider by name.
///
/// Filters out the matching entry from the `custom_providers:` sequence.
/// No-op if the section is missing or no entry matches. The entire
/// read-modify-write is done under the write lock to prevent TOCTOU races.
pub fn remove_provider(name: &str) -> Result<HermesWriteOutcome, AppError> {
    let _guard = hermes_write_lock().lock()?;
    let config = read_hermes_config()?;

    ensure_provider_writable(&config, name, "remove")?;

    let exists = config
        .get("custom_providers")
        .and_then(|v| v.as_sequence())
        .is_some_and(|seq| {
            seq.iter()
                .any(|p| p.get("name").and_then(|n| n.as_str()) == Some(name))
        });
    // PRODUCT-71: refuse to leave `model.provider` pointing at a removed entry.
    let backs_default = config
        .get("model")
        .and_then(|m| m.get("provider"))
        .and_then(|p| p.as_str())
        .is_some_and(|p| p == name);
    if exists && backs_default {
        return Err(AppError::localized(
            "hermes.providerBacksDefaultModel",
            format!("供应商 '{name}' 是当前默认模型的来源（model.provider），请先切换到其他供应商再移除"),
            format!(
                "Provider '{name}' is the default model provider (model.provider). Switch to another provider before removing it."
            ),
        ));
    }

    let mut providers: Vec<serde_yaml::Value> = config
        .get("custom_providers")
        .and_then(|v| v.as_sequence())
        .cloned()
        .unwrap_or_default();

    let original_len = providers.len();
    providers.retain(|p| p.get("name").and_then(|n| n.as_str()) != Some(name));
    if providers.len() == original_len {
        return Ok(HermesWriteOutcome::default());
    }

    let providers_value = serde_yaml::Value::Sequence(providers);
    write_yaml_section_to_config_locked("custom_providers", &providers_value)
}

// ============================================================================
// Model Config Functions
// ============================================================================

/// Get the `model` section as a typed config.
pub fn get_model_config() -> Result<Option<HermesModelConfig>, AppError> {
    let config = read_hermes_config()?;
    let Some(model_value) = config.get("model") else {
        return Ok(None);
    };
    let json_val = yaml_to_json(model_value)?;
    let model = serde_json::from_value(json_val)
        .map_err(|e| AppError::Config(format!("Failed to parse Hermes model config: {e}")))?;
    Ok(Some(model))
}

/// Set the `model` section.
pub fn set_model_config(model: &HermesModelConfig) -> Result<HermesWriteOutcome, AppError> {
    let json_val =
        serde_json::to_value(model).map_err(|e| AppError::JsonSerialize { source: e })?;
    let yaml_val = json_to_yaml(&json_val)?;
    write_yaml_section_to_config("model", &yaml_val)
}

/// Apply the top-level `model:` defaults when switching to a Hermes provider.
///
/// `model.provider` is **always** updated to the new provider id — without
/// this, switching to a provider whose settings lack a `models` list would
/// leave the runtime routing requests to the previously active provider.
///
/// `model.default` is only overwritten when the new provider declares at
/// least one model; otherwise the previous default is preserved so users
/// still have a runnable configuration (Hermes will surface a clear error
/// if the default no longer belongs to the active provider).
///
/// Existing fields in `model:` (`context_length` / `max_tokens` / `base_url`
/// / `extra`) are preserved via struct-update.
pub fn apply_switch_defaults(
    provider_id: &str,
    settings_config: &serde_json::Value,
) -> Result<HermesWriteOutcome, AppError> {
    let first_model_id = settings_config
        .get("models")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|m| m.get("id"))
        .and_then(|id| id.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let current = get_model_config()?.unwrap_or_default();
    let merged = HermesModelConfig {
        default: first_model_id.or(current.default.clone()),
        provider: Some(provider_id.to_string()),
        ..current
    };
    set_model_config(&merged)
}

// ============================================================================
// MCP Section Access (for mcp/hermes.rs to use in Phase 4)
// ============================================================================

/// Get the `mcp_servers` section as a YAML Mapping.
pub fn get_mcp_servers_yaml() -> Result<serde_yaml::Mapping, AppError> {
    let config = read_hermes_config()?;
    Ok(config
        .get("mcp_servers")
        .and_then(|v| v.as_mapping())
        .cloned()
        .unwrap_or_default())
}

/// Atomically read-modify-write the `mcp_servers` section under the write lock.
///
/// Prevents TOCTOU races when multiple sync operations run concurrently.
pub fn update_mcp_servers_yaml<F>(updater: F) -> Result<(), AppError>
where
    F: FnOnce(&mut serde_yaml::Mapping) -> Result<(), AppError>,
{
    let _guard = hermes_write_lock().lock()?;
    let config = read_hermes_config()?;
    let mut servers = config
        .get("mcp_servers")
        .and_then(|v| v.as_mapping())
        .cloned()
        .unwrap_or_default();
    updater(&mut servers)?;
    let value = serde_yaml::Value::Mapping(servers);
    write_yaml_section_to_config_locked("mcp_servers", &value)?;
    Ok(())
}

// ============================================================================
// YAML ↔ JSON Conversion Helpers
// ============================================================================

/// Convert a `serde_yaml::Value` to a `serde_json::Value`.
pub(crate) fn yaml_to_json(yaml: &serde_yaml::Value) -> Result<serde_json::Value, AppError> {
    // Serialize YAML value to string, then parse as JSON value.
    // This handles all type mappings correctly.
    let yaml_str = serde_yaml::to_string(yaml)
        .map_err(|e| AppError::Config(format!("Failed to serialize YAML value: {e}")))?;
    serde_yaml::from_str::<serde_json::Value>(&yaml_str)
        .map_err(|e| AppError::Config(format!("Failed to convert YAML to JSON: {e}")))
}

/// Convert a `serde_json::Value` to a `serde_yaml::Value`.
pub(crate) fn json_to_yaml(json: &serde_json::Value) -> Result<serde_yaml::Value, AppError> {
    let json_str = serde_json::to_string(json)
        .map_err(|e| AppError::Config(format!("Failed to serialize JSON value: {e}")))?;
    serde_yaml::from_str(&json_str)
        .map_err(|e| AppError::Config(format!("Failed to convert JSON to YAML: {e}")))
}

// ============================================================================
// Memory Files (~/.hermes/memories/{MEMORY,USER}.md)
// ============================================================================
//
// Hermes Agent persists two memory blobs on disk:
//   - `MEMORY.md` — agent's personal notes, snapshotted into the system prompt
//   - `USER.md`   — user profile, same treatment
// Entries are separated by a `§` on its own line. Hermes' own Web UI only
// exposes on/off toggles and character budgets — it has no content editor.
// CC Switch fills that gap by reading/writing the whole file as a markdown
// blob. Character budgets (`memory_char_limit`, `user_char_limit`) and enable
// flags (`memory_enabled`, `user_profile_enabled`) live at the top level of
// `config.yaml`; Hermes truncates over-budget content at load time.

/// Which of Hermes' two memory files to operate on. Tauri deserializes this
/// directly from the `"memory"` / `"user"` strings the frontend sends, so an
/// unknown value is rejected at the IPC boundary instead of deep in the stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum MemoryKind {
    Memory,
    User,
}

impl MemoryKind {
    fn filename(self) -> &'static str {
        match self {
            Self::Memory => "MEMORY.md",
            Self::User => "USER.md",
        }
    }
}

fn memories_dir() -> PathBuf {
    get_hermes_dir().join("memories")
}

/// Read a Hermes memory file as a markdown blob. Returns an empty string
/// when the file doesn't exist yet (first-run case).
pub fn read_memory(kind: MemoryKind) -> Result<String, AppError> {
    let path = memories_dir().join(kind.filename());
    match fs::read_to_string(&path) {
        Ok(content) => Ok(content),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(AppError::io(&path, e)),
    }
}

/// Atomically replace a Hermes memory file. `atomic_write` creates parent
/// directories as needed, so `~/.hermes/memories/` is materialized on first
/// write without a separate `create_dir_all` call.
pub fn write_memory(kind: MemoryKind, content: &str) -> Result<(), AppError> {
    let path = memories_dir().join(kind.filename());
    atomic_write(&path, content.as_bytes())
}

/// Character budget + enable flags for the two memory blobs, as configured
/// in Hermes' `config.yaml`. Defaults mirror `~/.hermes`'s own defaults so
/// callers get a usable budget bar even before the user edits config.yaml.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HermesMemoryLimits {
    pub memory: usize,
    pub user: usize,
    pub memory_enabled: bool,
    pub user_enabled: bool,
}

impl Default for HermesMemoryLimits {
    fn default() -> Self {
        Self {
            memory: 2200,
            user: 1375,
            memory_enabled: true,
            user_enabled: true,
        }
    }
}

/// Toggle the on/off flag for one of Hermes' two memory blobs, preserving all
/// other fields in the `memory:` section (character budgets, external provider
/// settings, etc.). Hermes stores the user-profile toggle under
/// `user_profile_enabled` (not `user_enabled`), so the mapping to on-disk keys
/// lives here rather than leaking to callers.
pub fn set_memory_enabled(kind: MemoryKind, enabled: bool) -> Result<HermesWriteOutcome, AppError> {
    let _guard = hermes_write_lock().lock()?;
    let config = read_hermes_config()?;

    let mut memory = match config.get("memory") {
        Some(serde_yaml::Value::Mapping(m)) => m.clone(),
        _ => serde_yaml::Mapping::new(),
    };

    let key = match kind {
        MemoryKind::Memory => "memory_enabled",
        MemoryKind::User => "user_profile_enabled",
    };
    memory.insert(
        serde_yaml::Value::String(key.to_string()),
        serde_yaml::Value::Bool(enabled),
    );

    write_yaml_section_to_config_locked("memory", &serde_yaml::Value::Mapping(memory))
}

/// Read memory budgets + toggles from `config.yaml`. Missing/unparsable
/// fields fall back to `HermesMemoryLimits::default()` rather than erroring,
/// so an empty or partially-populated config still yields a usable UI.
pub fn read_memory_limits() -> Result<HermesMemoryLimits, AppError> {
    let mut out = HermesMemoryLimits::default();
    let config = read_hermes_config()?;
    let Some(memory) = config.get("memory") else {
        return Ok(out);
    };

    if let Some(v) = memory.get("memory_char_limit").and_then(|v| v.as_u64()) {
        out.memory = v as usize;
    }
    if let Some(v) = memory.get("user_char_limit").and_then(|v| v.as_u64()) {
        out.user = v as usize;
    }
    if let Some(v) = memory.get("memory_enabled").and_then(|v| v.as_bool()) {
        out.memory_enabled = v;
    }
    if let Some(v) = memory.get("user_profile_enabled").and_then(|v| v.as_bool()) {
        out.user_enabled = v;
    }

    Ok(out)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests;
