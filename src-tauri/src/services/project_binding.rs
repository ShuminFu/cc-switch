//! 目录 → 项目（Profile）绑定（PRODUCT-12 / PRODUCT-63）
//!
//! 用户可以把一个目录（默认规范化为其 git 仓库根）绑定到某个项目。绑定本身
//! 只存放在 cc-switch 数据库的 settings 表（key `project_bindings`，设备本地，
//! 不随同步下载覆盖），**不会**往仓库里写任何标记文件。
//!
//! 应用绑定有两种方式（按分组决定）：
//! - **全局应用**：等同于在该分组上手动切换到绑定的项目（`ProfileService::apply`）。
//! - **项目级应用**（仅在工具本身有文档化的项目级配置机制时可用）：把项目快照
//!   中的供应商环境变量写进绑定目录内的项目级配置文件，全局配置保持不变，
//!   因此不同目录下的终端可以同时使用不同的供应商：
//!   - Claude Code：`<root>/.claude/settings.local.json` 的 `env`
//!     （Claude Code 的"本地项目设置"层，优先级高于用户级 `~/.claude/settings.json`，
//!     并且 Claude Code 默认把它加入 git 忽略）。
//!   - Gemini CLI：`<root>/.gemini/.env`（Gemini CLI 从工作目录向上查找
//!     `.gemini/.env`，先于 `~/.gemini/.env` 生效）。
//!
//!   其余应用没有在现有适配器中使用过的项目级配置机制（Codex 的凭据在
//!   `~/.codex/auth.json`，项目级 `config.toml` 需要信任且不承载凭据；OpenCode /
//!   OpenClaw / Hermes / Grok Build / Claude Desktop 均只有用户级配置），因此只能全局应用。
//!
//! 安全约束：
//! - 只写绑定目录内部的固定相对路径；路径上任何一级是符号链接都拒绝写入，
//!   写入前再次校验目标的规范路径仍位于绑定目录内。
//! - 修改已存在的项目文件前必须先在同目录生成备份（`<file>.ccswitch-backup-<时间戳>`），
//!   备份失败则拒绝写入；内容未变化时不写也不备份。
//! - 只增删 cc-switch 自己写入过的键（记录在绑定的 `managed` 中），文件里的其它
//!   内容原样保留；无法解析的现有文件拒绝改写。

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::database::Database;
use crate::error::AppError;
use crate::services::profile::{ProfilePayload, ProfileScope, ProfileService};
use crate::store::AppState;

/// settings 表中保存绑定列表的 key（设备本地，见 database/backup.rs）
pub const PROJECT_BINDINGS_KEY: &str = "project_bindings";

/// cc-switch 写入项目级配置文件时记录的受管键
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedProjectFile {
    pub scope: ProfileScope,
    /// 相对绑定目录的路径（如 `.claude/settings.local.json`）
    pub relative_path: String,
    /// 本次写入的键（下次写入时先移除不再需要的旧键）
    pub keys: Vec<String>,
}

/// 一条目录绑定
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectBinding {
    /// 规范化后的绝对目录（git 仓库根，或不在仓库中的目录本身）
    pub path: String,
    pub profile_id: String,
    /// 应用时作用的分组；空 = 项目已拍过快照的全部分组
    #[serde(default)]
    pub scopes: Vec<ProfileScope>,
    /// 支持项目级配置的分组是否写项目级文件（而不是切换全局配置）
    #[serde(default)]
    pub project_level: bool,
    #[serde(default)]
    pub managed: Vec<ManagedProjectFile>,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
}

/// "检测当前项目"的结果（CLI / 托盘 / UI 共用）
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedProject {
    /// 被检测的目录（规范化后）
    pub directory: String,
    /// 向上找到的 git 仓库根
    pub git_root: Option<String>,
    /// 命中的绑定（最长前缀匹配）
    pub binding: Option<ProjectBinding>,
    /// 命中绑定对应的项目名称
    pub profile_name: Option<String>,
}

/// 应用绑定的结果
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectApplyReport {
    pub profile_id: String,
    /// 通过全局切换应用的分组（调用方需为它们发事件、刷新托盘）
    pub global_scopes: Vec<ProfileScope>,
    /// 通过项目级文件应用的分组
    pub project_level_scopes: Vec<ProfileScope>,
    /// 写入的项目文件（绝对路径）
    pub written_files: Vec<String>,
    /// 改写前生成的备份文件（绝对路径）
    pub backups: Vec<String>,
    pub warnings: Vec<String>,
    /// 全局应用后建议停止代理（见 `ProfileService::apply`）
    pub should_stop_proxy: bool,
}

/// 项目级配置的落地格式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectFileFormat {
    /// JSON 对象，provider env 合并进顶层 `env` 对象
    JsonEnv,
    /// dotenv 文本，逐行合并
    DotEnv,
}

/// 有文档化项目级配置机制的分组 → (相对路径, 格式)
fn project_level_target(scope: ProfileScope) -> Option<(&'static str, ProjectFileFormat)> {
    match scope {
        ProfileScope::Claude => Some((".claude/settings.local.json", ProjectFileFormat::JsonEnv)),
        ProfileScope::Gemini => Some((".gemini/.env", ProjectFileFormat::DotEnv)),
        _ => None,
    }
}

/// 支持项目级应用的分组列表（供前端展示）
pub fn project_level_scopes() -> Vec<ProfileScope> {
    ProfileScope::ALL
        .into_iter()
        .filter(|scope| project_level_target(*scope).is_some())
        .collect()
}

// ───────────────────────────── 路径 ─────────────────────────────

/// 去掉 Windows `canonicalize` 产生的 `\\?\` 前缀（UNC 路径保持不变）
fn display_path(path: &Path) -> String {
    let s = path.to_string_lossy().to_string();
    #[cfg(windows)]
    {
        if let Some(rest) = s.strip_prefix(r"\\?\") {
            if !rest.starts_with("UNC\\") {
                return rest.to_string();
            }
        }
    }
    s
}

/// 从 `start` 向上查找包含 `.git`（目录，或 worktree/submodule 的 `.git` 文件）的目录
pub fn find_git_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|dir| dir.join(".git").exists())
        .map(Path::to_path_buf)
}

fn canonical_dir(path: &Path) -> Result<PathBuf, AppError> {
    if !path.is_absolute() {
        return Err(AppError::InvalidInput(format!(
            "Project directory must be an absolute path: {}",
            path.display()
        )));
    }
    let canonical = fs::canonicalize(path).map_err(|e| AppError::io(path, e))?;
    if !canonical.is_dir() {
        return Err(AppError::InvalidInput(format!(
            "Project path is not a directory: {}",
            path.display()
        )));
    }
    Ok(canonical)
}

/// 把用户选择的目录规范化为绑定目录：在 git 仓库内时取仓库根，否则取目录本身
pub fn normalize_project_dir(path: &Path) -> Result<PathBuf, AppError> {
    let canonical = canonical_dir(path)?;
    Ok(find_git_root(&canonical).unwrap_or(canonical))
}

/// 解析绑定目录内的固定相对路径，保证结果不会逃出 `root`。
///
/// - 相对路径只能由普通组件构成（拒绝 `..`、绝对路径、盘符）；
/// - 已存在的每一级（含目标文件）都不能是符号链接；
/// - 目标若已存在必须是普通文件。
fn resolve_inside(root: &Path, relative: &str) -> Result<PathBuf, AppError> {
    let root = fs::canonicalize(root).map_err(|e| AppError::io(root, e))?;
    let rel = Path::new(relative);
    let mut current = root.clone();
    for component in rel.components() {
        match component {
            Component::Normal(part) => current.push(part),
            _ => {
                return Err(AppError::InvalidInput(format!(
                    "Refusing project path outside the bound directory: {relative}"
                )))
            }
        }
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(AppError::InvalidInput(format!(
                    "Refusing to write through a symbolic link: {}",
                    current.display()
                )))
            }
            Ok(_) | Err(_) => {}
        }
    }
    if current.exists() && !current.is_file() {
        return Err(AppError::InvalidInput(format!(
            "Project config path exists but is not a regular file: {}",
            current.display()
        )));
    }
    Ok(current)
}

/// 创建父目录后再次确认其规范路径仍在绑定目录内（防御竞争条件下被替换为链接）
fn ensure_parent_inside(root: &Path, target: &Path) -> Result<(), AppError> {
    let root = fs::canonicalize(root).map_err(|e| AppError::io(root, e))?;
    let parent = target
        .parent()
        .ok_or_else(|| AppError::InvalidInput("Project config path has no parent".to_string()))?;
    fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    let canonical_parent = fs::canonicalize(parent).map_err(|e| AppError::io(parent, e))?;
    if !canonical_parent.starts_with(&root) {
        return Err(AppError::InvalidInput(format!(
            "Refusing to write outside the bound directory: {}",
            parent.display()
        )));
    }
    Ok(())
}

/// 在目标同目录生成带时间戳的备份；失败即返回错误（调用方据此拒绝写入）
fn backup_existing(target: &Path) -> Result<PathBuf, AppError> {
    let file_name = target
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "config".to_string());
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let mut backup = target.with_file_name(format!("{file_name}.ccswitch-backup-{stamp}"));
    let mut n = 1;
    while backup.exists() {
        backup = target.with_file_name(format!("{file_name}.ccswitch-backup-{stamp}-{n}"));
        n += 1;
    }
    fs::copy(target, &backup).map_err(|e| AppError::io(&backup, e))?;
    Ok(backup)
}

// ─────────────────────────── 存储 ───────────────────────────

pub fn load_bindings(db: &Database) -> Result<Vec<ProjectBinding>, AppError> {
    match db.get_setting(PROJECT_BINDINGS_KEY)? {
        None => Ok(Vec::new()),
        Some(raw) if raw.trim().is_empty() => Ok(Vec::new()),
        Some(raw) => serde_json::from_str(&raw)
            .map_err(|e| AppError::Config(format!("解析项目目录绑定失败: {e}"))),
    }
}

fn save_bindings(db: &Database, bindings: &[ProjectBinding]) -> Result<(), AppError> {
    let raw = serde_json::to_string(bindings)
        .map_err(|e| AppError::Config(format!("序列化项目目录绑定失败: {e}")))?;
    db.set_setting(PROJECT_BINDINGS_KEY, &raw)
}

/// 新建或替换某目录的绑定（目录会被规范化为 git 仓库根）
pub fn bind(
    db: &Database,
    path: &Path,
    profile_id: &str,
    scopes: Vec<ProfileScope>,
    project_level: bool,
) -> Result<ProjectBinding, AppError> {
    if db.get_profile(profile_id)?.is_none() {
        return Err(AppError::InvalidInput(format!(
            "Profile not found: {profile_id}"
        )));
    }
    let dir = display_path(&normalize_project_dir(path)?);
    let mut scopes = scopes;
    scopes.sort_by_key(|s| s.as_str());
    scopes.dedup();

    let now = chrono::Utc::now().timestamp();
    let mut bindings = load_bindings(db)?;
    let binding = match bindings.iter_mut().find(|b| b.path == dir) {
        Some(existing) => {
            // 换了项目时保留 managed：旧项目写入的键在下次写入时会被移除
            existing.profile_id = profile_id.to_string();
            existing.scopes = scopes;
            existing.project_level = project_level;
            existing.updated_at = now;
            existing.clone()
        }
        None => {
            let binding = ProjectBinding {
                path: dir,
                profile_id: profile_id.to_string(),
                scopes,
                project_level,
                managed: Vec::new(),
                created_at: now,
                updated_at: now,
            };
            bindings.push(binding.clone());
            binding
        }
    };
    bindings.sort_by(|a, b| a.path.cmp(&b.path));
    save_bindings(db, &bindings)?;
    Ok(binding)
}

/// 删除绑定（只删记录，不改动项目目录里的文件）；返回是否删除了记录
pub fn unbind(db: &Database, path: &str) -> Result<bool, AppError> {
    let mut bindings = load_bindings(db)?;
    let before = bindings.len();
    // 允许传入未规范化的路径：能规范化就按规范路径匹配，否则按原样匹配
    let normalized = normalize_project_dir(Path::new(path))
        .map(|p| display_path(&p))
        .ok();
    bindings.retain(|b| b.path != path && Some(&b.path) != normalized.as_ref());
    let removed = bindings.len() != before;
    if removed {
        save_bindings(db, &bindings)?;
    }
    Ok(removed)
}

/// 删除项目时一并移除指向它的绑定
pub fn remove_bindings_for_profile(db: &Database, profile_id: &str) -> Result<(), AppError> {
    let mut bindings = load_bindings(db)?;
    let before = bindings.len();
    bindings.retain(|b| b.profile_id != profile_id);
    if bindings.len() != before {
        save_bindings(db, &bindings)?;
    }
    Ok(())
}

/// 在绑定列表中为 `dir` 找最具体（路径最长）的祖先绑定
pub fn find_binding_for<'a>(
    bindings: &'a [ProjectBinding],
    dir: &Path,
) -> Option<&'a ProjectBinding> {
    bindings
        .iter()
        .filter(|b| dir.starts_with(Path::new(&b.path)))
        .max_by_key(|b| Path::new(&b.path).components().count())
}

/// 检测某目录所属的项目绑定（CLI、托盘和 UI 共用的"检测当前项目"入口）
pub fn detect(db: &Database, dir: &Path) -> Result<DetectedProject, AppError> {
    let canonical = canonical_dir(dir)?;
    let shown = PathBuf::from(display_path(&canonical));
    let bindings = load_bindings(db)?;
    let binding = find_binding_for(&bindings, &shown).cloned();
    let profile_name = match &binding {
        Some(b) => db.get_profile(&b.profile_id)?.map(|p| p.name),
        None => None,
    };
    Ok(DetectedProject {
        directory: display_path(&canonical),
        git_root: find_git_root(&canonical).map(|p| display_path(&p)),
        binding,
        profile_name,
    })
}

// ─────────────────────────── 应用 ───────────────────────────

fn effective_scopes(binding: &ProjectBinding, payload: &ProfilePayload) -> Vec<ProfileScope> {
    let candidates: Vec<ProfileScope> = if binding.scopes.is_empty() {
        ProfileScope::ALL.to_vec()
    } else {
        binding.scopes.clone()
    };
    candidates
        .into_iter()
        .filter(|scope| payload.scope_captured(*scope))
        .collect()
}

/// 从项目快照取某分组供应商的 env（字符串值）
fn provider_env_for_scope(
    state: &AppState,
    payload: &ProfilePayload,
    scope: ProfileScope,
) -> Result<Option<BTreeMap<String, String>>, AppError> {
    let Some(app) = scope.apps().first() else {
        return Ok(None);
    };
    let Some(Some(provider_id)) = payload.providers.get(app) else {
        return Ok(None);
    };
    let providers = state.db.get_all_providers(app.as_str())?;
    let Some(provider) = providers.get(provider_id) else {
        return Err(AppError::InvalidInput(format!(
            "provider '{provider_id}' no longer exists"
        )));
    };
    let env = provider
        .settings_config
        .get("env")
        .and_then(Value::as_object)
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    Ok(Some(env))
}

/// 合并 JSON 项目设置：只增删受管键，其余内容保持不变；返回新内容（None = 无变化）
fn merge_json_env(
    existing: Option<&str>,
    env: &BTreeMap<String, String>,
    previously_managed: &[String],
) -> Result<Option<String>, AppError> {
    let mut root: Value = match existing {
        Some(text) if !text.trim().is_empty() => serde_json::from_str(text).map_err(|e| {
            AppError::Config(format!(
                "existing project settings are not valid JSON, refusing to modify: {e}"
            ))
        })?,
        _ => Value::Object(Default::default()),
    };
    let original = root.clone();
    let obj = root.as_object_mut().ok_or_else(|| {
        AppError::Config("existing project settings are not a JSON object".to_string())
    })?;
    let env_value = obj
        .entry("env")
        .or_insert_with(|| Value::Object(Default::default()));
    let env_obj = env_value.as_object_mut().ok_or_else(|| {
        AppError::Config("existing project settings `env` is not an object".to_string())
    })?;
    for key in previously_managed {
        if !env.contains_key(key) {
            env_obj.remove(key);
        }
    }
    for (k, v) in env {
        env_obj.insert(k.clone(), Value::String(v.clone()));
    }
    if env_obj.is_empty() {
        obj.remove("env");
    }
    let unchanged = match existing {
        Some(_) => root == original,
        // 文件不存在且没有要写的键：不创建空文件
        None => root.as_object().is_some_and(|o| o.is_empty()),
    };
    if unchanged {
        return Ok(None);
    }
    let mut text =
        serde_json::to_string_pretty(&root).map_err(|e| AppError::JsonSerialize { source: e })?;
    text.push('\n');
    Ok(Some(text))
}

/// 合并 dotenv：替换/删除受管键所在行，保留注释与其它行；返回新内容（None = 无变化）
fn merge_dotenv(
    existing: Option<&str>,
    env: &BTreeMap<String, String>,
    previously_managed: &[String],
) -> Option<String> {
    let managed: BTreeSet<&str> = previously_managed
        .iter()
        .map(String::as_str)
        .chain(env.keys().map(String::as_str))
        .collect();
    let mut written = BTreeSet::new();
    let mut lines = Vec::new();
    for line in existing.unwrap_or_default().lines() {
        let key = line
            .trim_start()
            .strip_prefix("export ")
            .unwrap_or(line.trim_start())
            .split('=')
            .next()
            .map(str::trim)
            .unwrap_or_default();
        if !line.trim_start().starts_with('#') && line.contains('=') && managed.contains(key) {
            if let Some(value) = env.get(key) {
                if written.insert(key.to_string()) {
                    lines.push(format!("{key}={value}"));
                }
            }
            continue;
        }
        lines.push(line.to_string());
    }
    for (k, v) in env {
        if !written.contains(k) {
            lines.push(format!("{k}={v}"));
        }
    }
    let mut text = lines.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    if existing.map(|e| e == text).unwrap_or(text.is_empty()) {
        return None;
    }
    Some(text)
}

/// 把某分组的供应商 env 写进绑定目录的项目级配置文件
fn write_project_level_scope(
    state: &AppState,
    root: &Path,
    binding: &mut ProjectBinding,
    payload: &ProfilePayload,
    scope: ProfileScope,
    report: &mut ProjectApplyReport,
) -> Result<(), AppError> {
    let Some((relative, format)) = project_level_target(scope) else {
        return Err(AppError::InvalidInput(format!(
            "{} has no project-level configuration",
            scope.as_str()
        )));
    };
    let Some(env) = provider_env_for_scope(state, payload, scope)? else {
        report.warnings.push(format!(
            "[{}] the project has no provider saved for this app; project file not written",
            scope.as_str()
        ));
        return Ok(());
    };

    let target = resolve_inside(root, relative)?;
    let previously_managed: Vec<String> = binding
        .managed
        .iter()
        .find(|m| m.scope == scope && m.relative_path == relative)
        .map(|m| m.keys.clone())
        .unwrap_or_default();
    let existing = if target.exists() {
        Some(fs::read_to_string(&target).map_err(|e| AppError::io(&target, e))?)
    } else {
        None
    };
    let merged = match format {
        ProjectFileFormat::JsonEnv => {
            merge_json_env(existing.as_deref(), &env, &previously_managed)?
        }
        ProjectFileFormat::DotEnv => merge_dotenv(existing.as_deref(), &env, &previously_managed),
    };

    if let Some(content) = merged {
        ensure_parent_inside(root, &target)?;
        if existing.is_some() {
            let backup = backup_existing(&target).map_err(|e| {
                AppError::Message(format!(
                    "refusing to overwrite {} because its backup failed: {e}",
                    target.display()
                ))
            })?;
            report.backups.push(display_path(&backup));
        }
        crate::config::atomic_write_secret(&target, content.as_bytes())?;
        report.written_files.push(display_path(&target));
    }

    binding
        .managed
        .retain(|m| !(m.scope == scope && m.relative_path == relative));
    if !env.is_empty() {
        binding.managed.push(ManagedProjectFile {
            scope,
            relative_path: relative.to_string(),
            keys: env.keys().cloned().collect(),
        });
    }
    if matches!(scope, ProfileScope::Gemini) {
        report.warnings.push(format!(
            "[gemini] {relative} contains your API key; make sure it is listed in .gitignore"
        ));
    }
    Ok(())
}

/// 检测 `dir` 所属绑定并应用它。
///
/// 支持项目级配置的分组在 `project_level` 打开时写项目文件，其余分组走
/// 全局 `ProfileService::apply`。没有绑定时返回 `Ok(None)`。
pub fn apply_for_directory(
    state: &AppState,
    dir: &Path,
) -> Result<Option<ProjectApplyReport>, AppError> {
    let detected = detect(&state.db, dir)?;
    let Some(mut binding) = detected.binding else {
        return Ok(None);
    };
    let profile = state.db.get_profile(&binding.profile_id)?.ok_or_else(|| {
        AppError::InvalidInput(format!("Profile not found: {}", binding.profile_id))
    })?;
    let payload: ProfilePayload = serde_json::from_str(&profile.payload)
        .map_err(|e| AppError::Config(format!("解析 profile payload 失败: {e}")))?;

    let mut report = ProjectApplyReport {
        profile_id: binding.profile_id.clone(),
        ..Default::default()
    };
    let scopes = effective_scopes(&binding, &payload);
    if scopes.is_empty() {
        report.warnings.push(
            "the bound project has no saved configuration for the selected apps; nothing was changed"
                .to_string(),
        );
    }
    let root = PathBuf::from(&binding.path);
    for scope in scopes {
        if binding.project_level && project_level_target(scope).is_some() {
            match write_project_level_scope(
                state,
                &root,
                &mut binding,
                &payload,
                scope,
                &mut report,
            ) {
                Ok(()) => report.project_level_scopes.push(scope),
                Err(e) => report.warnings.push(format!("[{}] {e}", scope.as_str())),
            }
        } else {
            match ProfileService::apply(state, &binding.profile_id, scope) {
                Ok((warnings, stop)) => {
                    report.warnings.extend(warnings);
                    report.should_stop_proxy = stop;
                    report.global_scopes.push(scope);
                }
                Err(e) => report.warnings.push(format!("[{}] {e}", scope.as_str())),
            }
        }
    }

    // 记录受管键（只更新这一条绑定，避免覆盖并发修改的其它绑定）
    let mut bindings = load_bindings(&state.db)?;
    if let Some(stored) = bindings.iter_mut().find(|b| b.path == binding.path) {
        stored.managed = binding.managed.clone();
        save_bindings(&state.db, &bindings)?;
    }
    Ok(Some(report))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn git_root_is_found_from_nested_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::create_dir_all(repo.join("src/deep")).unwrap();
        assert_eq!(
            find_git_root(&repo.join("src/deep")).as_deref(),
            Some(repo.as_path())
        );
        // worktree：.git 是文件
        let wt = tmp.path().join("wt");
        fs::create_dir_all(&wt).unwrap();
        fs::write(wt.join(".git"), "gitdir: /elsewhere\n").unwrap();
        assert_eq!(find_git_root(&wt).as_deref(), Some(wt.as_path()));
    }

    #[test]
    fn normalize_rejects_relative_and_missing_paths() {
        assert!(normalize_project_dir(Path::new("relative/dir")).is_err());
        let tmp = tempfile::tempdir().unwrap();
        assert!(normalize_project_dir(&tmp.path().join("missing")).is_err());
        let file = tmp.path().join("file.txt");
        fs::write(&file, "x").unwrap();
        assert!(normalize_project_dir(&file).is_err());
    }

    #[test]
    fn longest_prefix_binding_wins() {
        let mk = |path: &str, id: &str| ProjectBinding {
            path: path.to_string(),
            profile_id: id.to_string(),
            scopes: vec![],
            project_level: false,
            managed: vec![],
            created_at: 0,
            updated_at: 0,
        };
        let bindings = vec![mk("/work", "outer"), mk("/work/api", "inner")];
        assert_eq!(
            find_binding_for(&bindings, Path::new("/work/api/src"))
                .unwrap()
                .profile_id,
            "inner"
        );
        assert_eq!(
            find_binding_for(&bindings, Path::new("/work/blog"))
                .unwrap()
                .profile_id,
            "outer"
        );
        // 组件级前缀：/work-other 不属于 /work
        assert!(find_binding_for(&bindings, Path::new("/work-other")).is_none());
    }

    #[test]
    fn resolve_inside_rejects_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(resolve_inside(tmp.path(), "../escape.json").is_err());
        assert!(resolve_inside(tmp.path(), "/etc/passwd").is_err());
        let ok = resolve_inside(tmp.path(), ".claude/settings.local.json").unwrap();
        assert!(ok.starts_with(fs::canonicalize(tmp.path()).unwrap()));
    }

    #[cfg(unix)]
    #[test]
    fn resolve_inside_rejects_symlinked_config_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        let outside = tmp.path().join("outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, root.join(".claude")).unwrap();
        let err = resolve_inside(&root, ".claude/settings.local.json").unwrap_err();
        assert!(err.to_string().contains("symbolic link"), "{err}");

        // 目标文件本身是链接也拒绝
        fs::create_dir_all(root.join(".gemini")).unwrap();
        std::os::unix::fs::symlink(outside.join("x"), root.join(".gemini/.env")).unwrap();
        assert!(resolve_inside(&root, ".gemini/.env").is_err());
    }

    #[test]
    fn json_merge_preserves_other_settings_and_drops_stale_managed_keys() {
        let existing = r#"{"permissions":{"allow":["Bash(ls)"]},"env":{"USER_VAR":"keep","ANTHROPIC_MODEL":"old"}}"#;
        let merged = merge_json_env(
            Some(existing),
            &env(&[
                ("ANTHROPIC_BASE_URL", "https://b"),
                ("ANTHROPIC_AUTH_TOKEN", "t"),
            ]),
            &["ANTHROPIC_MODEL".to_string()],
        )
        .unwrap()
        .expect("changed");
        let v: Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(v["permissions"]["allow"][0], "Bash(ls)");
        assert_eq!(v["env"]["USER_VAR"], "keep");
        assert_eq!(v["env"]["ANTHROPIC_BASE_URL"], "https://b");
        assert!(v["env"].get("ANTHROPIC_MODEL").is_none());

        // 再次合并相同内容：无变化
        assert!(merge_json_env(
            Some(&merged),
            &env(&[
                ("ANTHROPIC_BASE_URL", "https://b"),
                ("ANTHROPIC_AUTH_TOKEN", "t")
            ]),
            &[]
        )
        .unwrap()
        .is_none());

        // 无法解析的文件拒绝改写
        assert!(merge_json_env(Some("{not json"), &env(&[("A", "b")]), &[]).is_err());
    }

    #[test]
    fn dotenv_merge_keeps_comments_and_unmanaged_lines() {
        let existing = "# my settings\nDEBUG=1\nGEMINI_API_KEY=old\nGEMINI_MODEL=stale\n";
        let merged = merge_dotenv(
            Some(existing),
            &env(&[
                ("GEMINI_API_KEY", "new"),
                ("GOOGLE_GEMINI_BASE_URL", "https://g"),
            ]),
            &["GEMINI_MODEL".to_string()],
        )
        .expect("changed");
        assert_eq!(
            merged,
            "# my settings\nDEBUG=1\nGEMINI_API_KEY=new\nGOOGLE_GEMINI_BASE_URL=https://g\n"
        );
        assert!(merge_dotenv(
            Some(&merged),
            &env(&[
                ("GEMINI_API_KEY", "new"),
                ("GOOGLE_GEMINI_BASE_URL", "https://g")
            ]),
            &[]
        )
        .is_none());
    }

    #[test]
    fn backup_is_created_next_to_the_file() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("settings.local.json");
        fs::write(&file, "{}").unwrap();
        let b1 = backup_existing(&file).unwrap();
        let b2 = backup_existing(&file).unwrap();
        assert_ne!(b1, b2, "backups never overwrite each other");
        assert_eq!(b1.parent(), file.parent());
        assert_eq!(fs::read_to_string(b1).unwrap(), "{}");
    }

    #[test]
    fn project_level_is_offered_only_where_a_mechanism_exists() {
        assert_eq!(
            project_level_scopes(),
            vec![ProfileScope::Claude, ProfileScope::Gemini]
        );
        assert!(project_level_target(ProfileScope::Codex).is_none());
        assert!(project_level_target(ProfileScope::ClaudeDesktop).is_none());
    }

    #[test]
    fn bindings_roundtrip_and_follow_profile_deletion() -> Result<(), AppError> {
        let db = Database::memory()?;
        db.save_profile(&crate::database::Profile {
            id: "p1".into(),
            name: "Work".into(),
            payload: "{}".into(),
            sort_order: None,
            created_at: None,
            updated_at: None,
        })?;
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::create_dir_all(repo.join("pkg")).unwrap();

        // 绑定子目录 → 规范化到仓库根
        let b = bind(&db, &repo.join("pkg"), "p1", vec![], true)?;
        assert_eq!(
            PathBuf::from(&b.path),
            PathBuf::from(display_path(&fs::canonicalize(&repo).unwrap()))
        );
        assert!(bind(&db, &repo, "ghost", vec![], false).is_err());

        let detected = detect(&db, &repo.join("pkg"))?;
        assert_eq!(detected.profile_name.as_deref(), Some("Work"));
        assert_eq!(detected.git_root.as_deref(), Some(b.path.as_str()));

        // 重新绑定同一目录替换而非追加
        bind(&db, &repo, "p1", vec![ProfileScope::Codex], false)?;
        assert_eq!(load_bindings(&db)?.len(), 1);

        remove_bindings_for_profile(&db, "p1")?;
        assert!(load_bindings(&db)?.is_empty());
        assert!(detect(&db, &repo)?.binding.is_none());
        Ok(())
    }
}
