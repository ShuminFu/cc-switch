//! Gemini MCP 同步和导入模块

use serde_json::Value;

use crate::app_config::{AppType, MultiAppConfig};
use crate::error::AppError;

use super::common::{collect_enabled_servers, import_into_config};

fn should_sync_gemini_mcp() -> bool {
    // Gemini 未安装/未初始化时：~/.gemini 目录不存在。
    // 按用户偏好：目录缺失时跳过写入/删除，不创建任何文件或目录。
    crate::gemini_config::get_gemini_dir().exists()
}

/// 将 config.json 中 Gemini 的 enabled==true 项写入 Gemini MCP 配置
pub fn sync_enabled_to_gemini(config: &MultiAppConfig) -> Result<(), AppError> {
    if !should_sync_gemini_mcp() {
        return Ok(());
    }
    let enabled = collect_enabled_servers(&config.mcp.gemini);
    crate::gemini_mcp::set_mcp_servers_map(&enabled)
}

/// 读取 Gemini MCP 配置中的服务器（统一结构，尚未校验）
pub(crate) fn read_servers_from_gemini() -> Result<Vec<(String, Value)>, AppError> {
    Ok(crate::gemini_mcp::read_mcp_servers_map()?
        .into_iter()
        .collect())
}

/// 从 Gemini MCP 配置导入到统一结构（v3.7.0+）
/// 已存在的服务器将启用 Gemini 应用，不覆盖其他字段和应用状态
pub fn import_from_gemini(config: &mut MultiAppConfig) -> Result<usize, AppError> {
    let entries = read_servers_from_gemini()?;
    Ok(import_into_config(config, &AppType::Gemini, entries))
}

/// 将单个 MCP 服务器同步到 Gemini live 配置
pub fn sync_single_server_to_gemini(
    _config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    if !should_sync_gemini_mcp() {
        return Ok(());
    }
    // 读取现有的 MCP 配置
    let mut current = crate::gemini_mcp::read_mcp_servers_map()?;

    // 添加/更新当前服务器
    current.insert(id.to_string(), server_spec.clone());

    // 写回
    crate::gemini_mcp::set_mcp_servers_map(&current)
}

/// 从 Gemini live 配置中移除单个 MCP 服务器
pub fn remove_server_from_gemini(id: &str) -> Result<(), AppError> {
    if !should_sync_gemini_mcp() {
        return Ok(());
    }
    // 读取现有的 MCP 配置
    let mut current = crate::gemini_mcp::read_mcp_servers_map()?;

    // 移除指定服务器
    current.remove(id);

    // 写回
    crate::gemini_mcp::set_mcp_servers_map(&current)
}
