//! Descriptor-driven support for adjacent CLI tools (PRODUCT-43, first
//! increment).
//!
//! The built-in apps each have an [`crate::adapters::AppAdapter`] keyed by
//! `AppType`. File-config tools that only need "endpoint + key + model"
//! (Qwen Code, Aider, …) do not need a new `AppType`: a JSON descriptor names
//! the files and keys, and this module reads and writes them with the same
//! rules as the adapters:
//!
//! * a file that fails to parse is never overwritten;
//! * the previous content is backed up under
//!   `~/.cc-switch/backups/custom-tools/` and the write is atomic;
//! * only the keys named by the descriptor change; comments, ordering and
//!   unrelated keys stay as they are (`env` and flat `yaml` files are edited
//!   line by line, `json` via the JSONC document editor, `toml` via
//!   `toml_edit`).
//!
//! Users switch between saved **profiles** (endpoint/key/model sets) per tool;
//! profiles live in `~/.cc-switch/custom-tools/profiles.json` (owner-only).
//! Extra descriptors can be dropped into `~/.cc-switch/custom-tools/*.json`.
//! Secret values are never logged and never returned to the webview: views
//! carry only whether a secret is set.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::audit_log::{self, AuditEvent};
use crate::adapters::live_file::{self, WriteOptions};
use crate::error::AppError;

const DIR_NAME: &str = "custom-tools";
const PROFILES_FILE: &str = "profiles.json";
const BACKUP_APP: &str = "custom-tools";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum FileFormat {
    /// `KEY=value` lines (`export` prefix and quotes tolerated).
    Env,
    /// YAML whose managed keys are top-level scalars.
    Yaml,
    /// JSON or JSONC; keys are dotted paths.
    Json,
    /// TOML; keys are dotted paths.
    Toml,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolField {
    /// Stable field id used by profiles (`baseUrl`, `apiKey`, `model`, …).
    pub id: String,
    pub label: String,
    /// Path relative to the home directory (`~/` prefix optional).
    pub file: String,
    pub format: FileFormat,
    /// Key in the file (dotted path for json/toml).
    pub key: String,
    #[serde(default)]
    pub secret: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolDescriptor {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub homepage: Option<String>,
    /// Directory (relative to home) whose existence means "installed".
    #[serde(default)]
    pub detect_dir: Option<String>,
    pub fields: Vec<ToolField>,
    #[serde(default, skip_deserializing)]
    pub builtin: bool,
}

/// Qwen Code reads OpenAI-compatible settings from `~/.qwen/.env`.
/// Aider reads `~/.aider.conf.yml` (the YAML keys mirror its CLI flags).
const BUILTIN_DESCRIPTORS: &str = r#"[
  {
    "id": "qwen-code",
    "name": "Qwen Code",
    "homepage": "https://github.com/QwenLM/qwen-code",
    "detectDir": ".qwen",
    "fields": [
      { "id": "baseUrl", "label": "Base URL", "file": ".qwen/.env", "format": "env", "key": "OPENAI_BASE_URL" },
      { "id": "apiKey", "label": "API key", "file": ".qwen/.env", "format": "env", "key": "OPENAI_API_KEY", "secret": true },
      { "id": "model", "label": "Model", "file": ".qwen/.env", "format": "env", "key": "OPENAI_MODEL" }
    ]
  },
  {
    "id": "aider",
    "name": "Aider",
    "homepage": "https://aider.chat",
    "fields": [
      { "id": "baseUrl", "label": "OpenAI-compatible base URL", "file": ".aider.conf.yml", "format": "yaml", "key": "openai-api-base" },
      { "id": "apiKey", "label": "API key", "file": ".aider.conf.yml", "format": "yaml", "key": "openai-api-key", "secret": true },
      { "id": "model", "label": "Model", "file": ".aider.conf.yml", "format": "yaml", "key": "model" }
    ]
  }
]"#;

fn tools_dir() -> PathBuf {
    crate::config::get_app_config_dir().join(DIR_NAME)
}

fn home() -> PathBuf {
    crate::config::get_home_dir()
}

fn validate(desc: &ToolDescriptor) -> Result<(), String> {
    let id_ok = !desc.id.is_empty()
        && desc
            .id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !id_ok {
        return Err(format!("invalid tool id '{}'", desc.id));
    }
    if desc.fields.is_empty() {
        return Err("a tool needs at least one field".to_string());
    }
    for f in &desc.fields {
        let rel = f.file.trim_start_matches("~/");
        let p = Path::new(rel);
        if rel.is_empty()
            || p.is_absolute()
            || p.components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(format!(
                "field '{}': file must be a path inside the home directory",
                f.id
            ));
        }
        if f.key.trim().is_empty() {
            return Err(format!("field '{}': key is empty", f.id));
        }
        let key_ok = match f.format {
            FileFormat::Env => f.key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
            FileFormat::Yaml => !f.key.contains([' ', '\t', ':', '#']),
            FileFormat::Json | FileFormat::Toml => f.key.split('.').all(|s| !s.is_empty()),
        };
        if !key_ok {
            return Err(format!("field '{}': invalid key '{}'", f.id, f.key));
        }
    }
    Ok(())
}

fn builtins() -> Vec<ToolDescriptor> {
    let mut list: Vec<ToolDescriptor> =
        serde_json::from_str(BUILTIN_DESCRIPTORS).expect("built-in descriptors");
    for d in &mut list {
        d.builtin = true;
    }
    list
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DescriptorError {
    pub file: String,
    pub error: String,
}

/// Built-in descriptors plus valid user descriptors (a user file with the id
/// of a built-in replaces it).
fn load_descriptors_in(dir: &Path) -> (Vec<ToolDescriptor>, Vec<DescriptorError>) {
    let mut tools = builtins();
    let mut errors = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (tools, errors);
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|e| e == "json")
                && p.file_name().is_some_and(|n| n != PROFILES_FILE)
        })
        .collect();
    paths.sort();
    for path in paths {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let parsed = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str::<ToolDescriptor>(&t).map_err(|e| e.to_string()))
            .and_then(|d| validate(&d).map(|_| d));
        match parsed {
            Ok(d) => {
                tools.retain(|t| t.id != d.id);
                tools.push(d);
            }
            Err(error) => errors.push(DescriptorError { file: name, error }),
        }
    }
    (tools, errors)
}

fn resolve(home: &Path, file: &str) -> PathBuf {
    home.join(file.trim_start_matches("~/"))
}

// ─── Format codecs ───────────────────────────────────────────

fn env_key_of(line: &str) -> Option<&str> {
    let t = line.trim_start();
    if t.starts_with('#') {
        return None;
    }
    let t = t.strip_prefix("export ").unwrap_or(t);
    let (k, _) = t.split_once('=')?;
    let k = k.trim();
    (!k.is_empty()).then_some(k)
}

fn unquote_env(raw: &str) -> String {
    let v = raw.trim();
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        return v[1..v.len() - 1]
            .replace("\\\"", "\"")
            .replace("\\\\", "\\");
    }
    if v.len() >= 2 && v.starts_with('\'') && v.ends_with('\'') {
        return v[1..v.len() - 1].to_string();
    }
    // Unquoted: an inline comment starts at " #".
    match v.find(" #") {
        Some(i) => v[..i].trim_end().to_string(),
        None => v.to_string(),
    }
}

fn quote_env(value: &str) -> String {
    let plain = !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:@+,=%~".contains(c));
    if plain {
        value.to_string()
    } else {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

fn env_get(text: &str, key: &str) -> Option<String> {
    text.lines()
        .rfind(|l| env_key_of(l) == Some(key))
        .and_then(|l| l.split_once('=').map(|(_, v)| unquote_env(v)))
}

fn env_set(text: &str, key: &str, value: Option<&str>, eol: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let mut found = false;
    lines.retain_mut(|line| {
        if env_key_of(line) != Some(key) {
            return true;
        }
        match value {
            Some(v) if !found => {
                let export = line.trim_start().starts_with("export ");
                *line = format!(
                    "{}{key}={}",
                    if export { "export " } else { "" },
                    quote_env(v)
                );
                found = true;
                true
            }
            _ => false,
        }
    });
    if let (Some(v), false) = (value, found) {
        lines.push(format!("{key}={}", quote_env(v)));
    }
    let mut out = lines.join(eol);
    if !out.is_empty() {
        out.push_str(eol);
    }
    out
}

fn yaml_parse(text: &str, path: &Path) -> Result<serde_yaml::Value, AppError> {
    if text.trim().is_empty() {
        return Ok(serde_yaml::Value::Mapping(Default::default()));
    }
    let v: serde_yaml::Value = serde_yaml::from_str(text)
        .map_err(|e| AppError::Message(format!("{} is not valid YAML: {e}", path.display())))?;
    match v {
        serde_yaml::Value::Mapping(_) | serde_yaml::Value::Null => Ok(v),
        _ => Err(AppError::Message(format!(
            "{} is not a YAML mapping",
            path.display()
        ))),
    }
}

fn yaml_scalar_to_string(v: &serde_yaml::Value) -> Option<String> {
    match v {
        serde_yaml::Value::String(s) => Some(s.clone()),
        serde_yaml::Value::Number(n) => Some(n.to_string()),
        serde_yaml::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn yaml_get(doc: &serde_yaml::Value, key: &str) -> Option<String> {
    doc.get(key).and_then(yaml_scalar_to_string)
}

fn yaml_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn is_yaml_key_line(line: &str, key: &str) -> bool {
    line.strip_prefix(key)
        .and_then(|rest| rest.strip_prefix(':'))
        .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '\t']))
        || line
            .strip_prefix(&yaml_quote(key))
            .is_some_and(|rest| rest.starts_with(':'))
}

/// Set or remove a top-level scalar key line by line, then check the result
/// parses to exactly the intended mapping change.
fn yaml_set(
    text: &str,
    key: &str,
    value: Option<&str>,
    eol: &str,
    path: &Path,
) -> Result<String, AppError> {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let mut idx = None;
    for (i, line) in lines.iter().enumerate() {
        if is_yaml_key_line(line, key) {
            idx = Some(i);
        }
    }
    if let Some(i) = idx {
        // A block value (indented lines below the key) is not a scalar.
        if lines
            .get(i + 1)
            .is_some_and(|next| next.starts_with([' ', '\t', '-']) && !next.trim().is_empty())
        {
            return Err(AppError::Message(format!(
                "{}: '{key}' is not a single-line value; edit it by hand",
                path.display()
            )));
        }
        match value {
            Some(v) => lines[i] = format!("{key}: {}", yaml_quote(v)),
            None => {
                lines.remove(i);
            }
        }
    } else if let Some(v) = value {
        lines.push(format!("{key}: {}", yaml_quote(v)));
    }
    let mut out = lines.join(eol);
    if !out.is_empty() {
        out.push_str(eol);
    }
    let before = yaml_parse(text, path)?;
    let after = yaml_parse(&out, path)?;
    let mut expected = match before {
        serde_yaml::Value::Mapping(m) => m,
        _ => Default::default(),
    };
    let k = serde_yaml::Value::String(key.to_string());
    match value {
        Some(v) => {
            expected.insert(k, serde_yaml::Value::String(v.to_string()));
        }
        None => {
            expected.remove(&k);
        }
    }
    let after = match after {
        serde_yaml::Value::Mapping(m) => m,
        _ => Default::default(),
    };
    if after != expected {
        return Err(AppError::Message(format!(
            "{}: could not update '{key}' without changing other settings",
            path.display()
        )));
    }
    Ok(out)
}

fn json_get(doc: &Value, key: &str) -> Option<String> {
    let mut cur = doc;
    for seg in key.split('.') {
        cur = cur.get(seg)?;
    }
    match cur {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn json_set(doc: &mut Value, key: &str, value: Option<&str>) -> Result<(), AppError> {
    let segs: Vec<&str> = key.split('.').collect();
    let mut cur = doc;
    for seg in &segs[..segs.len() - 1] {
        if !cur.is_object() {
            return Err(AppError::Message(format!("'{key}' crosses a non-object")));
        }
        let map = cur.as_object_mut().expect("object");
        if value.is_none() && !map.contains_key(*seg) {
            return Ok(());
        }
        cur = map
            .entry(seg.to_string())
            .or_insert_with(|| Value::Object(Default::default()));
    }
    let map = cur
        .as_object_mut()
        .ok_or_else(|| AppError::Message(format!("'{key}' crosses a non-object")))?;
    let last = segs[segs.len() - 1].to_string();
    match value {
        Some(v) => {
            map.insert(last, Value::String(v.to_string()));
        }
        None => {
            map.remove(&last);
        }
    }
    Ok(())
}

fn toml_get(doc: &toml_edit::DocumentMut, key: &str) -> Option<String> {
    let mut item = doc.as_item();
    for seg in key.split('.') {
        item = item.get(seg)?;
    }
    let v = item.as_value()?;
    v.as_str()
        .map(str::to_string)
        .or_else(|| v.as_integer().map(|i| i.to_string()))
        .or_else(|| v.as_float().map(|f| f.to_string()))
        .or_else(|| v.as_bool().map(|b| b.to_string()))
}

fn toml_set(
    doc: &mut toml_edit::DocumentMut,
    key: &str,
    value: Option<&str>,
) -> Result<(), AppError> {
    let segs: Vec<&str> = key.split('.').collect();
    let mut table = doc.as_table_mut() as &mut dyn toml_edit::TableLike;
    for seg in &segs[..segs.len() - 1] {
        if value.is_none() && table.get(seg).is_none() {
            return Ok(());
        }
        let entry = table
            .entry(seg)
            .or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
        table = entry
            .as_table_like_mut()
            .ok_or_else(|| AppError::Message(format!("'{key}' crosses a non-table")))?;
    }
    let last = segs[segs.len() - 1];
    match value {
        Some(v) => {
            table.insert(last, toml_edit::value(v));
        }
        None => {
            table.remove(last);
        }
    }
    Ok(())
}

/// Read the managed values of one file. A missing file yields no values; a
/// file that does not parse is an error.
fn read_file_values(
    path: &Path,
    format: FileFormat,
    keys: &[&str],
) -> Result<BTreeMap<String, String>, AppError> {
    let mut out = BTreeMap::new();
    let Some(live) = live_file::read_text_lenient(path)? else {
        return Ok(out);
    };
    let text = live.text;
    match format {
        FileFormat::Env => {
            for k in keys {
                if let Some(v) = env_get(&text, k) {
                    out.insert(k.to_string(), v);
                }
            }
        }
        FileFormat::Yaml => {
            let doc = yaml_parse(&text, path)?;
            for k in keys {
                if let Some(v) = yaml_get(&doc, k) {
                    out.insert(k.to_string(), v);
                }
            }
        }
        FileFormat::Json => {
            let doc = if text.trim().is_empty() {
                Value::Object(Default::default())
            } else {
                crate::adapters::json5_doc::Json5Doc::parse(&text)?.to_value()?
            };
            for k in keys {
                if let Some(v) = json_get(&doc, k) {
                    out.insert(k.to_string(), v);
                }
            }
        }
        FileFormat::Toml => {
            let doc: toml_edit::DocumentMut = text.parse().map_err(|e| {
                AppError::Message(format!("{} is not valid TOML: {e}", path.display()))
            })?;
            for k in keys {
                if let Some(v) = toml_get(&doc, k) {
                    out.insert(k.to_string(), v);
                }
            }
        }
    }
    Ok(out)
}

/// Apply `changes` (key → `Some(value)` to set, `None` to remove) to one file.
fn write_file_values(
    path: &Path,
    format: FileFormat,
    changes: &[(&str, Option<&str>)],
    secret: bool,
) -> Result<bool, AppError> {
    let live = live_file::read_text_lenient(path)?;
    let (text, style) = match &live {
        Some(l) => (live_file::normalize_eol(&l.text, "\n"), l.style),
        None => (String::new(), live_file::TextStyle::default()),
    };
    let eol = "\n";
    let next = match format {
        FileFormat::Env => {
            let mut t = text.clone();
            for (k, v) in changes {
                t = env_set(&t, k, *v, eol);
            }
            t
        }
        FileFormat::Yaml => {
            let mut t = text.clone();
            for (k, v) in changes {
                t = yaml_set(&t, k, *v, eol, path)?;
            }
            t
        }
        FileFormat::Json => {
            let mut doc = if text.trim().is_empty() {
                Value::Object(Default::default())
            } else {
                crate::adapters::json5_doc::Json5Doc::parse(&text)?.to_value()?
            };
            for (k, v) in changes {
                json_set(&mut doc, k, *v)?;
            }
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
            }
            let mut opts = WriteOptions::backup(BACKUP_APP);
            if secret {
                opts = opts.secret();
            }
            return Ok(live_file::write_json_preserving(path, &doc, opts)?.written());
        }
        FileFormat::Toml => {
            let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e| {
                AppError::Message(format!("{} is not valid TOML: {e}", path.display()))
            })?;
            for (k, v) in changes {
                toml_set(&mut doc, k, *v)?;
            }
            doc.to_string()
        }
    };
    if live.is_some() && next == text {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    let content = style.apply(&next);
    let mut opts = WriteOptions::backup(BACKUP_APP);
    if secret {
        opts = opts.secret();
    }
    Ok(live_file::write_live_text(path, &content, opts)?.written())
}

fn group_by_file(desc: &ToolDescriptor) -> BTreeMap<(String, FileFormat), Vec<&ToolField>> {
    let mut groups: BTreeMap<(String, FileFormat), Vec<&ToolField>> = BTreeMap::new();
    for f in &desc.fields {
        groups
            .entry((f.file.trim_start_matches("~/").to_string(), f.format))
            .or_default()
            .push(f);
    }
    groups
}

/// Field id → current live value.
fn read_live(home: &Path, desc: &ToolDescriptor) -> Result<BTreeMap<String, String>, AppError> {
    let mut out = BTreeMap::new();
    for ((file, format), fields) in group_by_file(desc) {
        let keys: Vec<&str> = fields.iter().map(|f| f.key.as_str()).collect();
        let values = read_file_values(&resolve(home, &file), format, &keys)?;
        for f in fields {
            if let Some(v) = values.get(&f.key) {
                out.insert(f.id.clone(), v.clone());
            }
        }
    }
    Ok(out)
}

/// Write field values (empty string removes the key). Returns how many files
/// changed. Every file is parsed before any is written, so one unparsable
/// file leaves all files untouched.
fn write_live(
    home: &Path,
    desc: &ToolDescriptor,
    values: &BTreeMap<String, String>,
) -> Result<usize, AppError> {
    let groups = group_by_file(desc);
    for ((file, format), fields) in &groups {
        let keys: Vec<&str> = fields.iter().map(|f| f.key.as_str()).collect();
        read_file_values(&resolve(home, file), *format, &keys)?;
    }
    let mut changed = 0;
    for ((file, format), fields) in &groups {
        let changes: Vec<(&str, Option<&str>)> = fields
            .iter()
            .filter_map(|f| {
                values
                    .get(&f.id)
                    .map(|v| (f.key.as_str(), Some(v.as_str()).filter(|v| !v.is_empty())))
            })
            .collect();
        if changes.is_empty() {
            continue;
        }
        let secret = fields.iter().any(|f| f.secret);
        if write_file_values(&resolve(home, file), *format, &changes, secret)? {
            changed += 1;
        }
    }
    Ok(changed)
}

// ─── Profiles ────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolProfile {
    pub id: String,
    pub name: String,
    /// Field id → value (secrets included; this file is owner-only).
    pub values: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolProfiles {
    #[serde(default)]
    profiles: Vec<ToolProfile>,
    #[serde(default)]
    active: Option<String>,
}

type ProfilesFile = BTreeMap<String, ToolProfiles>;

fn load_profiles(dir: &Path) -> ProfilesFile {
    std::fs::read(dir.join(PROFILES_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_profiles(dir: &Path, file: &ProfilesFile) -> Result<(), AppError> {
    std::fs::create_dir_all(dir).map_err(|e| AppError::io(dir, e))?;
    let body =
        serde_json::to_string_pretty(file).map_err(|source| AppError::JsonSerialize { source })?;
    crate::config::atomic_write_secret(&dir.join(PROFILES_FILE), body.as_bytes())
}

/// What the webview sees of a profile: secret values are replaced by a flag.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileView {
    pub id: String,
    pub name: String,
    pub values: BTreeMap<String, String>,
    pub secrets_set: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub descriptor: ToolDescriptor,
    pub installed: bool,
    pub files: Vec<String>,
    /// Current live non-secret values.
    pub live: BTreeMap<String, String>,
    /// Secret fields that are set in the live files.
    pub live_secrets_set: Vec<String>,
    /// Set when a live file could not be read or parsed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_error: Option<String>,
    pub profiles: Vec<ProfileView>,
    pub active_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomToolsOverview {
    pub tools: Vec<ToolStatus>,
    pub descriptor_errors: Vec<DescriptorError>,
    pub descriptors_dir: String,
}

fn split_secrets(
    desc: &ToolDescriptor,
    values: &BTreeMap<String, String>,
) -> (BTreeMap<String, String>, Vec<String>) {
    let mut plain = BTreeMap::new();
    let mut secrets = Vec::new();
    for (k, v) in values {
        let secret = desc.fields.iter().any(|f| &f.id == k && f.secret);
        if secret {
            if !v.is_empty() {
                secrets.push(k.clone());
            }
        } else {
            plain.insert(k.clone(), v.clone());
        }
    }
    (plain, secrets)
}

fn overview_in(home: &Path, dir: &Path) -> CustomToolsOverview {
    let (descs, descriptor_errors) = load_descriptors_in(dir);
    let profiles = load_profiles(dir);
    let tools = descs
        .into_iter()
        .map(|desc| {
            let files: Vec<String> = group_by_file(&desc)
                .keys()
                .map(|(f, _)| resolve(home, f).to_string_lossy().to_string())
                .collect();
            let installed = desc
                .detect_dir
                .as_deref()
                .map(|d| resolve(home, d).is_dir())
                .unwrap_or(false)
                || files.iter().any(|f| Path::new(f).exists());
            let (live, live_secrets_set, live_error) = match read_live(home, &desc) {
                Ok(v) => {
                    let (plain, secrets) = split_secrets(&desc, &v);
                    (plain, secrets, None)
                }
                Err(e) => (BTreeMap::new(), Vec::new(), Some(e.to_string())),
            };
            let entry = profiles.get(&desc.id).cloned().unwrap_or_default();
            let views = entry
                .profiles
                .iter()
                .map(|p| {
                    let (values, secrets_set) = split_secrets(&desc, &p.values);
                    ProfileView {
                        id: p.id.clone(),
                        name: p.name.clone(),
                        values,
                        secrets_set,
                    }
                })
                .collect();
            ToolStatus {
                descriptor: desc,
                installed,
                files,
                live,
                live_secrets_set,
                live_error,
                profiles: views,
                active_profile: entry.active,
            }
        })
        .collect();
    CustomToolsOverview {
        tools,
        descriptor_errors,
        descriptors_dir: dir.to_string_lossy().to_string(),
    }
}

fn find_tool(dir: &Path, tool_id: &str) -> Result<ToolDescriptor, AppError> {
    load_descriptors_in(dir)
        .0
        .into_iter()
        .find(|d| d.id == tool_id)
        .ok_or_else(|| AppError::Message(format!("Unknown tool '{tool_id}'")))
}

/// Input for saving a profile: secret fields that are absent keep their
/// stored value; an empty string clears a field.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileInput {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub values: BTreeMap<String, String>,
}

fn save_profile_in(dir: &Path, tool_id: &str, input: ProfileInput) -> Result<String, AppError> {
    let desc = find_tool(dir, tool_id)?;
    let name = input.name.trim();
    if name.is_empty() {
        return Err(AppError::Message("Profile name is required".to_string()));
    }
    let mut file = load_profiles(dir);
    let entry = file.entry(tool_id.to_string()).or_default();
    let id = input
        .id
        .clone()
        .filter(|id| entry.profiles.iter().any(|p| &p.id == id))
        .unwrap_or_else(|| format!("p{}", chrono::Utc::now().timestamp_millis()));
    let previous = entry
        .profiles
        .iter()
        .find(|p| p.id == id)
        .map(|p| p.values.clone())
        .unwrap_or_default();
    let mut values = BTreeMap::new();
    for f in &desc.fields {
        let next = input
            .values
            .get(&f.id)
            .cloned()
            .or_else(|| f.secret.then(|| previous.get(&f.id).cloned()).flatten());
        if let Some(v) = next.map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) {
            values.insert(f.id.clone(), v);
        }
    }
    let profile = ToolProfile {
        id: id.clone(),
        name: name.to_string(),
        values,
    };
    match entry.profiles.iter_mut().find(|p| p.id == id) {
        Some(p) => *p = profile,
        None => entry.profiles.push(profile),
    }
    save_profiles(dir, &file)?;
    audit_log::record(
        AuditEvent::new("ui", "customTool.profile.save", "customTool", tool_id).detail(id.clone()),
    );
    Ok(id)
}

fn delete_profile_in(dir: &Path, tool_id: &str, profile_id: &str) -> Result<(), AppError> {
    let mut file = load_profiles(dir);
    if let Some(entry) = file.get_mut(tool_id) {
        entry.profiles.retain(|p| p.id != profile_id);
        if entry.active.as_deref() == Some(profile_id) {
            entry.active = None;
        }
    }
    save_profiles(dir, &file)?;
    audit_log::record(
        AuditEvent::new("ui", "customTool.profile.delete", "customTool", tool_id)
            .detail(profile_id.to_string()),
    );
    Ok(())
}

/// Write a profile to the tool's live files and mark it active. Fields the
/// profile does not set are removed from the live files.
fn activate_in(
    home: &Path,
    dir: &Path,
    tool_id: &str,
    profile_id: &str,
) -> Result<usize, AppError> {
    let desc = find_tool(dir, tool_id)?;
    let mut file = load_profiles(dir);
    let entry = file.entry(tool_id.to_string()).or_default();
    let profile = entry
        .profiles
        .iter()
        .find(|p| p.id == profile_id)
        .cloned()
        .ok_or_else(|| AppError::Message(format!("Unknown profile '{profile_id}'")))?;
    let mut values = BTreeMap::new();
    for f in &desc.fields {
        values.insert(
            f.id.clone(),
            profile.values.get(&f.id).cloned().unwrap_or_default(),
        );
    }
    let before = read_live(home, &desc).ok();
    let changed = write_live(home, &desc, &values)?;
    entry.active = Some(profile_id.to_string());
    save_profiles(dir, &file)?;
    let to_json = |m: &BTreeMap<String, String>| serde_json::to_value(m).ok();
    audit_log::record(
        AuditEvent::new("ui", "customTool.activate", "customTool", tool_id)
            .states(
                before.as_ref().and_then(to_json).as_ref(),
                read_live(home, &desc)
                    .ok()
                    .as_ref()
                    .and_then(to_json)
                    .as_ref(),
            )
            .detail(profile_id.to_string()),
    );
    Ok(changed)
}

/// Save the tool's current live values as a new profile.
fn import_live_in(home: &Path, dir: &Path, tool_id: &str, name: &str) -> Result<String, AppError> {
    let desc = find_tool(dir, tool_id)?;
    let values = read_live(home, &desc)?;
    if values.is_empty() {
        return Err(AppError::Message(format!(
            "{} has no settings to import",
            desc.name
        )));
    }
    let id = save_profile_in(
        dir,
        tool_id,
        ProfileInput {
            id: None,
            name: name.to_string(),
            values,
        },
    )?;
    let mut file = load_profiles(dir);
    if let Some(entry) = file.get_mut(tool_id) {
        entry.active = Some(id.clone());
    }
    save_profiles(dir, &file)?;
    Ok(id)
}

#[tauri::command]
pub fn list_custom_tools() -> CustomToolsOverview {
    overview_in(&home(), &tools_dir())
}

#[tauri::command]
pub fn save_custom_tool_profile(
    #[allow(non_snake_case)] toolId: String,
    profile: ProfileInput,
) -> Result<String, String> {
    save_profile_in(&tools_dir(), &toolId, profile).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_custom_tool_profile(
    #[allow(non_snake_case)] toolId: String,
    #[allow(non_snake_case)] profileId: String,
) -> Result<(), String> {
    delete_profile_in(&tools_dir(), &toolId, &profileId).map_err(|e| e.to_string())
}

/// Returns how many files were rewritten.
#[tauri::command]
pub fn activate_custom_tool_profile(
    #[allow(non_snake_case)] toolId: String,
    #[allow(non_snake_case)] profileId: String,
) -> Result<usize, String> {
    activate_in(&home(), &tools_dir(), &toolId, &profileId).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn import_custom_tool_live(
    #[allow(non_snake_case)] toolId: String,
    name: String,
) -> Result<String, String> {
    import_live_in(&home(), &tools_dir(), &toolId, &name).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Env {
        home: tempfile::TempDir,
        dir: tempfile::TempDir,
    }

    fn env() -> Env {
        Env {
            home: tempfile::tempdir().unwrap(),
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn values(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn builtin_descriptors_are_valid() {
        let list = builtins();
        assert!(list.iter().any(|d| d.id == "qwen-code"));
        assert!(list.iter().any(|d| d.id == "aider"));
        for d in &list {
            validate(d).unwrap();
        }
    }

    #[test]
    fn qwen_env_file_keeps_comments_and_unrelated_keys() {
        let _guard = crate::test_support::isolated_home();
        let e = env();
        let path = e.home.path().join(".qwen/.env");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "# my qwen settings\nOTHER=keep\nexport OPENAI_BASE_URL=https://old\nOPENAI_MODEL=old-model # inline\n",
        )
        .unwrap();
        let desc = find_tool(e.dir.path(), "qwen-code").unwrap();
        assert_eq!(
            read_live(e.home.path(), &desc).unwrap(),
            values(&[("baseUrl", "https://old"), ("model", "old-model")])
        );
        write_live(
            e.home.path(),
            &desc,
            &values(&[
                ("baseUrl", "https://relay.example.com/v1"),
                ("apiKey", "sk test \"quoted\""),
                ("model", ""),
            ]),
        )
        .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# my qwen settings\nOTHER=keep\n"));
        assert!(text.contains("export OPENAI_BASE_URL=https://relay.example.com/v1\n"));
        assert!(!text.contains("OPENAI_MODEL"));
        let live = read_live(e.home.path(), &desc).unwrap();
        assert_eq!(live["apiKey"], "sk test \"quoted\"");
    }

    #[test]
    fn aider_yaml_updates_scalars_and_preserves_comments() {
        let _guard = crate::test_support::isolated_home();
        let e = env();
        let path = e.home.path().join(".aider.conf.yml");
        std::fs::write(&path, "# aider\ndark-mode: true\nmodel: gpt-4o  # old\n").unwrap();
        let desc = find_tool(e.dir.path(), "aider").unwrap();
        write_live(
            e.home.path(),
            &desc,
            &values(&[
                ("baseUrl", "https://r.example/v1"),
                ("model", "openai/qwen3: coder"),
            ]),
        )
        .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# aider\ndark-mode: true\n"));
        let live = read_live(e.home.path(), &desc).unwrap();
        assert_eq!(live["model"], "openai/qwen3: coder");
        assert_eq!(live["baseUrl"], "https://r.example/v1");
    }

    #[test]
    fn unparsable_files_are_never_overwritten() {
        let _guard = crate::test_support::isolated_home();
        let e = env();
        let path = e.home.path().join(".aider.conf.yml");
        let broken = "model: [unclosed\n";
        std::fs::write(&path, broken).unwrap();
        let desc = find_tool(e.dir.path(), "aider").unwrap();
        assert!(write_live(e.home.path(), &desc, &values(&[("model", "x")])).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);
    }

    #[test]
    fn block_values_are_refused_instead_of_corrupted() {
        let path = Path::new("x.yml");
        let text = "model:\n  nested: 1\nother: 2\n";
        assert!(yaml_set(text, "model", Some("m"), "\n", path).is_err());
    }

    #[test]
    fn user_descriptors_cover_json_and_toml_and_bad_ones_are_reported() {
        let _guard = crate::test_support::isolated_home();
        let e = env();
        std::fs::write(
            e.dir.path().join("zed.json"),
            r#"{ "id": "zed-like", "name": "Zed-like", "fields": [
                { "id": "baseUrl", "label": "URL", "file": ".config/zed/settings.json", "format": "json", "key": "language_models.openai.api_url" },
                { "id": "model", "label": "Model", "file": ".tool/config.toml", "format": "toml", "key": "llm.model" }
            ] }"#,
        )
        .unwrap();
        std::fs::write(
            e.dir.path().join("evil.json"),
            r#"{ "id": "evil", "name": "Evil", "fields": [
                { "id": "x", "label": "x", "file": "../../etc/passwd", "format": "env", "key": "X" }
            ] }"#,
        )
        .unwrap();
        let zed = e.home.path().join(".config/zed/settings.json");
        std::fs::create_dir_all(zed.parent().unwrap()).unwrap();
        std::fs::write(&zed, "{\n  // theme stays\n  \"theme\": \"One Dark\"\n}\n").unwrap();
        let toml = e.home.path().join(".tool/config.toml");
        std::fs::create_dir_all(toml.parent().unwrap()).unwrap();
        std::fs::write(&toml, "# keep me\n[llm]\nmodel = \"old\"\n").unwrap();

        let overview = overview_in(e.home.path(), e.dir.path());
        assert_eq!(overview.descriptor_errors.len(), 1);
        assert_eq!(overview.descriptor_errors[0].file, "evil.json");
        let desc = find_tool(e.dir.path(), "zed-like").unwrap();
        write_live(
            e.home.path(),
            &desc,
            &values(&[("baseUrl", "https://relay/v1"), ("model", "new")]),
        )
        .unwrap();
        let zed_text = std::fs::read_to_string(&zed).unwrap();
        assert!(zed_text.contains("// theme stays"));
        assert!(zed_text.contains("https://relay/v1"));
        let toml_text = std::fs::read_to_string(&toml).unwrap();
        assert!(toml_text.contains("# keep me"));
        assert!(toml_text.contains("model = \"new\""));
    }

    #[test]
    fn profiles_hide_secrets_keep_them_on_edit_and_activate_to_live() {
        let _guard = crate::test_support::isolated_home();
        let e = env();
        let id = save_profile_in(
            e.dir.path(),
            "qwen-code",
            ProfileInput {
                id: None,
                name: "Relay".into(),
                values: values(&[
                    ("baseUrl", "https://relay/v1"),
                    ("apiKey", "sk-planted-secret"),
                    ("model", "qwen3-coder"),
                ]),
            },
        )
        .unwrap();
        // Edit without resending the key: the key is kept.
        save_profile_in(
            e.dir.path(),
            "qwen-code",
            ProfileInput {
                id: Some(id.clone()),
                name: "Relay 2".into(),
                values: values(&[("baseUrl", "https://relay2/v1"), ("model", "qwen3-coder")]),
            },
        )
        .unwrap();
        let overview = overview_in(e.home.path(), e.dir.path());
        let view = serde_json::to_string(&overview).unwrap();
        assert!(!view.contains("sk-planted-secret"));
        let qwen = overview
            .tools
            .iter()
            .find(|t| t.descriptor.id == "qwen-code")
            .unwrap();
        assert_eq!(qwen.profiles[0].secrets_set, vec!["apiKey".to_string()]);

        assert_eq!(
            activate_in(e.home.path(), e.dir.path(), "qwen-code", &id).unwrap(),
            1
        );
        let text = std::fs::read_to_string(e.home.path().join(".qwen/.env")).unwrap();
        assert!(text.contains("OPENAI_API_KEY=sk-planted-secret"));
        assert!(text.contains("OPENAI_BASE_URL=https://relay2/v1"));
        let overview = overview_in(e.home.path(), e.dir.path());
        let qwen = overview
            .tools
            .iter()
            .find(|t| t.descriptor.id == "qwen-code")
            .unwrap();
        assert_eq!(qwen.active_profile.as_deref(), Some(id.as_str()));
        assert!(qwen.installed);
        assert_eq!(qwen.live_secrets_set, vec!["apiKey".to_string()]);

        let imported = import_live_in(e.home.path(), e.dir.path(), "qwen-code", "Current").unwrap();
        delete_profile_in(e.dir.path(), "qwen-code", &id).unwrap();
        let overview = overview_in(e.home.path(), e.dir.path());
        let qwen = overview
            .tools
            .iter()
            .find(|t| t.descriptor.id == "qwen-code")
            .unwrap();
        assert_eq!(qwen.profiles.len(), 1);
        assert_eq!(qwen.profiles[0].id, imported);
    }
}
