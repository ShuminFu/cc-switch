//! 终端启动用的私有临时文件：随机文件名、`O_EXCL`、0600/0700（CCS-036）。

use std::path::PathBuf;

/// 写入 claude 配置文件，返回实际创建的私有临时文件路径
pub(super) fn write_claude_config(
    temp_dir: &std::path::Path,
    provider_id: &str,
    env_vars: &[(String, String)],
) -> Result<PathBuf, String> {
    let mut config_obj = serde_json::Map::new();
    let mut env_obj = serde_json::Map::new();

    for (key, value) in env_vars {
        env_obj.insert(key.clone(), serde_json::Value::String(value.clone()));
    }

    config_obj.insert("env".to_string(), serde_json::Value::Object(env_obj));

    let config_json =
        serde_json::to_string_pretty(&config_obj).map_err(|e| format!("序列化配置失败: {e}"))?;

    let prefix = format!("claude_{}_", sanitize_temp_file_component(provider_id));
    write_private_temp_file(temp_dir, &prefix, ".json", config_json.as_bytes(), false)
        .map_err(|e| format!("写入配置文件失败: {e}"))
}

/// 启动终端用的临时文件目录。
/// Linux 上 `/tmp` 为多用户共享，优先使用 `$XDG_RUNTIME_DIR`（0700，按用户隔离），
/// 否则回退到 `<app_config_dir>/tmp`（0700）；macOS 的 `$TMPDIR` 与 Windows 的 `%TEMP%`
/// 本身已按用户隔离，直接使用系统临时目录。
pub(super) fn private_launch_temp_dir() -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;

        if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute() && dir.is_dir())
        {
            return dir;
        }

        let dir = crate::config::get_app_config_dir().join("tmp");
        if std::fs::create_dir_all(&dir).is_ok()
            && std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).is_ok()
        {
            return dir;
        }
    }

    std::env::temp_dir()
}

/// 将 provider id 等外部输入收敛为安全的文件名片段：仅保留 ASCII 字母数字、`-`、`_`，并限制长度。
pub(super) fn sanitize_temp_file_component(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(32)
        .collect();
    if sanitized.is_empty() {
        "provider".to_string()
    } else {
        sanitized
    }
}

/// 以随机文件名 + `O_EXCL` 在 `dir` 下创建私有临时文件并写入内容，持久化后返回路径。
/// Unix 上权限为 0600（`executable` 时 0700），避免同机其他用户读取或抢占文件名；
/// 文件不会随句柄释放而删除，由启动脚本或调用方负责清理。
pub(super) fn write_private_temp_file(
    dir: &std::path::Path,
    prefix: &str,
    suffix: &str,
    content: &[u8],
    executable: bool,
) -> Result<PathBuf, String> {
    use std::io::Write;

    let mut builder = tempfile::Builder::new();
    builder.prefix(prefix).suffix(suffix).rand_bytes(12);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if executable { 0o700 } else { 0o600 };
        builder.permissions(std::fs::Permissions::from_mode(mode));
    }
    #[cfg(not(unix))]
    let _ = executable;

    let mut file = builder
        .tempfile_in(dir)
        .map_err(|e| format!("创建临时文件失败: {e}"))?;
    file.write_all(content)
        .and_then(|_| file.flush())
        .map_err(|e| format!("写入临时文件失败: {e}"))?;
    let (_, path) = file.keep().map_err(|e| format!("保存临时文件失败: {e}"))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::shell::shell_single_quote;
    use crate::platform::terminal::posix::build_provider_launcher_script;

    mod private_temp_files {
        use super::*;

        #[test]
        fn write_private_temp_file_returns_distinct_private_paths() {
            let dir = tempfile::tempdir().unwrap();
            let a = write_private_temp_file(dir.path(), "cc_switch_test_", ".json", b"a", false)
                .unwrap();
            let b = write_private_temp_file(dir.path(), "cc_switch_test_", ".json", b"b", false)
                .unwrap();

            assert_ne!(a, b);
            assert_eq!(a.parent(), Some(dir.path()));
            assert_eq!(std::fs::read(&a).unwrap(), b"a");
            assert_eq!(std::fs::read(&b).unwrap(), b"b");

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                for path in [&a, &b] {
                    let mode = std::fs::metadata(path).unwrap().permissions().mode();
                    assert_eq!(mode & 0o077, 0, "{} mode {mode:o}", path.display());
                }
            }
        }

        #[cfg(unix)]
        #[test]
        fn write_private_temp_file_executable_is_owner_only() {
            use std::os::unix::fs::PermissionsExt;

            let dir = tempfile::tempdir().unwrap();
            let path =
                write_private_temp_file(dir.path(), "cc_switch_test_", ".sh", b"", true).unwrap();
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700);
        }

        #[test]
        fn write_claude_config_creates_private_unique_files_with_sanitized_name() {
            let dir = tempfile::tempdir().unwrap();
            let env_vars = vec![("ANTHROPIC_AUTH_TOKEN".to_string(), "sk-secret".to_string())];

            let a = write_claude_config(dir.path(), "../evil id/x", &env_vars).unwrap();
            let b = write_claude_config(dir.path(), "../evil id/x", &env_vars).unwrap();

            assert_ne!(a, b);
            for path in [&a, &b] {
                assert_eq!(path.parent(), Some(dir.path()));
                let name = path.file_name().unwrap().to_string_lossy();
                assert!(name.starts_with("claude_evilidx_"), "{name}");
                assert!(name.ends_with(".json"), "{name}");

                let value: serde_json::Value =
                    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
                assert_eq!(value["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-secret");

                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = std::fs::metadata(path).unwrap().permissions().mode();
                    assert_eq!(mode & 0o077, 0, "mode {mode:o}");
                }
            }
        }

        #[test]
        fn sanitize_temp_file_component_keeps_safe_chars_only() {
            assert_eq!(
                sanitize_temp_file_component("my-provider_1"),
                "my-provider_1"
            );
            assert_eq!(sanitize_temp_file_component("a/../b c$"), "abc");
            assert_eq!(sanitize_temp_file_component("../"), "provider");
            assert_eq!(sanitize_temp_file_component(&"x".repeat(100)).len(), 32);
        }

        #[test]
        fn provider_launcher_script_removes_config_before_exec() {
            let script = build_provider_launcher_script(
                "/tmp/it's dir/claude_x.json",
                "claude --settings x",
                "",
                "exec '/bin/zsh'",
            );

            assert!(script.contains("cc_switch_config='/tmp/it'\"'\"'s dir/claude_x.json'"));
            let run = script.find("claude --settings x").unwrap();
            let cleanup = script.find("rm -f \"$cc_switch_config\" \"$0\"\n").unwrap();
            let exec = script.find("exec '/bin/zsh'").unwrap();
            assert!(run < cleanup && cleanup < exec, "{script}");
        }

        /// 回归：exec 结尾时 EXIT trap 不触发，配置文件（含 API Key）与脚本必须在 exec 前被删除。
        #[cfg(unix)]
        #[test]
        fn provider_launcher_script_deletes_files_even_when_ending_with_exec() {
            let dir = tempfile::tempdir().unwrap();
            let config = write_claude_config(
                dir.path(),
                "p1",
                &[("ANTHROPIC_AUTH_TOKEN".to_string(), "sk".to_string())],
            )
            .unwrap();
            let marker = dir.path().join("ran");
            let provider_command =
                format!("touch {}", shell_single_quote(&marker.to_string_lossy()));
            let content = build_provider_launcher_script(
                &config.to_string_lossy(),
                &provider_command,
                "",
                "exec true",
            );
            let script = write_private_temp_file(
                dir.path(),
                "cc_switch_launcher_",
                ".sh",
                content.as_bytes(),
                true,
            )
            .unwrap();

            let status = std::process::Command::new("sh")
                .arg(&script)
                .status()
                .unwrap();

            assert!(status.success());
            assert!(marker.exists(), "provider command should run");
            assert!(!config.exists(), "config file must be removed before exec");
            assert!(
                !script.exists(),
                "launcher script must be removed before exec"
            );
        }
    }
}
