#![allow(non_snake_case)]

use indexmap::IndexMap;

use std::str::FromStr;
use tauri::State;

use crate::error::AppError;

use crate::app_config::AppType;
use crate::claude_mcp;
use crate::services::McpService;
use crate::store::AppState;

/// 获取 Claude MCP 状态
#[tauri::command]
pub async fn get_claude_mcp_status() -> Result<claude_mcp::McpStatus, AppError> {
    claude_mcp::get_mcp_status()
}

/// 读取 mcp.json 文本内容
#[tauri::command]
#[specta::specta]
pub async fn read_claude_mcp_config() -> Result<Option<String>, AppError> {
    claude_mcp::read_mcp_json()
}

/// 新增或更新一个 MCP 服务器条目
#[tauri::command]
#[specta::specta]
pub async fn upsert_claude_mcp_server(
    id: String,
    spec: serde_json::Value,
) -> Result<bool, AppError> {
    claude_mcp::upsert_mcp_server(&id, spec)
}

/// 删除一个 MCP 服务器条目
#[tauri::command]
#[specta::specta]
pub async fn delete_claude_mcp_server(id: String) -> Result<bool, AppError> {
    claude_mcp::delete_mcp_server(&id)
}

/// 校验命令是否在 PATH 中可用（不执行）
#[tauri::command]
#[specta::specta]
pub async fn validate_mcp_command(cmd: String) -> Result<bool, AppError> {
    claude_mcp::validate_command_in_path(&cmd)
}

// ============================================================================
// v3.7.0 新增：统一 MCP 管理命令
// ============================================================================

use crate::app_config::McpServer;

/// 获取所有 MCP 服务器（统一结构）
#[tauri::command]
pub async fn get_mcp_servers(
    state: State<'_, AppState>,
) -> Result<IndexMap<String, McpServer>, AppError> {
    McpService::get_all_servers(&state)
}

/// 添加或更新 MCP 服务器
#[tauri::command]
pub async fn upsert_mcp_server(
    state: State<'_, AppState>,
    server: McpServer,
) -> Result<(), AppError> {
    McpService::upsert_server(&state, server)
}

/// 删除 MCP 服务器
#[tauri::command]
#[specta::specta]
pub async fn delete_mcp_server(state: State<'_, AppState>, id: String) -> Result<bool, AppError> {
    McpService::delete_server(&state, &id)
}

/// 切换 MCP 服务器在指定应用的启用状态
#[tauri::command]
#[specta::specta]
pub async fn toggle_mcp_app(
    state: State<'_, AppState>,
    server_id: String,
    app: String,
    enabled: bool,
) -> Result<(), AppError> {
    let app_ty = AppType::from_str(&app)?;
    McpService::toggle_app(&state, &server_id, app_ty, enabled)
}

/// 从所有应用导入 MCP 服务器（复用已有的导入逻辑）
#[tauri::command]
#[specta::specta]
pub async fn import_mcp_from_apps(state: State<'_, AppState>) -> Result<usize, AppError> {
    McpService::import_from_all_apps(&state)
}
