//! CLI 工具版本探测与安装/更新命令。业务逻辑见 `services::cli_tools`。

use crate::services::cli_tools::{
    self, ToolInstallationReport, ToolVersion, WslShellPreferenceInput,
};
use std::collections::HashMap;

#[tauri::command]
pub async fn get_tool_versions(
    tools: Option<Vec<String>>,
    wsl_shell_by_tool: Option<HashMap<String, WslShellPreferenceInput>>,
) -> Result<Vec<ToolVersion>, String> {
    Ok(cli_tools::get_tool_versions(tools, wsl_shell_by_tool).await)
}

#[tauri::command]
pub async fn run_tool_lifecycle_action(
    tools: Vec<String>,
    action: String,
    wsl_shell_by_tool: Option<HashMap<String, WslShellPreferenceInput>>,
) -> Result<(), String> {
    cli_tools::run_tool_lifecycle_action(tools, &action, wsl_shell_by_tool).await
}

/// 探测各工具的安装分布：枚举所有安装、标记冲突、生成锚定升级命令。只读、无副作用。
#[tauri::command]
pub async fn probe_tool_installations(
    tools: Vec<String>,
) -> Result<Vec<ToolInstallationReport>, String> {
    cli_tools::probe_tool_installations(tools).await
}
