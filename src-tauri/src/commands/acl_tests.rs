//! 命令域与 ACL 的回归测试（CCS-180）。
//!
//! 用真实的 `generate_context!()`（含 build.rs 生成的 app manifest 与 capabilities）
//! 构建 mock 应用，从 IPC 入口验证：已授予的命令可调用，未授予的命令以 "not allowed" 被拒。

use super::registry::{command_name, COMMAND_DOMAINS};
use std::collections::BTreeSet;
use tauri::test::{get_ipc_response, mock_builder, MockRuntime, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{Manager, WebviewWindow};

// 与真实命令同名的替身：ACL 只按命令名判定，替身避免依赖 AppState。
#[tauri::command]
fn get_providers() -> &'static str {
    "ok"
}

#[tauri::command]
fn delete_env_vars() -> &'static str {
    "ok"
}

#[tauri::command]
fn write_workspace_file() -> &'static str {
    "ok"
}

/// 已注册到 handler、但不属于任何命令域（因此没有任何权限）的命令
#[tauri::command]
fn unlisted_command() -> &'static str {
    "ok"
}

fn mock_app() -> tauri::App<MockRuntime> {
    mock_builder()
        .invoke_handler(tauri::generate_handler![
            get_providers,
            delete_env_vars,
            write_workspace_file,
            unlisted_command
        ])
        .build(tauri::generate_context!(test = true))
        .expect("build mock app")
}

fn main_window(app: &tauri::App<MockRuntime>) -> WebviewWindow<MockRuntime> {
    app.get_webview_window("main").unwrap_or_else(|| {
        tauri::WebviewWindowBuilder::new(app, "main", Default::default())
            .build()
            .expect("main window")
    })
}

fn invoke(
    window: &WebviewWindow<MockRuntime>,
    cmd: &str,
    url: &str,
) -> Result<String, serde_json::Value> {
    get_ipc_response(
        window,
        InvokeRequest {
            cmd: cmd.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: url.parse().unwrap(),
            body: tauri::ipc::InvokeBody::default(),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .map(|body| body.deserialize::<String>().unwrap())
}

fn assert_not_allowed(result: Result<String, serde_json::Value>, cmd: &str) {
    let err = result.expect_err(&format!("{cmd} should be rejected by the ACL"));
    assert!(
        err.to_string().contains("not allowed"),
        "{cmd}: unexpected error {err}"
    );
}

const LOCAL_URL: &str = "tauri://localhost";

#[test]
fn granted_commands_are_callable_from_the_main_window() {
    let app = mock_app();
    let main = main_window(&app);
    assert_eq!(invoke(&main, "get_providers", LOCAL_URL).unwrap(), "ok");
    assert_eq!(invoke(&main, "delete_env_vars", LOCAL_URL).unwrap(), "ok");
    assert_eq!(
        invoke(&main, "write_workspace_file", LOCAL_URL).unwrap(),
        "ok"
    );
}

#[test]
fn commands_without_a_permission_are_not_allowed() {
    let app = mock_app();
    let main = main_window(&app);
    assert_not_allowed(
        invoke(&main, "unlisted_command", LOCAL_URL),
        "unlisted_command",
    );
}

#[test]
fn commands_are_not_allowed_outside_the_granted_window_or_origin() {
    let app = mock_app();
    let other = tauri::WebviewWindowBuilder::new(&app, "other", Default::default())
        .build()
        .expect("other window");
    for cmd in ["get_providers", "delete_env_vars", "write_workspace_file"] {
        assert_not_allowed(invoke(&other, cmd, LOCAL_URL), cmd);
    }

    let main = main_window(&app);
    for cmd in ["get_providers", "delete_env_vars"] {
        assert_not_allowed(invoke(&main, cmd, "https://example.com/"), cmd);
    }
}

#[test]
fn every_command_belongs_to_exactly_one_domain() {
    let mut seen = BTreeSet::new();
    for domain in COMMAND_DOMAINS {
        assert!(!domain.commands.is_empty(), "empty domain {}", domain.id);
        assert_eq!(
            domain.privileged,
            domain.id.starts_with("privileged-"),
            "privileged domains are named privileged-*: {}",
            domain.id
        );
        for path in domain.commands {
            let name = command_name(path);
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                "bad command name {name:?}"
            );
            assert!(
                seen.insert(name),
                "{name} is listed in more than one domain"
            );
        }
    }
    assert!(seen.len() > 250, "only {} commands", seen.len());
}

fn capability_permissions(json: &str) -> (Vec<String>, Vec<String>) {
    let value: serde_json::Value = serde_json::from_str(json).expect("capability json");
    let windows = value["windows"]
        .as_array()
        .expect("windows")
        .iter()
        .map(|w| w.as_str().unwrap().to_string())
        .collect();
    let permissions = value["permissions"]
        .as_array()
        .expect("permissions")
        .iter()
        .filter_map(|p| p.as_str())
        .map(str::to_string)
        .collect();
    (windows, permissions)
}

#[test]
fn privileged_domains_are_only_granted_by_the_privileged_capability() {
    let (default_windows, default_permissions) =
        capability_permissions(include_str!("../../capabilities/default.json"));
    let (privileged_windows, privileged_permissions) =
        capability_permissions(include_str!("../../capabilities/privileged.json"));
    assert_eq!(default_windows, ["main"]);
    assert_eq!(privileged_windows, ["main"]);

    let app_sets = |permissions: &[String]| -> BTreeSet<String> {
        permissions
            .iter()
            .filter(|p| !p.contains(':'))
            .cloned()
            .collect()
    };
    let standard: BTreeSet<String> = COMMAND_DOMAINS
        .iter()
        .filter(|d| !d.privileged)
        .map(|d| d.id.to_string())
        .collect();
    let privileged: BTreeSet<String> = COMMAND_DOMAINS
        .iter()
        .filter(|d| d.privileged)
        .map(|d| d.id.to_string())
        .collect();

    // default.json 只授予标准域（不得直接授予单个命令或特权域）
    assert_eq!(app_sets(&default_permissions), standard);
    // privileged.json 只授予特权域
    assert_eq!(app_sets(&privileged_permissions), privileged);
    assert_eq!(privileged_permissions.len(), privileged.len());
}
