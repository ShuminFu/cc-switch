//! 各应用 MCP 模块共用的投影 / 导入逻辑
//!
//! - [`collect_enabled_servers`]：旧版按客户端分开的 `McpConfig` 中 enabled==true 的项
//! - [`valid_imports`] / [`imported_server`]：把应用 live 配置读出的服务器合并进统一结构
//! - [`import_into_config`]：基于上面两者的 `MultiAppConfig` 导入（兼容层）

use serde_json::Value;
use std::collections::HashMap;

use crate::app_config::{AppType, McpApps, McpConfig, McpServer, MultiAppConfig};

use super::validation::{extract_server_spec, validate_server_spec};

/// 返回已启用的 MCP 服务器（过滤 enabled==true）
pub(crate) fn collect_enabled_servers(cfg: &McpConfig) -> HashMap<String, Value> {
    let mut out = HashMap::new();
    for (id, entry) in cfg.servers.iter() {
        let enabled = entry
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !enabled {
            continue;
        }
        match extract_server_spec(entry) {
            Ok(spec) => {
                out.insert(id.clone(), spec);
            }
            Err(err) => {
                log::warn!("跳过无效的 MCP 条目 '{id}': {err}");
            }
        }
    }
    out
}

/// 过滤掉校验失败的导入项：单项失败不中止，记录日志后继续处理
pub(crate) fn valid_imports(
    app: &AppType,
    entries: Vec<(String, Value)>,
) -> impl Iterator<Item = (String, Value)> + '_ {
    entries
        .into_iter()
        .filter(move |(id, spec)| match validate_server_spec(spec) {
            Ok(()) => true,
            Err(e) => {
                log::warn!("跳过无效 {} MCP 服务器 '{id}': {e}", app.as_str());
                false
            }
        })
}

/// 从应用导入的新服务器：默认仅启用该应用
pub(crate) fn imported_server(app: &AppType, id: String, spec: Value) -> McpServer {
    let mut apps = McpApps::default();
    apps.set_enabled_for(app, true);
    McpServer {
        name: id.clone(),
        id,
        server: spec,
        apps,
        description: None,
        homepage: None,
        docs: None,
        tags: Vec::new(),
    }
}

/// 把应用 live 配置读出的服务器合并进 `config.mcp.servers`（v3.7.0+ 统一结构），
/// 返回发生变化的条目数。
///
/// 已存在的服务器只启用该应用，不覆盖其他字段和应用状态。
pub(crate) fn import_into_config(
    config: &mut MultiAppConfig,
    app: &AppType,
    entries: Vec<(String, Value)>,
) -> usize {
    let servers = config.mcp.servers.get_or_insert_with(HashMap::new);
    let mut changed = 0;

    for (id, spec) in valid_imports(app, entries) {
        if let Some(existing) = servers.get_mut(&id) {
            if !existing.apps.is_enabled_for(app) {
                existing.apps.set_enabled_for(app, true);
                changed += 1;
                log::info!("MCP 服务器 '{id}' 已启用 {} 应用", app.as_str());
            }
        } else {
            servers.insert(id.clone(), imported_server(app, id.clone(), spec));
            changed += 1;
            log::info!("导入新 MCP 服务器 '{id}'（来自 {}）", app.as_str());
        }
    }

    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entries() -> Vec<(String, Value)> {
        vec![
            ("echo".into(), json!({"type": "stdio", "command": "echo"})),
            ("broken".into(), json!({"type": "stdio"})),
        ]
    }

    #[test]
    fn import_into_config_adds_new_servers_for_only_that_app() {
        let mut config = MultiAppConfig::default();
        let changed = import_into_config(&mut config, &AppType::Hermes, entries());
        assert_eq!(changed, 1);

        let servers = config.mcp.servers.as_ref().unwrap();
        assert!(!servers.contains_key("broken"));
        let echo = &servers["echo"];
        assert_eq!(echo.name, "echo");
        assert_eq!(echo.apps.enabled_apps(), vec![AppType::Hermes]);
    }

    #[test]
    fn import_into_config_only_enables_app_on_existing_servers() {
        let mut config = MultiAppConfig::default();
        let mut existing = imported_server(&AppType::Claude, "echo".into(), json!({"x": 1}));
        existing.description = Some("keep".into());
        config
            .mcp
            .servers
            .get_or_insert_with(HashMap::new)
            .insert("echo".into(), existing);

        assert_eq!(
            import_into_config(&mut config, &AppType::Codex, entries()),
            1
        );
        // 再次导入不再计为变化
        assert_eq!(
            import_into_config(&mut config, &AppType::Codex, entries()),
            0
        );

        let echo = &config.mcp.servers.as_ref().unwrap()["echo"];
        assert!(echo.apps.claude && echo.apps.codex);
        assert_eq!(echo.server, json!({"x": 1}));
        assert_eq!(echo.description.as_deref(), Some("keep"));
    }

    #[test]
    fn collect_enabled_servers_skips_disabled_entries() {
        let mut cfg = McpConfig::default();
        cfg.servers.insert(
            "on".into(),
            json!({"enabled": true, "server": {"type": "stdio", "command": "a"}}),
        );
        cfg.servers.insert(
            "off".into(),
            json!({"enabled": false, "server": {"type": "stdio", "command": "b"}}),
        );
        let out = collect_enabled_servers(&cfg);
        assert_eq!(out.len(), 1);
        assert!(out.contains_key("on"));
    }
}
