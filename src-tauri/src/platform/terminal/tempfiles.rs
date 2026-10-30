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
    #[cfg(unix)]
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

    /// `private_launch_temp_dir` 的 Linux 目录选择（CCS-223）。
    /// `XDG_RUNTIME_DIR` 为进程级环境变量，以 `serial` 与其他改写环境变量的测试互斥；
    /// 应用配置目录通过线程级 `paths::scoped` 注入。
    #[cfg(target_os = "linux")]
    mod launch_temp_dir {
        use super::*;
        use std::os::unix::fs::PermissionsExt;

        /// 改写进程环境变量，释放时恢复原值
        pub(super) struct EnvVarGuard(Vec<(&'static str, Option<std::ffi::OsString>)>);

        impl EnvVarGuard {
            pub(super) fn new() -> Self {
                Self(Vec::new())
            }

            fn remember(&mut self, key: &'static str) {
                if !self.0.iter().any(|(k, _)| *k == key) {
                    self.0.push((key, std::env::var_os(key)));
                }
            }

            pub(super) fn set(&mut self, key: &'static str, value: impl AsRef<std::ffi::OsStr>) {
                self.remember(key);
                std::env::set_var(key, value);
            }

            pub(super) fn remove(&mut self, key: &'static str) {
                self.remember(key);
                std::env::remove_var(key);
            }
        }

        impl Drop for EnvVarGuard {
            fn drop(&mut self) {
                for (key, value) in self.0.drain(..).rev() {
                    match value {
                        Some(value) => std::env::set_var(key, value),
                        None => std::env::remove_var(key),
                    }
                }
            }
        }

        fn mode_of(path: &std::path::Path) -> u32 {
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777
        }

        #[test]
        #[serial_test::serial]
        fn uses_absolute_xdg_runtime_dir() {
            let home = tempfile::tempdir().unwrap();
            let runtime = tempfile::tempdir().unwrap();
            let _paths = crate::paths::scoped(crate::paths::Paths::with_home(home.path()));
            let mut env = EnvVarGuard::new();
            env.set("XDG_RUNTIME_DIR", runtime.path());

            assert_eq!(private_launch_temp_dir(), runtime.path());
            assert!(
                !home.path().join(".cc-switch").join("tmp").exists(),
                "fallback dir must not be created when XDG_RUNTIME_DIR is usable"
            );
        }

        #[test]
        #[serial_test::serial]
        fn missing_xdg_runtime_dir_falls_back_to_private_app_config_tmp() {
            let home = tempfile::tempdir().unwrap();
            let _paths = crate::paths::scoped(crate::paths::Paths::with_home(home.path()));
            let mut env = EnvVarGuard::new();
            env.remove("XDG_RUNTIME_DIR");

            let dir = private_launch_temp_dir();
            assert_eq!(dir, crate::config::get_app_config_dir().join("tmp"));
            assert!(dir.starts_with(home.path()), "{}", dir.display());
            assert!(dir.is_dir());
            assert_eq!(mode_of(&dir), 0o700);
        }

        #[test]
        #[serial_test::serial]
        fn relative_or_nonexistent_xdg_runtime_dir_falls_back() {
            let home = tempfile::tempdir().unwrap();
            let _paths = crate::paths::scoped(crate::paths::Paths::with_home(home.path()));
            let expected = crate::config::get_app_config_dir().join("tmp");
            let missing = home.path().join("no-such-runtime-dir");
            let mut env = EnvVarGuard::new();

            for value in [
                std::ffi::OsString::from("relative/runtime"),
                std::ffi::OsString::from(""),
                missing.into_os_string(),
            ] {
                env.set("XDG_RUNTIME_DIR", &value);
                let dir = private_launch_temp_dir();
                assert_eq!(dir, expected, "XDG_RUNTIME_DIR={value:?}");
                assert_eq!(mode_of(&dir), 0o700, "XDG_RUNTIME_DIR={value:?}");
            }
        }

        #[test]
        #[serial_test::serial]
        fn existing_world_readable_fallback_dir_is_tightened_to_0700() {
            let home = tempfile::tempdir().unwrap();
            let _paths = crate::paths::scoped(crate::paths::Paths::with_home(home.path()));
            let mut env = EnvVarGuard::new();
            env.remove("XDG_RUNTIME_DIR");

            let dir = crate::config::get_app_config_dir().join("tmp");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert_eq!(mode_of(&dir), 0o755);

            assert_eq!(private_launch_temp_dir(), dir);
            assert_eq!(mode_of(&dir), 0o700);
        }
    }

    /// CCS-036 的 Linux 端到端验证（CCS-223）：在无头 X（`xvfb-run`）下真实拉起 xterm，
    /// 由 PATH 上的桩 `claude` 记录 `--settings` 文件的路径、权限与内容。
    /// 需要 `/usr/bin/xterm` 与 `DISPLAY`，因此默认忽略：
    /// `xvfb-run -a cargo test --manifest-path src-tauri/Cargo.toml --lib -- --ignored ccs036_e2e`
    #[cfg(target_os = "linux")]
    mod ccs036_e2e {
        use super::launch_temp_dir::EnvVarGuard;
        use crate::platform::terminal::launch_terminal_with_env;
        use std::os::unix::fs::PermissionsExt;
        use std::path::{Path, PathBuf};
        use std::time::{Duration, Instant};

        const STUB_CLAUDE: &str = r#"#!/bin/sh
cfg=
while [ $# -gt 0 ]; do
  if [ "$1" = "--settings" ]; then shift; cfg="$1"; fi
  shift
done
out="$CCS036_LOG_DIR/$$"
cp "$cfg" "$out.json"
{
  echo "path=$cfg"
  echo "mode=$(stat -c %a "$cfg")"
  echo "ls=$(ls -l "$cfg")"
  for f in "$(dirname "$cfg")"/cc_switch_launcher_*.sh; do
    m=$(stat -c %a "$f" 2>/dev/null) && echo "launcher=$m"
  done
} > "$out.tmp"
sleep 1
mv "$out.tmp" "$out.meta"
"#;

        #[derive(Debug)]
        struct Observation {
            path: PathBuf,
            mode: String,
            ls: String,
            launcher_modes: Vec<String>,
            config: serde_json::Value,
        }

        fn wait_until(what: &str, timeout: Duration, mut done: impl FnMut() -> bool) {
            let deadline = Instant::now() + timeout;
            while !done() {
                assert!(Instant::now() < deadline, "timed out waiting for {what}");
                std::thread::sleep(Duration::from_millis(100));
            }
        }

        fn read_observations(log_dir: &Path) -> Vec<Observation> {
            let mut observations = Vec::new();
            for entry in std::fs::read_dir(log_dir).unwrap() {
                let meta_path = entry.unwrap().path();
                if meta_path.extension().and_then(|e| e.to_str()) != Some("meta") {
                    continue;
                }
                let meta = std::fs::read_to_string(&meta_path).unwrap();
                let field = |key: &str| {
                    meta.lines()
                        .find_map(|line| line.strip_prefix(&format!("{key}=")))
                        .unwrap_or_default()
                        .to_string()
                };
                let launcher_modes = meta
                    .lines()
                    .filter_map(|line| line.strip_prefix("launcher="))
                    .map(str::to_string)
                    .collect();
                let config =
                    std::fs::read_to_string(meta_path.with_extension("json")).unwrap_or_default();
                observations.push(Observation {
                    path: PathBuf::from(field("path")),
                    mode: field("mode"),
                    ls: field("ls"),
                    launcher_modes,
                    config: serde_json::from_str(&config).unwrap_or_default(),
                });
            }
            observations
        }

        fn leftover_launch_files(dir: &Path) -> Vec<String> {
            std::fs::read_dir(dir)
                .map(|entries| {
                    entries
                        .filter_map(|entry| entry.ok())
                        .map(|entry| entry.file_name().to_string_lossy().into_owned())
                        .filter(|name| {
                            name.starts_with("claude_") || name.starts_with("cc_switch_launcher_")
                        })
                        .collect()
                })
                .unwrap_or_default()
        }

        /// 兜底结束本次测试拉起的 xterm（命令行中包含临时目录路径）
        struct KillTerminals(PathBuf);

        impl Drop for KillTerminals {
            fn drop(&mut self) {
                let _ = std::process::Command::new("pkill")
                    .arg("-f")
                    .arg(self.0.as_os_str())
                    .status();
            }
        }

        fn run(xdg_runtime_dir: Option<&Path>) {
            assert!(
                Path::new("/usr/bin/xterm").exists(),
                "ccs036_e2e needs /usr/bin/xterm"
            );
            assert!(
                std::env::var_os("DISPLAY").is_some(),
                "ccs036_e2e needs an X display; run it under xvfb-run -a"
            );

            let home = tempfile::tempdir().unwrap();
            let work = tempfile::tempdir().unwrap();
            let _paths = crate::paths::scoped(crate::paths::Paths::with_home(home.path()));
            let _settings = crate::settings::scoped_settings_store();
            crate::settings::update_settings_with(|current| {
                let mut next = current.clone();
                next.preferred_terminal = Some("xterm".to_string());
                next
            })
            .unwrap();
            assert_eq!(
                crate::settings::get_preferred_terminal().as_deref(),
                Some("xterm")
            );

            let bin_dir = work.path().join("bin");
            let log_dir = work.path().join("log");
            std::fs::create_dir_all(&bin_dir).unwrap();
            std::fs::create_dir_all(&log_dir).unwrap();
            let stub = bin_dir.join("claude");
            std::fs::write(&stub, STUB_CLAUDE).unwrap();
            std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
            // 启动脚本最终 `exec` 交互式 sh；POSIX sh 交互启动时读取 $ENV，借此让窗口自行退出
            let exit_rc = work.path().join("exit.rc");
            std::fs::write(&exit_rc, "exit 0\n").unwrap();

            let mut env = EnvVarGuard::new();
            match xdg_runtime_dir {
                Some(dir) => env.set("XDG_RUNTIME_DIR", dir),
                None => env.remove("XDG_RUNTIME_DIR"),
            }
            let path = std::env::var_os("PATH").unwrap_or_default();
            let mut search = vec![bin_dir.clone()];
            search.extend(std::env::split_paths(&path));
            env.set("PATH", std::env::join_paths(search).unwrap());
            env.set("SHELL", "/bin/sh");
            env.set("ENV", &exit_rc);
            env.set("CCS036_LOG_DIR", &log_dir);

            let temp_dir = super::private_launch_temp_dir();
            match xdg_runtime_dir {
                Some(dir) => assert_eq!(temp_dir, dir),
                None => {
                    assert_eq!(temp_dir, home.path().join(".cc-switch").join("tmp"));
                    let mode = std::fs::metadata(&temp_dir).unwrap().permissions().mode();
                    assert_eq!(mode & 0o777, 0o700);
                }
            }
            let _kill = KillTerminals(temp_dir.clone());

            let env_a = vec![
                ("ANTHROPIC_AUTH_TOKEN".to_string(), "sk-provA".to_string()),
                (
                    "ANTHROPIC_BASE_URL".to_string(),
                    "https://a.example".to_string(),
                ),
            ];
            let env_b = vec![
                ("ANTHROPIC_AUTH_TOKEN".to_string(), "sk-provB".to_string()),
                (
                    "ANTHROPIC_BASE_URL".to_string(),
                    "https://b.example".to_string(),
                ),
            ];
            launch_terminal_with_env(env_a, "provA", None).unwrap();
            launch_terminal_with_env(env_b, "provB", None).unwrap();

            wait_until("both stub claude runs", Duration::from_secs(60), || {
                read_observations(&log_dir).len() >= 2
            });
            wait_until(
                "launch files to be removed",
                Duration::from_secs(60),
                || leftover_launch_files(&temp_dir).is_empty(),
            );

            let mut observations = read_observations(&log_dir);
            assert_eq!(observations.len(), 2, "{observations:#?}");
            observations.sort_by_key(|o| o.config["env"]["ANTHROPIC_AUTH_TOKEN"].to_string());
            assert_ne!(observations[0].path, observations[1].path);
            for (observation, provider) in observations.iter().zip(["provA", "provB"]) {
                eprintln!("ccs036_e2e {provider}: {observation:?}");
                assert_eq!(observation.path.parent(), Some(temp_dir.as_path()));
                let name = observation.path.file_name().unwrap().to_string_lossy();
                assert!(name.starts_with(&format!("claude_{provider}_")), "{name}");
                assert_eq!(observation.mode, "600", "{observation:?}");
                assert!(observation.ls.starts_with("-rw------- "), "{observation:?}");
                assert!(
                    !observation.launcher_modes.is_empty()
                        && observation.launcher_modes.iter().all(|m| m == "700"),
                    "{observation:?}"
                );
                assert_eq!(
                    observation.config["env"]["ANTHROPIC_AUTH_TOKEN"],
                    format!("sk-{provider}")
                );
                assert!(!observation.path.exists(), "{observation:?}");
            }
            assert!(leftover_launch_files(&temp_dir).is_empty());
        }

        #[test]
        #[ignore = "needs xterm and an X display (xvfb-run -a)"]
        #[serial_test::serial]
        fn ccs036_e2e_xdg_runtime_dir() {
            let runtime = tempfile::tempdir().unwrap();
            std::fs::set_permissions(runtime.path(), std::fs::Permissions::from_mode(0o700))
                .unwrap();
            run(Some(runtime.path()));
        }

        #[test]
        #[ignore = "needs xterm and an X display (xvfb-run -a)"]
        #[serial_test::serial]
        fn ccs036_e2e_app_config_fallback() {
            run(None);
        }
    }
}
