//! CLI 工具（Claude Code / Codex / Gemini CLI / Grok Build / OpenCode / OpenClaw / Hermes）的
//! 本地版本探测、远端最新版本查询、安装分布枚举与安装/更新命令生成。

mod installations;
mod lifecycle_cmd;
mod lifecycle_run;
mod remote_versions;
mod search_paths;
mod version_probe;

pub use installations::ToolInstallationReport;
pub use lifecycle_run::{
    cancel_run as cancel_tool_lifecycle_action, ToolLifecycleOutputLine,
    DEFAULT_TOOL_LIFECYCLE_TIMEOUT, TOOL_LIFECYCLE_OUTPUT_EVENT,
};
pub use version_probe::ToolVersion;

use installations::{enumerate_tool_installations, is_conflicting};
use lifecycle_cmd::{build_tool_lifecycle_command, lifecycle_invocation, plan_command_for};
use std::collections::HashMap;
#[cfg(target_os = "windows")]
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;
use version_probe::get_single_tool_version_impl;

const VALID_TOOLS: [&str; 7] = [
    "claude", "codex", "gemini", "grok", "opencode", "openclaw", "hermes",
];

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WslShellPreferenceInput {
    #[serde(default)]
    pub wsl_shell: Option<String>,
    #[serde(default)]
    pub wsl_shell_flag: Option<String>,
}

/// 探测各工具的本地版本与远端最新版本；`tools` 为空时探测全部受支持工具。
pub async fn get_tool_versions(
    tools: Option<Vec<String>>,
    wsl_shell_by_tool: Option<HashMap<String, WslShellPreferenceInput>>,
) -> Vec<ToolVersion> {
    let requested: Vec<&str> = if let Some(tools) = tools.as_ref() {
        let set: std::collections::HashSet<&str> = tools.iter().map(|s| s.as_str()).collect();
        VALID_TOOLS
            .iter()
            .copied()
            .filter(|t| set.contains(t))
            .collect()
    } else {
        VALID_TOOLS.to_vec()
    };
    let mut results = Vec::new();

    for tool in requested {
        let pref = wsl_shell_by_tool.as_ref().and_then(|m| m.get(tool));
        let tool_wsl_shell = pref.and_then(|p| p.wsl_shell.as_deref());
        let tool_wsl_shell_flag = pref.and_then(|p| p.wsl_shell_flag.as_deref());

        results.push(get_single_tool_version_impl(tool, tool_wsl_shell, tool_wsl_shell_flag).await);
    }

    results
}

/// 安装 / 更新所选工具：输出逐行交给 `on_line`，可经 [`cancel_tool_lifecycle_action`]
/// 按 `run_id` 取消，超过 `timeout`（默认 [`DEFAULT_TOOL_LIFECYCLE_TIMEOUT`]）即终止。
/// 任一工具失败即返回其输出末尾若干行。
pub async fn run_tool_lifecycle_action<F>(
    tools: Vec<String>,
    action: &str,
    wsl_shell_by_tool: Option<HashMap<String, WslShellPreferenceInput>>,
    run_id: String,
    timeout: Option<Duration>,
    mut on_line: F,
) -> Result<(), String>
where
    F: FnMut(ToolLifecycleOutputLine) + Send,
{
    let action = ToolLifecycleAction::from_str(action)?;
    let requested = normalize_requested_tools(&tools);
    if requested.is_empty() {
        return Err("No supported tools selected".to_string());
    }

    let label = match action {
        ToolLifecycleAction::Install => "tool_install",
        ToolLifecycleAction::Update => "tool_update",
    };

    // 先登记再做 build：锚定探测期间点取消也能生效（spawn 前会检查取消信号）。
    let (_guard, cancel) = lifecycle_run::register_run(&run_id);

    // build 阶段含锚定探测（对每个工具跑 `--version` 定位命令行实际命中那处），
    // 放进 blocking 线程，避免阻塞 async runtime。
    let invocation = tokio::task::spawn_blocking(move || {
        let command_line =
            build_tool_lifecycle_command(&requested, action, wsl_shell_by_tool.as_ref())?;
        lifecycle_invocation(&command_line, label)
    })
    .await
    .map_err(|e| format!("tool lifecycle task join error: {e}"))??;

    lifecycle_run::run_streaming(
        invocation.program,
        &invocation.args,
        timeout.unwrap_or(DEFAULT_TOOL_LIFECYCLE_TIMEOUT),
        cancel,
        |stream, line| {
            on_line(ToolLifecycleOutputLine {
                run_id: run_id.clone(),
                stream,
                line,
            })
        },
    )
    .await
}

/// 取文本末尾最多 `n` 行（npm / pip 的关键错误通常出现在输出尾部）。
fn last_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

fn normalize_requested_tools(tools: &[String]) -> Vec<&'static str> {
    let set: std::collections::HashSet<&str> = tools.iter().map(|s| s.as_str()).collect();
    VALID_TOOLS
        .iter()
        .copied()
        .filter(|tool| set.contains(tool))
        .collect()
}

#[derive(Debug, Clone, Copy)]
enum ToolLifecycleAction {
    Install,
    Update,
}

impl FromStr for ToolLifecycleAction {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "install" => Ok(Self::Install),
            "update" => Ok(Self::Update),
            _ => Err(format!("Unsupported tool action: {value}")),
        }
    }
}

/// 探测各工具的安装分布：枚举所有安装、标记冲突、生成锚定升级命令。只读、无副作用。
/// 诊断按钮、升级前确认、升级后补诊共用此命令，各取所需字段——避免对同一份枚举结果
/// 散落多套下游判定。
pub async fn probe_tool_installations(
    tools: Vec<String>,
) -> Result<Vec<ToolInstallationReport>, String> {
    let requested = normalize_requested_tools(&tools);
    if requested.is_empty() {
        return Err("No supported tools selected".to_string());
    }
    tokio::task::spawn_blocking(move || {
        requested
            .into_iter()
            .map(|tool| {
                let installs = enumerate_tool_installations(tool);
                let (command, needs_confirmation, anchored) = plan_command_for(tool, &installs);
                let is_conflict = is_conflicting(&installs);
                ToolInstallationReport {
                    tool: tool.to_string(),
                    installs,
                    is_conflict,
                    needs_confirmation,
                    command,
                    anchored,
                }
            })
            .collect()
    })
    .await
    .map_err(|e| format!("probe task join error: {e}"))
}

#[cfg(target_os = "windows")]
fn wsl_distro_for_tool(tool: &str) -> Option<String> {
    let override_dir = match tool {
        "claude" => crate::settings::override_dir(&crate::app_config::AppType::Claude),
        "codex" => crate::settings::override_dir(&crate::app_config::AppType::Codex),
        "gemini" => crate::settings::override_dir(&crate::app_config::AppType::Gemini),
        "grok" => crate::settings::override_dir(&crate::app_config::AppType::GrokBuild),
        "opencode" => crate::settings::override_dir(&crate::app_config::AppType::OpenCode),
        "openclaw" => crate::settings::override_dir(&crate::app_config::AppType::OpenClaw),
        "hermes" => crate::settings::override_dir(&crate::app_config::AppType::Hermes),
        _ => None,
    }?;

    wsl_distro_from_path(&override_dir)
}

/// 从 UNC 路径中提取 WSL 发行版名称
/// 支持 `\\wsl$\Ubuntu\...` 和 `\\wsl.localhost\Ubuntu\...` 两种格式
#[cfg(target_os = "windows")]
fn wsl_distro_from_path(path: &Path) -> Option<String> {
    use std::path::{Component, Prefix};
    let Some(Component::Prefix(prefix)) = path.components().next() else {
        return None;
    };
    match prefix.kind() {
        Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
            let server_name = server.to_string_lossy();
            if server_name.eq_ignore_ascii_case("wsl$")
                || server_name.eq_ignore_ascii_case("wsl.localhost")
            {
                let distro = share.to_string_lossy().to_string();
                if !distro.is_empty() {
                    return Some(distro);
                }
            }
            None
        }
        _ => None,
    }
}
