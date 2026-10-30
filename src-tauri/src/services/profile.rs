//! 项目 Profile 编排服务
//!
//! Profile 是**全应用共享的项目实体**（用户拥有的项目就那几个），payload
//! 按 app 分槽存配置快照（供应商 / MCP / Skills / Prompt）。快照与应用
//! 均**按分组（scope）操作**：Claude Code 与 Codex 的工作目录往往不同
//! （各在各的项目里），因此各组独立指向自己的当前项目、只拍/只应用组内
//! 槽位，互不牵连；重命名/删除作用于共享实体本身。
//! 应用（apply）时复用现有切换原语批量落地：
//! - 供应商：`ProviderService::switch`（内建代理接管热切换与接管下禁切官方）
//! - MCP：`McpService::toggle_app`（改标志 + 单 server 物化）
//! - Skills：`SkillService::toggle_app`（改标志 + 单 skill 物化）
//! - Prompt：`PromptService::enable_prompt`（互斥激活 + 原子写 live）
//!
//! apply 为 best-effort：单项失败收集为 warning 继续，不整体回滚。
//!
//! **基线（baseline）**：从"不使用项目"首次应用项目时，先把分组当前状态拍成
//! 设备本地的基线快照；之后选择"不使用项目"会恢复该基线（PRODUCT-68），
//! 因此首次应用项目不会无声丢失未保存的工作配置。

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::app_config::AppType;
use crate::database::Profile;
use crate::error::AppError;
use crate::services::{McpService, PromptService, ProviderService, SkillService};
use crate::store::AppState;

/// Profile 操作的应用分组：项目实体全应用共享，但快照/应用/当前指针按组进行。
///
/// Claude Code 与 Claude Desktop 的供应商在 cc-switch 中是独立切换的，
/// 因此各自拥有独立的项目分组。两者 live 文件零交集
///（`~/.claude` / `Application Support/Claude-3p`），分组切换互不干扰。
///
/// 每个应用一个分组（PRODUCT-17）。各应用在快照中支持的维度不同，见
/// [`ProfileScope::captures_provider`] / [`ProfileScope::captures_mcp`] /
/// [`ProfileScope::captures_skills`]：
/// - Claude Code / Codex / Gemini / Grok Build：供应商 + MCP + Skills + Prompt
/// - Claude Desktop：供应商
/// - OpenCode / Hermes：MCP + Skills + Prompt（叠加模式应用没有"当前供应商"）
/// - OpenClaw：仅 Prompt（不支持 MCP / Skills，且为叠加模式）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProfileScope {
    #[serde(rename = "claude")]
    Claude,
    #[serde(rename = "claude-desktop")]
    ClaudeDesktop,
    #[serde(rename = "codex")]
    Codex,
    #[serde(rename = "gemini")]
    Gemini,
    #[serde(rename = "grokbuild")]
    GrokBuild,
    #[serde(rename = "opencode")]
    OpenCode,
    #[serde(rename = "openclaw")]
    OpenClaw,
    #[serde(rename = "hermes")]
    Hermes,
}

impl ProfileScope {
    /// 全部分组（扩展新分组时同步扩展 apps/for_app 与前端 scope.ts 镜像）
    pub const ALL: [ProfileScope; 8] = [
        ProfileScope::Claude,
        ProfileScope::ClaudeDesktop,
        ProfileScope::Codex,
        ProfileScope::Gemini,
        ProfileScope::GrokBuild,
        ProfileScope::OpenCode,
        ProfileScope::OpenClaw,
        ProfileScope::Hermes,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            ProfileScope::Claude => "claude",
            ProfileScope::ClaudeDesktop => "claude-desktop",
            ProfileScope::Codex => "codex",
            ProfileScope::Gemini => "gemini",
            ProfileScope::GrokBuild => "grokbuild",
            ProfileScope::OpenCode => "opencode",
            ProfileScope::OpenClaw => "openclaw",
            ProfileScope::Hermes => "hermes",
        }
    }

    /// 产品名（托盘子菜单标签用，不进 i18n）
    pub fn label(&self) -> &'static str {
        match self {
            ProfileScope::Claude => "Claude Code",
            ProfileScope::ClaudeDesktop => "Claude Desktop",
            ProfileScope::Codex => "Codex",
            ProfileScope::Gemini => "Gemini",
            ProfileScope::GrokBuild => "Grok Build",
            ProfileScope::OpenCode => "OpenCode",
            ProfileScope::OpenClaw => "OpenClaw",
            ProfileScope::Hermes => "Hermes",
        }
    }

    pub fn parse(value: &str) -> Result<Self, AppError> {
        Self::ALL
            .into_iter()
            .find(|scope| scope.as_str() == value)
            .ok_or_else(|| AppError::InvalidInput(format!("Unknown profile scope: {value}")))
    }

    /// 组内受管应用（快照与 apply 只作用于这些 app 的槽位）
    pub fn apps(&self) -> &'static [AppType] {
        match self {
            ProfileScope::Claude => &[AppType::Claude],
            ProfileScope::ClaudeDesktop => &[AppType::ClaudeDesktop],
            ProfileScope::Codex => &[AppType::Codex],
            ProfileScope::Gemini => &[AppType::Gemini],
            ProfileScope::GrokBuild => &[AppType::GrokBuild],
            ProfileScope::OpenCode => &[AppType::OpenCode],
            ProfileScope::OpenClaw => &[AppType::OpenClaw],
            ProfileScope::Hermes => &[AppType::Hermes],
        }
    }

    /// 应用页 → 所属分组（每个应用都有自己的分组）
    pub fn for_app(app: &AppType) -> Option<Self> {
        Some(match app {
            AppType::Claude => ProfileScope::Claude,
            AppType::ClaudeDesktop => ProfileScope::ClaudeDesktop,
            AppType::Codex => ProfileScope::Codex,
            AppType::Gemini => ProfileScope::Gemini,
            AppType::GrokBuild => ProfileScope::GrokBuild,
            AppType::OpenCode => ProfileScope::OpenCode,
            AppType::OpenClaw => ProfileScope::OpenClaw,
            AppType::Hermes => ProfileScope::Hermes,
        })
    }

    /// 快照是否记录该应用的"当前供应商"。
    ///
    /// 叠加模式应用（OpenCode / OpenClaw / Hermes）把所有供应商同时写进 live，
    /// 没有"当前供应商"的概念；在这些应用上调用 `ProviderService::switch`
    /// 会把供应商追加进 live 配置，不适合由项目快照驱动。
    pub fn captures_provider(app: &AppType) -> bool {
        !app.is_additive_mode()
    }

    /// 快照是否记录该应用的 MCP 启用集合（OpenClaw 不支持 MCP）
    pub fn captures_mcp(app: &AppType) -> bool {
        !matches!(app, AppType::OpenClaw)
    }

    /// 快照是否记录该应用的 Skills 启用集合（OpenClaw 不支持 Skills）
    pub fn captures_skills(app: &AppType) -> bool {
        !matches!(app, AppType::OpenClaw)
    }

    /// apply 前是否需要关闭该应用的代理接管（叠加模式应用没有接管）
    fn has_takeover(app: &AppType) -> bool {
        !app.is_additive_mode()
    }
}

/// 按 app 分槽的载荷容器；字段名与 AppType 的 serde 形式一致
///
/// 旧版本只写 claude / claude-desktop / codex 三个 key；缺失的 key 反序列化为
/// 默认值（`None` = 该侧从未拍过快照），因此新增应用槽位向后兼容。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PerApp<T> {
    pub claude: T,
    #[serde(rename = "claude-desktop")]
    pub claude_desktop: T,
    pub codex: T,
    pub gemini: T,
    pub grokbuild: T,
    pub opencode: T,
    pub openclaw: T,
    pub hermes: T,
}

impl<T> PerApp<T> {
    pub fn get(&self, app: &AppType) -> Option<&T> {
        Some(match app {
            AppType::Claude => &self.claude,
            AppType::ClaudeDesktop => &self.claude_desktop,
            AppType::Codex => &self.codex,
            AppType::Gemini => &self.gemini,
            AppType::GrokBuild => &self.grokbuild,
            AppType::OpenCode => &self.opencode,
            AppType::OpenClaw => &self.openclaw,
            AppType::Hermes => &self.hermes,
        })
    }

    pub fn get_mut(&mut self, app: &AppType) -> Option<&mut T> {
        Some(match app {
            AppType::Claude => &mut self.claude,
            AppType::ClaudeDesktop => &mut self.claude_desktop,
            AppType::Codex => &mut self.codex,
            AppType::Gemini => &mut self.gemini,
            AppType::GrokBuild => &mut self.grokbuild,
            AppType::OpenCode => &mut self.opencode,
            AppType::OpenClaw => &mut self.openclaw,
            AppType::Hermes => &mut self.hermes,
        })
    }
}

/// Profile 的 JSON 快照结构（与前端 TS 类型严格对应）
///
/// 所有槽位都是 Option：None = 该侧从未拍过快照（应用时不动），
/// 与"拍到的就是空集/无激活项"（Some(空)，应用时清空启用）严格区分——
/// 在 Codex 页选中一个只在 Claude 页建过的项目不能误清 Codex 的启用状态。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProfilePayload {
    /// 每 app 的当前供应商 id
    pub providers: PerApp<Option<String>>,
    /// 每 app 启用的 MCP server id 集合
    pub mcp: PerApp<Option<Vec<String>>>,
    /// 每 app 启用的 Skill id 集合
    pub skills: PerApp<Option<Vec<String>>>,
    /// 每 app 激活的 prompt id
    pub prompts: PerApp<Option<String>>,
}

impl ProfilePayload {
    /// 用另一份快照覆盖本载荷中某分组的槽位，其余分组原样保留
    /// （"以当前状态更新"只更新发起页所属分组，避免把别的应用
    /// 正处于其他项目的状态串进来）
    pub fn merge_scope_from(&mut self, other: &ProfilePayload, scope: ProfileScope) {
        for app in scope.apps() {
            if let (Some(dst), Some(src)) = (self.providers.get_mut(app), other.providers.get(app))
            {
                *dst = src.clone();
            }
            if let (Some(dst), Some(src)) = (self.mcp.get_mut(app), other.mcp.get(app)) {
                *dst = src.clone();
            }
            if let (Some(dst), Some(src)) = (self.skills.get_mut(app), other.skills.get(app)) {
                *dst = src.clone();
            }
            if let (Some(dst), Some(src)) = (self.prompts.get_mut(app), other.prompts.get(app)) {
                *dst = src.clone();
            }
        }
    }

    /// 某分组是否拍过快照（任一槽位非 None 即视为拍过）
    pub fn scope_captured(&self, scope: ProfileScope) -> bool {
        scope.apps().iter().any(|app| {
            self.providers.get(app).is_some_and(|s| s.is_some())
                || self.mcp.get(app).is_some_and(|s| s.is_some())
                || self.skills.get(app).is_some_and(|s| s.is_some())
                || self.prompts.get(app).is_some_and(|s| s.is_some())
        })
    }
}

/// 计算从当前启用状态到目标集合的最小 toggle 集
///
/// 返回 (需要执行的 (id, enabled) 列表, payload 中已不存在于 DB 的悬空 id 列表)
fn plan_toggles(
    current: &[(String, bool)],
    target_ids: &[String],
) -> (Vec<(String, bool)>, Vec<String>) {
    let existing: HashSet<&str> = current.iter().map(|(id, _)| id.as_str()).collect();
    let target: HashSet<&str> = target_ids.iter().map(|s| s.as_str()).collect();

    let toggles = current
        .iter()
        .filter(|(id, enabled)| target.contains(id.as_str()) != *enabled)
        .map(|(id, enabled)| (id.clone(), !enabled))
        .collect();

    let dangling = target_ids
        .iter()
        .filter(|id| !existing.contains(id.as_str()))
        .cloned()
        .collect();

    (toggles, dangling)
}

fn parse_payload(raw: &str) -> Result<ProfilePayload, AppError> {
    serde_json::from_str(raw)
        .map_err(|e| AppError::Config(format!("解析 profile payload 失败: {e}")))
}

fn serialize_payload(payload: &ProfilePayload) -> Result<String, AppError> {
    serde_json::to_string(payload)
        .map_err(|e| AppError::Config(format!("序列化 profile payload 失败: {e}")))
}

/// Claude Desktop 当前供应商是否走本地路由（代理模式）。
///
/// Claude Desktop 的本地路由不是"接管"，`proxy_config.enabled` 里没有它的
/// 记录；但它依赖代理服务运行，因此 apply 后不能据"无接管"就停代理（PRODUCT-69）。
fn claude_desktop_uses_local_routing(state: &AppState) -> bool {
    let app = AppType::ClaudeDesktop;
    let Ok(Some(current)) = crate::settings::get_effective_current_provider(&state.db, &app) else {
        return false;
    };
    state
        .db
        .get_all_providers(app.as_str())
        .ok()
        .and_then(|providers| providers.get(&current).cloned())
        .is_some_and(|provider| {
            crate::claude_desktop_config::provider_mode(&provider)
                == crate::provider::ClaudeDesktopMode::Proxy
        })
}

/// apply / clear 之后是否建议停止代理服务：既没有任何应用处于接管，
/// 也没有 Claude Desktop 本地路由依赖代理。
fn should_stop_proxy_after_apply(state: &AppState) -> bool {
    !state.db.is_live_takeover_active_sync() && !claude_desktop_uses_local_routing(state)
}

pub struct ProfileService;

impl ProfileService {
    /// 抓取分组内应用的当前配置状态生成快照（组外槽位保持默认值）
    pub fn snapshot_current(
        state: &AppState,
        scope: ProfileScope,
    ) -> Result<ProfilePayload, AppError> {
        let mut payload = ProfilePayload::default();
        let mcp_servers = state.db.get_all_mcp_servers()?;
        let skills = state.db.get_all_installed_skills()?;

        for app in scope.apps().iter() {
            if ProfileScope::captures_provider(app) {
                if let Some(slot) = payload.providers.get_mut(app) {
                    *slot = crate::settings::get_effective_current_provider(&state.db, app)?;
                }
            }
            if ProfileScope::captures_mcp(app) {
                if let Some(slot) = payload.mcp.get_mut(app) {
                    *slot = Some(
                        mcp_servers
                            .values()
                            .filter(|s| s.apps.is_enabled_for(app))
                            .map(|s| s.id.clone())
                            .collect(),
                    );
                }
            }
            if ProfileScope::captures_skills(app) {
                if let Some(slot) = payload.skills.get_mut(app) {
                    *slot = Some(
                        skills
                            .values()
                            .filter(|s| s.apps.is_enabled_for(app))
                            .map(|s| s.id.clone())
                            .collect(),
                    );
                }
            }
            if let Some(slot) = payload.prompts.get_mut(app) {
                *slot = state
                    .db
                    .get_prompts(app.as_str())?
                    .values()
                    .find(|p| p.enabled)
                    .map(|p| p.id.clone());
            }
        }
        Ok(payload)
    }

    /// 列出所有项目（项目实体全应用共享，current 标记按分组单独读取）
    pub fn list(state: &AppState) -> Result<Vec<Profile>, AppError> {
        state.db.get_all_profiles()
    }

    /// 创建新项目：只拍发起页所属分组的当前状态，其余分组槽位留 None
    /// （其他应用可能正处于别的项目，不能替用户拍进来）
    pub fn create(state: &AppState, name: &str, scope: ProfileScope) -> Result<Profile, AppError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(AppError::InvalidInput("Profile name is empty".to_string()));
        }
        let payload = Self::snapshot_current(state, scope)?;
        let now = chrono::Utc::now().timestamp();
        let profile = Profile {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            payload: serialize_payload(&payload)?,
            sort_order: None,
            created_at: Some(now),
            updated_at: Some(now),
        };
        state.db.save_profile(&profile)?;
        Ok(profile)
    }

    /// 更新项目：重命名（作用于共享实体）和/或以当前状态重拍快照
    /// （resnapshot 只覆盖 scope 分组的槽位，其余分组原样保留；
    /// 快照重拍仅由 [`Self::apply`] 切换前的自动保存触发，UI 不再暴露手动入口）
    pub fn update(
        state: &AppState,
        id: &str,
        name: Option<String>,
        resnapshot: bool,
        scope: Option<ProfileScope>,
    ) -> Result<Profile, AppError> {
        let mut profile = state
            .db
            .get_profile(id)?
            .ok_or_else(|| AppError::InvalidInput(format!("Profile not found: {id}")))?;

        if let Some(name) = name {
            let name = name.trim().to_string();
            if name.is_empty() {
                return Err(AppError::InvalidInput("Profile name is empty".to_string()));
            }
            profile.name = name;
        }
        if resnapshot {
            let scope = scope.ok_or_else(|| {
                AppError::InvalidInput("Resnapshot requires a profile scope".to_string())
            })?;
            let mut payload = parse_payload(&profile.payload)?;
            payload.merge_scope_from(&Self::snapshot_current(state, scope)?, scope);
            profile.payload = serialize_payload(&payload)?;
        }
        profile.updated_at = Some(chrono::Utc::now().timestamp());
        state.db.save_profile(&profile)?;
        Ok(profile)
    }

    /// 删除项目；若删除的是某分组当前激活项目，一并清除该分组的激活标记，
    /// 并移除指向该项目的目录绑定（不再指向一个不存在的项目）
    pub fn delete(state: &AppState, id: &str) -> Result<(), AppError> {
        state.db.delete_profile(id)?;
        for scope in ProfileScope::ALL {
            if state.db.get_current_profile_id(scope.as_str())?.as_deref() == Some(id) {
                state.db.set_current_profile_id(scope.as_str(), None)?;
            }
        }
        if let Err(e) = crate::services::project_binding::remove_bindings_for_profile(&state.db, id)
        {
            log::warn!("删除项目 '{id}' 后清理目录绑定失败: {e}");
        }
        Ok(())
    }

    /// 应用项目快照（best-effort，返回 warnings）
    ///
    /// 只作用于发起页所属分组内的应用，不碰其他分组的配置与 current 标记。
    /// 该分组从未拍过快照时不改动任何配置，仅标记 current 并返回提示
    /// （下次从该项目切走时，自动保存会补拍该侧快照）。
    ///
    /// **切换前会自动保存旧项目**：若当前分组已绑定到另一个项目，先把当前
    /// 状态写入那个旧项目（仅当前分组槽位），再加载目标项目。这样切走后
    /// 旧项目仍保留离开时的配置，回来时状态一致。自动保存失败时作为 warning
    /// 继续，不阻塞切换。
    ///
    /// **从"不使用项目"首次应用时先拍基线**：当前没有项目时，把分组当前状态
    /// 存为设备本地基线，[`Self::clear_current`] 会恢复它（PRODUCT-68）。
    ///
    /// 返回 `(warnings, should_stop_proxy)`：当当前分组内所有接管都被关闭、
    /// 其它应用也没有接管、且 Claude Desktop 不依赖本地路由时，建议调用者停止
    /// 代理服务，以便 Claude Desktop 的"本地路由"总开关同步显示为关闭。
    pub fn apply(
        state: &AppState,
        profile_id: &str,
        scope: ProfileScope,
    ) -> Result<(Vec<String>, bool), AppError> {
        let mut warnings = Vec::new();

        let profile = state
            .db
            .get_profile(profile_id)?
            .ok_or_else(|| AppError::InvalidInput(format!("Profile not found: {profile_id}")))?;

        match state.db.get_current_profile_id(scope.as_str())? {
            // 自动保存旧项目当前状态（仅当前分组），失败不阻塞切换
            Some(current_id) if current_id != profile_id => {
                if let Err(e) = Self::update(state, &current_id, None, true, Some(scope)) {
                    warnings.push(format!(
                        "autosave profile '{current_id}' before switch failed: {e}"
                    ));
                }
            }
            Some(_) => {}
            // 从"不使用项目"进入：先拍基线，供"不使用项目"恢复
            None => {
                let saved = Self::snapshot_current(state, scope).and_then(|baseline| {
                    state
                        .db
                        .set_profile_baseline(scope.as_str(), Some(&serialize_payload(&baseline)?))
                });
                if let Err(e) = saved {
                    warnings.push(format!(
                        "saving the configuration before projects failed: {e}"
                    ));
                }
            }
        }

        // 自动保存可能刚写过同一条记录（重新应用当前项目时），重新读取以免用旧 payload
        let profile = state.db.get_profile(profile_id)?.unwrap_or(profile);
        let payload = parse_payload(&profile.payload)?;

        if !payload.scope_captured(scope) {
            warnings.push(format!(
                "no {} configuration captured in this project yet; marked as current without changes (it will be saved automatically when you switch away)",
                scope.as_str()
            ));
        }

        Self::apply_payload(state, &payload, scope, &mut warnings)?;

        state
            .db
            .set_current_profile_id(scope.as_str(), Some(profile_id))?;

        Ok((warnings, should_stop_proxy_after_apply(state)))
    }

    /// "不使用项目"：清除分组的 current 标记；若存在首次应用项目前拍下的基线，
    /// 先把当前状态自动保存回当前项目，再恢复基线并删除它（PRODUCT-68）。
    ///
    /// 没有基线时（旧版本创建的状态或基线已被消费）只清标记、不改配置，
    /// 与历史行为一致。返回值与 [`Self::apply`] 相同。
    pub fn clear_current(
        state: &AppState,
        scope: ProfileScope,
    ) -> Result<(Vec<String>, bool), AppError> {
        let mut warnings = Vec::new();
        let current_id = state.db.get_current_profile_id(scope.as_str())?;
        let baseline = state.db.get_profile_baseline(scope.as_str())?;

        let Some(current_id) = current_id else {
            return Ok((warnings, false));
        };
        let Some(baseline_raw) = baseline else {
            state.db.set_current_profile_id(scope.as_str(), None)?;
            return Ok((warnings, false));
        };

        if let Err(e) = Self::update(state, &current_id, None, true, Some(scope)) {
            warnings.push(format!(
                "autosave profile '{current_id}' before leaving it failed: {e}"
            ));
        }

        match parse_payload(&baseline_raw) {
            Ok(baseline) => Self::apply_payload(state, &baseline, scope, &mut warnings)?,
            Err(e) => warnings.push(format!(
                "the configuration saved before projects is unreadable and was not restored: {e}"
            )),
        }
        state.db.set_profile_baseline(scope.as_str(), None)?;
        state.db.set_current_profile_id(scope.as_str(), None)?;

        Ok((warnings, should_stop_proxy_after_apply(state)))
    }

    /// 把一份快照落地到分组内的应用（best-effort，失败写入 warnings）
    fn apply_payload(
        state: &AppState,
        payload: &ProfilePayload,
        scope: ProfileScope,
        warnings: &mut Vec<String>,
    ) -> Result<(), AppError> {
        for app in scope.apps().iter() {
            let app_str = app.as_str();

            // 1. 切换项目前无条件关闭当前应用的代理接管。
            // 接管态下 live 文件属于代理；用户希望切换工作目录时总是退出当前
            // 代理环境，再按快照写入真实供应商配置。叠加模式应用没有接管。
            if ProfileScope::has_takeover(app) {
                if let Err(e) = state.proxy_service.disable_takeover_for_app_sync(app) {
                    warnings.push(format!(
                        "[{app_str}] auto-disable proxy takeover before profile switch failed: {e}"
                    ));
                }
            }

            // 2. 供应商
            if let Some(Some(target_pid)) = payload.providers.get(app) {
                let providers = state.db.get_all_providers(app_str)?;
                if !ProfileScope::captures_provider(app) {
                    // 叠加模式应用不按快照切换供应商（防御：旧/手改 payload）
                } else if !providers.contains_key(target_pid) {
                    warnings.push(format!(
                        "[{app_str}] provider '{target_pid}' no longer exists, skipped"
                    ));
                } else {
                    let current = crate::settings::get_effective_current_provider(&state.db, app)?;
                    if current.as_deref() != Some(target_pid.as_str()) {
                        match ProviderService::switch(state, app.clone(), target_pid) {
                            Ok(result) => warnings.extend(result.warnings),
                            Err(e) => warnings.push(format!(
                                "[{app_str}] switch provider '{target_pid}' failed: {e}"
                            )),
                        }
                    }
                }
            }

            // 3. MCP diff（最小 toggle：仅动目标态≠当前态的条目；None = 该侧未拍过，不动）
            if let Some(Some(target_ids)) = payload.mcp.get(app) {
                let servers = state.db.get_all_mcp_servers()?;
                let current: Vec<(String, bool)> = servers
                    .values()
                    .map(|s| (s.id.clone(), s.apps.is_enabled_for(app)))
                    .collect();
                let (toggles, dangling) = plan_toggles(&current, target_ids);
                for id in dangling {
                    warnings.push(format!("[{app_str}] MCP '{id}' no longer exists, skipped"));
                }
                for (id, enabled) in toggles {
                    if let Err(e) = McpService::toggle_app(state, &id, app.clone(), enabled) {
                        warnings.push(format!(
                            "[{app_str}] toggle MCP '{id}' -> {enabled} failed: {e}"
                        ));
                    }
                }
            }

            // 4. Skills diff（SkillService 返回 anyhow::Result，收进 warning）
            if let Some(Some(target_ids)) = payload.skills.get(app) {
                let skills = state.db.get_all_installed_skills()?;
                let current: Vec<(String, bool)> = skills
                    .values()
                    .map(|s| (s.id.clone(), s.apps.is_enabled_for(app)))
                    .collect();
                let (toggles, dangling) = plan_toggles(&current, target_ids);
                for id in dangling {
                    warnings.push(format!(
                        "[{app_str}] skill '{id}' no longer exists, skipped"
                    ));
                }
                for (id, enabled) in toggles {
                    if let Err(e) = SkillService::toggle_app(&state.db, &id, app, enabled) {
                        warnings.push(format!(
                            "[{app_str}] toggle skill '{id}' -> {enabled} failed: {e}"
                        ));
                    }
                }
            }

            // 5. Prompt（None = 不动；已激活则幂等跳过，避免无谓的文件写与备份）
            if let Some(Some(target_prompt)) = payload.prompts.get(app) {
                let prompts = state.db.get_prompts(app_str)?;
                match prompts.get(target_prompt) {
                    None => warnings.push(format!(
                        "[{app_str}] prompt '{target_prompt}' no longer exists, skipped"
                    )),
                    Some(p) if p.enabled => {}
                    Some(_) => {
                        if let Err(e) =
                            PromptService::enable_prompt(state, app.clone(), target_prompt)
                        {
                            warnings.push(format!(
                                "[{app_str}] enable prompt '{target_prompt}' failed: {e}"
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_payload_serde_roundtrip() {
        let payload = ProfilePayload {
            providers: PerApp {
                claude: Some("p1".into()),
                claude_desktop: Some("d1".into()),
                gemini: Some("g1".into()),
                ..Default::default()
            },
            mcp: PerApp {
                claude: Some(ids(&["m1", "m2"])),
                claude_desktop: Some(vec![]),
                hermes: Some(ids(&["h1"])),
                ..Default::default()
            },
            skills: PerApp {
                claude: Some(vec![]),
                claude_desktop: Some(vec![]),
                codex: Some(ids(&["s1"])),
                opencode: Some(vec![]),
                ..Default::default()
            },
            prompts: PerApp {
                codex: Some("pr1".into()),
                openclaw: Some("oc".into()),
                grokbuild: Some("gb".into()),
                ..Default::default()
            },
        };
        let json = serde_json::to_string(&payload).unwrap();
        // per-app key 必须与 AppType 的 serde 形式一致（claude-desktop 是连字符）
        for key in [
            "claude",
            "claude-desktop",
            "codex",
            "gemini",
            "grokbuild",
            "opencode",
            "openclaw",
            "hermes",
        ] {
            assert!(json.contains(&format!("\"{key}\"")), "missing key {key}");
            assert_eq!(
                serde_json::to_string(&app_from_serde(key)).unwrap(),
                format!("\"{key}\""),
                "slot key must match AppType serde form"
            );
        }
        let back: ProfilePayload = serde_json::from_str(&json).unwrap();
        assert_eq!(back, payload);
    }

    fn app_from_serde(value: &str) -> AppType {
        serde_json::from_str(&format!("\"{value}\"")).expect("valid app type")
    }

    #[test]
    fn test_payload_tolerates_missing_fields() {
        // 前向兼容：旧版/部分字段缺失时应落到 None（"该侧未拍过"）而不是报错，
        // 应用时对缺失槽位不做任何改动
        let back: ProfilePayload =
            serde_json::from_str(r#"{"providers":{"claude":"p1"},"mcp":{"claude":["m1"]}}"#)
                .unwrap();
        assert_eq!(back.providers.claude, Some("p1".to_string()));
        assert_eq!(back.providers.claude_desktop, None);
        assert_eq!(back.providers.codex, None);
        assert_eq!(back.mcp.claude, Some(ids(&["m1"])));
        assert_eq!(back.mcp.claude_desktop, None);
        assert_eq!(back.mcp.codex, None, "missing slot means untouched");
        assert_eq!(back.prompts.codex, None);
        assert_eq!(back.mcp.gemini, None, "new app slots default to untouched");
        assert_eq!(back.prompts.hermes, None);

        let empty: ProfilePayload = serde_json::from_str("{}").unwrap();
        assert_eq!(empty, ProfilePayload::default());
    }

    #[test]
    fn test_legacy_three_app_payload_is_not_captured_for_new_scopes() {
        let legacy: ProfilePayload = serde_json::from_str(
            r#"{"providers":{"claude":"p1","claude-desktop":null,"codex":"c1"},
                "mcp":{"claude":[],"claude-desktop":[],"codex":[]},
                "skills":{"claude":[],"claude-desktop":[],"codex":[]},
                "prompts":{"claude":null,"claude-desktop":null,"codex":null}}"#,
        )
        .unwrap();
        assert!(legacy.scope_captured(ProfileScope::Claude));
        assert!(legacy.scope_captured(ProfileScope::Codex));
        for scope in [
            ProfileScope::Gemini,
            ProfileScope::GrokBuild,
            ProfileScope::OpenCode,
            ProfileScope::OpenClaw,
            ProfileScope::Hermes,
        ] {
            assert!(
                !legacy.scope_captured(scope),
                "{scope:?} must be untouched by a legacy payload"
            );
        }
    }

    #[test]
    fn test_merge_scope_from_only_touches_scope_slots() {
        // 项目 A：两侧都已拍过快照
        let mut payload = ProfilePayload {
            providers: PerApp {
                claude: Some("p1".into()),
                claude_desktop: Some("d1".into()),
                codex: Some("c1".into()),
                gemini: Some("g1".into()),
                ..Default::default()
            },
            mcp: PerApp {
                claude: Some(ids(&["m1"])),
                claude_desktop: Some(vec![]),
                codex: Some(ids(&["m9"])),
                ..Default::default()
            },
            ..Default::default()
        };
        // 在 Claude 页"以当前状态更新"：只覆盖 claude 组槽位
        let fresh = ProfilePayload {
            providers: PerApp {
                claude: Some("p2".into()),
                claude_desktop: None,
                codex: Some("SHOULD-NOT-LEAK".into()),
                gemini: Some("SHOULD-NOT-LEAK".into()),
                ..Default::default()
            },
            mcp: PerApp {
                claude: Some(ids(&["m2"])),
                claude_desktop: Some(vec![]),
                ..Default::default()
            },
            ..Default::default()
        };
        payload.merge_scope_from(&fresh, ProfileScope::Claude);

        assert_eq!(payload.providers.claude, Some("p2".to_string()));
        assert_eq!(
            payload.providers.claude_desktop,
            Some("d1".to_string()),
            "claude-desktop slot is in its own scope, untouched by claude merge"
        );
        assert_eq!(payload.mcp.claude, Some(ids(&["m2"])));
        // codex / gemini 侧完好：既没被覆盖也没被 fresh 的值污染
        assert_eq!(payload.providers.codex, Some("c1".to_string()));
        assert_eq!(payload.providers.gemini, Some("g1".to_string()));
        assert_eq!(payload.mcp.codex, Some(ids(&["m9"])));
    }

    #[test]
    fn test_scope_captured_detects_per_scope_snapshot() {
        let mut payload = ProfilePayload::default();
        for scope in ProfileScope::ALL {
            assert!(!payload.scope_captured(scope));
        }

        // 只拍过 claude 组（哪怕拍到的是空集）
        payload.mcp.claude = Some(vec![]);
        assert!(payload.scope_captured(ProfileScope::Claude));
        assert!(!payload.scope_captured(ProfileScope::ClaudeDesktop));
        assert!(!payload.scope_captured(ProfileScope::Codex));

        // Desktop 槽位属于独立的 claude-desktop 组
        let mut desktop_only = ProfilePayload::default();
        desktop_only.providers.claude_desktop = Some("d1".into());
        assert!(desktop_only.scope_captured(ProfileScope::ClaudeDesktop));
        assert!(!desktop_only.scope_captured(ProfileScope::Claude));

        // OpenClaw 只有 prompt 一个维度
        let mut openclaw_only = ProfilePayload::default();
        openclaw_only.prompts.openclaw = Some("p".into());
        assert!(openclaw_only.scope_captured(ProfileScope::OpenClaw));
        assert!(!openclaw_only.scope_captured(ProfileScope::Hermes));
    }

    #[test]
    fn test_per_app_get_supports_every_app() {
        let mut per: PerApp<Option<String>> = PerApp::default();
        for scope in ProfileScope::ALL {
            for app in scope.apps() {
                assert!(per.get(app).is_some());
                *per.get_mut(app).unwrap() = Some(app.as_str().to_string());
            }
        }
        assert_eq!(per.grokbuild.as_deref(), Some("grokbuild"));
        assert_eq!(per.claude_desktop.as_deref(), Some("claude-desktop"));
    }

    #[test]
    fn test_scope_serde_and_parse_roundtrip() {
        for scope in ProfileScope::ALL {
            // DB 存储字符串（as_str/parse）与 JSON 序列化必须是同一形式
            assert_eq!(
                serde_json::to_string(&scope).unwrap(),
                format!("\"{}\"", scope.as_str())
            );
            assert_eq!(ProfileScope::parse(scope.as_str()).unwrap(), scope);
            // 托盘事件 id 形如 profile_<scope>_<uuid>，scope 不得含下划线
            assert!(!scope.as_str().contains('_'));
            assert!(!scope.label().is_empty());
        }
        assert!(ProfileScope::parse("gemini").is_ok());
        assert!(ProfileScope::parse("grok_build").is_err());
        assert!(ProfileScope::parse("").is_err());
    }

    #[test]
    fn test_scope_app_grouping() {
        // 每个应用独立成组；组内应用与 for_app 反向映射必须一致
        assert_eq!(ProfileScope::Claude.apps(), &[AppType::Claude]);
        assert_eq!(
            ProfileScope::ClaudeDesktop.apps(),
            &[AppType::ClaudeDesktop]
        );
        assert_eq!(ProfileScope::Codex.apps(), &[AppType::Codex]);
        for scope in ProfileScope::ALL {
            for app in scope.apps() {
                assert_eq!(ProfileScope::for_app(app), Some(scope));
                assert_eq!(scope.as_str(), app.as_str());
            }
        }
    }

    #[test]
    fn test_capabilities_per_app() {
        assert!(ProfileScope::captures_provider(&AppType::Gemini));
        assert!(ProfileScope::captures_provider(&AppType::GrokBuild));
        assert!(!ProfileScope::captures_provider(&AppType::OpenCode));
        assert!(!ProfileScope::captures_provider(&AppType::OpenClaw));
        assert!(!ProfileScope::captures_provider(&AppType::Hermes));
        assert!(ProfileScope::captures_mcp(&AppType::Hermes));
        assert!(!ProfileScope::captures_mcp(&AppType::OpenClaw));
        assert!(!ProfileScope::captures_skills(&AppType::OpenClaw));
        assert!(ProfileScope::captures_skills(&AppType::OpenCode));
    }

    #[test]
    fn test_plan_toggles_minimal_diff() {
        let current = vec![
            ("a".to_string(), true),  // 目标含 a：不动
            ("b".to_string(), false), // 目标含 b：开
            ("c".to_string(), true),  // 目标不含 c：关
            ("d".to_string(), false), // 目标不含 d：不动
        ];
        let (toggles, dangling) = plan_toggles(&current, &ids(&["a", "b", "ghost"]));
        assert_eq!(
            toggles,
            vec![("b".to_string(), true), ("c".to_string(), false)]
        );
        assert_eq!(dangling, ids(&["ghost"]));
    }

    #[test]
    fn test_plan_toggles_empty_target_disables_all_enabled() {
        let current = vec![("a".to_string(), true), ("b".to_string(), false)];
        let (toggles, dangling) = plan_toggles(&current, &[]);
        assert_eq!(toggles, vec![("a".to_string(), false)]);
        assert!(dangling.is_empty());
    }
}
