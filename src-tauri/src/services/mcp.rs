use indexmap::IndexMap;

use crate::app_config::{AppType, McpServer};
use crate::error::AppError;
use crate::mcp;
use crate::store::AppState;

/// MCP 相关业务逻辑（v3.7.0 统一结构）
pub struct McpService;

impl McpService {
    /// 获取所有 MCP 服务器（统一结构）
    pub fn get_all_servers(state: &AppState) -> Result<IndexMap<String, McpServer>, AppError> {
        state.db.get_all_mcp_servers()
    }

    /// 添加或更新 MCP 服务器
    ///
    /// SVC-12: the live projection runs *before* the DB save. If any app's
    /// live write fails, the apps already touched are reverted to the previous
    /// server state and the DB is left unchanged, so the UI never shows a
    /// half-applied edit.
    pub fn upsert_server(state: &AppState, server: McpServer) -> Result<(), AppError> {
        // 先做 schema 校验：无效定义不得写入数据库或任何 live 配置
        mcp::validate_server_spec(&server.server)?;

        // 读取旧状态：用于处理“编辑时取消勾选某个应用”的场景（需要从对应 live 配置中移除）
        let prev = state.db.get_all_mcp_servers()?.shift_remove(&server.id);

        let mut touched: Vec<AppType> = Vec::new();
        for adapter in crate::adapters::mcp_adapters() {
            let app = adapter.app();
            let wanted = server.apps.is_enabled_for(&app);
            let had = prev.as_ref().is_some_and(|p| p.apps.is_enabled_for(&app));
            let result = if wanted {
                Self::sync_server_to_app_no_config(&server, &app)
            } else if had {
                Self::remove_server_from_app(state, &server.id, &app)
            } else {
                continue;
            };
            if let Err(err) = result {
                Self::revert_live(&server.id, prev.as_ref(), &touched);
                return Err(err);
            }
            touched.push(app);
        }

        if let Err(err) = state.db.save_mcp_server(&server) {
            Self::revert_live(&server.id, prev.as_ref(), &touched);
            return Err(err);
        }

        Ok(())
    }

    /// Put `apps`' live MCP entry for `id` back to what `prev` describes
    /// (best effort; failures are logged, the original error is what the
    /// caller reports).
    fn revert_live(id: &str, prev: Option<&McpServer>, apps: &[AppType]) {
        for app in apps {
            let result = match prev {
                Some(prev) if prev.apps.is_enabled_for(app) => {
                    Self::sync_server_to_app_no_config(prev, app)
                }
                _ => crate::adapters::adapter_for(app)
                    .mcp()
                    .map_or(Ok(()), |projector| projector.remove_server(id)),
            };
            if let Err(err) = result {
                log::warn!(
                    "Failed to revert MCP server {id} in {} live config: {err}",
                    app.as_str()
                );
            }
        }
    }

    /// 删除 MCP 服务器
    pub fn delete_server(state: &AppState, id: &str) -> Result<bool, AppError> {
        let server = state.db.get_all_mcp_servers()?.shift_remove(id);

        if let Some(server) = server {
            state.db.delete_mcp_server(id)?;

            // 从所有应用的 live 配置中移除
            Self::remove_server_from_all_apps(state, id, &server)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// 切换指定应用的启用状态
    ///
    /// The live file is changed first; the DB flag is saved only when that
    /// succeeded, and a failed DB save reverts the live change (SVC-12).
    pub fn toggle_app(
        state: &AppState,
        server_id: &str,
        app: AppType,
        enabled: bool,
    ) -> Result<(), AppError> {
        let mut servers = state.db.get_all_mcp_servers()?;

        if let Some(server) = servers.get_mut(server_id) {
            let prev = server.clone();
            server.apps.set_enabled_for(&app, enabled);

            if enabled {
                Self::sync_server_to_app(state, server, &app)?;
            } else {
                Self::remove_server_from_app(state, server_id, &app)?;
            }

            if let Err(err) = state.db.save_mcp_server(server) {
                Self::revert_live(server_id, Some(&prev), std::slice::from_ref(&app));
                return Err(err);
            }
        }

        Ok(())
    }

    /// 将 MCP 服务器同步到指定应用
    fn sync_server_to_app(
        _state: &AppState,
        server: &McpServer,
        app: &AppType,
    ) -> Result<(), AppError> {
        Self::sync_server_to_app_no_config(server, app)
    }

    fn sync_server_to_app_no_config(server: &McpServer, app: &AppType) -> Result<(), AppError> {
        match crate::adapters::adapter_for(app).mcp() {
            Some(projector) => projector.upsert_server(&server.id, &server.server),
            None => {
                // Claude Desktop 3P profiles and OpenClaw (Issue #4834) do not
                // consume cc-switch MCP servers.
                log::debug!("{} does not use CC Switch MCP sync, skipping", app.as_str());
                Ok(())
            }
        }
    }

    /// 从所有曾启用过该服务器的应用中移除
    fn remove_server_from_all_apps(
        state: &AppState,
        id: &str,
        server: &McpServer,
    ) -> Result<(), AppError> {
        // 从所有曾启用的应用中移除
        for app in server.apps.enabled_apps() {
            Self::remove_server_from_app(state, id, &app)?;
        }
        Ok(())
    }

    fn remove_server_from_app(_state: &AppState, id: &str, app: &AppType) -> Result<(), AppError> {
        match crate::adapters::adapter_for(app).mcp() {
            Some(projector) => projector.remove_server(id),
            None => {
                log::debug!("{} does not use CC Switch MCP sync, skipping", app.as_str());
                Ok(())
            }
        }
    }

    /// 手动同步所有启用的 MCP 服务器到对应的应用。
    ///
    /// Best-effort：单个应用投影失败（如 ~/.claude.json 坏 JSON）不阻断
    /// 其余应用——各应用的 live 文件互相独立，一处损坏没有理由让其他
    /// 应用的 MCP 状态陈旧。全部跑完后若有失败，聚合成一个错误上报，
    /// 保留调用方的可见性。
    pub fn sync_all_enabled(state: &AppState) -> Result<(), AppError> {
        let servers = Self::get_all_servers(state)?;

        let mut failures: Vec<String> = Vec::new();
        for app in AppType::all() {
            if let Err(err) = Self::project_servers_to_app(state, &servers, &app) {
                log::warn!("同步 MCP 到 {app:?} 失败: {err}");
                failures.push(format!("{}: {err}", app.as_str()));
            }
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(AppError::Message(format!(
                "部分应用 MCP 同步失败: {}",
                failures.join("; ")
            )))
        }
    }

    /// 只把启用状态投影到单个应用。某个应用的 live 被整体重写后用它做
    /// 定向重投影，避免把无关应用的失败面（如 ~/.claude.json 坏 JSON）
    /// 牵连进目标应用的关键路径。
    pub fn sync_enabled_for_app(state: &AppState, app: &AppType) -> Result<(), AppError> {
        let servers = Self::get_all_servers(state)?;
        Self::project_servers_to_app(state, &servers, app)
    }

    fn project_servers_to_app(
        _state: &AppState,
        servers: &IndexMap<String, McpServer>,
        app: &AppType,
    ) -> Result<(), AppError> {
        let Some(projector) = crate::adapters::adapter_for(app).mcp() else {
            return Ok(());
        };

        let mut enabled: Vec<(&str, &serde_json::Value)> = Vec::new();
        let mut disabled: Vec<&str> = Vec::new();
        for server in servers.values() {
            if server.apps.is_enabled_for(app) {
                enabled.push((server.id.as_str(), &server.server));
            } else {
                disabled.push(server.id.as_str());
            }
        }

        // 一次投影整个期望状态：单文件应用每次同步最多重写一次 live 文件
        projector.project(&enabled, &disabled)
    }

    /// 从指定应用的 live 配置导入 MCP 服务器，返回新增的服务器数量。
    ///
    /// 已存在的服务器只为该应用打开启用开关，不覆盖其他字段（与导入模块语义保持一致）。
    /// 导入是读取已有配置，不会反向写回任何应用的 live 配置；显式编辑、
    /// 启用/禁用或手动同步时再执行写回。
    pub fn import_from_app(state: &AppState, app: &AppType) -> Result<usize, AppError> {
        let Some(projector) = crate::adapters::adapter_for(app).mcp() else {
            return Ok(0);
        };

        let mut temp_config = crate::app_config::MultiAppConfig::default();
        let count = projector.import_servers(&mut temp_config)?;

        let mut new_count = 0;
        if count > 0 {
            if let Some(servers) = &temp_config.mcp.servers {
                let mut existing = state.db.get_all_mcp_servers()?;
                for server in servers.values() {
                    let to_save = if let Some(existing_server) = existing.get(&server.id) {
                        // Already known and enabled for this app: nothing to
                        // write. Startup imports from every live config
                        // (CFG-40), and a no-op save would still bump the
                        // row's sync_rev and trigger an auto-sync upload on
                        // every launch.
                        if existing_server.apps.is_enabled_for(app) {
                            continue;
                        }
                        let mut merged = existing_server.clone();
                        merged.apps.set_enabled_for(app, true);
                        merged
                    } else {
                        new_count += 1;
                        server.clone()
                    };

                    state.db.save_mcp_server(&to_save)?;
                    existing.insert(to_save.id.clone(), to_save);
                }
            }
        }

        Ok(new_count)
    }

    /// 从 Claude 导入 MCP（v3.7.0 已更新为统一结构）
    pub fn import_from_claude(state: &AppState) -> Result<usize, AppError> {
        Self::import_from_app(state, &AppType::Claude)
    }

    /// 从 Codex 导入 MCP（v3.7.0 已更新为统一结构）
    pub fn import_from_codex(state: &AppState) -> Result<usize, AppError> {
        Self::import_from_app(state, &AppType::Codex)
    }

    /// 从 Gemini 导入 MCP（v3.7.0 已更新为统一结构）
    pub fn import_from_gemini(state: &AppState) -> Result<usize, AppError> {
        Self::import_from_app(state, &AppType::Gemini)
    }

    /// 从 Grok Build 的 `[mcp_servers]` 导入 MCP。
    pub fn import_from_grokbuild(state: &AppState) -> Result<usize, AppError> {
        Self::import_from_app(state, &AppType::GrokBuild)
    }

    /// 从 OpenCode 导入 MCP（v3.9.2+ 新增）
    pub fn import_from_opencode(state: &AppState) -> Result<usize, AppError> {
        Self::import_from_app(state, &AppType::OpenCode)
    }

    /// 从 Hermes 导入 MCP
    pub fn import_from_hermes(state: &AppState) -> Result<usize, AppError> {
        Self::import_from_app(state, &AppType::Hermes)
    }

    /// 从所有支持 MCP 的应用导入服务器，返回新导入的数量。
    ///
    /// Best-effort：单个应用导入失败（如坏 config.toml）不阻断其余应用；
    /// 全部跑完后若有失败，聚合成一个错误上报——历史实现逐应用
    /// `unwrap_or(0)` 吞错，坏文件只会表现为"导入成功 0 个"，用户
    /// 无从得知哪个应用出了问题。
    pub fn import_from_all_apps(state: &AppState) -> Result<usize, AppError> {
        let mut total = 0;
        let mut failures: Vec<String> = Vec::new();

        for adapter in crate::adapters::mcp_adapters() {
            let app = adapter.app().as_str().to_string();
            match Self::import_from_app(state, &adapter.app()) {
                Ok(count) => total += count,
                Err(err) => {
                    log::warn!("从 {app} 导入 MCP 失败: {err}");
                    failures.push(format!("{app}: {err}"));
                }
            }
        }

        if failures.is_empty() {
            Ok(total)
        } else {
            Err(AppError::Message(format!(
                "已导入 {total} 个，部分应用导入失败: {}",
                failures.join("; ")
            )))
        }
    }
}
