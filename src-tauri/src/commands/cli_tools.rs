//! CLI 工具版本探测与安装/更新命令。业务逻辑见 `services::cli_tools`。

use crate::services::cli_tools::{
    self, ToolInstallationReport, ToolVersion, WslShellPreferenceInput,
};
use std::collections::HashMap;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

#[tauri::command]
pub async fn get_tool_versions(
    tools: Option<Vec<String>>,
    wsl_shell_by_tool: Option<HashMap<String, WslShellPreferenceInput>>,
) -> Result<Vec<ToolVersion>, String> {
    Ok(cli_tools::get_tool_versions(tools, wsl_shell_by_tool).await)
}

/// 安装 / 更新所选工具。输出逐行以 `tool-lifecycle-output` 事件推送（载荷带 `runId`），
/// `timeoutSecs` 缺省为 20 分钟；可用 `cancel_tool_lifecycle_action` 按 `runId` 取消。
#[tauri::command]
pub async fn run_tool_lifecycle_action(
    app: AppHandle,
    tools: Vec<String>,
    action: String,
    wsl_shell_by_tool: Option<HashMap<String, WslShellPreferenceInput>>,
    run_id: Option<String>,
    timeout_secs: Option<u64>,
) -> Result<(), String> {
    let run_id = run_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let timeout = timeout_secs.map(|secs| Duration::from_secs(secs.max(1)));
    cli_tools::run_tool_lifecycle_action(
        tools,
        &action,
        wsl_shell_by_tool,
        run_id,
        timeout,
        |line| {
            if let Err(e) = app.emit(cli_tools::TOOL_LIFECYCLE_OUTPUT_EVENT, line) {
                log::debug!("emit tool lifecycle output failed: {e}");
            }
        },
    )
    .await
}

/// 取消进行中的安装 / 更新；`runId` 缺省时取消全部。返回是否命中了进行中的运行。
#[tauri::command]
pub fn cancel_tool_lifecycle_action(run_id: Option<String>) -> bool {
    cli_tools::cancel_tool_lifecycle_action(run_id.as_deref())
}

/// 探测各工具的安装分布：枚举所有安装、标记冲突、生成锚定升级命令。只读、无副作用。
#[tauri::command]
pub async fn probe_tool_installations(
    tools: Vec<String>,
) -> Result<Vec<ToolInstallationReport>, String> {
    cli_tools::probe_tool_installations(tools).await
}
