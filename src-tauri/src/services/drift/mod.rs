//! Live 配置漂移检测与处理（CCS-166）
//!
//! 把各应用 live 文件的当前内容，与"按当前供应商重新写一遍 live"的预期输出
//!（经 [`crate::config::capture_writes`] 捕获、不落盘，含 MCP 重投影）逐键比较：
//!
//! - **残留占位符**：live 带有代理接管占位符，但代理并未在为该应用接管
//!   （上次异常退出 / 恢复失败的遗留），客户端此时无法工作；
//! - **外部改动**：应用外（用户手改、其它工具）新增或修改的字段。下次切换供应商
//!   时 live 会按预期输出整体重写，这些改动随之丢失。
//!
//! 每份报告可以：修复占位符（[`DriftResolution::Reconcile`]）、把外部字段并入该应用
//! 的通用配置片段（[`DriftResolution::Fold`]，之后切换到任何启用通用配置的供应商都
//! 保留它们）、或忽略（[`DriftResolution::Ignore`]，记录指纹，同样的漂移不再提示）。
//!
//! 报告只给出字段路径与变化类型，不携带字段值（live 里有 API Key）。

pub mod watcher;

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::app_config::AppType;
use crate::apps::registry::{with_caps, AppCaps};
use crate::error::AppError;
use crate::services::provider::{LiveFilePreview, ProviderService};
use crate::store::AppState;

/// 漂移报告推送给前端的事件名
pub const DRIFT_EVENT: &str = "live-config-drift";

/// 对象嵌套超过这一层后整体比较（`env.FOO` 粒度，不再细分 `env.FOO.bar`）
const MAX_DIFF_DEPTH: usize = 2;

/// 单个字段相对预期输出的变化
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(specta::Type))]
pub enum DriftChange {
    /// live 有、预期输出没有：下次写 live 时会被删除
    Added,
    /// 两边都有但值不同：下次写 live 时会被改回
    Modified,
    /// 预期输出有、live 没有：下次写 live 时会被加回
    Removed,
}

/// 一处字段漂移
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(specta::Type))]
pub struct DriftField {
    /// live 文件绝对路径
    pub file: String,
    /// 文件内的键路径（`["hooks"]`、`["env", "FOO"]`）；空数组表示整个文件
    pub path: Vec<String>,
    pub change: DriftChange,
    /// 能否并入通用配置片段（应用支持片段，且该字段不是凭据 / 供应商专属字段）
    pub foldable: bool,
}

/// 单个应用的漂移报告
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(specta::Type))]
pub struct DriftReport {
    /// 应用标识（`claude` / `codex` / ...）
    pub app: String,
    /// 代理正在为该应用接管：占位符属于预期，只报告新增的顶层字段
    pub takeover_active: bool,
    /// live 残留代理接管占位符（代理并未接管该应用）
    pub leftover_placeholder: bool,
    pub fields: Vec<DriftField>,
    /// 预期输出涉及的 live 文件（文件监听据此过滤事件）
    pub files: Vec<String>,
    /// 本次漂移的指纹（字段路径 + 变化 + 当前值摘要），用于忽略同样的漂移
    pub fingerprint: String,
    /// 用户已忽略同样指纹的漂移
    pub ignored: bool,
}

impl DriftReport {
    /// 是否存在任何漂移
    pub fn has_drift(&self) -> bool {
        self.leftover_placeholder || !self.fields.is_empty()
    }

    /// 是否需要提示用户（有漂移且未被忽略）
    pub fn needs_attention(&self) -> bool {
        self.has_drift() && !self.ignored
    }
}

/// 用户对漂移报告的处理
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[cfg_attr(test, derive(specta::Type))]
pub enum DriftResolution {
    /// 修复残留的接管占位符（备份 → 当前供应商 → 清理占位符 三层兜底）
    Reconcile,
    /// 把 live 中这些字段（[`DriftField::path`]）并入通用配置片段
    Fold { paths: Vec<Vec<String>> },
    /// 忽略当前这份漂移；同样指纹的漂移不再提示
    Ignore,
}

pub struct DriftService;

impl DriftService {
    /// 参与漂移检测的应用：经代理接管、live 只承载当前供应商的切换模式应用
    pub fn supported_apps() -> impl Iterator<Item = &'static AppType> {
        with_caps(AppCaps::PROXY_TAKEOVER)
            .filter(|d| !d.app.is_additive_mode())
            .map(|d| &d.app)
    }

    /// 检测全部支持的应用；单个应用失败只记录警告
    pub fn detect_all(state: &AppState) -> Vec<DriftReport> {
        Self::supported_apps()
            .filter_map(|app| match Self::detect(state, app) {
                Ok(report) => Some(report),
                Err(e) => {
                    log::warn!("检测 {} live 配置漂移失败: {e}", app.as_str());
                    None
                }
            })
            .collect()
    }

    /// 检测单个应用的 live 配置漂移
    pub fn detect(state: &AppState, app: &AppType) -> Result<DriftReport, AppError> {
        Self::ensure_supported(app)?;
        let adapter = crate::apps::adapter(app);

        let takeover_active = Self::takeover_active(state, app);
        let leftover_placeholder = !takeover_active
            && adapter
                .takeover()
                .zip(adapter.read_live().ok())
                .is_some_and(|(takeover, live)| takeover.has_placeholder(&live));

        let files = match Self::current_provider(state, app)? {
            Some(provider) => {
                ProviderService::capture_live_write(state, app, &provider, true)?.files
            }
            None => Vec::new(),
        };

        let mut fields = Vec::new();
        for file in &files {
            diff_file(file, &mut fields);
        }
        if takeover_active {
            // 接管改写了凭据 / 端点（嵌套字段），这些属于预期；只剩用户新增的顶层字段
            fields.retain(|field| field.change == DriftChange::Added && field.path.len() == 1);
        }

        let foldable = FoldSource::from_live(app);
        for field in &mut fields {
            field.foldable = field.change != DriftChange::Removed
                && foldable
                    .as_ref()
                    .is_some_and(|source| source.contains(&field.path));
        }

        let fingerprint = fingerprint(app, leftover_placeholder, &fields, &files);
        let ignored = state
            .db
            .get_setting(&ignored_key(app))?
            .is_some_and(|stored| stored == fingerprint);

        Ok(DriftReport {
            app: app.as_str().to_string(),
            takeover_active,
            leftover_placeholder,
            fields,
            files: files.into_iter().map(|file| file.path).collect(),
            fingerprint,
            ignored,
        })
    }

    /// 执行用户选择的处理，返回处理后的新报告
    pub async fn resolve(
        state: &AppState,
        app: &AppType,
        resolution: DriftResolution,
    ) -> Result<DriftReport, AppError> {
        Self::ensure_supported(app)?;
        match resolution {
            DriftResolution::Reconcile => Self::reconcile(state, app).await?,
            DriftResolution::Fold { paths } => Self::fold(state, app, &paths)?,
            DriftResolution::Ignore => {
                let report = Self::detect(state, app)?;
                state
                    .db
                    .set_setting(&ignored_key(app), &report.fingerprint)?;
            }
        }
        Self::detect(state, app)
    }

    async fn reconcile(state: &AppState, app: &AppType) -> Result<(), AppError> {
        if Self::takeover_active(state, app) {
            return Err(AppError::localized(
                "drift.reconcile.takeover_active",
                "代理正在接管该应用，接管占位符属于预期，无需修复",
                "The proxy is taking over this app; its placeholders are expected",
            ));
        }
        state
            .proxy_service
            .repair_takeover_leftover_for_app(app)
            .await
            .map_err(AppError::Message)
    }

    /// 把 live 中的 `paths` 并入通用配置片段，并让当前供应商启用通用配置
    fn fold(state: &AppState, app: &AppType, paths: &[Vec<String>]) -> Result<(), AppError> {
        if paths.is_empty() {
            return Ok(());
        }
        let source = FoldSource::from_live(app).ok_or_else(|| {
            AppError::localized(
                "drift.fold.unsupported",
                "该应用的 live 配置没有可并入通用配置的字段",
                "This app's live config has no fields that can join the common config",
            )
        })?;

        let old_snippet = state.db.get_config_snippet(app.as_str())?;
        let new_snippet = source.merge_into(old_snippet.as_deref().unwrap_or(""), paths)?;

        // 与 set_common_config_snippet 一致：先把隐式使用旧片段的供应商标记为显式启用
        if let Some(legacy) = old_snippet.as_deref().filter(|s| !s.trim().is_empty()) {
            ProviderService::migrate_legacy_common_config_usage(state, app.clone(), legacy)?;
        }
        state
            .db
            .set_config_snippet(app.as_str(), Some(new_snippet))?;
        state.db.set_config_snippet_cleared(app.as_str(), false)?;

        if let Some(mut provider) = Self::current_provider(state, app)? {
            let enabled = provider
                .meta
                .as_ref()
                .and_then(|meta| meta.common_config_enabled)
                == Some(true);
            if !enabled {
                provider
                    .meta
                    .get_or_insert_with(Default::default)
                    .common_config_enabled = Some(true);
                crate::services::provider::normalize_provider_common_config_for_storage(
                    state.db.as_ref(),
                    app,
                    &mut provider,
                )?;
                state.db.save_provider(app.as_str(), &provider)?;
            }
        }

        // live 已带有这些字段；按新片段重写一次，使 live 与预期输出重新一致
        ProviderService::sync_current_provider_for_app(state, app.clone())
    }

    fn ensure_supported(app: &AppType) -> Result<(), AppError> {
        if Self::supported_apps().any(|supported| supported == app) {
            Ok(())
        } else {
            Err(AppError::localized(
                "drift.app.unsupported",
                format!("{} 不支持 live 配置漂移检测", app.as_str()),
                format!(
                    "{} does not support live config drift detection",
                    app.as_str()
                ),
            ))
        }
    }

    /// 代理正在运行且为该应用开启了接管
    fn takeover_active(state: &AppState, app: &AppType) -> bool {
        let enabled = futures::executor::block_on(state.db.get_proxy_config_for_app(app.as_str()))
            .map(|config| config.enabled)
            .unwrap_or(false);
        enabled && futures::executor::block_on(state.proxy_service.is_running())
    }

    fn current_provider(
        state: &AppState,
        app: &AppType,
    ) -> Result<Option<crate::provider::Provider>, AppError> {
        let Some(id) = crate::settings::get_effective_current_provider(&state.db, app)? else {
            return Ok(None);
        };
        state.db.get_provider_by_id(&id, app.as_str())
    }
}

fn ignored_key(app: &AppType) -> String {
    format!("live_drift_ignored_{}", app.as_str())
}

fn fingerprint(
    app: &AppType,
    leftover_placeholder: bool,
    fields: &[DriftField],
    files: &[LiveFilePreview],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(app.as_str());
    hasher.update([u8::from(leftover_placeholder)]);
    for field in fields {
        hasher.update([0x1e]);
        hasher.update(&field.file);
        for segment in &field.path {
            hasher.update([0x1f]);
            hasher.update(segment);
        }
        hasher.update(format!("{:?}", field.change));
        // 同一路径被改成另一个值也算新的漂移
        let actual = files
            .iter()
            .find(|file| file.path == field.file)
            .and_then(|file| file.before.as_deref())
            .and_then(|text| parse_structured(Path::new(&field.file), text))
            .and_then(|value| value_at(&value, &field.path).cloned());
        hasher.update(actual.map(|value| value.to_string()).unwrap_or_default());
    }
    format!("{:x}", hasher.finalize())
}

/// 比较单个文件的当前内容（`before`）与预期输出（`after`）
fn diff_file(file: &LiveFilePreview, out: &mut Vec<DriftField>) {
    let path = Path::new(&file.path);
    let mut push = |segments: Vec<String>, change: DriftChange| {
        out.push(DriftField {
            file: file.path.clone(),
            path: segments,
            change,
            foldable: false,
        })
    };

    match (file.before.as_deref(), file.after.as_deref()) {
        (None, None) => {}
        (Some(_), None) => push(Vec::new(), DriftChange::Added),
        (None, Some(_)) => push(Vec::new(), DriftChange::Removed),
        (Some(before), Some(after)) => {
            match (
                parse_structured(path, before),
                parse_structured(path, after),
            ) {
                (Some(actual), Some(expected)) => {
                    diff_values(&expected, &actual, &mut Vec::new(), &mut push)
                }
                _ if before != after => push(Vec::new(), DriftChange::Modified),
                _ => {}
            }
        }
    }
}

fn diff_values(
    expected: &Value,
    actual: &Value,
    prefix: &mut Vec<String>,
    push: &mut impl FnMut(Vec<String>, DriftChange),
) {
    match (expected, actual) {
        (Value::Object(expected), Value::Object(actual)) if prefix.len() < MAX_DIFF_DEPTH => {
            for (key, actual_value) in actual {
                prefix.push(key.clone());
                match expected.get(key) {
                    None => push(prefix.clone(), DriftChange::Added),
                    Some(expected_value) => diff_values(expected_value, actual_value, prefix, push),
                }
                prefix.pop();
            }
            for key in expected.keys().filter(|key| !actual.contains_key(*key)) {
                prefix.push(key.clone());
                push(prefix.clone(), DriftChange::Removed);
                prefix.pop();
            }
        }
        _ if expected != actual => push(prefix.clone(), DriftChange::Modified),
        _ => {}
    }
}

/// 按扩展名把 live 文件解析为 JSON 结构（JSON / TOML / `.env`）；不认识的格式返回 `None`
fn parse_structured(path: &Path, text: &str) -> Option<Value> {
    let text = text.trim_start_matches('\u{feff}');
    if path.file_name().and_then(|name| name.to_str()) == Some(".env") {
        let env = crate::gemini_config::parse_env_file(text);
        return Some(Value::Object(
            env.into_iter()
                .map(|(key, value)| (key, Value::String(value)))
                .collect(),
        ));
    }
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("json") => serde_json::from_str(text).ok(),
        Some("toml") => toml::from_str::<toml::Value>(text)
            .ok()
            .and_then(|value| serde_json::to_value(value).ok()),
        _ => None,
    }
}

fn value_at<'a>(value: &'a Value, path: &[String]) -> Option<&'a Value> {
    path.iter()
        .try_fold(value, |current, segment| current.get(segment.as_str()))
}

/// live 中可共享的部分（[`crate::apps::CommonSnippet::extract`] 剥掉凭据与供应商
/// 专属字段后的结果）：只有出现在这里的字段才能并入通用配置。
enum FoldSource {
    Json(Map<String, Value>),
    Toml(toml_edit::DocumentMut),
}

impl FoldSource {
    fn from_live(app: &AppType) -> Option<Self> {
        let adapter = crate::apps::adapter(app);
        let common = adapter.common_snippet()?;
        let live = adapter.read_live().ok()?;
        let text = common.extract(&live).ok()?;
        if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&text) {
            return Some(Self::Json(map));
        }
        text.parse::<toml_edit::DocumentMut>().ok().map(Self::Toml)
    }

    fn contains(&self, path: &[String]) -> bool {
        let Some((first, rest)) = path.split_first() else {
            return false;
        };
        match self {
            Self::Json(map) => map
                .get(first)
                .is_some_and(|value| value_at(value, rest).is_some()),
            Self::Toml(doc) => {
                let mut item = doc.as_item().get(first.as_str());
                for segment in rest {
                    item = item.and_then(|item| item.get(segment.as_str()));
                }
                item.is_some_and(|item| !item.is_none())
            }
        }
    }

    /// 把 `paths` 处的 live 值写进现有片段（同格式），返回新片段文本
    fn merge_into(&self, snippet: &str, paths: &[Vec<String>]) -> Result<String, AppError> {
        if let Some(path) = paths.iter().find(|path| !self.contains(path)) {
            return Err(AppError::localized(
                "drift.fold.field_not_foldable",
                format!("字段 {} 不能并入通用配置", path.join(".")),
                format!("Field {} cannot join the common config", path.join(".")),
            ));
        }

        match self {
            Self::Json(source) => {
                let mut target = if snippet.trim().is_empty() {
                    Map::new()
                } else {
                    match serde_json::from_str::<Value>(snippet) {
                        Ok(Value::Object(map)) => map,
                        _ => {
                            return Err(AppError::Message(
                                "现有通用配置片段不是 JSON 对象".to_string(),
                            ))
                        }
                    }
                };
                let source = Value::Object(source.clone());
                for path in paths {
                    let value = value_at(&source, path).cloned().unwrap_or(Value::Null);
                    set_json_path(&mut target, path, value);
                }
                serde_json::to_string_pretty(&Value::Object(target))
                    .map_err(|source| AppError::JsonSerialize { source })
            }
            Self::Toml(source) => {
                let mut target = snippet
                    .parse::<toml_edit::DocumentMut>()
                    .map_err(|e| AppError::Message(format!("现有通用配置片段 TOML 无效: {e}")))?;
                for path in paths {
                    let mut item = source.as_item();
                    for segment in path {
                        item = &item[segment.as_str()];
                    }
                    let item = item.clone();
                    let mut slot = target.as_item_mut();
                    for segment in path {
                        slot = &mut slot[segment.as_str()];
                    }
                    *slot = item;
                }
                Ok(target.to_string())
            }
        }
    }
}

fn set_json_path(target: &mut Map<String, Value>, path: &[String], value: Value) {
    let Some((last, parents)) = path.split_last() else {
        return;
    };
    let mut current = target;
    for segment in parents {
        let entry = current
            .entry(segment.clone())
            .or_insert_with(|| Value::Object(Map::new()));
        if !entry.is_object() {
            *entry = Value::Object(Map::new());
        }
        current = entry.as_object_mut().expect("just ensured an object");
    }
    current.insert(last.clone(), value);
}

#[cfg(test)]
mod tests;
